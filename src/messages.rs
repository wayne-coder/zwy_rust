//! 消息读取：初次导入 + 增量轮询（实时追加）。
//!
//! 微信 4.x 的消息表名为 `Msg_<md5(会话username)>`，发送者用分片内
//! `Name2Id.rowid` 表示；消息正文可能是 UTF-8、微信容器头 + 明文，或 zstd 帧。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{Local, TimeZone, Timelike};
use md5::{Digest, Md5};

use crate::decrypt::{self, KeySpec};
use crate::model::{ImportResult, MsgRow, QuoteRef};
use crate::sqlite::{Conn, Row};

/// 导入与实时监控会同时对消息库做解密/查询；串行化避免临时文件互相覆盖。
static DECRYPT_LOCK: Mutex<()> = Mutex::new(());

/// 微信一般只允许在消息发出后 2 分钟内撤回，这里多留几分钟余量。
///
/// 用于两处：① 导入 / 监控单独扫描撤回提示时，时间窗延伸到时段结束之后，
/// 接住「时段末尾发出的消息在时段结束后才被撤回」；② 监控到结束时间后再多
/// 跑一个窗口，而不是一到点就停。
pub const REVOKE_WINDOW_SECS: i64 = 300;

/// 监控时每次额外回看的撤回提示时间窗（秒）。
///
/// 微信会把撤回提示回填到原消息的位置，提示的 sort 可能和原消息相同甚至更早，
/// 增量水位（`sort_seq > last_sort`）取不到它；按时间回看一段才能读到。
const REVOKE_LOOKBACK_SECS: i64 = 600;

pub enum ImportMsg {
    Status(String),
    Done(Box<ImportResult>),
    Failed(String),
}

pub enum MonitorMsg {
    Rows(Vec<MsgRow>),
    Status(String),
    Stopped(String),
}

#[derive(Clone)]
pub struct ImportRequest {
    pub account_dir: PathBuf,
    pub workdir: PathBuf,
    pub keys: HashMap<String, KeySpec>,
    /// 直接选中的联系人会话
    pub contacts: Vec<String>,
    /// (群 username, 选中的成员；空 = 整个群)
    pub groups: Vec<(String, Vec<String>)>,
    /// username → 显示名
    pub name_of: HashMap<String, String>,
    pub start_ts: i64,
    pub end_ts: i64,
}

/// 解析 "YYYY-MM-DD" + "H:MM" 为本地时区的时间戳区间。
pub fn parse_range(date: &str, from: &str, to: &str) -> Result<(i64, i64), String> {
    let date = date.trim();
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| format!("日期格式应为 YYYY-MM-DD（当前：{date}）"))?;
    let parse_hm = |text: &str| -> Result<chrono::NaiveTime, String> {
        let t = text.trim();
        let mut parts = t.split(':');
        let h: u32 = parts
            .next()
            .unwrap_or("")
            .trim()
            .parse()
            .map_err(|_| format!("时间格式应为 H:MM（当前：{t}）"))?;
        let m: u32 = parts
            .next()
            .unwrap_or("0")
            .trim()
            .parse()
            .map_err(|_| format!("时间格式应为 H:MM（当前：{t}）"))?;
        if parts.next().is_some() || h > 23 || m > 59 {
            return Err(format!("时间格式应为 H:MM（当前：{t}）"));
        }
        chrono::NaiveTime::from_hms_opt(h, m, 0).ok_or_else(|| format!("时间无效：{t}"))
    };
    range_to_ts(d, parse_hm(from)?, parse_hm(to)?)
}

/// 由日期与起止时间（均为本地时间）换算时间戳区间。
pub fn range_to_ts(
    date: chrono::NaiveDate,
    from: chrono::NaiveTime,
    to: chrono::NaiveTime,
) -> Result<(i64, i64), String> {
    if to < from {
        return Err("结束时间早于开始时间。".to_string());
    }
    let start = date.and_time(from);
    let end = date.and_time(to);
    let tz = Local;
    let s = tz
        .from_local_datetime(&start)
        .single()
        .ok_or_else(|| "开始时间无效。".to_string())?
        .timestamp();
    let e = tz
        .from_local_datetime(&end)
        .single()
        .ok_or_else(|| "结束时间无效。".to_string())?
        .timestamp();
    Ok((s, e))
}

struct Collected {
    rows: Vec<MsgRow>,
    max_sort: i64,
    warnings: Vec<String>,
}

/// 初次导入：读取区间内全部消息。
pub fn run(req: &ImportRequest, status: &dyn Fn(String)) -> Result<ImportResult, String> {
    let c = collect(req, None, status)?;
    let mut summary = format!(
        "共 {} 条消息 · 联系人 {} 个 · 群聊 {} 个",
        c.rows.len(),
        req.contacts.len(),
        req.groups.len()
    );
    if !c.warnings.is_empty() {
        summary.push_str(&format!(" · {} 个分片跳过", c.warnings.len()));
    }
    Ok(ImportResult {
        rows: c.rows,
        summary,
    })
}

/// 增量读取：仅取 `sort_seq > since_sort` 的新消息（供监控使用）。
pub fn poll_new(req: &ImportRequest, since_sort: i64) -> Result<Vec<MsgRow>, String> {
    Ok(collect(req, Some(since_sort), &|_| {})?.rows)
}

fn collect(
    req: &ImportRequest,
    since_sort: Option<i64>,
    status: &dyn Fn(String),
) -> Result<Collected, String> {
    let _guard = DECRYPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let db_files = message_dbs(&req.account_dir);
    if db_files.is_empty() {
        return Err("没有找到消息数据库（message_N.db）。".to_string());
    }

    let mut targets: Vec<(String, Option<HashSet<String>>, String)> = Vec::new();
    for c in &req.contacts {
        targets.push((table_for(c), None, c.clone()));
    }
    for (g, members) in &req.groups {
        let filter = if members.is_empty() {
            None
        } else {
            Some(members.iter().cloned().collect::<HashSet<_>>())
        };
        targets.push((table_for(g), filter, g.clone()));
    }

    let mut rows: Vec<MsgRow> = Vec::new();
    // 会话也参与去重：同一成员在不同群聊里的消息必须各自保留。
    let mut seen: HashSet<(i64, String, String, String)> = HashSet::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut max_sort = since_sort.unwrap_or(req.start_ts.saturating_mul(1000));
    // 撤回提示的独立扫描窗口（不受增量 sort 水位 / 时段结束限制）。
    let (notice_from, notice_to) =
        revoke_notice_window(since_sort, req.start_ts, req.end_ts, Local::now().timestamp());

    for (rel, path) in db_files {
        let Some(spec) = req.keys.get(&rel) else {
            warnings.push(format!("{rel} 无密钥，已跳过"));
            continue;
        };
        status(format!("正在解密 {rel}…"));
        let dst = req.workdir.join(rel.replace('\\', "__"));
        if let Err(e) = decrypt::decrypt_with_wal(&path, &dst, spec) {
            warnings.push(format!("{rel} 解密失败: {e}"));
            continue;
        }
        let conn = match Conn::open_ro(&dst) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("{rel} 打开失败: {e}"));
                continue;
            }
        };

        let existing: HashSet<String> = conn
            .query("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'Msg_%'")
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.text(0))
            .collect();
        if existing.is_empty() {
            continue;
        }

        status(format!("正在读取 {rel} 的消息…"));
        let name2id = load_name2id(&conn);
        for (table, filter, chat) in &targets {
            if !existing.contains(table) {
                continue;
            }
            let since_sql = match since_sort {
                Some(s) => format!(" AND sort_seq > {s}"),
                None => String::new(),
            };
            let sql = format!(
                "SELECT local_type, real_sender_id, create_time, message_content, compress_content, sort_seq, local_id \
                 FROM \"{table}\" WHERE create_time >= {} AND create_time <= {}{} \
                 ORDER BY sort_seq, local_id",
                req.start_ts, req.end_ts, since_sql
            );
            let qrows = match conn.query(&sql) {
                Ok(v) => v,
                Err(e) => {
                    warnings.push(format!("{rel} 查询 {table} 失败: {e}"));
                    continue;
                }
            };
            for r in qrows {
                let row = row_to_msg(&conn, table, &r, chat, &name2id, &req.name_of);
                // 撤回提示是系统消息（发送人为空），不属于任何分组成员；
                // 它也不再进引用列表（用户 2026-11-25），按普通消息的成员过滤丢掉。
                if let Some(f) = filter {
                    // 撤回提示要参与「找到被撤回原消息」，即使它的发送人为空 /
                    // 不在所选成员里也要放进来；它不会进注单列表。
                    if row.kind != "撤回" && !f.contains(&row.sender) {
                        continue;
                    }
                }
                if !seen.insert((
                    row.sort,
                    row.chat.clone(),
                    row.sender.clone(),
                    row.content.clone(),
                )) {
                    continue;
                }
                max_sort = max_sort.max(row.sort);
                rows.push(row);
            }

            // 撤回提示单独再扫一遍，不受 `sort_seq` 增量水位约束：微信会把提示
            // 回填到原消息的位置（sort 相同甚至更早），按增量水位过滤会永远
            // 读不到，原注单就会一直留在「未撤回」。主查询已经按 `seen` 去重，
            // 同一行不会重复进入列表。
            let notice_sql = format!(
                "SELECT local_type, real_sender_id, create_time, message_content, compress_content, sort_seq, local_id \
                 FROM \"{table}\" WHERE create_time >= {notice_from} AND create_time <= {notice_to} \
                 AND ((local_type & 0xFFFF) IN (10000, 10002) \
                      OR message_content LIKE '%revokemsg%' \
                      OR compress_content LIKE '%revokemsg%') \
                 ORDER BY sort_seq, local_id"
            );
            match conn.query(&notice_sql) {
                Ok(notices) => {
                    for r in notices {
                        let row = row_to_msg(&conn, table, &r, chat, &name2id, &req.name_of);
                        // 只有解码后确认是撤回提示的才补进来。
                        if row.kind != "撤回" {
                            continue;
                        }
                        if !seen.insert((
                            row.sort,
                            row.chat.clone(),
                            row.sender.clone(),
                            row.content.clone(),
                        )) {
                            continue;
                        }
                        max_sort = max_sort.max(row.sort);
                        rows.push(row);
                    }
                }
                Err(e) => warnings.push(format!("{rel} 查询 {table} 撤回提示失败: {e}")),
            }
        }
    }

    rows.sort_by_key(|r| r.sort);
    Ok(Collected {
        rows,
        max_sort,
        warnings,
    })
}

/// 撤回提示的独立扫描时间窗（秒）。
///
/// - 全量导入：`[start, end + 撤回窗口]`。时段末尾发出的消息可能在该时段
///   结束之后才被撤回（提示的 create_time 晚于 end），不能按主消息的
///   `create_time <= end` 一起过滤，否则重新导入时会把已撤回的原消息带回来。
/// - 增量监控：从增量水位 / 当前时间中较小者往前回看一段，覆盖 sort 不前进
///   （回填到原消息位置）的提示，这类提示用 `sort_seq > last_sort` 永远取不到。
pub(crate) fn revoke_notice_window(
    since_sort: Option<i64>,
    start_ts: i64,
    end_ts: i64,
    now: i64,
) -> (i64, i64) {
    match since_sort {
        Some(s) => {
            // sort 一般是毫秒时间戳；`.min(now)` 兜住不可预期的异常水位。
            let base = (s / 1000).min(now);
            (base.saturating_sub(REVOKE_LOOKBACK_SECS), now)
        }
        None => (start_ts, end_ts.saturating_add(REVOKE_WINDOW_SECS)),
    }
}

/// 启动实时监控线程：每 `interval` 检查消息库是否有变化，有则增量读取。
pub fn start_monitor(
    req: ImportRequest,
    since_sort: i64,
    interval: Duration,
    ctx: eframe::egui::Context,
) -> (Arc<AtomicBool>, Receiver<MonitorMsg>) {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);

    std::thread::spawn(move || {
        let mut last_sort = since_sort;
        // 0 表示首轮必查：补上「初次导入完成」到「监控线程启动」之间到达的消息。
        let mut last_sig: u64 = 0;
        let _ = tx.send(MonitorMsg::Status("实时监控中".to_string()));
        ctx.request_repaint();

        loop {
            // 分成 200ms 小片，停止时能及时响应。
            let slices = (interval.as_millis() / 200).max(1);
            for _ in 0..slices {
                if stop2.load(Ordering::Relaxed) {
                    final_poll(&req, last_sort, &tx);
                    let _ = tx.send(MonitorMsg::Stopped("已停止".to_string()));
                    ctx.request_repaint();
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }

            if Local::now().timestamp()
                > req.end_ts.saturating_add(REVOKE_WINDOW_SECS)
            {
                // 到点前再做一次补拉，避免漏掉最后一个轮询间隔内的消息。
                // 结束时间之后仍多跑一个撤回窗口：时段末尾发出的注单可能在
                // 时段结束之后才被撤回，提示落在 end_ts 之后，主消息查询看不到。
                final_poll(&req, last_sort, &tx);
                let _ = tx.send(MonitorMsg::Stopped("已到结束时间".to_string()));
                ctx.request_repaint();
                return;
            }

            let sig = db_signature(&req.account_dir);
            if sig == last_sig {
                continue;
            }
            last_sig = sig;

            match collect(&req, Some(last_sort), &|_| {}) {
                Ok(c) => {
                    last_sort = last_sort.max(c.max_sort);
                    if !c.rows.is_empty() {
                        // 新引用要立刻弹引用列表：先确保主窗口没被最小化。
                        // 主窗口不可见时 eframe 可能不再跑 UI，光 request_repaint
                        // 不一定能把引用窗口弹出来，所以这里直接把主窗口恢复。
                        if c.rows.iter().any(|r| r.kind == "引用") {
                            ctx.send_viewport_cmd_to(
                                eframe::egui::ViewportId::ROOT,
                                eframe::egui::ViewportCommand::Minimized(false),
                            );
                            ctx.send_viewport_cmd_to(
                                eframe::egui::ViewportId::ROOT,
                                eframe::egui::ViewportCommand::Visible(true),
                            );
                        }
                        let _ = tx.send(MonitorMsg::Rows(c.rows));
                        ctx.request_repaint();
                    }
                }
                Err(e) => {
                    let _ = tx.send(MonitorMsg::Status(format!("读取新消息失败：{e}")));
                    ctx.request_repaint();
                }
            }
        }
    });

    (stop, rx)
}

fn final_poll(req: &ImportRequest, since_sort: i64, tx: &Sender<MonitorMsg>) {
    if let Ok(c) = collect(req, Some(since_sort), &|_| {}) {
        if !c.rows.is_empty() {
            let _ = tx.send(MonitorMsg::Rows(c.rows));
        }
    }
}

/// 消息库（含 WAL）的大小/修改时间签名；没变化就跳过昂贵的解密查询。
fn db_signature(account: &Path) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for (_, p) in message_dbs(account) {
        let mut paths = vec![p.clone()];
        if let Some(name) = p.file_name() {
            paths.push(p.with_file_name(format!("{}-wal", name.to_string_lossy())));
        }
        for path in paths {
            if let Ok(m) = fs::metadata(&path) {
                let len = m.len();
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                h = h.wrapping_mul(1099511628211) ^ len;
                h = h.wrapping_mul(1099511628211) ^ t;
            }
        }
    }
    h
}

fn message_dbs(account: &Path) -> Vec<(String, PathBuf)> {
    let dir = account.join("db_storage").join("message");
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("message_") || !name.ends_with(".db") {
            continue;
        }
        let stem = &name[..name.len() - 3];
        if !stem["message_".len()..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push((format!("message\\{name}"), e.path()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn table_for(user: &str) -> String {
    let mut h = Md5::new();
    h.update(user.as_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(32);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("Msg_{hex}")
}

fn load_name2id(conn: &Conn) -> HashMap<i64, String> {
    let mut out = HashMap::new();
    if let Ok(rows) = conn.query("SELECT rowid, user_name FROM Name2Id") {
        for r in rows {
            out.insert(r.int(0), r.text(1));
        }
    }
    out
}

/// 把 `Msg_*` 表的一行解析成列表用的消息；撤回提示顺带解析出原消息 sort。
///
/// 查询列的顺序固定为：`local_type, real_sender_id, create_time,
/// message_content, compress_content, sort_seq, local_id`。
fn row_to_msg(
    conn: &Conn,
    table: &str,
    r: &Row,
    chat: &str,
    name2id: &HashMap<i64, String>,
    name_of: &HashMap<String, String>,
) -> MsgRow {
    let mtype = r.int(0);
    let sender_id = r.int(1);
    let sender = name2id.get(&sender_id).cloned().unwrap_or_default();
    let ts = r.int(2);
    let content_bytes = r.bytes(3);
    let compress = r.bytes(4);
    let (kind, content, link_url, quote, revoke_msgid) =
        decode_message(mtype, &content_bytes, &compress);
    // 撤回提示：用 `newmsgid` 精确查出被撤回原消息的 sort。
    // 旧库没有 server_id 列时查询失败，保持 None 退回原来的匹配方式。
    let revoke_target = revoke_msgid.and_then(|msgid| {
        // 原消息一定在撤回提示之前，微信撤回窗口只有两分钟；
        // 加时间范围既走 create_time 索引，也避免匹配到同 id 的怪数据。
        let since = ts.saturating_sub(600);
        let sql = format!(
            "SELECT sort_seq, create_time FROM \"{table}\" \
             WHERE server_id = {msgid} AND create_time BETWEEN {since} AND {ts} LIMIT 1"
        );
        // 旧库没有 server_id 列 → 查询失败 → None（退回原匹配方式）；
        // 查询成功但找不到 → Some(0)（有 id 也不许再猜别的消息）。
        match conn.query(&sql) {
            Err(_) => None,
            Ok(rows) => Some(
                rows.first()
                    .map(|r| {
                        let sort_seq = r.int(0);
                        if sort_seq > 0 {
                            sort_seq
                        } else {
                            r.int(1).saturating_mul(1000)
                        }
                    })
                    .filter(|s| *s > 0)
                    .unwrap_or(0),
            ),
        }
    });
    let sort_seq = r.int(5);
    let sort = if sort_seq > 0 {
        sort_seq
    } else {
        ts.saturating_mul(1000)
    };
    let nick = if sender.is_empty() {
        "系统".to_string()
    } else {
        name_of
            .get(&sender)
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| sender.clone())
    };
    MsgRow {
        kind,
        nick,
        content,
        time: format_time(ts),
        sort,
        chat: chat.to_string(),
        sender,
        link_url,
        quote,
        revoke_target,
    }
}

fn format_time(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0).single() {
        Some(dt) => format!("{}:{:02}", dt.hour(), dt.minute()),
        None => String::new(),
    }
}

// ---------------- 正文解码 ----------------

fn decode_message(
    mtype: i64,
    content: &[u8],
    compress: &[u8],
) -> (String, String, Option<String>, Option<QuoteRef>, Option<i64>) {
    let (mut kind, fallback) = kind_of(mtype);
    let mut text = match kind.as_str() {
        "文字" | "系统" | "链接" | "其他" | "红包" => {
            let mut t = decode_text(content);
            if t.is_none() && !compress.is_empty() {
                t = decode_text(compress);
            }
            t.unwrap_or_default()
        }
        // 10002 也可能是撤回 / 系统提示，先解出正文再判断。
        _ if mtype & 0xFFFF_FFFF == 10002 => {
            let mut t = decode_text(content);
            if t.is_none() && !compress.is_empty() {
                t = decode_text(compress);
            }
            t.unwrap_or_default()
        }
        _ => String::new(),
    };
    let mut link_url = None;
    let mut quote = None;
    // 撤回提示里 `newmsgid` 指向被撤回原消息的服务端 id；能精确匹配时
    // 就不再靠「同发送人最近一条」猜，避免把没撤回的消息误记成已撤回。
    let mut revoke_msgid: Option<i64> = None;

    if kind == "链接" && !text.is_empty() {
        // 链接与「引用」都走 appmsg（type 49）。引用消息的 <type>57</type>
        // 和被引用的原文都在 <refermsg> 里；普通链接才取 <url>。
        let unescaped = text.replace("&lt;", "<").replace("&gt;", ">");
        let xml = if text.contains("<refermsg") {
            text.as_str()
        } else {
            unescaped.as_str()
        };
        if extract_tag(xml, "type").as_deref() == Some("57") || xml.contains("<refermsg") {
            kind = "引用".to_string();
            quote = parse_quote(xml);
        } else {
            link_url = extract_tag(xml, "url").or_else(|| extract_tag(xml, "link"));
        }
        if let Some(title) = extract_title(xml) {
            text = title;
        }
    }

    if kind == "文字" || kind == "系统" {
        text = strip_sender_prefix(&text);
    }
    // 撤回提示（系统消息 XML：<sysmsg type="revokemsg">）单独归为「撤回」，
    // 导入时直接丢弃（2026-11-25 起不再进引用列表）；正文取其中的 content / replacemsg。
    let system_like = kind == "系统" || mtype & 0xFFFF_FFFF == 10002;
    if text.contains("revokemsg")
        || (system_like && text.contains("撤回了一条消息"))
    {
        kind = "撤回".to_string();
        // 有些库把 sysmsg 存成转义后的文本（`&lt;newmsgid&gt;`），先还原再取标签。
        let xml = text.replace("&lt;", "<").replace("&gt;", ">");
        revoke_msgid = extract_tag(&xml, "newmsgid")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .filter(|v| *v > 0);
        text = extract_tag(&xml, "content")
            .or_else(|| extract_tag(&xml, "replacemsg"))
            .unwrap_or_else(|| "撤回了一条消息".to_string());
        text = strip_sender_prefix(&text);
    }
    if text.trim().is_empty() {
        if kind == "引用" {
            text = "[引用]".to_string();
        } else {
            text = fallback.to_string();
        }
    }
    (kind, text, link_url, quote, revoke_msgid)
}

/// 解析「引用」消息里的 `<refermsg>`：被引用正文、发送人、时间。
fn parse_quote(xml: &str) -> Option<QuoteRef> {
    let open = xml.find("<refermsg")?;
    let start = open + xml[open..].find('>')? + 1;
    let end = xml[start..].find("</refermsg>")? + start;
    let body = &xml[start..end];
    let msg_type = extract_tag(body, "type")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut content = extract_tag(body, "content").unwrap_or_default();
    // 群里的被引用内容常带 `wxid_xxx:` 前缀；图片 / 文件则是另一段 XML。
    content = strip_sender_prefix(&content);
    if content.trim_start().starts_with('<') {
        if let Some(title) = extract_title(&content) {
            content = title;
        }
    }
    // 图片引用没有可读正文，统一显示成「[图片]」，同时保留类型供定位使用。
    if msg_type == 3 {
        content = "[图片]".to_string();
    }
    let from = extract_tag(body, "displayname").unwrap_or_default();
    // 群聊里 fromusr 是群 username，chatusr 才是真正的发送人。
    let from_usr = extract_tag(body, "chatusr")
        .filter(|s| !s.trim().is_empty())
        .or_else(|| extract_tag(body, "fromusr"))
        .unwrap_or_default();
    let create_time = extract_tag(body, "createtime")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    if content.trim().is_empty() && from.is_empty() {
        None
    } else {
        Some(QuoteRef {
            content: content.trim().to_string(),
            from,
            from_usr,
            create_time,
            msg_type,
        })
    }
}

/// 群消息正文常带 `wxid_xxx:\n` 前缀（发送者账号），去掉它只留真正的内容。
fn strip_sender_prefix(s: &str) -> String {
    if let Some(pos) = s.find('\n') {
        let head = s[..pos].trim_end_matches('\r');
        if !head.is_empty()
            && head.ends_with(':')
            && head.len() < 80
            && !head.contains(' ')
        {
            let rest = s[pos + 1..].trim_start_matches(['\r', '\n']);
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    s.to_string()
}

fn kind_of(mtype: i64) -> (String, &'static str) {
    if mtype == 8_594_229_559_345 {
        return ("红包".to_string(), "[红包]");
    }
    let mut base = mtype;
    if base > 0xFFFF {
        let low = base & 0xFFFF_FFFF;
        base = if is_known_type(low) { low } else { low & 0xFF };
    }
    match base {
        1 => ("文字".to_string(), "[文本]"),
        3 => ("图片".to_string(), "[图片]"),
        34 => ("语音".to_string(), "[语音]"),
        43 => ("视频".to_string(), "[视频]"),
        47 | 11000 => ("表情".to_string(), "[表情]"),
        48 => ("位置".to_string(), "[位置]"),
        49 => ("链接".to_string(), "[链接]"),
        50 => ("通话".to_string(), "[通话]"),
        10000 => ("系统".to_string(), "[系统消息]"),
        _ => (format!("类型{base}"), "[消息]"),
    }
}

fn is_known_type(t: i64) -> bool {
    matches!(t, 1 | 3 | 34 | 43 | 47 | 48 | 49 | 50 | 10000 | 11000)
}

fn clean_text(s: &str) -> String {
    let s = s.split('\u{1}').next().unwrap_or("");
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\n' || c == '\r' || c == '\t' {
            out.push(c);
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out.trim().to_string()
}

fn looks_like_text(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .any(|c| c.is_alphanumeric() || c.is_ascii_punctuation() || (c as u32) > 0x2000)
}

fn decode_text(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    // zstd 帧（可能有 0..4 字节的前缀）
    for off in 0..=4usize {
        if let Some(rest) = bytes.get(off..) {
            if rest.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
                if let Some(s) = zstd_decode(rest) {
                    let c = clean_text(&s);
                    if looks_like_text(&c) {
                        return Some(c);
                    }
                }
            }
        }
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        let c = clean_text(s);
        if looks_like_text(&c) {
            return Some(c);
        }
    }
    // 微信容器头 + UTF-8 明文：优先从第 10 字节开始试
    let mut offsets: Vec<usize> = vec![10];
    offsets.extend(0..16);
    offsets.sort_unstable();
    offsets.dedup();
    for off in offsets {
        if off >= bytes.len() {
            continue;
        }
        let chunk = &bytes[off..];
        for split in [&b"\x01\x00"[..], &b"\x01"[..]] {
            let part = match find_subslice(chunk, split) {
                Some(i) => &chunk[..i],
                None => chunk,
            };
            if let Ok(s) = std::str::from_utf8(part) {
                let c = clean_text(s);
                if looks_like_text(&c) {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn zstd_decode(bytes: &[u8]) -> Option<String> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(bytes).ok()?;
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok()?;
    if out.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&out).to_string())
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn extract_title(xml: &str) -> Option<String> {
    extract_tag(xml, "title")
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let t = xml[start..end].trim();
    // 微信 XML 常把标题 / 链接包在 CDATA 里。
    let t = t
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(t)
        .trim();
    if t.is_empty() {
        None
    } else {
        Some(t.replace("&amp;", "&").replace("&#38;", "&"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_message, extract_tag, extract_title, parse_quote, revoke_notice_window,
        REVOKE_LOOKBACK_SECS, REVOKE_WINDOW_SECS,
    };

    #[test]
    fn extracts_cdata_title_and_url() {
        let xml = "<msg><appmsg><title><![CDATA[H5 链接]]></title>\
                   <url><![CDATA[https://example.com/a?x=1&amp;y=2]]></url>\
                   </appmsg></msg>";
        assert_eq!(extract_title(xml).as_deref(), Some("H5 链接"));
        assert_eq!(
            extract_tag(xml, "url").as_deref(),
            Some("https://example.com/a?x=1&y=2")
        );
    }

    #[test]
    fn quote_message_is_parsed_and_classified() {
        let xml = "<msg><appmsg><title>收到，跟一手</title><type>57</type>                   <refermsg><type>1</type><fromusr>wxid_a</fromusr>                   <displayname>张三</displayname><content>三中三 01,02,03 各10</content>                   <createtime>1700000000</createtime></refermsg></appmsg></msg>";
        let (kind, text, url, quote, _) = decode_message(49, xml.as_bytes(), &[]);
        assert_eq!(kind, "引用");
        assert_eq!(text, "收到，跟一手");
        assert!(url.is_none());
        let quote = quote.expect("quote");
        assert_eq!(quote.content, "三中三 01,02,03 各10");
        assert_eq!(quote.from, "张三");
        assert_eq!(quote.from_usr, "wxid_a");
        assert_eq!(quote.create_time, 1_700_000_000);
        assert_eq!(quote.msg_type, 1);
        assert!(parse_quote("<msg></msg>").is_none());
    }

    #[test]
    fn image_quote_uses_group_member_and_image_type() {
        let xml = "<msg><appmsg><title>退</title><type>57</type><refermsg>\
                   <content>wxid_member:\n&lt;?xml version=\"1.0\"?&gt;\
                   &lt;msg&gt;&lt;img aeskey=\"x\" /&gt;&lt;/msg&gt;</content>\
                   <createtime>1700000000</createtime>\
                   <displayname>锦鲤</displayname>\
                   <fromusr>123@chatroom</fromusr>\
                   <chatusr>wxid_member</chatusr>\
                   <type>3</type></refermsg></appmsg></msg>";
        let (kind, _, _, quote, _) = decode_message(49, xml.as_bytes(), &[]);
        assert_eq!(kind, "引用");
        let quote = quote.expect("quote");
        assert_eq!(quote.msg_type, 3);
        assert_eq!(quote.content, "[图片]");
        // 群聊里 fromusr 是群，chatusr 才是被引用消息的发送人。
        assert_eq!(quote.from_usr, "wxid_member");
        assert_eq!(quote.from, "锦鲤");
    }

    #[test]
    fn normal_link_message_keeps_url() {
        let xml = "<msg><appmsg><title>下注说明</title><type>5</type>                   <url>https://example.com/bet</url></appmsg></msg>";
        let (kind, text, url, quote, _) = decode_message(49, xml.as_bytes(), &[]);
        assert_eq!(kind, "链接");
        assert_eq!(text, "下注说明");
        assert_eq!(url.as_deref(), Some("https://example.com/bet"));
        assert!(quote.is_none());
    }

    #[test]
    fn missing_tags_are_none() {
        assert!(extract_title("<msg></msg>").is_none());
        assert!(extract_tag("<msg></msg>", "url").is_none());
    }

    #[test]
    fn revoke_system_message_is_classified() {
        let xml = "<?xml version=\"1.0\"?><sysmsg type=\"revokemsg\"><revokemsg>\
                   <content>\"一羊迁徙\" 撤回了一条消息</content>\
                   <newmsgid>7380000000000000123</newmsgid>\
                   <revoketime>0</revoketime></revokemsg></sysmsg>";
        let (kind, text, url, quote, msgid) = decode_message(10000, xml.as_bytes(), &[]);
        assert_eq!(kind, "撤回");
        assert_eq!(text, "\"一羊迁徙\" 撤回了一条消息");
        assert!(url.is_none());
        assert!(quote.is_none());
        assert_eq!(msgid, Some(7380000000000000123));

        // 10002 的纯文本撤回提示也能识别（没有 newmsgid）。
        let (kind, text, _, _, msgid) =
            decode_message(10002, "wxid_x:\n\"程程\" 撤回了一条消息".as_bytes(), &[]);
        assert_eq!(kind, "撤回");
        assert!(text.contains("撤回了一条消息"), "{text}");
        assert_eq!(msgid, None);
    }

    /// 全量导入时撤回提示的扫描窗口要覆盖到时段结束之后：时段末尾发出的
    /// 消息可能到点之后才被撤回，否则重新导入会把已撤回的原消息带回来。
    #[test]
    fn revoke_notice_window_covers_after_import_end() {
        let (from, to) = revoke_notice_window(None, 1_000, 2_000, 9_999);
        assert_eq!(from, 1_000, "窗口从导入开始时间起");
        assert_eq!(to, 2_000 + REVOKE_WINDOW_SECS, "窗口延伸到时段结束之后");
    }

    /// 增量监控时窗口从增量水位往前回看：微信会把撤回提示回填到原消息的
    /// 位置，提示的 sort 可能不前进，按 `sort_seq > last_sort` 取不到。
    #[test]
    fn revoke_notice_window_looks_back_from_incremental_watermark() {
        let since_ms = 1_700_000_000_000; // 水位 = 1_700_000_000 秒
        let now = 1_700_000_030;
        let (from, to) =
            revoke_notice_window(Some(since_ms), 1_600_000_000, 1_800_000_000, now);
        assert_eq!(to, now);
        assert_eq!(from, 1_700_000_000 - REVOKE_LOOKBACK_SECS);

        // 水位是不可预期的异常值时以当前时间兜底，不扫出离谱的窗口。
        let (from, to) = revoke_notice_window(Some(i64::MAX), 0, 1, now);
        assert_eq!(to, now);
        assert_eq!(from, now - REVOKE_LOOKBACK_SECS);
    }
}

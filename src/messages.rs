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
use crate::model::{ImportResult, MsgRow};
use crate::sqlite::Conn;

/// 导入与实时监控会同时对消息库做解密/查询；串行化避免临时文件互相覆盖。
static DECRYPT_LOCK: Mutex<()> = Mutex::new(());

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
    let mut seen: HashSet<(i64, String, String)> = HashSet::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut max_sort = since_sort.unwrap_or(req.start_ts.saturating_mul(1000));

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
                let mtype = r.int(0);
                let sender_id = r.int(1);
                let sender = name2id.get(&sender_id).cloned().unwrap_or_default();
                if let Some(f) = filter {
                    if !f.contains(&sender) {
                        continue;
                    }
                }
                let ts = r.int(2);
                let content_bytes = r.bytes(3);
                let compress = r.bytes(4);
                let (kind, content) = decode_message(mtype, &content_bytes, &compress);
                let sort_seq = r.int(5);
                let sort = if sort_seq > 0 {
                    sort_seq
                } else {
                    ts.saturating_mul(1000)
                };

                if !seen.insert((sort, sender.clone(), content.clone())) {
                    continue;
                }
                max_sort = max_sort.max(sort);

                let nick = if sender.is_empty() {
                    "系统".to_string()
                } else {
                    req.name_of
                        .get(&sender)
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .unwrap_or_else(|| sender.clone())
                };
                rows.push(MsgRow {
                    kind,
                    nick,
                    content: truncate_chars(&content, 10),
                    time: format_time(ts),
                    sort,
                    chat: chat.clone(),
                    sender: sender.clone(),
                });
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

            if Local::now().timestamp() > req.end_ts {
                // 结束前再做一次补拉，避免漏掉最后一个轮询间隔内的消息。
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

fn format_time(ts: i64) -> String {
    match Local.timestamp_opt(ts, 0).single() {
        Some(dt) => format!("{}:{:02}", dt.hour(), dt.minute()),
        None => String::new(),
    }
}

// ---------------- 正文解码 ----------------

fn decode_message(mtype: i64, content: &[u8], compress: &[u8]) -> (String, String) {
    let (kind, fallback) = kind_of(mtype);
    let mut text = match kind.as_str() {
        "文字" | "系统" | "链接" | "其他" | "红包" => {
            let mut t = decode_text(content);
            if t.is_none() && !compress.is_empty() {
                t = decode_text(compress);
            }
            t.unwrap_or_default()
        }
        _ => String::new(),
    };

    if kind == "链接" && !text.is_empty() {
        if let Some(title) = extract_title(&text) {
            text = title;
        }
    }

    if kind == "文字" || kind == "系统" {
        text = strip_sender_prefix(&text);
    }
    if text.trim().is_empty() {
        text = fallback.to_string();
    }
    (kind, text)
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

fn truncate_chars(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut it = cleaned.chars();
    let mut out: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        out.push('…');
    }
    out.trim().to_string()
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
    let start = xml.find("<title>")? + "<title>".len();
    let end = xml[start..].find("</title>")? + start;
    let t = xml[start..end].trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

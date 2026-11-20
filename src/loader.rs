//! 后台加载：定位目录 → 提取/校验密钥 → 解密 → 查询 → 准备头像。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::avatars;
use crate::cache::{self, CachedKey};
use crate::decrypt::{self, KeySpec};
use crate::keyextract::{self, Candidate};
use crate::locate;
use crate::messages::{self, ImportRequest};
use crate::model::{AppData, Group, Person};
use crate::sqlite::Conn;

const REL_CONTACT: &str = "contact\\contact.db";
const REL_HEAD: &str = "head_image\\head_image.db";

pub enum Msg {
    Status(String),
    Done(Box<AppData>),
    Failed(String),
}

/// 命令行自检：跑完整个数据链路并返回摘要（不启动界面）。
pub fn selftest() -> Result<String, String> {
    let ctx = egui::Context::default();
    let (tx, _rx) = std::sync::mpsc::channel();
    let d = load(&ctx, &tx)?;
    let member_count: usize = d.groups.iter().map(|g| g.members.len()).sum();
    let mut out = String::new();
    out.push_str(&format!(
        "账号: {} ({})\n数据目录: {}\n密钥来源: {}\n联系人: {}\n群聊: {} (成员合计 {})\n头像: 本地 {} / 补拉 {} / 缺失 {}\n耗时: {} ms\n",
        d.self_name,
        d.self_wxid,
        d.data_dir,
        d.key_source,
        d.contacts.len(),
        d.groups.len(),
        member_count,
        d.cached_avatars,
        d.downloaded_avatars,
        d.missing_avatars,
        d.elapsed_ms
    ));
    out.push_str(&format!(
        "数据库密钥: {} 个（message 目录内 {} 个）\n",
        d.keys.len(),
        d.keys
            .keys()
            .filter(|k| k.starts_with("message\\"))
            .count()
    ));
    out.push_str("\n前 8 位联系人:\n");
    for p in d.contacts.iter().take(8) {
        out.push_str(&format!(
            "  {} | {} | avatar={}\n",
            p.display_name,
            p.username,
            p.avatar.as_ref().map(|a| format!("{}x{}", a.width, a.height)).unwrap_or_else(|| "无".into())
        ));
    }
    out.push_str("\n前 5 个群聊:\n");
    for g in d.groups.iter().take(5) {
        out.push_str(&format!(
            "  {} ({} 人, 头像={})\n",
            g.name,
            g.members.len(),
            g.avatar.is_some()
        ));
        for m in g.members.iter().take(3) {
            out.push_str(&format!(
                "      - {}{}\n",
                m.display_name,
                if m.is_owner { " [群主]" } else { "" }
            ));
        }
    }
    Ok(out)
}

/// 删除临时目录里的解密明文库（退出/重载时调用）。
pub fn cleanup_temp() {
    let dir = std::env::temp_dir().join("WechatViewer").join("work");
    let _ = fs::remove_dir_all(dir);
}

/// 命令行消息自检：读取指定会话在某个时段的消息并返回前 40 行。
pub fn import_selftest(
    user: &str,
    date: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    since_sort: Option<i64>,
) -> Result<String, String> {
    let ctx = egui::Context::default();
    let (tx, _rx) = std::sync::mpsc::channel();
    let data = load(&ctx, &tx)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let date = date.filter(|s| !s.is_empty()).unwrap_or(&today);
    let from = from.filter(|s| !s.is_empty()).unwrap_or("00:00");
    let to = to.filter(|s| !s.is_empty()).unwrap_or("23:59");
    let (start_ts, end_ts) = messages::parse_range(date, from, to)?;

    let mut name_of: HashMap<String, String> = HashMap::new();
    for p in &data.contacts {
        name_of.insert(p.username.clone(), p.display_name.clone());
    }
    for g in &data.groups {
        name_of
            .entry(g.username.clone())
            .or_insert_with(|| g.name.clone());
        for m in &g.members {
            name_of.insert(m.username.clone(), m.display_name.clone());
        }
    }
    name_of.insert(data.self_wxid.clone(), data.self_name.clone());

    let is_group = user.ends_with("@chatroom");
    let req = ImportRequest {
        account_dir: data.account_path.clone(),
        workdir: data.workdir.clone(),
        keys: data.keys.clone(),
        contacts: if is_group { Vec::new() } else { vec![user.to_string()] },
        groups: if is_group {
            vec![(user.to_string(), Vec::new())]
        } else {
            Vec::new()
        },
        name_of,
        start_ts,
        end_ts,
    };

    let (rows, summary) = if let Some(s) = since_sort {
        let rows = messages::poll_new(&req, s)?;
        let n = rows.len();
        (rows, format!("增量读取（sort_seq > {s}）：{n} 条"))
    } else {
        let res = messages::run(&req, &|_| {})?;
        (res.rows, res.summary)
    };
    let mut kinds: HashMap<String, usize> = HashMap::new();
    for r in &rows {
        *kinds.entry(r.kind.clone()).or_default() += 1;
    }
    let mut out = format!(
        "{}\n会话: {}\n时段: {} {} - {}\n类型分布: {:?}\n\n前 40 行:\n",
        summary, user, date, from, to, kinds
    );
    for r in rows.iter().take(40) {
        out.push_str(&format!(
            "{} | {} | {} | {}\n",
            r.kind, r.nick, r.content, r.time
        ));
    }
    Ok(out)
}

pub fn start(ctx: egui::Context, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        match load(&ctx, &tx) {
            Ok(data) => {
                let _ = tx.send(Msg::Done(Box::new(data)));
            }
            Err(e) => {
                let _ = tx.send(Msg::Failed(e));
            }
        }
        ctx.request_repaint();
    });
}

struct Reporter {
    tx: Sender<Msg>,
    ctx: egui::Context,
    start: Instant,
    verbose: bool,
}

impl Reporter {
    fn status(&self, s: impl Into<String>) {
        let s = s.into();
        if self.verbose {
            eprintln!("[wv][{:>6.2}s] {}", self.start.elapsed().as_secs_f32(), s);
        }
        let _ = self.tx.send(Msg::Status(s));
        self.ctx.request_repaint();
    }
}

fn read_page1(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; decrypt::PAGE_SZ];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn find_key(cands: &[Candidate], page1: &[u8]) -> Option<KeySpec> {
    // 标准库：salt 取文件头，优先（否则会把普通库误判成「明文头」库）
    for c in cands {
        if decrypt::verify_key(&c.key, page1, None) {
            return Some(KeySpec { key: c.key, salt: None });
        }
    }
    for c in cands {
        if let Some(salt) = c.salt {
            if decrypt::verify_key(&c.key, page1, Some(salt)) {
                return Some(KeySpec { key: c.key, salt: Some(salt) });
            }
        }
    }
    None
}

fn cached_to_candidates(cached: &[CachedKey]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for c in cached {
        if c.key.len() == 32 {
            if let Ok(key) = c.key[..32].try_into() {
                out.push(Candidate { key, salt: None });
            }
        } else if c.key.len() == 48 {
            if let (Ok(key), Ok(salt)) = (c.key[..32].try_into(), c.key[32..].try_into()) {
                out.push(Candidate { key, salt: Some(salt) });
            }
        }
    }
    out
}

fn db_mtime(account: &Path) -> u64 {
    fs::metadata(account.join("db_storage").join(REL_CONTACT))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 递归列出 db_storage 下所有 .db（rel 使用反斜杠，如 `message\message_0.db`）。
fn collect_db_files(account: &Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>, depth: usize) {
        if depth > 3 {
            return;
        }
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if e.file_name().to_string_lossy().eq_ignore_ascii_case("migrate") {
                    continue;
                }
                walk(base, &p, out, depth + 1);
            } else {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".db") {
                    if let Ok(rel) = p.strip_prefix(base) {
                        out.push((rel.to_string_lossy().to_string(), p.clone()));
                    }
                }
            }
        }
    }
    let base = account.join("db_storage");
    let mut out = Vec::new();
    walk(&base, &base, &mut out, 0);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn decrypt_checked(
    src: &Path,
    dst: &Path,
    spec: &KeySpec,
    rep: &Reporter,
) -> Result<Conn, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        decrypt::decrypt_with_wal(src, dst, spec).map_err(|e| format!("解密失败: {e}"))?;
        match Conn::open_ro(dst) {
            Ok(conn) => match conn.query("PRAGMA quick_check") {
                Ok(rows) if rows.first().map(|r| r.text(0)) == Some("ok".to_string()) => {
                    return Ok(conn)
                }
                Ok(_) => last = "完整性校验未通过".to_string(),
                Err(e) => last = e,
            },
            Err(e) => last = e,
        }
        if attempt < 2 {
            rep.status("微信正在整理数据，正在重试…");
            std::thread::sleep(Duration::from_millis(400));
        }
    }
    Err(format!("数据库校验失败（微信可能正在写入），请稍后重试：{last}"))
}

const SYSTEM_CONTACTS: &[&str] = &[
    "filehelper", "weixin", "notifymessage", "fmessage", "medianote",
    "floatbottle", "newsapp", "brandsessionholder", "officialaccounts",
    "qqmail", "tmessage", "qmessage", "qqsync", "voipapp", "voicevoipapp",
    "voiceinputapp", "readerapp", "weibo", "facebookapp", "twitterapp",
    "linkedinapp", "qy_notify", "exmail_tips",
];

#[derive(Clone)]
struct ContactRow {
    id: i64,
    username: String,
    nick: String,
    remark: String,
    alias: String,
    url: Option<String>,
}

impl ContactRow {
    fn display(&self) -> String {
        if !self.remark.is_empty() {
            self.remark.clone()
        } else if !self.nick.is_empty() {
            self.nick.clone()
        } else {
            self.username.clone()
        }
    }
}

fn load(ctx: &egui::Context, tx: &Sender<Msg>) -> Result<AppData, String> {
    let started = Instant::now();
    let rep = Reporter {
        tx: tx.clone(),
        ctx: ctx.clone(),
        start: started,
        verbose: std::env::var_os("WV_DEBUG").is_some(),
    };

    // 清掉上次可能残留的解密明文库，避免隐私数据长期留在磁盘上。
    cleanup_temp();

    rep.status("正在定位微信数据目录…");
    let data_root = locate::find_account_root().ok_or_else(|| {
        "未找到微信 4.x 的数据目录（xwechat_files）。\n\
         请确认已安装并登录过微信 4.x 桌面版。"
            .to_string()
    })?;
    let accounts = locate::account_dirs(&data_root);
    if accounts.is_empty() {
        return Err(format!(
            "在 {} 下没有找到含 db_storage 的账号目录。",
            data_root.display()
        ));
    }

    rep.status("正在从微信进程读取数据库密钥…");
    let t_scan = Instant::now();
    let (live, logs) = keyextract::extract_all();
    let cached = cache::load();
    let live_set: HashSet<Candidate> = live.iter().copied().collect();
    rep.status(format!("内存扫描完成：{} 个候选密钥，用时 {:.2}s", live.len(), t_scan.elapsed().as_secs_f32()));

    let mut cands: Vec<Candidate> = live;
    cands.extend(cached_to_candidates(&cached));
    cands.sort_by_key(|c| (c.key, c.salt));
    cands.dedup_by(|a, b| a.key == b.key && a.salt == b.salt);

    if cands.is_empty() {
        return Err(format!(
            "无法获得微信数据库密钥。\n\n{}\n\n\
             密钥只存在于已登录的微信进程内存中，请：\n\
             1. 确认微信已登录并保持运行；\n\
             2. 若微信以管理员身份运行，本程序也需以管理员身份运行。",
            logs.join("\n")
        ));
    }

    rep.status("正在校验密钥并选择账号…");
    // 每个账号都试一遍，能解开最多库的账号即为当前登录账号。
    let mut best: Option<(PathBuf, HashMap<String, KeySpec>, usize, u64)> = None;
    for acct in &accounts {
        // 一次把该账号所有库（含消息分片）的密钥都校验出来，后面导入注单直接用。
        let mut map: HashMap<String, KeySpec> = HashMap::new();
        for (rel, path) in collect_db_files(acct) {
            if let Some(page1) = read_page1(&path) {
                if let Some(k) = find_key(&cands, &page1) {
                    map.insert(rel, k);
                }
            }
        }
        let score = map.len();
        if score == 0 {
            continue;
        }
        let mtime = db_mtime(acct);
        let better = match &best {
            None => true,
            Some((_, _, bs, bm)) => score > *bs || (score == *bs && mtime > *bm),
        };
        if better {
            best = Some((acct.clone(), map, score, mtime));
        }
    }

    let (account, keys, _, _) = best.ok_or_else(|| {
        format!(
            "找到了密钥，但没有任何账号目录能通过校验（可能微信刚重新登录，密钥已更换）。\n\
             请保持微信登录后点击「重新读取」。\n\n{}",
            logs.join("\n")
        )
    })?;

    let contact_spec = keys.get(REL_CONTACT).copied().ok_or_else(|| {
        "contact.db 没有可用密钥，无法读取联系人与群聊。\n\
         请确认微信已登录且保持运行后重试。"
            .to_string()
    })?;

    // 记录密钥来源，用于界面提示
    let key_source = if live_set.iter().any(|c| c.key == contact_spec.key) {
        "微信进程内存".to_string()
    } else {
        "本地密钥缓存".to_string()
    };

    // 更新缓存（保留其它账号的条目）
    {
        let account_name = account
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut entries: Vec<CachedKey> = cached
            .into_iter()
            .filter(|c| c.account != account_name)
            .collect();
        for (rel, spec) in &keys {
            let mut key = spec.key.to_vec();
            if let Some(salt) = spec.salt {
                key.extend_from_slice(&salt);
            }
            entries.push(CachedKey {
                account: account_name.clone(),
                rel: rel.clone(),
                key,
            });
        }
        cache::save(&entries);
    }

    let account_name = account
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let wxid = locate::wxid_from_account_dir(&account);
    let workdir = std::env::temp_dir()
        .join("WechatViewer")
        .join("work")
        .join(&account_name);
    fs::create_dir_all(&workdir).map_err(|e| format!("无法创建临时目录: {e}"))?;

    rep.status("正在解密联系人数据库…");
    let contact_dst = workdir.join("contact.db");
    let conn = decrypt_checked(
        &account.join("db_storage").join(REL_CONTACT),
        &contact_dst,
        &contact_spec,
        &rep,
    )?;

    rep.status("正在读取联系人与群聊…");
    let rows = conn
        .query("SELECT id, username, nick_name, remark, alias, big_head_url FROM contact ORDER BY id")
        .map_err(|e| format!("查询联系人失败: {e}"))?;

    let all_rows: Vec<ContactRow> = rows
        .iter()
        .map(|r| ContactRow {
            id: r.int(0),
            username: r.text(1),
            nick: r.text(2),
            remark: r.text(3),
            alias: r.text(4),
            url: {
                let u = r.text(5);
                if u.is_empty() { None } else { Some(u) }
            },
        })
        .collect();

    let mut by_id: HashMap<i64, usize> = HashMap::new();
    let mut url_by_name: HashMap<String, String> = HashMap::new();
    for (i, r) in all_rows.iter().enumerate() {
        by_id.insert(r.id, i);
        if let Some(u) = &r.url {
            url_by_name.entry(r.username.clone()).or_insert_with(|| u.clone());
        }
    }

    let mut contacts: Vec<Person> = Vec::new();
    for r in &all_rows {
        if r.username.is_empty() || r.username.ends_with("@chatroom") {
            continue;
        }
        if r.username.starts_with("gh_") {
            continue;
        }
        let lower = r.username.to_ascii_lowercase();
        if SYSTEM_CONTACTS.contains(&lower.as_str()) {
            continue;
        }
        contacts.push(Person {
            username: r.username.clone(),
            nick_name: r.nick.clone(),
            remark: r.remark.clone(),
            alias: r.alias.clone(),
            display_name: r.display(),
            is_owner: false,
            is_self: r.username == wxid,
            avatar: None,
        });
    }
    contacts.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));

    // ---- 群聊 ----
    let rooms = conn
        .query("SELECT id, username, owner FROM chat_room")
        .unwrap_or_default();
    let mut room_meta: HashMap<String, (i64, String)> = HashMap::new();
    for r in &rooms {
        room_meta.insert(r.text(1), (r.int(0), r.text(2)));
    }
    let mut members_by_room: HashMap<i64, Vec<i64>> = HashMap::new();
    if let Ok(mrows) = conn.query("SELECT room_id, member_id FROM chatroom_member") {
        for r in &mrows {
            members_by_room.entry(r.int(0)).or_default().push(r.int(1));
        }
    }

    let mut group_names: Vec<(String, ContactRow)> = Vec::new();
    let mut seen_rooms: HashSet<String> = HashSet::new();
    for r in &all_rows {
        if r.username.ends_with("@chatroom") && !r.username.is_empty() {
            seen_rooms.insert(r.username.clone());
            group_names.push((r.username.clone(), r.clone()));
        }
    }
    for (username, _) in room_meta.iter() {
        if !seen_rooms.contains(username) {
            if let Some(i) = all_rows.iter().position(|r| &r.username == username) {
                group_names.push((username.clone(), all_rows[i].clone()));
            } else {
                group_names.push((
                    username.clone(),
                    ContactRow {
                        id: 0,
                        username: username.clone(),
                        nick: String::new(),
                        remark: String::new(),
                        alias: String::new(),
                        url: None,
                    },
                ));
            }
        }
    }

    let mut groups: Vec<Group> = Vec::new();
    for (username, row) in group_names {
        let (room_id, owner) = room_meta
            .get(&username)
            .cloned()
            .map(|(id, o)| (Some(id), o))
            .unwrap_or((None, String::new()));
        let mut members: Vec<Person> = Vec::new();
        if let Some(rid) = room_id {
            if let Some(ids) = members_by_room.get(&rid) {
                for mid in ids {
                    let Some(&idx) = by_id.get(mid) else { continue };
                    let c = &all_rows[idx];
                    members.push(Person {
                        username: c.username.clone(),
                        nick_name: c.nick.clone(),
                        remark: c.remark.clone(),
                        alias: c.alias.clone(),
                        display_name: c.display(),
                        is_owner: c.username == owner,
                        is_self: c.username == wxid,
                        avatar: None,
                    });
                }
            }
        }
        members.sort_by(|a, b| {
            b.is_owner
                .cmp(&a.is_owner)
                .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
        });
        // 微信里没有自定义群名的群，界面会显示成员昵称组合；这里保持一致。
        let raw_name = row.display();
        let name = if raw_name == username {
            let names: Vec<String> = members
                .iter()
                .take(3)
                .map(|m| m.display_name.clone())
                .collect();
            if names.is_empty() {
                "群聊".to_string()
            } else {
                names.join("、")
            }
        } else {
            raw_name
        };
        groups.push(Group {
            username: username.clone(),
            name,
            owner,
            members,
            has_member_list: room_id.is_some(),
            avatar: None,
        });
    }
    groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // ---- 头像 ----
    rep.status("正在读取头像…");
    let mut avatar_bytes: HashMap<String, Vec<u8>> = HashMap::new();
    let mut cached_avatar_count = 0usize;
    if let Some(spec) = keys.get(REL_HEAD).copied() {
        let head_dst = workdir.join("head_image.db");
        if let Ok(hconn) = decrypt_checked(
            &account.join("db_storage").join(REL_HEAD),
            &head_dst,
            &spec,
            &rep,
        ) {
            if let Ok(hrows) = hconn.query("SELECT username, image_buffer FROM head_image") {
                for r in &hrows {
                    if let Some(blob) = r.blob(1) {
                        if !blob.is_empty() {
                            avatar_bytes.insert(r.text(0), blob.to_vec());
                        }
                    }
                }
                cached_avatar_count = avatar_bytes.len();
            }
        }
    }

    // 缺失头像用微信 CDN 补拉（最多 80 个，4 线程）
    let mut missing_urls: Vec<(String, String)> = Vec::new();
    let mut seen_missing: HashSet<String> = HashSet::new();
    {
        let mut push = |name: &str| {
            if !avatar_bytes.contains_key(name) && seen_missing.insert(name.to_string()) {
                if let Some(u) = url_by_name.get(name) {
                    missing_urls.push((name.to_string(), u.clone()));
                }
            }
        };
        for p in &contacts {
            push(&p.username);
        }
        for g in &groups {
            push(&g.username);
            for m in &g.members {
                push(&m.username);
            }
        }
    }
    missing_urls.truncate(80);
    let downloaded = if missing_urls.is_empty() {
        Vec::new()
    } else {
        rep.status(format!("正在补全 {} 个头像…", missing_urls.len()));
        avatars::download_many(&missing_urls, 4, 4000)
    };
    for (name, bytes) in &downloaded {
        avatar_bytes.entry(name.clone()).or_insert_with(|| bytes.clone());
    }

    let mut decoded_cache: HashMap<String, Option<crate::model::Avatar>> = HashMap::new();
    let mut decode = |name: &str| -> Option<crate::model::Avatar> {
        if let Some(v) = decoded_cache.get(name) {
            return v.clone();
        }
        let v = avatar_bytes.get(name).and_then(|b| avatars::decode(b));
        decoded_cache.insert(name.to_string(), v.clone());
        v
    };

    for p in contacts.iter_mut() {
        p.avatar = decode(&p.username);
    }
    for g in groups.iter_mut() {
        g.avatar = decode(&g.username);
        for m in g.members.iter_mut() {
            m.avatar = decode(&m.username);
        }
    }

    let mut missing_avatars = 0usize;
    for p in &contacts {
        if p.avatar.is_none() {
            missing_avatars += 1;
        }
    }
    for g in &groups {
        if g.avatar.is_none() {
            missing_avatars += 1;
        }
        for m in &g.members {
            if m.avatar.is_none() {
                missing_avatars += 1;
            }
        }
    }

    let self_row = all_rows.iter().find(|r| r.username == wxid);
    let self_name = self_row
        .map(|r| r.display())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| wxid.clone());

    rep.status("整理完成");
    Ok(AppData {
        self_wxid: wxid,
        self_name,
        account_dir: account_name,
        account_path: account.clone(),
        workdir: workdir.clone(),
        keys,
        data_dir: data_root.display().to_string(),
        contacts,
        groups,
        cached_avatars: cached_avatar_count,
        downloaded_avatars: downloaded.len(),
        missing_avatars,
        elapsed_ms: started.elapsed().as_millis(),
        key_source,
    })
}

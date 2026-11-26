//! 已撤回消息的本地记录。
//!
//! 收到撤回提示时，尽量找出被撤回的原消息（正文 / 会话 / 发送人 / 时间），
//! 记到 `%LOCALAPPDATA%\WechatViewer\revoked.txt`；重新导入时据此把原消息
//! 过滤掉，不回注单列表。不会修改微信数据库。
//!
//! 文本格式（制表符分隔，一行一条；内容里的特殊字符已转义）：
//!
//! ```text
//! wxv-revoked	1
//! revoked	<sort>	<chat>	<sender>	<time>	<nick>	<kind>	<content>
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

/// (sort, 会话 username, 发送者 username)
pub type RevokeKey = (i64, String, String);

#[derive(Clone, Debug, PartialEq)]
pub struct RevokedRow {
    pub sort: i64,
    pub chat: String,
    pub sender: String,
    /// 显示时间，如 `21:03`
    pub time: String,
    /// 发送人显示名
    pub nick: String,
    pub kind: String,
    /// 被撤回消息的原文
    pub content: String,
}

impl RevokedRow {
    pub fn key(&self) -> RevokeKey {
        (self.sort, self.chat.clone(), self.sender.clone())
    }
}

const MAGIC: &str = "wxv-revoked\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("revoked.txt")
}

pub fn load() -> HashMap<RevokeKey, RevokedRow> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return HashMap::new();
    };
    parse(&text)
}

pub fn save(rows: &HashMap<RevokeKey, RevokedRow>) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(rows)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(rows: &HashMap<RevokeKey, RevokedRow>) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    let mut keys: Vec<&RevokeKey> = rows.keys().collect();
    keys.sort();
    for key in keys {
        let r = &rows[key];
        out.push_str("revoked\t");
        out.push_str(&r.sort.to_string());
        out.push('\t');
        for field in [&r.chat, &r.sender, &r.time, &r.nick, &r.kind, &r.content] {
            out.push_str(&crate::edits::escape(field));
            out.push('\t');
        }
        // 最后一个字段后不补制表符，避免行尾多一列。
        out.pop();
        out.push('\n');
    }
    out
}

pub fn parse(text: &str) -> HashMap<RevokeKey, RevokedRow> {
    if !text.starts_with(MAGIC) {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(8, '\t');
        if parts.next() != Some("revoked") {
            continue;
        }
        let Some(sort) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        let (Some(chat), Some(sender), Some(time), Some(nick), Some(kind), Some(content)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let row = RevokedRow {
            sort,
            chat: crate::edits::unescape(chat),
            sender: crate::edits::unescape(sender),
            time: crate::edits::unescape(time),
            nick: crate::edits::unescape(nick),
            kind: crate::edits::unescape(kind),
            content: crate::edits::unescape(content),
        };
        out.insert(row.key(), row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_revoked_rows() {
        let mut map = HashMap::new();
        map.insert(
            (123, "1@chatroom".to_string(), "wxid_a".to_string()),
            RevokedRow {
                sort: 123,
                chat: "1@chatroom".to_string(),
                sender: "wxid_a".to_string(),
                time: "21:03".to_string(),
                nick: "张三".to_string(),
                kind: "文字".to_string(),
                content: "三中三 01,02,03 各10\n二中二 04,05 各5".to_string(),
            },
        );
        assert_eq!(parse(&serialize(&map)), map);
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("revoked\t1\ta\tb\tc\td\te\tf\n").is_empty());
    }

    #[test]
    fn malformed_lines_are_ignored() {
        let text = "wxv-revoked\t1\nrevoked\tbad\ta\tb\n";
        assert!(parse(text).is_empty());
    }
}

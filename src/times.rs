//! 注单时间的本地编辑保存。
//!
//! 详情面板里改过的时间按 `(sort, 会话, 发送者)` 存到
//! `%LOCALAPPDATA%\WechatViewer\times.txt`；重新导入或重启后，同一行消息
//! 会优先显示保存过的时间。不会修改微信数据库。
//!
//! 文本格式（制表符分隔，一行一条；内容里的特殊字符已转义）：
//!
//! ```text
//! wxv-times	1
//! time	<sort>	<chat>	<sender>	<time>
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::edits::EditKey;

const MAGIC: &str = "wxv-times\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("times.txt")
}

pub fn load() -> HashMap<EditKey, String> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return HashMap::new();
    };
    parse(&text)
}

pub fn save(times: &HashMap<EditKey, String>) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(times)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(times: &HashMap<EditKey, String>) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    let mut keys: Vec<&EditKey> = times.keys().collect();
    keys.sort();
    for key in keys {
        let (sort, chat, sender) = key;
        out.push_str("time\t");
        out.push_str(&sort.to_string());
        out.push('\t');
        out.push_str(&escape(chat));
        out.push('\t');
        out.push_str(&escape(sender));
        out.push('\t');
        out.push_str(&escape(&times[key]));
        out.push('\n');
    }
    out
}

pub fn parse(text: &str) -> HashMap<EditKey, String> {
    if !text.starts_with(MAGIC) {
        return HashMap::new();
    }
    let mut out = HashMap::new();
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(5, '\t');
        if parts.next() != Some("time") {
            continue;
        }
        let Some(sort) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        let (Some(chat), Some(sender), Some(time)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        out.insert((sort, unescape(chat), unescape(sender)), unescape(time));
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_edits() {
        let mut map = HashMap::new();
        map.insert(
            (123, "1@chatroom".to_string(), "wxid_a".to_string()),
            "12:34".to_string(),
        );
        let loaded = parse(&serialize(&map));
        assert_eq!(loaded, map);
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("time\t1\ta\tb\tc\n").is_empty());
    }
}

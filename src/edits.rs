//! 注单原文的本地编辑保存。
//!
//! 详情面板里改过的原文按 `(sort, 会话, 发送者)` 存到
//! `%LOCALAPPDATA%\WechatViewer\edits.txt`；重新导入或重启后，同一行消息
//! 会优先显示保存过的文本。不会修改微信数据库。
//!
//! 文本格式（制表符分隔，一行一条；内容里的特殊字符已转义）：
//!
//! ```text
//! wxv-edits	1
//! edit	<sort>	<chat>	<sender>	<text>
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

/// (sort, 会话 username, 发送者 username)
pub type EditKey = (i64, String, String);

const MAGIC: &str = "wxv-edits\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("edits.txt")
}

pub fn load() -> HashMap<EditKey, String> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return HashMap::new();
    };
    parse(&text)
}

pub fn save(edits: &HashMap<EditKey, String>) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(edits)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(edits: &HashMap<EditKey, String>) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    let mut keys: Vec<&EditKey> = edits.keys().collect();
    keys.sort();
    for key in keys {
        let (sort, chat, sender) = key;
        out.push_str("edit\t");
        out.push_str(&sort.to_string());
        out.push('\t');
        out.push_str(&escape(chat));
        out.push('\t');
        out.push_str(&escape(sender));
        out.push('\t');
        out.push_str(&escape(&edits[key]));
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
        if parts.next() != Some("edit") {
            continue;
        }
        let Some(sort) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        let (Some(chat), Some(sender), Some(content)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        out.insert(
            (sort, unescape(chat), unescape(sender)),
            unescape(content),
        );
    }
    out
}

pub(crate) fn escape(s: &str) -> String {
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

pub(crate) fn unescape(s: &str) -> String {
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
    fn roundtrip_keeps_multiline_content() {
        let mut map = HashMap::new();
        map.insert(
            (123, "1@chatroom".to_string(), "wxid_a".to_string()),
            "特码 1,2,3各10\n二中二 1,2,3 5".to_string(),
        );
        let loaded = parse(&serialize(&map));
        assert_eq!(loaded, map);
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("edit\t1\ta\tb\tc\n").is_empty());
    }

    #[test]
    fn malformed_lines_are_ignored() {
        let text = "wxv-edits\t1\nedit\tbad\ta\tb\tc\nedit\t1\ta\tb\n";
        assert!(parse(text).is_empty());
    }
}

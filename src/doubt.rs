//! 「取消存疑」标记。
//!
//! 详情面板里手动确认过的注单，按 `(sort, 会话, 发送者)` 存到
//! `%LOCALAPPDATA%\WechatViewer\doubt.txt`；重新导入或重启后仍保持「不存疑」。
//! 只影响本程序的显示，不会修改微信数据。

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::edits::{escape, unescape, EditKey};

const MAGIC: &str = "wxv-doubt\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("doubt.txt")
}

pub fn load() -> HashSet<EditKey> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return HashSet::new();
    };
    parse(&text)
}

pub fn save(keys: &HashSet<EditKey>) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(keys)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

fn serialize(keys: &HashSet<EditKey>) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    let mut keys: Vec<&EditKey> = keys.iter().collect();
    keys.sort();
    for (sort, chat, sender) in keys {
        out.push_str("clear\t");
        out.push_str(&sort.to_string());
        out.push('\t');
        out.push_str(&escape(chat));
        out.push('\t');
        out.push_str(&escape(sender));
        out.push('\n');
    }
    out
}

fn parse(text: &str) -> HashSet<EditKey> {
    if !text.starts_with(MAGIC) {
        return HashSet::new();
    }
    let mut out = HashSet::new();
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(4, '\t');
        if parts.next() != Some("clear") {
            continue;
        }
        let Some(sort) = parts.next().and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        let (Some(chat), Some(sender)) = (parts.next(), parts.next()) else {
            continue;
        };
        out.insert((sort, unescape(chat), unescape(sender)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_cleared_keys() {
        let mut set = HashSet::new();
        set.insert((123, "1@chatroom".to_string(), "wxid_a".to_string()));
        assert_eq!(parse(&serialize(&set)), set);
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("clear\t1\ta\tb\n").is_empty());
    }
}

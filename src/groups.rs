//! 会员分组持久化：把「联系人 / 群成员」的 username 组合存到本地。
//!
//! 存储位置与密钥缓存一致：`%LOCALAPPDATA%\WechatViewer\groups.txt`。
//! 文本格式（制表符分隔，一行一条）：
//!
//! ```text
//! wxv-groups	1
//! group	A 组
//! contact	wxid_xxx
//! member	123456@chatroom	wxid_yyy
//! ```
//!
//! 读取时忽略未知行与损坏行（旧版本写过的 `chat` 整群行也会被忽略）；
//! 写入先落临时文件再替换，避免中断留下半个文件。

use std::env;
use std::fs;
use std::path::PathBuf;

use crate::model::MemberGroup;

const MAGIC: &str = "wxv-groups\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("groups.txt")
}

pub fn load() -> Vec<MemberGroup> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    parse(&text)
}

pub fn save(groups: &[MemberGroup]) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(groups)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(groups: &[MemberGroup]) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for g in groups {
        out.push_str("group\t");
        out.push_str(&g.name);
        out.push('\n');
        let mut contacts: Vec<&String> = g.contacts.iter().collect();
        contacts.sort();
        for u in contacts {
            out.push_str("contact\t");
            out.push_str(u);
            out.push('\n');
        }
        let mut members: Vec<&(String, String)> = g.members.iter().collect();
        members.sort();
        for (chat, member) in members {
            out.push_str("member\t");
            out.push_str(chat);
            out.push('\t');
            out.push_str(member);
            out.push('\n');
        }
    }
    out
}

pub fn parse(text: &str) -> Vec<MemberGroup> {
    if !text.starts_with(MAGIC) {
        return Vec::new();
    }
    let mut out: Vec<MemberGroup> = Vec::new();
    let mut current: Option<usize> = None;
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next() {
            Some("group") => {
                let Some(name) = parts.next().map(str::trim) else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                out.push(MemberGroup {
                    name: name.to_string(),
                    ..Default::default()
                });
                current = Some(out.len() - 1);
            }
            Some("contact") => {
                if let (Some(i), Some(u)) = (current, parts.next()) {
                    if !u.is_empty() {
                        out[i].contacts.insert(u.to_string());
                    }
                }
            }
            Some("member") => {
                if let (Some(i), Some(chat), Some(member)) = (current, parts.next(), parts.next()) {
                    if !chat.is_empty() && !member.is_empty() {
                        out[i].members.insert((chat.to_string(), member.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{parse, serialize};
    use crate::model::{MemberGroup, MsgRow};
    use std::collections::HashSet;

    fn row(chat: &str, sender: &str) -> MsgRow {
        MsgRow {
            kind: "文字".into(),
            nick: sender.into(),
            content: "x".into(),
            time: "20:00".into(),
            sort: 1,
            chat: chat.into(),
            sender: sender.into(),
            link_url: None,
            quote: None,
        }
    }

    #[test]
    fn roundtrip_keeps_all_members() {
        let mut g = MemberGroup {
            name: "A 组".into(),
            ..Default::default()
        };
        g.contacts.insert("wxid_a".into());
        g.members.insert(("1@chatroom".into(), "wxid_b".into()));

        let loaded = parse(&serialize(&[g]));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "A 组");
        assert!(loaded[0].contacts.contains("wxid_a"));
        assert!(loaded[0].members.contains(&("1@chatroom".into(), "wxid_b".into())));
    }

    #[test]
    fn parse_ignores_unknown_and_malformed_lines() {
        // `chat` 是旧版本写过的整群行，现在没有群聊维度，应被忽略。
        let text =
            "wxv-groups\t1\nbogus\tline\ncontact\twxid_orphan\ngroup\tB\nchat\t9@chatroom\nmember\tonlyone\n";
        let groups = parse(text);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "B");
        assert!(groups[0].members.is_empty());
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("group\tA\n").is_empty());
    }

    #[test]
    fn matches_contact_and_member_only() {
        let mut g = MemberGroup {
            name: "A".into(),
            ..Default::default()
        };
        g.contacts.insert("wxid_c".into());
        g.members.insert(("2@chatroom".into(), "wxid_m".into()));

        assert!(g.matches(&row("wxid_c", "wxid_c")));
        assert!(g.matches(&row("2@chatroom", "wxid_m")));
        // 没有整群维度：同群其他成员的消息不属于本组。
        assert!(!g.matches(&row("2@chatroom", "wxid_other")));
        assert!(!g.matches(&row("3@chatroom", "wxid_m")));
    }

    #[test]
    fn replace_selection_overwrites_and_dedupes() {
        let mut g = MemberGroup::default();
        let contacts: HashSet<String> = ["wxid_a".into()].into_iter().collect();
        let members: HashSet<(String, String)> =
            [("1@chatroom".into(), "wxid_b".into())].into_iter().collect();

        g.replace_selection(&contacts, &members);
        assert_eq!(g.counts(), (1, 1));

        let other: HashSet<String> = ["wxid_c".into()].into_iter().collect();
        g.replace_selection(&other, &HashSet::new());
        assert_eq!(g.counts(), (1, 0));
        assert!(g.contacts.contains("wxid_c"));
        assert!(!g.contacts.contains("wxid_a"));
    }
}

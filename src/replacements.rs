//! 「替换词」：转换前把 A 词替换成 B 词。
//!
//! 例如把常写错的 `复试` 替换为 `复式`，或把某些方言写法换成标准玩法词。
//! 规则保存在 `%LOCALAPPDATA%\WechatViewer\replacements.txt`，按添加顺序逐条替换。
//!
//! 文本格式（制表符分隔，内容里的特殊字符已转义）：
//!
//! ```text
//! wxv-replace	1
//! rule	复试	复式
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;

pub type Rule = (String, String);

const MAGIC: &str = "wxv-replace\t1";

/// 内置默认替换词（当前这台机器上配置好的版本），换电脑时直接使用。
const DEFAULT_REPLACEMENTS: &str = include_str!("defaults/replacements.txt");

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("replacements.txt")
}

pub fn load() -> Vec<Rule> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return default_rules();
    };
    if !text.starts_with(MAGIC) {
        return default_rules();
    }
    parse(&text)
}

/// 编译进 exe 的默认替换词。
pub fn default_rules() -> Vec<Rule> {
    parse(DEFAULT_REPLACEMENTS)
}

pub fn save(rules: &[Rule]) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(rules)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(rules: &[Rule]) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for (from, to) in rules {
        if from.is_empty() {
            continue;
        }
        out.push_str("rule\t");
        out.push_str(&escape(from));
        out.push('\t');
        out.push_str(&escape(to));
        out.push('\n');
    }
    out
}

pub fn parse(text: &str) -> Vec<Rule> {
    if !text.starts_with(MAGIC) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(3, '\t');
        if parts.next() != Some("rule") {
            continue;
        }
        let (Some(from), Some(to)) = (parts.next(), parts.next()) else {
            continue;
        };
        let from = unescape(from);
        if from.is_empty() {
            continue;
        }
        out.push((from, unescape(to)));
    }
    out
}

/// 按顺序把规则应用到一段文本。空原文规则跳过。
pub fn apply(text: &str, rules: &[Rule]) -> String {
    let mut out = text.to_string();
    for (from, to) in rules {
        if from.is_empty() {
            continue;
        }
        if out.contains(from.as_str()) {
            out = out.replace(from.as_str(), to);
        }
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
    fn apply_in_order() {
        let rules = vec![
            ("复试".to_string(), "复式".to_string()),
            ("阿".to_string(), "各".to_string()),
        ];
        assert_eq!(apply("三中三 1,2,3阿10 复试", &rules), "三中三 1,2,3各10 复式");
    }

    #[test]
    fn roundtrip_keeps_rules() {
        let rules = vec![
            ("复试".to_string(), "复式".to_string()),
            ("a\tb".to_string(), "c\nd".to_string()),
        ];
        assert_eq!(parse(&serialize(&rules)), rules);
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("rule\tA\tB\n").is_empty());
    }

    #[test]
    fn default_rules_are_embedded() {
        let rules = default_rules();
        assert!(rules.contains(&("X".to_string(), "拖".to_string())));
        assert!(rules.contains(&("复试".to_string(), "复式".to_string())));
        assert!(rules.contains(&("三三".to_string(), "三中三".to_string())));
        // 当前配置里带 emoji 的写法，以及更多拖 / 乘号写法。
        assert!(rules.contains(&("三🀄️三".to_string(), "三中三".to_string())));
        assert!(rules.contains(&("二🀄️二".to_string(), "二中二".to_string())));
        assert!(rules.contains(&("×".to_string(), "拖".to_string())));
    }
}

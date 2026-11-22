//! 「引用列表」的导入过滤规则。
//!
//! 目前只有一项：原文完全等于某些文本时不导入注单列表（默认「1」「2」，
//! 对应群里常见的报名/占位消息）。链接类型的注单不走这里——它们固定只进
//! 引用列表，见 `ui::fill_orders`。
//!
//! 存储：`%LOCALAPPDATA%\WechatViewer\reference_filter.txt`

use std::env;
use std::fs;
use std::path::PathBuf;

const MAGIC: &str = "wxv-reference-filter\t1";

#[derive(Clone, Debug)]
pub struct Filter {
    /// 与原文（去掉首尾空白后）完全相等时要忽略的文本。
    pub ignore_exact: Vec<String>,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            ignore_exact: vec!["1".to_string(), "2".to_string()],
        }
    }
}

impl Filter {
    pub fn is_ignored(&self, text: &str) -> bool {
        let text = text.trim();
        !text.is_empty() && self.ignore_exact.iter().any(|item| item == text)
    }

    /// 把多行输入整理成过滤列表：去空白、去空行、去重，保留原顺序。
    pub fn from_lines(lines: &str) -> Self {
        let mut out: Vec<String> = Vec::new();
        for line in lines.lines() {
            let item = line.trim();
            if item.is_empty() || out.iter().any(|x| x == item) {
                continue;
            }
            out.push(item.to_string());
        }
        Self { ignore_exact: out }
    }

    pub fn to_lines(&self) -> String {
        self.ignore_exact.join("\n")
    }
}

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("reference_filter.txt")
}

pub fn load() -> Filter {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return Filter::default();
    };
    parse(&text)
}

pub fn save(filter: &Filter) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(filter)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

fn serialize(filter: &Filter) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for item in &filter.ignore_exact {
        out.push_str("ignore\t");
        out.push_str(item);
        out.push('\n');
    }
    out
}

fn parse(text: &str) -> Filter {
    if !text.starts_with(MAGIC) {
        return Filter::default();
    }
    let mut items = Vec::new();
    for line in text.lines().skip(1) {
        let item = line
            .strip_prefix("ignore\t")
            .or_else(|| line.strip_prefix("ignore "))
            .unwrap_or(line)
            .trim();
        if item.is_empty() || items.iter().any(|x: &String| x == item) {
            continue;
        }
        items.push(item.to_string());
    }
    Filter {
        ignore_exact: items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ignores_plain_one_and_two() {
        let f = Filter::default();
        assert!(f.is_ignored("1"));
        assert!(f.is_ignored(" 2 "));
        assert!(!f.is_ignored("11"));
        assert!(!f.is_ignored("2.0"));
        assert!(!f.is_ignored(""));
    }

    #[test]
    fn roundtrip_keeps_empty_list() {
        let empty = Filter::from_lines("\n  \n");
        assert!(empty.ignore_exact.is_empty());
        let back = parse(&serialize(&empty));
        assert!(back.ignore_exact.is_empty());
        assert!(!back.is_ignored("1"));
    }

    #[test]
    fn lines_are_trimmed_deduped_and_ordered() {
        let f = Filter::from_lines(" 1 \n2\n1\n\n3");
        assert_eq!(f.ignore_exact, vec!["1", "2", "3"]);
        assert_eq!(f.to_lines(), "1\n2\n3");
    }
}

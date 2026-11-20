//! 定位微信 4.x 的本地数据目录与账号目录。
//!
//! 优先级：微信配置文件里的保存路径 → 常见「文档」目录 → 各盘符根目录。
//! 账号目录以「其下存在 db_storage 子目录」为准。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

fn has_account_dirs(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && p.join("db_storage").is_dir() {
            return true;
        }
    }
    false
}

/// 在给定根目录下寻找「包含账号目录」的目录。
fn locate_under(root: &Path) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let candidates = [
        root.to_path_buf(),
        root.join("xwechat_files"),
        root.join("WeChat Files"),
    ];
    for c in candidates {
        if has_account_dirs(&c) {
            return Some(c);
        }
    }
    None
}

/// 从配置文件内容中提取第一个 Windows 路径（兼容纯路径 / JSON / ini）。
fn extract_path(text: &str) -> Option<PathBuf> {
    let t = text.trim_start_matches('\u{feff}').trim();
    if t.is_empty() {
        return None;
    }
    let chars: Vec<char> = t.chars().collect();
    for i in 0..chars.len().saturating_sub(2) {
        if chars[i].is_ascii_alphabetic() && chars[i + 1] == ':' && (chars[i + 2] == '\\' || chars[i + 2] == '/')
        {
            let mut end = i + 3;
            while end < chars.len() {
                let c = chars[end];
                if c.is_whitespace()
                    || c.is_control()
                    || matches!(c, '"' | '\'' | ',' | '}' | ']' | ';')
                {
                    break;
                }
                end += 1;
            }
            let s: String = chars[i..end].iter().collect();
            if s.len() > 3 {
                return Some(PathBuf::from(s));
            }
        }
    }
    None
}

fn config_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for key in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = env_path(key) {
            out.push(base.join("Tencent").join("xwechat"));
            out.push(base.join("Tencent").join("xwechat").join("config"));
            out.push(base.join("Tencent").join("WeChat"));
        }
    }
    out
}

fn path_from_configs() -> Option<PathBuf> {
    for dir in config_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let Ok(raw) = fs::read(&p) else {
                continue;
            };
            let text = String::from_utf8_lossy(&raw[..raw.len().min(8192)]);
            if let Some(root) = extract_path(&text) {
                if let Some(hit) = locate_under(&root) {
                    return Some(hit);
                }
            }
        }
    }
    None
}

fn common_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(profile) = env_path("USERPROFILE") {
        out.push(profile.join("Documents"));
        out.push(profile.clone());
    }
    if let Some(docs) = env_path("USERPROFILE") {
        out.push(docs.join("OneDrive").join("Documents"));
    }
    // 各盘符根目录（微信常被装到 D:\ 并就近存放数据）
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if root.is_dir() {
            out.push(root);
        }
    }
    out
}

/// 返回包含 `<账号>/db_storage` 的目录，例如 `D:\xwechat_files`。
pub fn find_account_root() -> Option<PathBuf> {
    if let Some(p) = path_from_configs() {
        return Some(p);
    }
    for root in common_roots() {
        if let Some(hit) = locate_under(&root) {
            return Some(hit);
        }
    }
    None
}

/// 列出所有含 db_storage 的账号目录。
pub fn account_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && p.join("db_storage").is_dir() {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// 账号目录名去掉末尾的 `_xxxx` 哈希后缀，得到 wxid。
pub fn wxid_from_account_dir(dir: &Path) -> String {
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(i) = name.rfind('_') {
        let suffix = &name[i + 1..];
        if suffix.len() == 4 && suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
            return name[..i].to_string();
        }
    }
    name
}

//! 「信息码」：提前录入分析师给的组合；之后录入的注单只要「地区 + 玩法 + 组合」
//! 撞上，就标黄提醒。与开奖、赔率、金额结算无关，纯粹是两个组合集合求交集。
//!
//! 组合展开与录入口径直接复用 `convert` 与 `settle::token_numbers`，保证
//! 「信息码」和「注单」对同一个组合的理解完全一致。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::convert::{self, BetItem, ConvertOutput};
use crate::keywords::{Field, KeywordSettings};
use crate::odds::region_index;
use crate::settle;

pub const REGION_CHARS: [&str; 3] = ["澳", "港", "老"];

/// 一条信息码：只存归一后的原文，解析结果每次按当前关键词设置现算。
#[derive(Clone, Debug)]
pub struct InfoCodeEntry {
    pub text: String,
    pub at: i64,
}

/// 组合身份：地区 + 玩法 + 组合键。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodeKey {
    pub region: usize,
    pub play: String,
    pub key: String,
}

impl CodeKey {
    pub fn region_char(&self) -> &'static str {
        REGION_CHARS.get(self.region).copied().unwrap_or("澳")
    }
}

#[derive(Clone, Debug, Default)]
pub struct OrderInfoHits {
    pub combos: Vec<(CodeKey, f64)>,
    #[allow(dead_code)]
    pub total: f64,
}

/// 一条信息码原文的解析结果（弹窗左列 / 右表用）。
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct InfoCodeLine {
    pub text: String,
    pub at: i64,
    pub ok: bool,
    pub reason: String,
    pub keys: Vec<CodeKey>,
}

/// 录入框逐行校验结果。
#[derive(Clone, Debug)]
pub struct InputLine {
    pub line: usize,
    pub text: String,
    pub ok: bool,
    pub reason: String,
    pub combos: usize,
}

/// 注单里的玩法归一成信息码支持的几类。
fn canonical_play(play: &str) -> Option<&'static str> {
    match play {
        "特码" => Some("特码"),
        "平码" => Some("平码"),
        "三中三" => Some("三中三"),
        "二中二" => Some("二中二"),
        "二全中" => Some("二中二"),
        "三中二" => Some("三中二"),
        "特碰" => Some("特碰"),
        "平特" => Some("平特"),
        _ => None,
    }
}

fn key_of_nums(nums: &[u32]) -> String {
    let mut nums = nums.to_vec();
    nums.sort_unstable();
    nums.dedup();
    nums.iter()
        .map(|n| format!("{n:02}"))
        .collect::<Vec<_>>()
        .join("-")
}

fn all_numbers(content: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for tok in content.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        for n in settle::token_numbers(tok) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

fn combinations<T: Clone>(items: &[T], k: usize) -> Vec<Vec<T>> {
    if k == 0 || k > items.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = Vec::with_capacity(k);
    fn walk<T: Clone>(
        items: &[T],
        k: usize,
        start: usize,
        cur: &mut Vec<T>,
        out: &mut Vec<Vec<T>>,
    ) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..items.len() {
            cur.push(items[i].clone());
            walk(items, k, i + 1, cur, out);
            cur.pop();
        }
    }
    walk(items, k, 0, &mut cur, &mut out);
    out
}

fn strip_group(s: &str) -> &str {
    s.trim()
        .trim_matches(['(', ')', '（', '）', '「', '」', '[', ']', '【', '】'])
}

/// 拖注分组：`(蛇)拖(猪)拖(鸡)` → [[蛇覆盖的号码], …]。
fn drag_groups(content: &str, mul_sep: &str) -> Option<Vec<Vec<u32>>> {
    if mul_sep.is_empty() || !content.contains(mul_sep) {
        return None;
    }
    let mut groups = Vec::new();
    for part in content.split(mul_sep) {
        let nums = settle::group_numbers(strip_group(part));
        if nums.is_empty() {
            return None;
        }
        groups.push(nums);
    }
    (groups.len() >= 2).then_some(groups)
}

/// 组合玩法展开成具体的 k 码组合。
fn combo_keys(content: &str, size: usize, mul_sep: &str) -> Vec<String> {
    if let Some(groups) = drag_groups(content, mul_sep) {
        if groups.len() == size {
            let mut keys = BTreeSet::new();
            let mut cur: Vec<u32> = Vec::with_capacity(size);
            fn walk(
                groups: &[Vec<u32>],
                i: usize,
                size: usize,
                cur: &mut Vec<u32>,
                out: &mut BTreeSet<String>,
            ) {
                if i == groups.len() {
                    // 和结算的 distinct_drag_count 一致：同一个组合里号码重复
                    // （如 8尾 与 蛇 共享 38）不算一注，否则会把总金额摊薄。
                    let mut sorted = cur.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    if sorted.len() == size {
                        out.insert(key_of_nums(cur));
                    }
                    return;
                }
                for n in &groups[i] {
                    cur.push(*n);
                    walk(groups, i + 1, size, cur, out);
                    cur.pop();
                }
            }
            walk(&groups, 0, size, &mut cur, &mut keys);
            return keys.into_iter().collect();
        }
    }
    let nums = all_numbers(content);
    combinations(&nums, size)
        .into_iter()
        .map(|c| key_of_nums(&c))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn flat_special_keys(content: &str) -> Vec<String> {
    let toks: Vec<&str> = content
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if toks.len() >= 4
        && toks
            .iter()
            .all(|t| t.chars().all(|c| c.is_ascii_digit()))
    {
        let nums: Vec<u32> = toks
            .iter()
            .filter_map(|t| t.parse::<u32>().ok())
            .collect();
        return vec![key_of_nums(&nums)];
    }
    toks.into_iter().map(str::to_string).collect()
}

/// 一条注单明细 → 它覆盖的组合（每注金额已摊到单个组合）。
pub fn expand_bet(bet: &BetItem, mul_sep: &str) -> Vec<(CodeKey, f64)> {
    let Some(play) = canonical_play(&bet.play) else {
        return Vec::new();
    };
    let region = region_index(&bet.lottery);
    let keys: Vec<String> = match play {
        "特码" | "平码" => all_numbers(&bet.content)
            .into_iter()
            .map(|n| format!("{n:02}"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        "三中三" | "三中二" => combo_keys(&bet.content, 3, mul_sep),
        "二中二" | "特碰" => combo_keys(&bet.content, 2, mul_sep),
        "平特" => flat_special_keys(&bet.content),
        _ => Vec::new(),
    };
    if keys.is_empty() {
        return Vec::new();
    }
    let per = bet.amount.max(0.0) / keys.len() as f64;
    keys.into_iter()
        .map(|key| {
            (
                CodeKey {
                    region,
                    play: play.to_string(),
                    key,
                },
                per,
            )
        })
        .collect()
}

/// 一条注单（一行消息）是否踩中信息码。
pub fn order_hits(
    conv: &ConvertOutput,
    keys: &HashSet<CodeKey>,
    mul_sep: &str,
) -> Option<OrderInfoHits> {
    let mut map: BTreeMap<CodeKey, f64> = BTreeMap::new();
    for bet in &conv.bets {
        for (key, amount) in expand_bet(bet, mul_sep) {
            if keys.contains(&key) {
                *map.entry(key).or_insert(0.0) += amount;
            }
        }
    }
    if map.is_empty() {
        return None;
    }
    let combos: Vec<(CodeKey, f64)> = map.into_iter().collect();
    let total = combos.iter().map(|(_, a)| *a).sum();
    Some(OrderInfoHits { combos, total })
}

fn region_prefix(s: &str, settings: &KeywordSettings) -> bool {
    let mut kws: Vec<&str> = ["澳", "港", "老"].to_vec();
    for f in [
        Field::HongKong,
        Field::Macau,
        Field::OldMacau,
        Field::BothSides,
        Field::ThreeSides,
    ] {
        kws.extend(settings.get(f).iter().map(String::as_str));
    }
    kws.iter()
        .filter(|k| !k.is_empty())
        .any(|k| s.starts_with(k))
}

/// 录入框一行 → 解析结果；成功时返回归一后的原文和组合键。
fn parse_one(
    raw: &str,
    settings: &KeywordSettings,
    default_region: usize,
    default_play: &str,
    mul_sep: &str,
) -> Result<(String, Vec<CodeKey>), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("空行".to_string());
    }
    let has_region = region_prefix(trimmed, settings);
    let mut body = trimmed.to_string();
    if !convert::has_play_keyword(&body, settings) {
        if default_play.is_empty() {
            return Err("未识别类型（请写玩法，或选择默认类型）".to_string());
        }
        body = format!("{default_play} {body}");
    }
    let line = if has_region {
        body
    } else {
        let r = REGION_CHARS.get(default_region).copied().unwrap_or("澳");
        format!("{r} {body}")
    };
    let out = convert::convert_with_replacements(
        &format!("{line} 各1"),
        settings,
        &[],
    );
    if out.invalid || out.bets.is_empty() {
        let reason = out
            .reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "没有展开出任何组合".to_string());
        return Err(reason);
    }
    let mut keys: Vec<CodeKey> = Vec::new();
    for bet in &out.bets {
        for (key, _) in expand_bet(bet, mul_sep) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    if keys.is_empty() {
        return Err("没有展开出任何组合".to_string());
    }
    Ok((line, keys))
}

/// 多行录入框逐行校验。
pub fn parse_input(
    text: &str,
    settings: &KeywordSettings,
    default_region: usize,
    default_play: &str,
    mul_sep: &str,
) -> Vec<InputLine> {
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match parse_one(raw, settings, default_region, default_play, mul_sep) {
            Ok((line, keys)) => out.push(InputLine {
                line: i + 1,
                text: line,
                ok: true,
                reason: String::new(),
                combos: keys.len(),
            }),
            Err(reason) => out.push(InputLine {
                line: i + 1,
                text: String::new(),
                ok: false,
                reason,
                combos: 0,
            }),
        }
    }
    out
}

/// 已存的信息码 → 逐条解析结果 + 全部组合键。
pub fn build_lines(
    entries: &[InfoCodeEntry],
    settings: &KeywordSettings,
    mul_sep: &str,
) -> (Vec<InfoCodeLine>, HashSet<CodeKey>) {
    let mut lines = Vec::new();
    let mut keys = HashSet::new();
    for e in entries {
        match parse_one(&e.text, settings, 0, "", mul_sep) {
            Ok((_, list)) => {
                keys.extend(list.iter().cloned());
                lines.push(InfoCodeLine {
                    text: e.text.clone(),
                    at: e.at,
                    ok: true,
                    reason: String::new(),
                    keys: list,
                });
            }
            Err(reason) => lines.push(InfoCodeLine {
                text: e.text.clone(),
                at: e.at,
                ok: false,
                reason,
                keys: Vec::new(),
            }),
        }
    }
    (lines, keys)
}

// ── 持久化 ────────────────────────────────────────────────────────────────

const MAGIC: &str = "wxv-infocode\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("info_codes.txt")
}

pub fn load() -> Vec<InfoCodeEntry> {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return Vec::new();
    };
    if !text.starts_with(MAGIC) {
        return Vec::new();
    }
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let (text, at) = line.split_once('\t')?;
            if text.trim().is_empty() {
                return None;
            }
            Some(InfoCodeEntry {
                text: text.to_string(),
                at: at.parse().unwrap_or(0),
            })
        })
        .collect()
}

pub fn save(entries: &[InfoCodeEntry]) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut out = String::from(MAGIC);
    out.push('\n');
    for e in entries {
        out.push_str(&e.text.replace(['\t', '\n', '\r'], " "));
        out.push('\t');
        out.push_str(&e.at.to_string());
        out.push('\n');
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, out).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keywords::KeywordSettings;

    #[test]
    fn info_line_expands_and_defaults_type() {
        let s = KeywordSettings::default();
        let lines = parse_input("01 02 03", &s, 1, "三中三", "拖");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ok, "{:?}", lines[0]);
        assert!(lines[0].text.starts_with("港 三中三"));
        assert_eq!(lines[0].combos, 1);

        let bad = parse_input("01 02 03", &s, 0, "", "拖");
        assert!(!bad[0].ok);
    }

    #[test]
    fn order_hits_match_type_region_and_combo() {
        let s = KeywordSettings::default();
        let parsed = parse_input("三中三 01 02 03 04", &s, 0, "", "拖");
        assert!(parsed[0].ok, "{:?}", parsed[0]);
        assert_eq!(parsed[0].combos, 4);

        let entries = vec![InfoCodeEntry {
            text: parsed[0].text.clone(),
            at: 0,
        }];
        let (lines, keys) = build_lines(&entries, &s, "拖");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ok, "{:?}", lines[0]);

        let out = convert::convert_with_replacements(
            "澳 三中三 01 02 03 各10",
            &s,
            &[],
        );
        let hit = order_hits(&out, &keys, "拖").expect("应该命中");
        assert_eq!(hit.combos.len(), 1);
        assert!((hit.total - 10.0).abs() < 1e-6, "{:?}", hit);

        // 地区不同不命中。
        let hk = convert::convert_with_replacements(
            "港 三中三 01 02 03 各10",
            &s,
            &[],
        );
        assert!(order_hits(&hk, &keys, "拖").is_none());
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    #[test]
    fn shared_drag_number_is_not_counted_twice() {
        let bet = BetItem {
            line: String::new(),
            lottery: "澳门".to_string(),
            play: "三中三".to_string(),
            content: "(8尾)拖(蛇)拖(虎)".to_string(),
            stake: "各组 1".to_string(),
            bets: 76.0,
            amount: 76.0,
            attr_total: false,
            validate_content: None,
        };
        let keys = expand_bet(&bet, "拖");
        assert_eq!(keys.len(), 76);
        let (_, amount) = keys
            .iter()
            .find(|(k, _)| k.key == "05-14-18")
            .expect("05-14-18 应该命中");
        assert!((*amount - 1.0).abs() < 1e-6, "{amount}");
    }
}

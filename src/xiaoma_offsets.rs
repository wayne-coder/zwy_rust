//! 小码表的减额：查重减额、按阈值减额都写进同一张减额表，口径与参考项目
//! `zwy_wails_v1` 的小码减额表（`entry.xiaoMaStakeOffsets`）一致。
//!
//! 减额值：正 = 减额，负 = 加额。行键为 `地区|玩法|组合`，组合形如 `01-02-03`。
//! 减额表与减额记录存在 `%LOCALAPPDATA%\WechatViewer\xiaoma_offsets.txt`，
//! 不修改微信数据库。
//!
//! 文本格式（制表符分隔，文本字段里的 `\`、制表符、换行已转义）：
//!
//! ```text
//! wxv-xiaoma-offsets	1
//! offset	<行键>	<金额>
//! log	<种类>	<说明>	<是否已撤回 0/1>	<行键>=<金额>;<行键>=<金额>
//! ```

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::risk::XiaoMaRow;

/// 行键 → 减额金额（正 = 减额，负 = 加额）
pub type OffsetMap = BTreeMap<String, f64>;

const MAGIC: &str = "wxv-xiaoma-offsets\t1";

/// 一笔减额（或撤回前的那一笔）。`delta` 就是写进减额表的增量，撤回时取反写回去。
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub at: String,
    /// 「减额」「查重减额」
    pub kind: String,
    pub desc: String,
    pub undone: bool,
    pub delta: OffsetMap,
}

#[derive(Clone, Debug, Default)]
pub struct Store {
    pub offsets: OffsetMap,
    /// 旧的在前、新的在后
    pub log: Vec<LogEntry>,
}

/// 行键：地区（0 澳 / 1 港 / 2 老澳门）、玩法（二全中与二中二分开）、组合。
pub fn row_key(region: usize, play: &str, combo: &str) -> String {
    format!("{region}|{play}|{combo}")
}

/// 小码行对应的减额键。
pub fn key_of(row: &XiaoMaRow) -> String {
    row_key(row.region, &row.play, &row.key)
}

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("xiaoma_offsets.txt")
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn add_into(offsets: &mut OffsetMap, delta: &OffsetMap) {
    for (key, amount) in delta {
        let next = offsets.get(key).copied().unwrap_or(0.0) + amount;
        if next.abs() < 1e-9 {
            offsets.remove(key);
        } else {
            offsets.insert(key.clone(), next);
        }
    }
}

impl Store {
    pub fn load() -> Self {
        fs::read_to_string(store_path())
            .map(|text| parse(&text))
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = store_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("txt.tmp");
        if fs::write(&tmp, serialize(self)).is_err() {
            return;
        }
        let _ = fs::rename(&tmp, &path);
    }

    /// 写入一笔减额。`delta` 里为 0 的键不写；全为 0 时什么都不记。
    pub fn apply(&mut self, delta: OffsetMap, kind: &str, desc: String) {
        let delta: OffsetMap = delta.into_iter().filter(|(_, v)| v.abs() > 1e-9).collect();
        if delta.is_empty() {
            return;
        }
        add_into(&mut self.offsets, &delta);
        self.log.push(LogEntry {
            at: now(),
            kind: kind.to_string(),
            desc,
            undone: false,
            delta,
        });
        self.save();
    }

    /// 撤回第 `index` 笔：把它的增量取反写回减额表。已撤回的不动。
    pub fn undo(&mut self, index: usize) {
        let Some(entry) = self.log.get(index) else {
            return;
        };
        if entry.undone {
            return;
        }
        let negated: OffsetMap = entry.delta.iter().map(|(k, v)| (k.clone(), -v)).collect();
        add_into(&mut self.offsets, &negated);
        self.log[index].undone = true;
        self.save();
    }
}

fn esc(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

fn unesc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn serialize(store: &Store) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for (key, amount) in &store.offsets {
        out.push_str(&format!("offset\t{}\t{}\n", esc(key), amount));
    }
    for entry in &store.log {
        let delta: Vec<String> = entry
            .delta
            .iter()
            .map(|(k, v)| format!("{}={}", esc(k), v))
            .collect();
        out.push_str(&format!(
            "log\t{}\t{}\t{}\t{}\n",
            esc(&entry.kind),
            esc(&format!("{} {}", entry.at, entry.desc)),
            if entry.undone { 1 } else { 0 },
            delta.join(";"),
        ));
    }
    out
}

fn parse(text: &str) -> Store {
    let mut store = Store::default();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["offset", key, amount] => {
                if let Ok(v) = amount.parse::<f64>() {
                    store.offsets.insert(unesc(key), v);
                }
            }
            ["log", kind, text, undone, delta] => {
                let mut delta_map = OffsetMap::new();
                for pair in delta.split(';').filter(|p| !p.is_empty()) {
                    if let Some((k, v)) = pair.split_once('=') {
                        if let Ok(v) = v.parse::<f64>() {
                            delta_map.insert(unesc(k), v);
                        }
                    }
                }
                // 时间与说明拼在一个字段里存，时间固定 19 个字符（yyyy-mm-dd HH:MM:SS）。
                let text = unesc(text);
                let (at, desc) = if text.len() >= 20 && text.is_char_boundary(19) {
                    (text[..19].to_string(), text[20..].to_string())
                } else {
                    (String::new(), text)
                };
                store.log.push(LogEntry {
                    at,
                    kind: unesc(kind),
                    desc,
                    undone: *undone == "1",
                    delta: delta_map,
                });
            }
            _ => {}
        }
    }
    store
}

/// 把减额表作用到小码行上，返回新行。语义是「整行按金额减」：投注额减 d，
/// 风险 / 回水 / 盈亏按同一比例缩放（与参考项目 `applyXiaoMaOffsets` 相同）。
/// 没有下限，减穿就是负数。
///
/// 减额表里有、但当前没有对应行的键（投注记录被删光了），补一行负数，
/// 这笔减额才不会在界面上悄悄消失。
pub fn apply_to_rows(mut rows: Vec<XiaoMaRow>, offsets: &OffsetMap) -> Vec<XiaoMaRow> {
    if offsets.is_empty() {
        return rows;
    }
    let mut seen: HashSet<String> = HashSet::new();
    for row in rows.iter_mut() {
        let key = key_of(row);
        seen.insert(key.clone());
        let Some(&amount) = offsets.get(&key) else {
            continue;
        };
        let next = row.stake - amount;
        let ratio = if row.stake > 0.0 { next / row.stake } else { 1.0 };
        row.stake = next;
        row.water *= ratio;
        row.risk *= ratio;
        row.profit *= ratio;
    }
    for (key, &amount) in offsets {
        if seen.contains(key) || amount.abs() < 1e-9 {
            continue;
        }
        if let Some(orphan) = orphan_row(key, amount) {
            rows.push(orphan);
        }
    }
    rows
}

/// 键还原成一行（没有投注记录时）。风险与盈亏没有依据，记为 0。
fn orphan_row(key: &str, amount: f64) -> Option<XiaoMaRow> {
    let mut parts = key.splitn(3, '|');
    let region: usize = parts.next()?.parse().ok()?;
    let play = parts.next()?;
    let combo = parts.next()?;
    let label = if play == "二全中" { "二中二" } else { play };
    Some(XiaoMaRow {
        key: combo.to_string(),
        label: label.to_string(),
        region,
        play: play.to_string(),
        stake: -amount,
        water: 0.0,
        risk: 0.0,
        profit: 0.0,
        hit: false,
        time: String::new(),
        orphan: true,
    })
}

/// 这一行超出过滤值多少：四舍五入到整数。`> 0` 才算「超出」，余量不足 1 的行不入表。
pub fn excess_of(stake: f64, threshold: f64) -> f64 {
    (stake - threshold).round()
}

/// 减额列里的行：某地区、某类型、有真实投注（不是孤行）、超出过滤值的，按投注额降序。
pub fn column_rows(rows: &[XiaoMaRow], region: usize, label: &str, threshold: f64) -> Vec<XiaoMaRow> {
    let mut out: Vec<XiaoMaRow> = rows
        .iter()
        .filter(|r| !r.orphan && r.region == region && r.label == label)
        .filter(|r| excess_of(r.stake, threshold) > 0.0)
        .cloned()
        .collect();
    out.sort_by(|a, b| b.stake.partial_cmp(&a.stake).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 「整列减额」：每行减掉自己超出阈值的部分（`excess_of`），只看 `region` 这一区的行。
pub fn excess_delta(rows: &[XiaoMaRow], region: usize, threshold: f64) -> OffsetMap {
    let mut delta = OffsetMap::new();
    for row in rows.iter().filter(|r| !r.orphan && r.region == region) {
        let excess = excess_of(row.stake, threshold);
        if excess > 0.0 {
            delta.insert(key_of(row), excess);
        }
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_round_trips_offsets_and_log() {
        let mut store = Store::default();
        store.offsets.insert(row_key(0, "三中三", "01-02-03"), 5.0);
        store.log.push(LogEntry {
            at: "2026-11-24 10:00:00".into(),
            kind: "减额".into(),
            desc: "澳门\t过滤值 30".into(),
            undone: true,
            delta: OffsetMap::from([(row_key(0, "三中三", "01-02-03"), 5.0)]),
        });
        let back = parse(&serialize(&store));
        assert_eq!(back.offsets, store.offsets);
        assert_eq!(back.log.len(), 1);
        assert!(back.log[0].undone);
        assert_eq!(back.log[0].at, "2026-11-24 10:00:00");
        assert_eq!(back.log[0].desc, "澳门\t过滤值 30");
        assert_eq!(back.log[0].delta, store.log[0].delta);
    }

    #[test]
    fn orphan_row_keeps_offset_without_bets() {
        let offsets = OffsetMap::from([(row_key(1, "二全中", "07-08"), -3.0)]);
        let rows = apply_to_rows(Vec::new(), &offsets);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].region, 1);
        assert_eq!(rows[0].label, "二中二");
        assert_eq!(rows[0].stake, 3.0);
    }

    #[test]
    fn excess_skips_rows_at_or_below_threshold() {
        let mk = |stake: f64, key: &str| XiaoMaRow {
            key: key.into(),
            label: "三中三".into(),
            play: "三中三".into(),
            region: 0,
            stake,
            water: 0.0,
            risk: 0.0,
            profit: 0.0,
            hit: false,
            time: String::new(),
            orphan: false,
        };
        let rows = vec![mk(80.0, "01-02-03"), mk(30.4, "04-05-06"), mk(10.0, "07-08-09")];
        let delta = excess_delta(&rows, 0, 30.0);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[&row_key(0, "三中三", "01-02-03")], 50.0);
    }
}

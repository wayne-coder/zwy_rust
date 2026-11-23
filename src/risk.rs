//! 「风险」页面数据层：把当前注单按参考项目 `zwy_wails_v1` 右列面板的口径
//! （`panel compact-panel detail-col-right`）汇总成三栏投注表和小码表。
//!
//! 口径与参考项目 `lib/betPanel.ts` / `lib/rowMetrics.ts` 一致：
//! - 整张面板**摘掉投注人**，赔率一律取「默认」；
//! - 特码 / 平码：固定 49 行，属性词行按覆盖号码摊开；
//! - 平特：12 生肖 + 10 尾数固定 22 行，其余号码组追加在末尾；
//! - 连肖：只出有注的组合，按规格分子页签；
//! - 特码的风险 = 整区总投注 − 整区回水 − 该项目投注额 × 该项目赔率；
//! - 小码表一个组合一行，风险 = 投注额 × 名义倍率，盈亏 = 投注额 − 中奖 − 回水。

use std::collections::{BTreeMap, BTreeSet};

use crate::convert::BetItem;
use crate::draw::{Draws, RegionDraw};
use crate::odds::{region_index, OddsConfig, OddsSet, REGION_LABELS};
use crate::settle;

pub const REGION_CHARS: [&str; 3] = ["澳", "港", "老"];
pub const REGION_NAMES: [&str; 3] = REGION_LABELS;

/// 小码面板里的四种玩法（顺序即概率卡片的顺序：三中三、特碰、二中二、三中二）。
pub const PLAY_LABELS: [&str; 4] = ["三中三", "特碰", "二中二", "三中二"];
pub const TOTAL_COMBOS: [f64; 4] = [18424.0, 1176.0, 1176.0, 18424.0];

#[derive(Clone)]
pub struct BetRow {
    pub bet: BetItem,
    pub region: usize,
    pub time: String,
}

impl BetRow {
    pub fn new(bet: BetItem, time: &str) -> Self {
        let region = region_index(&bet.lottery);
        Self {
            bet,
            region,
            time: time.to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelList {
    Special,
    LianXiao,
    FlatSpecial,
    FlatNumber,
}

#[derive(Clone, Default)]
pub struct PanelRow {
    pub key: String,
    pub label: String,
    pub stake: f64,
    pub risk: f64,
    pub hit: bool,
    /// 号码行才有（界面按波色上色）
    pub num: Option<u32>,
}

#[derive(Clone, Default)]
pub struct PanelColumn {
    pub total_stake: f64,
    pub total_water: f64,
    pub rows: Vec<PanelRow>,
}

/// 三栏投注表。`lian_size` 只对连肖的**行**生效，区头合计始终按全部规格算。
pub fn panel(
    list: PanelList,
    bets: &[BetRow],
    draws: &Draws,
    odds: &OddsConfig,
    odds_group: Option<&str>,
    selected_lian_size: usize,
) -> Vec<PanelColumn> {
    (0..3)
        .map(|r| build_column(list, bets, r, draws, odds, odds_group, selected_lian_size))
        .collect()
}

fn list_matches(list: PanelList, play: &str) -> bool {
    match list {
        PanelList::Special => play == "特码",
        PanelList::FlatNumber => play == "平码",
        PanelList::FlatSpecial => play == "平特" || play == "四中一" || play == "五中一",
        PanelList::LianXiao => play.ends_with("连肖"),
    }
}

fn draw_of(draws: &Draws, region: usize) -> &RegionDraw {
    draws.get(REGION_NAMES[region])
}

fn odds_of<'a>(
    odds: &'a OddsConfig,
    group: Option<&str>,
    region: usize,
) -> &'a OddsSet {
    odds.resolve("", group, region)
}

fn per_bet(bet: &BetItem) -> f64 {
    let bets = bet.bets.max(1.0);
    bet.amount.max(0.0) / bets
}

fn covered_numbers(content: &str) -> Vec<u32> {
    let mut set = BTreeSet::new();
    for tok in settle::pick_tokens(content) {
        set.extend(settle::token_numbers(tok));
    }
    set.into_iter().collect()
}

fn num_key(n: u32) -> String {
    format!("{n:02}")
}

fn num_label(n: u32) -> String {
    format!("{n:02}{}", settle::zodiac_of(n).unwrap_or(""))
}

fn joined_numbers(nums: &[u32]) -> String {
    nums.iter()
        .map(|n| num_key(*n))
        .collect::<Vec<_>>()
        .join("-")
}

fn intersects(nums: &[u32], hit: &BTreeSet<u32>) -> bool {
    nums.iter().any(|n| hit.contains(n))
}

fn add_hit_row(
    map: &mut BTreeMap<String, (f64, bool)>,
    key: String,
    stake: f64,
    hit: bool,
) {
    if stake <= 0.0 {
        return;
    }
    let e = map.entry(key).or_insert((0.0, false));
    e.0 += stake;
    e.1 |= hit;
}

fn water_of(bet: &BetItem, odds: &OddsSet) -> f64 {
    match bet.play.as_str() {
        "特码" => bet.amount * odds.value("rw_special_number") / 100.0,
        "平码" => bet.amount * odds.value("rw_flat_number") / 100.0,
        "平特" => {
            let groups = flat_special_groups(&bet.content);
            let per = per_bet(bet);
            groups
                .iter()
                .map(|g| {
                    let key = if g.1.len() == 5 {
                        "rw_flat_special_5"
                    } else {
                        "rw_flat_special_4"
                    };
                    per * odds.value(key) / 100.0
                })
                .sum()
        }
        "四中一" => bet.amount * odds.value("rw_zhongyi_4") / 100.0,
        "五中一" => bet.amount * odds.value("rw_zhongyi_5") / 100.0,
        _ if bet.play.ends_with("连肖") => bet.amount * odds.value("rw_lianxiao") / 100.0,
        _ => 0.0,
    }
}

fn build_column(
    list: PanelList,
    bets: &[BetRow],
    region: usize,
    draws: &Draws,
    odds: &OddsConfig,
    odds_group: Option<&str>,
    selected_lian_size: usize,
) -> PanelColumn {
    let set = odds_of(odds, odds_group, region);
    let draw = draw_of(draws, region);
    let all7: BTreeSet<u32> = draw
        .draw
        .iter()
        .copied()
        .chain(draw.special)
        .collect();
    let draw6: BTreeSet<u32> = draw.draw.iter().copied().collect();

    let mut col = PanelColumn::default();
    for b in bets
        .iter()
        .filter(|b| b.region == region && list_matches(list, &b.bet.play))
    {
        col.total_stake += b.bet.amount.max(0.0);
        col.total_water += water_of(&b.bet, set);
    }

    match list {
        PanelList::Special => {
            let mut by_num: BTreeMap<String, (f64, bool)> = BTreeMap::new();
            for b in bets
                .iter()
                .filter(|b| b.region == region && b.bet.play == "特码")
            {
                let bet = &b.bet;
                if bet.attr_total {
                    let nums = covered_numbers(&bet.content);
                    if !nums.is_empty() {
                        let per = bet.amount.max(0.0) / nums.len() as f64;
                        for n in nums {
                            add_hit_row(&mut by_num, n.to_string(), per, draw.special == Some(n));
                        }
                    }
                } else {
                    let per = per_bet(bet);
                    for tok in settle::pick_tokens(&bet.content) {
                        for n in settle::token_numbers(tok) {
                            add_hit_row(&mut by_num, n.to_string(), per, draw.special == Some(n));
                        }
                    }
                }
            }
            let base = col.total_stake - col.total_water;
            let multiplier = set.value("special_number");
            for num in 1..=49u32 {
                let (stake, hit) = by_num.get(&num.to_string()).copied().unwrap_or((0.0, false));
                col.rows.push(PanelRow {
                    key: num_key(num),
                    label: num_label(num),
                    stake,
                    risk: base - stake * multiplier,
                    hit,
                    num: Some(num),
                });
            }
        }
        PanelList::FlatNumber => {
            let mut by_num: BTreeMap<String, (f64, bool)> = BTreeMap::new();
            for b in bets
                .iter()
                .filter(|b| b.region == region && b.bet.play == "平码")
            {
                let bet = &b.bet;
                let per = per_bet(bet);
                for tok in settle::pick_tokens(&bet.content) {
                    for n in settle::token_numbers(tok) {
                        add_hit_row(&mut by_num, n.to_string(), per, draw6.contains(&n));
                    }
                }
            }
            for num in 1..=49u32 {
                let (stake, hit) = by_num.get(&num.to_string()).copied().unwrap_or((0.0, false));
                col.rows.push(PanelRow {
                    key: num_key(num),
                    label: num_label(num),
                    stake,
                    risk: 0.0,
                    hit,
                    num: Some(num),
                });
            }
        }
        PanelList::FlatSpecial => {
            let mut by_key: BTreeMap<String, (f64, bool)> = BTreeMap::new();
            for b in bets.iter().filter(|b| {
                b.region == region
                    && (b.bet.play == "平特" || b.bet.play == "四中一" || b.bet.play == "五中一")
            }) {
                let bet = &b.bet;
                let per = per_bet(bet);
                match bet.play.as_str() {
                    "平特" => {
                        for (key, nums) in flat_special_groups(&bet.content) {
                            add_hit_row(&mut by_key, key, per, intersects(&nums, &all7));
                        }
                    }
                    play @ ("四中一" | "五中一") => {
                        let size = if play == "四中一" { 4 } else { 5 };
                        let nums = covered_numbers(&bet.content);
                        for combo in combinations(&nums, size) {
                            let key = format!("{play} {}", joined_numbers(&combo));
                            add_hit_row(&mut by_key, key, per, intersects(&combo, &all7));
                        }
                    }
                    _ => {}
                }
            }
            let fixed: Vec<String> = settle::zodiac_names()
                .into_iter()
                .map(str::to_string)
                .chain((0..=9).map(|t| format!("{t}尾")))
                .collect();
            for key in &fixed {
                let (stake, hit) = by_key.get(key).copied().unwrap_or((0.0, false));
                col.rows.push(PanelRow {
                    key: key.clone(),
                    label: key.clone(),
                    stake,
                    risk: 0.0,
                    hit,
                    num: None,
                });
            }
            for (key, (stake, hit)) in &by_key {
                if fixed.contains(key) {
                    continue;
                }
                col.rows.push(PanelRow {
                    key: key.clone(),
                    label: key.clone(),
                    stake: *stake,
                    risk: 0.0,
                    hit: *hit,
                    num: None,
                });
            }
        }
        PanelList::LianXiao => {
            let winning: BTreeSet<String> = all7
                .iter()
                .filter_map(|n| settle::zodiac_of(*n))
                .map(str::to_string)
                .collect();
            let mut by_key: BTreeMap<String, (f64, bool)> = BTreeMap::new();
            for b in bets
                .iter()
                .filter(|b| b.region == region && b.bet.play.ends_with("连肖"))
            {
                let bet = &b.bet;
                let Some(size) = parse_lian_size(&bet.play) else {
                    continue;
                };
                if size != selected_lian_size.max(2) {
                    // 区头合计已按全部规格算过；行只出当前子页签的规格。
                    continue;
                }
                let picks = lian_zodiacs(&bet.content);
                let per = per_bet(bet);
                for combo in string_combinations(&picks, size) {
                    let key = combo.join("-");
                    let hit = combo.iter().all(|z| winning.contains(z));
                    let e = by_key.entry(key).or_insert((0.0, false));
                    e.0 += per;
                    e.1 |= hit;
                }
            }
            for (key, (stake, hit)) in by_key {
                col.rows.push(PanelRow {
                    key: key.clone(),
                    label: key,
                    stake,
                    risk: 0.0,
                    hit,
                    num: None,
                });
            }
        }
    }
    col
}

/// 平特的分组（带行键）：生肖 / 尾数各一组，裸数字合成一组，属性词自成一键。
fn flat_special_groups(content: &str) -> Vec<(String, Vec<u32>)> {
    let mut out: Vec<(String, Vec<u32>)> = Vec::new();
    let mut direct: Vec<u32> = Vec::new();
    for tok in settle::pick_tokens(content) {
        if let Some(nums) = settle::zodiac_numbers(tok) {
            out.push((tok.to_string(), nums.to_vec()));
            continue;
        }
        if let Some(d) = tok.strip_suffix('尾').and_then(|s| s.parse::<u32>().ok()) {
            out.push((
                tok.to_string(),
                (1..=49u32).filter(|n| settle::tail_of(*n) == d).collect(),
            ));
            continue;
        }
        if tok.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = tok.parse::<u32>() {
                if (1..=49).contains(&n) && !direct.contains(&n) {
                    direct.push(n);
                }
            }
            continue;
        }
        if let Some(nums) = settle::attr_numbers(tok) {
            out.push((tok.to_string(), nums));
        }
    }
    if !direct.is_empty() {
        direct.sort_unstable();
        out.push((joined_numbers(&direct), direct));
    }
    out
}

fn parse_lian_size(play: &str) -> Option<usize> {
    fn value(c: char) -> Option<usize> {
        match c {
            '二' | '2' => Some(2),
            '三' | '3' => Some(3),
            '四' | '4' => Some(4),
            '五' | '5' => Some(5),
            '六' | '6' => Some(6),
            '七' | '7' => Some(7),
            '八' | '8' => Some(8),
            '九' | '9' => Some(9),
            '十' => Some(10),
            _ => None,
        }
    }
    let head = play.strip_suffix("连肖")?;
    head.chars().next().and_then(value)
}

fn lian_zodiacs(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tok in settle::pick_tokens(content) {
        if settle::zodiac_numbers(tok).is_some() {
            out.insert(tok.to_string());
        } else if tok.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = tok.parse::<u32>() {
                if let Some(z) = settle::zodiac_of(n) {
                    out.insert(z.to_string());
                }
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

fn string_combinations(values: &BTreeSet<String>, k: usize) -> Vec<Vec<String>> {
    let items: Vec<String> = values.iter().cloned().collect();
    combinations(&items, k)
}

// ── 小码表 ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct XiaoMaRow {
    pub key: String,
    /// 类型标签（二全中在展示上并入二中二）
    pub label: String,
    pub region: usize,
    pub stake: f64,
    /// 回水（统计卡片「总回水」用）
    pub water: f64,
    pub risk: f64,
    pub profit: f64,
    pub hit: bool,
    pub time: String,
}

struct Agg {
    play: String,
    stake: f64,
    win: f64,
    rebate: f64,
    hit: bool,
    time: String,
}

/// 组合玩法全部展开成「一个组合一行」，忽略投注人。
pub fn xiaoma(
    bets: &[BetRow],
    draws: &Draws,
    odds: &OddsConfig,
    odds_group: Option<&str>,
    mul_sep: &str,
) -> Vec<XiaoMaRow> {
    let mut agg: BTreeMap<(usize, String, String), Agg> = BTreeMap::new();
    for b in bets {
        let bet = &b.bet;
        let Some(play_idx) = play_index(&bet.play) else {
            continue;
        };
        let set = odds_of(odds, odds_group, b.region);
        let draw = draw_of(draws, b.region);
        let draw6: BTreeSet<u32> = draw.draw.iter().copied().collect();
        let special = draw.special;
        let size = if play_idx == 0 || play_idx == 3 { 3 } else { 2 };
        let label = PLAY_LABELS[play_idx].to_string();
        let per = per_bet(bet);
        let rebate_rate = rebate_rate(&bet.play, set);
        for combo in bet_combos(bet, size, mul_sep) {
            let hits = combo.iter().filter(|n| draw6.contains(n)).count();
            let (hit, win) = match bet.play.as_str() {
                "三中三" => (
                    hits == 3,
                    if hits == 3 {
                        per * set.value("three_in_three")
                    } else {
                        0.0
                    },
                ),
                "三中二" => {
                    let win = if hits == 3 {
                        per * set.value("three_in_two_sa")
                    } else if hits == 2 {
                        per * set.value("three_in_two_liang")
                    } else {
                        0.0
                    };
                    (hits >= 2, win)
                }
                "二中二" => (
                    hits == 2,
                    if hits == 2 {
                        per * set.value("two_in_two")
                    } else {
                        0.0
                    },
                ),
                "二全中" => (
                    hits == 2,
                    if hits == 2 {
                        per * set.value("two_all_hit")
                    } else {
                        0.0
                    },
                ),
                // 特碰：一对号码里一个是特码、另一个是平码。
                "特碰" => {
                    let hit = combo.len() == 2
                        && ((special == Some(combo[0]) && draw6.contains(&combo[1]))
                            || (special == Some(combo[1]) && draw6.contains(&combo[0])));
                    (
                        hit,
                        if hit {
                            per * set.value("special_hit")
                        } else {
                            0.0
                        },
                    )
                }
                _ => (false, 0.0),
            };
            let key = format!("{}\t{}", bet.play, joined_numbers(&combo));
            let e = agg
                .entry((b.region, label.clone(), key))
                .or_insert_with(|| Agg {
                    play: bet.play.clone(),
                    stake: 0.0,
                    win: 0.0,
                    rebate: 0.0,
                    hit: false,
                    time: String::new(),
                });
            e.stake += per;
            e.win += win;
            e.rebate += per * rebate_rate / 100.0;
            e.hit |= hit;
            if b.time > e.time {
                e.time = b.time.clone();
            }
        }
    }
    agg.into_iter()
        .map(|((region, label, composite), a)| {
            // 风险按名义倍率算；倍率与玩法一一对应（二全中走自己那一档）。
            let set = odds_of(odds, odds_group, region);
            let nominal = nominal_multiplier(&a.play, set);
            let key = composite
                .split_once('\t')
                .map(|(_, k)| k.to_string())
                .unwrap_or(composite);
            XiaoMaRow {
                key,
                label,
                region,
                stake: a.stake,
                water: a.rebate,
                risk: a.stake * nominal,
                profit: a.stake - a.win - a.rebate,
                hit: a.hit,
                time: a.time,
            }
        })
        .collect()
}

fn play_index(play: &str) -> Option<usize> {
    match play {
        "三中三" => Some(0),
        "特碰" => Some(1),
        "二中二" | "二全中" => Some(2),
        "三中二" => Some(3),
        _ => None,
    }
}

fn nominal_multiplier(play: &str, odds: &OddsSet) -> f64 {
    match play {
        "三中三" => odds.value("three_in_three"),
        "三中二" => odds.value("three_in_two"),
        "特碰" => odds.value("special_hit"),
        "二全中" => odds.value("two_all_hit"),
        _ => odds.value("two_in_two"),
    }
}

fn rebate_rate(play: &str, odds: &OddsSet) -> f64 {
    match play {
        "三中三" => odds.value("rw_three_in_three"),
        "三中二" => odds.value("rw_three_in_two"),
        "特碰" => odds.value("rw_special_hit"),
        "二全中" => odds.value("rw_two_all_hit"),
        _ => odds.value("rw_two_in_two"),
    }
}

/// 一条组合注展开后的全部组合（去重）。拖注（互碰）按每段各出一个号。
fn bet_combos(bet: &BetItem, size: usize, mul_sep: &str) -> Vec<Vec<u32>> {
    let mut out: BTreeSet<Vec<u32>> = BTreeSet::new();
    if !mul_sep.is_empty() && bet.content.contains(mul_sep) {
        let groups: Vec<Vec<u32>> = bet
            .content
            .split(mul_sep)
            .map(settle::group_numbers)
            .collect();
        if groups.len() == size && groups.iter().all(|g| !g.is_empty()) {
            for_each_drag(&groups, &mut |combo| {
                let mut combo = combo.to_vec();
                combo.sort_unstable();
                out.insert(combo);
            });
            return out.into_iter().collect();
        }
    }
    let covered = covered_numbers(&bet.content);
    for combo in combinations(&covered, size) {
        out.insert(combo);
    }
    out.into_iter().collect()
}

fn for_each_drag(groups: &[Vec<u32>], f: &mut impl FnMut(&[u32])) {
    let mut cur = Vec::with_capacity(groups.len());
    fn walk(groups: &[Vec<u32>], i: usize, cur: &mut Vec<u32>, f: &mut impl FnMut(&[u32])) {
        if i == groups.len() {
            f(cur);
            return;
        }
        for n in &groups[i] {
            cur.push(*n);
            walk(groups, i + 1, cur, f);
            cur.pop();
        }
    }
    walk(groups, 0, &mut cur, f);
}

// ── 中奖概率 ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
pub struct PlayStat {
    /// 去重组合数
    pub distinct: usize,
    /// 展开后的总注数
    pub total_bets: usize,
}

/// 四种组合玩法的「已收集组合数 / 总注数」。`region` 为 None 时统计全部地区。
pub fn play_stats(bets: &[BetRow], region: Option<usize>, mul_sep: &str) -> [PlayStat; 4] {
    let mut stats = [PlayStat::default(); 4];
    let mut seen: [BTreeSet<u64>; 4] = Default::default();
    for b in bets {
        if region.is_some_and(|r| b.region != r) {
            continue;
        }
        let bet = &b.bet;
        let Some(play_idx) = play_index(&bet.play) else {
            continue;
        };
        let size = if play_idx == 0 || play_idx == 3 { 3 } else { 2 };
        for combo in bet_combos(bet, size, mul_sep) {
            stats[play_idx].total_bets += 1;
            if let Some(code) = encode_combo(&combo) {
                if seen[play_idx].insert(code) {
                    stats[play_idx].distinct += 1;
                }
            }
        }
    }
    stats
}

fn encode_combo(nums: &[u32]) -> Option<u64> {
    match nums.len() {
        2 => Some((nums[0].min(nums[1]) * 100 + nums[0].max(nums[1])) as u64),
        3 => {
            let mut v = nums.to_vec();
            v.sort_unstable();
            Some((v[0] * 10000 + v[1] * 100 + v[2]) as u64)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{Draws, RegionDraw};

    fn bet(play: &str, content: &str, amount: f64, bets: f64) -> BetRow {
        BetRow::new(
            BetItem {
                line: String::new(),
                lottery: "澳门".to_string(),
                play: play.to_string(),
                content: content.to_string(),
                stake: String::new(),
                bets,
                amount,
                attr_total: false,
                validate_content: None,
            },
            "12:00:00",
        )
    }

    fn draws() -> Draws {
        let macau = RegionDraw {
            draw: vec![1, 2, 3, 4, 5, 6],
            special: Some(7),
        };
        Draws {
            macau,
            ..Default::default()
        }
    }

    #[test]
    fn special_panel_spreads_and_computes_risk() {
        let bets = vec![bet("特码", "01,02", 100.0, 2.0)];
        let odds = OddsConfig::default();
        let cols = panel(
            PanelList::Special,
            &bets,
            &draws(),
            &odds,
            None,
            2,
        );
        let col = &cols[0];
        assert_eq!(col.rows.len(), 49);
        assert!((col.total_stake - 100.0).abs() < 1e-6);
        let sum: f64 = col.rows.iter().map(|r| r.stake).sum();
        assert!((sum - 100.0).abs() < 1e-6);
        let r1 = col.rows.iter().find(|r| r.key == "01").unwrap();
        assert!((r1.stake - 50.0).abs() < 1e-6);
        assert!(!r1.hit); // 开奖特码是 07
        // base = 100 - 100*4% = 96；风险 = 96 - 50*47
        assert!((r1.risk - (96.0 - 50.0 * 47.0)).abs() < 1e-6);
        let empty = col.rows.iter().find(|r| r.key == "03").unwrap();
        assert!((empty.risk - 96.0).abs() < 1e-6);
    }

    #[test]
    fn lian_xiao_only_shows_selected_size() {
        let bets = vec![
            bet("二连肖", "鼠,牛", 100.0, 1.0),
            bet("三连肖", "鼠,牛,虎", 100.0, 1.0),
        ];
        let odds = OddsConfig::default();
        let cols = panel(PanelList::LianXiao, &bets, &draws(), &odds, None, 2);
        let col = &cols[0];
        assert_eq!(col.rows.len(), 1);
        assert_eq!(col.rows[0].key, "牛-鼠");
        // 区头合计仍按全部规格（两注 200）
        assert!((col.total_stake - 200.0).abs() < 1e-6);
    }

    #[test]
    fn flat_special_has_22_fixed_rows_plus_extras() {
        let bets = vec![bet("平特", "虎,0尾,01,02,03,04", 400.0, 3.0)];
        let odds = OddsConfig::default();
        let cols = panel(PanelList::FlatSpecial, &bets, &draws(), &odds, None, 2);
        let col = &cols[0];
        assert_eq!(col.rows.len(), 23);
        assert_eq!(col.rows[22].key, "01-02-03-04");
    }

    #[test]
    fn xiaoma_expands_combinations_and_counts_coverage() {
        let bets = vec![bet("三中三", "01,02,03,04", 400.0, 4.0)];
        let odds = OddsConfig::default();
        let rows = xiaoma(&bets, &draws(), &odds, None, "拖");
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert!((row.stake - 100.0).abs() < 1e-6);
            assert!((row.risk - 100.0 * 750.0).abs() < 1e-6);
            assert_eq!(row.label, "三中三");
        }
        let stats = play_stats(&bets, None, "拖");
        assert_eq!(stats[0].distinct, 4);
        assert_eq!(stats[0].total_bets, 4);
    }

    #[test]
    fn xiaoma_uses_selected_group_odds() {
        let bets = vec![bet("三中三", "01,02,03", 100.0, 1.0)];
        let mut odds = OddsConfig::default();
        odds.ensure_group("A组");
        let group = odds.groups.get_mut("A组").unwrap();
        *group[0].value_mut("three_in_three").unwrap() = 100.0;
        *group[0].value_mut("rw_three_in_three").unwrap() = 10.0;

        let rows = xiaoma(&bets, &draws(), &odds, Some("A组"), "拖");
        assert_eq!(rows.len(), 1);
        assert!((rows[0].risk - 100.0 * 100.0).abs() < 1e-6, "{:?}", rows[0].risk);
        assert!((rows[0].water - 10.0).abs() < 1e-6, "{:?}", rows[0].water);

        // 全部分组（None）仍走默认赔率。
        let default_rows = xiaoma(&bets, &draws(), &odds, None, "拖");
        assert!(
            (default_rows[0].risk - 100.0 * 750.0).abs() < 1e-6,
            "{:?}",
            default_rows[0].risk
        );
    }
}

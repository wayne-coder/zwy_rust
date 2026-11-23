//! 对奖：按当前开奖号码判定每一注明细是否中奖，并算出中奖注额 / 中奖金额 / 回水。
//!
//! 赔率与回水由 `odds::OddsSet` 传入（默认 / 分组 / 会员个性化在 odds 模块解析），
//! 默认值与参考项目 `zwy_wails_v1` 保持一致。

use std::collections::BTreeSet;

use crate::convert::BetItem;
use crate::draw::RegionDraw;
use crate::odds::OddsSet;

/// 一注明细的对奖结果。
#[derive(Clone, Debug, Default)]
pub struct Eval {
    pub hit: bool,
    /// 中奖的那部分本金（中奖注额）
    pub win_stake: f64,
    /// 中奖金额 = 中奖注额 × 赔率
    pub win_amount: f64,
    /// 这一注的回水（按总投注 × 回水比例）
    pub rebate: f64,
    pub odds: f64,
    /// 归集用的玩法标签
    pub label: String,
    /// 中奖明细拆分：同一注里可能有不同赔率（三中二中2/中3、连肖含马、
    /// 平特 4/5 码、二中二带砍）。为空时用 (label, win_stake, odds) 一条。
    pub parts: Vec<(String, f64, f64)>,
}

impl Eval {
    /// 追加一段中奖明细；相同标签 + 赔率合并本金。
    pub fn add_part(&mut self, label: &str, stake: f64, odds: f64) {
        if stake <= 0.0 {
            return;
        }
        let key100 = (odds * 100.0).round() as i64;
        if let Some(part) = self.parts.iter_mut().find(|(l, _, o)| {
            l == label && (*o * 100.0).round() as i64 == key100
        }) {
            part.1 += stake;
        } else {
            self.parts.push((label.to_string(), stake, odds));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat {
    Special,
    FlatNumber,
    FlatSpecial,
    Combo2,
    TwoAllHit,
    Combo3,
    ThreeInTwo,
    SpecialHit,
    LianXiao(usize),
    LianWei(usize),
    BuZhong(usize),
    ZhongYi(usize),
    LiuXiao,
}

fn category(play: &str) -> Option<Cat> {
    match play {
        "特码" => return Some(Cat::Special),
        "平码" => return Some(Cat::FlatNumber),
        "平特" => return Some(Cat::FlatSpecial),
        "二中二" => return Some(Cat::Combo2),
        "二全中" => return Some(Cat::TwoAllHit),
        "三中三" => return Some(Cat::Combo3),
        "三中二" => return Some(Cat::ThreeInTwo),
        "特碰" => return Some(Cat::SpecialHit),
        "六肖中一" => return Some(Cat::LiuXiao),
        _ => {}
    }
    let size = |c: char| match c {
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
    };
    let head_size = |suffix: &str| -> Option<usize> {
        play.strip_suffix(suffix)?.chars().next().and_then(size)
    };
    if let Some(n) = head_size("连肖") {
        return Some(Cat::LianXiao(n));
    }
    if let Some(n) = head_size("连尾") {
        return Some(Cat::LianWei(n));
    }
    if let Some(n) = head_size("不中") {
        return Some(Cat::BuZhong(n));
    }
    if let Some(n) = head_size("中一") {
        return Some(Cat::ZhongYi(n));
    }
    None
}

// ── 号码表 ────────────────────────────────────────────────────────────────

const ZODIACS: &[(&str, &[u32])] = &[
    ("马", &[1, 13, 25, 37, 49]),
    ("蛇", &[2, 14, 26, 38]),
    ("龙", &[3, 15, 27, 39]),
    ("兔", &[4, 16, 28, 40]),
    ("虎", &[5, 17, 29, 41]),
    ("牛", &[6, 18, 30, 42]),
    ("鼠", &[7, 19, 31, 43]),
    ("猪", &[8, 20, 32, 44]),
    ("狗", &[9, 21, 33, 45]),
    ("鸡", &[10, 22, 34, 46]),
    ("猴", &[11, 23, 35, 47]),
    ("羊", &[12, 24, 36, 48]),
];

pub fn zodiac_of(n: u32) -> Option<&'static str> {
    ZODIACS
        .iter()
        .find(|(_, nums)| nums.contains(&n))
        .map(|(z, _)| *z)
}

pub(crate) fn tail_of(n: u32) -> u32 {
    n % 10
}

pub fn zodiac_numbers(name: &str) -> Option<&'static [u32]> {
    ZODIACS.iter().find(|(z, _)| *z == name).map(|(_, n)| *n)
}

fn head_numbers(n: u32) -> Vec<u32> {
    if n == 0 {
        (1..=9).collect()
    } else {
        let base = n * 10;
        (base..=base + 9).filter(|v| *v <= 49).collect()
    }
}

fn static_attr(kw: &str) -> Option<&'static [u32]> {
    const JIAQIN: &[u32] = &[1, 6, 8, 9, 10, 12, 13, 18, 20, 21, 22, 24, 25, 30, 32, 33, 34, 36, 37, 42, 44, 45, 46, 48, 49];
    const YESHOU: &[u32] = &[2, 3, 4, 5, 7, 11, 14, 15, 16, 17, 19, 23, 26, 27, 28, 29, 31, 35, 38, 39, 40, 41, 43, 47];
    const RED: &[u32] = &[1, 2, 7, 8, 12, 13, 18, 19, 23, 24, 29, 30, 34, 35, 40, 45, 46];
    const BLUE: &[u32] = &[3, 4, 9, 10, 14, 15, 20, 25, 26, 31, 36, 37, 41, 42, 47, 48];
    const GREEN: &[u32] = &[5, 6, 11, 16, 17, 21, 22, 27, 28, 32, 33, 38, 39, 43, 44, 49];
    const HE_DA: &[u32] = &[7, 8, 9, 16, 17, 18, 19, 25, 26, 27, 28, 29, 34, 35, 36, 37, 38, 39, 43, 44, 45, 46, 47, 48, 49];
    const HE_XIAO: &[u32] = &[1, 2, 3, 4, 5, 6, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 30, 31, 32, 33, 40, 41, 42];
    const DA_DAN: &[u32] = &[25, 27, 29, 31, 33, 35, 37, 39, 41, 43, 45, 47, 49];
    const XIAO_DAN: &[u32] = &[1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23];
    const DA_SHUANG: &[u32] = &[26, 28, 30, 32, 34, 36, 38, 40, 42, 44, 46, 48];
    const XIAO_SHUANG: &[u32] = &[2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24];
    const RED_DAN: &[u32] = &[1, 7, 13, 19, 23, 29, 35, 45];
    const BLUE_DAN: &[u32] = &[3, 9, 15, 25, 31, 37, 41, 47];
    const GREEN_DAN: &[u32] = &[5, 11, 17, 21, 27, 33, 39, 43, 49];
    const RED_SHUANG: &[u32] = &[2, 8, 12, 18, 24, 30, 34, 40, 46];
    const BLUE_SHUANG: &[u32] = &[4, 10, 14, 20, 26, 36, 42, 48];
    const GREEN_SHUANG: &[u32] = &[6, 16, 22, 28, 32, 38, 44];
    const RED_DA: &[u32] = &[29, 30, 34, 35, 40, 45, 46];
    const BLUE_DA: &[u32] = &[25, 26, 31, 36, 37, 41, 42, 47, 48];
    const GREEN_DA: &[u32] = &[27, 28, 32, 33, 38, 39, 43, 44, 49];
    const RED_XIAO: &[u32] = &[1, 2, 7, 8, 12, 13, 18, 19, 23, 24];
    const BLUE_XIAO: &[u32] = &[3, 4, 9, 10, 14, 15, 20];
    const GREEN_XIAO: &[u32] = &[5, 6, 11, 16, 17, 21, 22];
    Some(match kw {
        "家禽" => JIAQIN,
        "野兽" => YESHOU,
        "红波" => RED,
        "蓝波" => BLUE,
        "绿波" => GREEN,
        "合大" => HE_DA,
        "合小" => HE_XIAO,
        "大单" => DA_DAN,
        "小单" => XIAO_DAN,
        "大双" => DA_SHUANG,
        "小双" => XIAO_SHUANG,
        "红单" => RED_DAN,
        "蓝单" => BLUE_DAN,
        "绿单" => GREEN_DAN,
        "红双" => RED_SHUANG,
        "蓝双" => BLUE_SHUANG,
        "绿双" => GREEN_SHUANG,
        "红大" => RED_DA,
        "蓝大" => BLUE_DA,
        "绿大" => GREEN_DA,
        "红小" => RED_XIAO,
        "蓝小" => BLUE_XIAO,
        "绿小" => GREEN_XIAO,
        _ => return None,
    })
}

pub(crate) fn attr_numbers(kw: &str) -> Option<Vec<u32>> {
    match kw {
        "大" => return Some((25..=49).collect()),
        "小" => return Some((1..=24).collect()),
        "单" => return Some((1..=49).step_by(2).collect()),
        "双" => return Some((2..=48).step_by(2).collect()),
        _ => {}
    }
    static_attr(kw).map(|v| v.to_vec())
}

/// 内容里的一个词展开成号码集合。
pub fn token_numbers(tok: &str) -> Vec<u32> {
    if tok.is_empty() {
        return Vec::new();
    }
    if tok.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = tok.parse::<u32>() {
            return if (1..=49).contains(&n) { vec![n] } else { Vec::new() };
        }
        return Vec::new();
    }
    if let Some(nums) = zodiac_numbers(tok) {
        return nums.to_vec();
    }
    if let Some(d) = tok.strip_suffix('尾').and_then(|s| s.parse::<u32>().ok()) {
        return (1..=49).filter(|n| tail_of(*n) == d).collect();
    }
    if let Some(d) = tok.strip_suffix('头').and_then(|s| s.parse::<u32>().ok()) {
        return head_numbers(d);
    }
    attr_numbers(tok).unwrap_or_default()
}

/// 枚举 n 个元素里所有 k 元组合（索引），每个组合回调一次。
fn for_each_combination(n: usize, k: usize, f: &mut impl FnMut(&[usize])) {
    if k == 0 || k > n {
        return;
    }
    fn walk(
        n: usize,
        k: usize,
        start: usize,
        cur: &mut Vec<usize>,
        f: &mut impl FnMut(&[usize]),
    ) {
        if cur.len() == k {
            f(cur);
            return;
        }
        let need = k - cur.len();
        for i in start..=n - need {
            cur.push(i);
            walk(n, k, i + 1, cur, f);
            cur.pop();
        }
    }
    let mut cur = Vec::with_capacity(k);
    walk(n, k, 0, &mut cur, f);
}

/// 解析拖注里的一个分组：去掉括号，按逗号拆成若干 token，全部展开成号码。
pub fn group_numbers(group: &str) -> Vec<u32> {
    let group = group
        .trim()
        .trim_matches(['(', ')', '（', '）', '「', '」', '[', ']', '【', '】']);
    let mut out: Vec<u32> = Vec::new();
    for tok in group.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        for n in token_numbers(tok) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// 拖注互碰的实际组合数：每组各出一个号，同一组合里号码重复的跳过。
pub fn distinct_drag_count(groups: &[Vec<u32>]) -> f64 {
    if groups.is_empty() {
        return 0.0;
    }
    fn walk(groups: &[Vec<u32>], index: usize, used: &mut Vec<u32>) -> f64 {
        if index == groups.len() {
            return 1.0;
        }
        let mut count = 0.0;
        for &n in &groups[index] {
            if used.contains(&n) {
                continue;
            }
            used.push(n);
            count += walk(groups, index + 1, used);
            used.pop();
        }
        count
    }
    let mut used = Vec::new();
    walk(groups, 0, &mut used)
}

/// 枚举拖注的所有实际组合（每组各出一个号，跳过组间重复号码）。
fn for_each_drag_combo(groups: &[Vec<u32>], f: &mut impl FnMut(&[u32])) {
    fn walk(
        groups: &[Vec<u32>],
        index: usize,
        used: &mut Vec<u32>,
        f: &mut impl FnMut(&[u32]),
    ) {
        if index == groups.len() {
            f(used);
            return;
        }
        for &n in &groups[index] {
            if used.contains(&n) {
                continue;
            }
            used.push(n);
            walk(groups, index + 1, used, f);
            used.pop();
        }
    }
    let mut used = Vec::new();
    walk(groups, 0, &mut used, f);
}

/// 特碰 / 二中二带砍：一对号码里恰有一个特码、另一个是平码。
fn drag_special_pairs(
    groups: &[Vec<u32>],
    special: u32,
    draw6: &BTreeSet<u32>,
) -> f64 {
    let mut units = 0.0;
    for_each_drag_combo(groups, &mut |path| {
        if path.len() == 2
            && path.contains(&special)
            && path
                .iter()
                .any(|n| *n != special && draw6.contains(n))
        {
            units += 1.0;
        }
    });
    units
}

/// 拖注的命中分布（下标 = 命中个数），跳过组间重复号码。
fn distinct_hit_distribution(
    groups: &[Vec<u32>],
    draw: &BTreeSet<u32>,
) -> Vec<f64> {
    let mut dist = vec![0.0f64; groups.len() + 1];
    fn walk(
        groups: &[Vec<u32>],
        draw: &BTreeSet<u32>,
        index: usize,
        used: &mut Vec<u32>,
        hits: usize,
        dist: &mut [f64],
    ) {
        if index == groups.len() {
            dist[hits] += 1.0;
            return;
        }
        for &n in &groups[index] {
            if used.contains(&n) {
                continue;
            }
            used.push(n);
            walk(
                groups,
                draw,
                index + 1,
                used,
                hits + usize::from(draw.contains(&n)),
                dist,
            );
            used.pop();
        }
    }
    let mut used = Vec::new();
    walk(groups, draw, 0, &mut used, 0, &mut dist);
    dist
}

fn comb(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut out = 1.0f64;
    for i in 0..k {
        out *= (n - i) as f64 / (i + 1) as f64;
    }
    out.round()
}

/// 平特的分组：每个生肖 / 尾数各算一组；裸数字归为一组 4/5 码组。
fn flat_special_groups(content: &str) -> Vec<Vec<u32>> {
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut direct: Vec<u32> = Vec::new();
    for tok in pick_tokens(content) {
        if let Some(nums) = zodiac_numbers(tok) {
            groups.push(nums.to_vec());
            continue;
        }
        if let Some(d) = tok.strip_suffix('尾').and_then(|s| s.parse::<u32>().ok()) {
            groups.push((1..=49).filter(|n| tail_of(*n) == d).collect());
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
        if let Some(nums) = attr_numbers(tok) {
            groups.push(nums);
        }
    }
    if !direct.is_empty() {
        groups.push(direct);
    }
    groups
}

pub(crate) fn pick_tokens(content: &str) -> Vec<&str> {
    content
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// 十二生肖的固定顺序（与号码表一致），平特面板的 22 个固定格按它排。
pub fn zodiac_names() -> Vec<&'static str> {
    ZODIACS.iter().map(|(z, _)| *z).collect()
}

/// 号码的波色名（'红波' / '蓝波' / '绿波'），查不到返回 None。
pub fn wave_of(n: u32) -> Option<&'static str> {
    for (name, kw) in [("红波", "红波"), ("蓝波", "蓝波"), ("绿波", "绿波")] {
        if static_attr(kw).is_some_and(|nums| nums.contains(&n)) {
            return Some(name);
        }
    }
    None
}

fn lian_zodiacs(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for tok in pick_tokens(content) {
        if let Some(nums) = zodiac_numbers(tok) {
            let _ = nums;
            out.insert(tok.to_string());
        } else if tok.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = tok.parse::<u32>() {
                if let Some(z) = zodiac_of(n) {
                    out.insert(z.to_string());
                }
            }
        }
    }
    out
}

fn lian_tails(content: &str) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for tok in pick_tokens(content) {
        if let Some(d) = tok.strip_suffix('尾').and_then(|s| s.parse::<u32>().ok()) {
            out.insert(d);
        } else if tok.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = tok.parse::<u32>() {
                out.insert(tail_of(n));
            }
        }
    }
    out
}

fn rebate_rate(cat: Cat, covered: usize, odds: &OddsSet) -> f64 {
    match cat {
        Cat::Special => odds.value("rw_special_number"),
        Cat::FlatNumber => odds.value("rw_flat_number"),
        Cat::FlatSpecial => {
            if covered == 5 {
                odds.value("rw_flat_special_5")
            } else {
                odds.value("rw_flat_special_4")
            }
        }
        Cat::TwoAllHit => odds.value("rw_two_all_hit"),
        Cat::Combo2 => odds.value("rw_two_in_two"),
        Cat::Combo3 => odds.value("rw_three_in_three"),
        Cat::ThreeInTwo => odds.value("rw_three_in_two"),
        Cat::SpecialHit => odds.value("rw_special_hit"),
        Cat::LianXiao(_) => odds.value("rw_lianxiao"),
        Cat::LianWei(_) => odds.value("rw_lianwei"),
        Cat::BuZhong(_) => odds.value("rw_buzhong"),
        Cat::ZhongYi(n) => match n {
            4 => odds.value("rw_zhongyi_4"),
            5 => odds.value("rw_zhongyi_5"),
            _ => odds.value("rw_zhongyi"),
        },
        Cat::LiuXiao => odds.value("rw_liuxiao"),
    }
}

/// 对一条明细做对奖。
pub fn evaluate(bet: &BetItem, draw: &RegionDraw, mul_sep: &str, odds: &OddsSet) -> Eval {
    let Some(cat) = category(&bet.play) else {
        return Eval::default();
    };
    let amount = bet.amount.max(0.0);
    let bets = bet.bets.max(1.0);
    let per_bet = amount / bets;
    let mut ev = Eval {
        label: bet.play.clone(),
        ..Default::default()
    };
    if !draw.ready() || amount <= 0.0 {
        return ev;
    }
    let draw6: BTreeSet<u32> = draw.draw.iter().copied().collect();
    let all7: BTreeSet<u32> = draw
        .draw
        .iter()
        .copied()
        .chain(draw.special)
        .collect();
    let special = draw.special.unwrap_or(0);

    let tokens = pick_tokens(&bet.content);
    let mut covered: Vec<u32> = {
        let mut set = BTreeSet::new();
        for t in &tokens {
            set.extend(token_numbers(t));
        }
        set.into_iter().collect()
    };
    // 拖注的号码写在括号分组里，不能只按逗号切。
    if bet.content.contains(mul_sep) {
        for g in bet.content.split(mul_sep) {
            covered.extend(group_numbers(g));
        }
        covered.sort_unstable();
        covered.dedup();
    }
    let covered_set: BTreeSet<u32> = covered.iter().copied().collect();
    ev.rebate = amount * rebate_rate(cat, covered.len(), odds) / 100.0;

    match cat {
        Cat::Special => {
            if covered_set.contains(&special) {
                if bet.attr_total {
                    let multiplier = if special == 49 {
                        odds.value("special_attr_49")
                    } else {
                        odds.value("special_attr")
                    };
                    ev.hit = true;
                    ev.odds = multiplier;
                    ev.win_stake = amount;
                    ev.win_amount = amount * multiplier;
                } else {
                    let multiplier = odds.value("special_number");
                    ev.hit = true;
                    ev.odds = multiplier;
                    ev.win_stake = per_bet;
                    ev.win_amount = per_bet * multiplier;
                }
            }
        }
        Cat::FlatNumber => {
            let hits = covered_set.intersection(&draw6).count();
            if hits > 0 {
                let multiplier = odds.value("flat_number");
                ev.hit = true;
                ev.odds = multiplier;
                ev.win_stake = per_bet * hits as f64;
                ev.win_amount = ev.win_stake * multiplier;
            }
        }
        Cat::FlatSpecial => {
            // 平特按「组」结算：每个生肖 / 尾数 / 4-5 个号码组各一注。
            // 中奖判定看开奖的 7 个号（6 平码 + 特码）；一组里任意一个号开出即中。
            let groups = flat_special_groups(&bet.content);
            let groups = if groups.is_empty() {
                vec![covered.clone()]
            } else {
                groups
            };
            let mut win_stake = 0.0;
            let mut win_amount = 0.0;
            let mut rebate = 0.0;
            for group in &groups {
                let size = group.len();
                let (multiplier, rw) = if size == 5 {
                    (
                        odds.value("flat_special_5"),
                        odds.value("rw_flat_special_5"),
                    )
                } else {
                    (
                        odds.value("flat_special_4"),
                        odds.value("rw_flat_special_4"),
                    )
                };
                rebate += per_bet * rw / 100.0;
                if group.iter().any(|n| all7.contains(n)) {
                    win_stake += per_bet;
                    win_amount += per_bet * multiplier;
                    ev.add_part(&bet.play, per_bet, multiplier);
                }
            }
            ev.rebate = rebate;
            if win_stake > 0.0 {
                ev.hit = true;
                ev.win_stake = win_stake.min(amount);
                ev.win_amount = win_amount;
                ev.odds = win_amount / win_stake;
            }
        }
        Cat::LianXiao(n) => {
            let picks = lian_zodiacs(&bet.content);
            let winning: BTreeSet<String> = all7
                .iter()
                .filter_map(|num| zodiac_of(*num))
                .map(str::to_string)
                .collect();
            // 每个「N 肖组合」独立结算：组合里每个生肖都开出来才中；
            // 含「马」的组合走马档赔率，不含的走普通档。
            let hit_zodiacs: Vec<String> = picks
                .iter()
                .filter(|z| winning.contains(*z))
                .cloned()
                .collect();
            if hit_zodiacs.len() >= n {
                let mut stake = 0.0;
                let mut win_amount = 0.0;
                for_each_combination(hit_zodiacs.len(), n, &mut |idx| {
                    let has_ma = idx.iter().any(|i| hit_zodiacs[*i] == "马");
                    let multiplier = lianxiao_odds(n, has_ma, odds);
                    stake += per_bet;
                    win_amount += per_bet * multiplier;
                    ev.add_part(&bet.play, per_bet, multiplier);
                });
                if stake > 0.0 {
                    ev.hit = true;
                    ev.win_stake = stake.min(amount);
                    ev.win_amount = win_amount;
                    ev.odds = win_amount / stake;
                }
            }
        }
        Cat::LianWei(n) => {
            let picks = lian_tails(&bet.content);
            let winning: BTreeSet<u32> = all7.iter().map(|num| tail_of(*num)).collect();
            // 与连肖同形；含 0尾 的组合走 0尾档。
            let hit_tails: Vec<u32> = picks
                .iter()
                .filter(|t| winning.contains(*t))
                .copied()
                .collect();
            if hit_tails.len() >= n {
                let mut stake = 0.0;
                let mut win_amount = 0.0;
                for_each_combination(hit_tails.len(), n, &mut |idx| {
                    let has_zero = idx.iter().any(|i| hit_tails[*i] == 0);
                    let multiplier = lianwei_odds(n, has_zero, odds);
                    stake += per_bet;
                    win_amount += per_bet * multiplier;
                    ev.add_part(&bet.play, per_bet, multiplier);
                });
                if stake > 0.0 {
                    ev.hit = true;
                    ev.win_stake = stake.min(amount);
                    ev.win_amount = win_amount;
                    ev.odds = win_amount / stake;
                }
            }
        }
        Cat::BuZhong(n) => {
            let hits = covered_set.intersection(&all7).count();
            let misses = covered.len().saturating_sub(hits);
            let units = comb(misses, n);
            if units > 0.0 && covered.len() >= n {
                ev.hit = true;
                ev.odds = buzhong_odds(n, odds);
                ev.win_stake = (units * per_bet).min(amount);
                ev.win_amount = units * per_bet * ev.odds;
            }
        }
        Cat::ZhongYi(n) => {
            let hits = covered_set.intersection(&all7).count();
            let total_units = bets;
            let zero_units = comb(covered.len().saturating_sub(hits), n);
            let units = if n >= 4 {
                if hits > 0 {
                    1.0
                } else {
                    0.0
                }
            } else {
                (total_units - zero_units).max(0.0)
            };
            if units > 0.0 {
                ev.hit = true;
                ev.odds = zhongyi_odds(n, odds);
                ev.win_stake = (units * per_bet).min(amount);
                ev.win_amount = units * per_bet * ev.odds;
            }
        }
        Cat::LiuXiao => {
            let picks = lian_zodiacs(&bet.content);
            if let Some(z) = zodiac_of(special) {
                if picks.contains(z) {
                    let multiplier = if special == 49 {
                        odds.value("liuxiao_zhongyi_49")
                    } else {
                        odds.value("liuxiao_zhongyi")
                    };
                    ev.hit = true;
                    ev.odds = multiplier;
                    ev.win_stake = amount;
                    ev.win_amount = amount * multiplier;
                }
            }
        }
        Cat::Combo2 | Cat::TwoAllHit | Cat::Combo3 | Cat::ThreeInTwo | Cat::SpecialHit => {
            let size = match cat {
                Cat::Combo3 | Cat::ThreeInTwo => 3,
                _ => 2,
            };
            // 拖注：每个分组各出一个号，按分组算命中分布。
            // 普通写法：从全部号码里选 size 个，中奖组合数 = C(命中, h)·C(没中, size-h)。
            // 只有「分组数 = 玩法档位」才是互碰；分组数对不上时按复式号码池结算，
            // 与参考项目 expandComboNumbers 的兜底一致。
            let drag_groups: Option<Vec<Vec<u32>>> = if bet.content.contains(mul_sep) {
                Some(
                    bet.content
                        .split(mul_sep)
                        .map(group_numbers)
                        .collect(),
                )
            } else {
                None
            };
            let exact_drag = drag_groups
                .as_ref()
                .filter(|groups| groups.len() == size);
            let drag_dist: Option<Vec<f64>> =
                exact_drag.map(|groups| distinct_hit_distribution(groups, &draw6));
            let n_cover = covered.len();
            let hit_cover = covered_set.intersection(&draw6).count();
            let unit = |h: usize| -> f64 {
                match &drag_dist {
                    Some(dist) => dist.get(h).copied().unwrap_or(0.0),
                    None => {
                        if h > hit_cover || h > size {
                            0.0
                        } else {
                            comb(hit_cover, h)
                                * comb(n_cover.saturating_sub(hit_cover), size - h)
                        }
                    }
                }
            };
            match cat {
                Cat::Combo2 | Cat::TwoAllHit => {
                    let units = unit(size);
                    // 二中二带砍：一对号码里一个特码 + 一个平码。二全中永不带砍。
                    let dai_units = if cat == Cat::Combo2 {
                        if let Some(groups) = exact_drag {
                            drag_special_pairs(groups, special, &draw6)
                        } else if covered_set.contains(&special) {
                            covered_set.intersection(&draw6).count() as f64
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };
                    if units + dai_units > 0.0 {
                        let normal_multiplier = if cat == Cat::TwoAllHit {
                            odds.value("two_all_hit")
                        } else {
                            odds.value("two_in_two")
                        };
                        let dai_multiplier = odds.value("two_in_two_dai_kan");
                        ev.hit = true;
                        ev.win_stake = ((units + dai_units) * per_bet).min(amount);
                        ev.win_amount = units * per_bet * normal_multiplier
                            + dai_units * per_bet * dai_multiplier;
                        ev.odds = if ev.win_stake > 0.0 {
                            ev.win_amount / ev.win_stake
                        } else {
                            0.0
                        };
                        if units > 0.0 {
                            ev.add_part(&bet.play, units * per_bet, normal_multiplier);
                        }
                        if dai_units > 0.0 {
                            ev.add_part(
                                "二中二带砍",
                                dai_units * per_bet,
                                dai_multiplier,
                            );
                        }
                    }
                }
                Cat::Combo3 => {
                    let units = unit(size);
                    if units > 0.0 {
                        let multiplier = odds.value("three_in_three");
                        ev.hit = true;
                        ev.odds = multiplier;
                        ev.win_stake = (units * per_bet).min(amount);
                        ev.win_amount = units * per_bet * multiplier;
                    }
                }
                Cat::ThreeInTwo => {
                    let liang = unit(2);
                    let sa = unit(3);
                    if liang + sa > 0.0 {
                        let m_liang = odds.value("three_in_two_liang");
                        let m_sa = odds.value("three_in_two_sa");
                        ev.hit = true;
                        ev.win_stake = ((liang + sa) * per_bet).min(amount);
                        ev.win_amount = liang * per_bet * m_liang + sa * per_bet * m_sa;
                        ev.odds = if ev.win_stake > 0.0 {
                            ev.win_amount / ev.win_stake
                        } else {
                            0.0
                        };
                        if liang > 0.0 {
                            ev.add_part(&bet.play, liang * per_bet, m_liang);
                        }
                        if sa > 0.0 {
                            ev.add_part(&bet.play, sa * per_bet, m_sa);
                        }
                    }
                }
                Cat::SpecialHit => {
                    let units = if let Some(groups) = exact_drag {
                        drag_special_pairs(groups, special, &draw6)
                    } else if covered_set.contains(&special) {
                        covered_set.intersection(&draw6).count() as f64
                    } else {
                        0.0
                    };
                    if units > 0.0 {
                        let multiplier = odds.value("special_hit");
                        ev.hit = true;
                        ev.odds = multiplier;
                        ev.win_stake = (units * per_bet).min(amount);
                        ev.win_amount = units * per_bet * multiplier;
                    }
                }
                _ => {}
            }
        }
    }
    ev
}

fn lianxiao_odds(n: usize, has_ma: bool, odds: &OddsSet) -> f64 {
    let key = match (n, has_ma) {
        (2, false) => "lianxiao_2",
        (3, false) => "lianxiao_3",
        (4, false) => "lianxiao_4",
        (5, false) => "lianxiao_5",
        (2, true) => "lianxiao_ma_2",
        (3, true) => "lianxiao_ma_3",
        (4, true) => "lianxiao_ma_4",
        (5, true) => "lianxiao_ma_5",
        _ => return 0.0,
    };
    odds.value(key)
}

fn lianwei_odds(n: usize, has_zero: bool, odds: &OddsSet) -> f64 {
    let key = match (n, has_zero) {
        (2, false) => "lianwei_2",
        (3, false) => "lianwei_3",
        (4, false) => "lianwei_4",
        (2, true) => "lianwei_zero_2",
        (3, true) => "lianwei_zero_3",
        (4, true) => "lianwei_zero_4",
        _ => return 0.0,
    };
    odds.value(key)
}

fn buzhong_odds(n: usize, odds: &OddsSet) -> f64 {
    let key = match n {
        5 => "buzhong_5",
        6 => "buzhong_6",
        7 => "buzhong_7",
        8 => "buzhong_8",
        9 => "buzhong_9",
        10 => "buzhong_10",
        _ => return 0.0,
    };
    odds.value(key)
}

fn zhongyi_odds(n: usize, odds: &OddsSet) -> f64 {
    let key = match n {
        3 => "zhongyi_3",
        4 => "zhongyi_4",
        5 => "zhongyi_5",
        _ => return 0.0,
    };
    odds.value(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::BetItem;

    fn bet(play: &str, content: &str, amount: f64, bets: f64) -> BetItem {
        BetItem {
            play: play.to_string(),
            content: content.to_string(),
            amount,
            bets,
            stake: "各组 10".to_string(),
            ..Default::default()
        }
    }

    fn odds() -> OddsSet {
        OddsSet::default()
    }

    fn draw(draw: &[u32], special: u32) -> RegionDraw {
        RegionDraw {
            draw: draw.to_vec(),
            special: Some(special),
        }
    }

    #[test]
    fn special_number_hits_only_on_special() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let hit = evaluate(&bet("特码", "07", 100.0, 1.0), &d, "拖", &odds());
        assert!(hit.hit);
        assert_eq!(hit.win_amount, 4700.0);
        let miss = evaluate(&bet("特码", "01", 100.0, 1.0), &d, "拖", &odds());
        assert!(!miss.hit);
    }

    #[test]
    fn combos_count_winning_units() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        // 二中二：01,02,03,04 共 6 注，开 4 个 → C(4,2)=6 注全中。
        let e = evaluate(&bet("二中二", "01,02,03,04", 60.0, 6.0), &d, "拖", &odds());
        assert!(e.hit);
        assert_eq!(e.win_amount, 6.0 * 10.0 * 66.0);
        // 三中三：01..05 共 C(5,3)=10 注；开 01,02,03,04,06,07 只中 4 个
        // → C(4,3)=4 注中。
        let d = draw(&[1, 2, 3, 4, 6, 7], 8);
        let e = evaluate(&bet("三中三", "01,02,03,04,05", 100.0, 10.0), &d, "拖", &odds());
        assert_eq!(e.win_amount, 4.0 * 10.0 * 750.0);
    }

    #[test]
    fn lian_xiao_requires_all_zodiacs() {
        let d = draw(&[1, 13, 2, 14, 3, 15], 4);
        // 1 马、13 马、2 蛇、14 蛇 → 二连肖 马蛇 中。
        let e = evaluate(&bet("二连肖", "马,蛇", 10.0, 1.0), &d, "拖", &odds());
        assert!(e.hit);
        // 含「马」走马的档位（4.0）。
        assert_eq!(e.win_amount, 10.0 * 4.0);
        // 三连肖 马蛇龙：龙（3/15/27/39）也开了 3 → 中。
        let e = evaluate(&bet("三连肖", "马,蛇,龙", 10.0, 1.0), &d, "拖", &odds());
        assert!(e.hit);
    }

    #[test]
    fn drag_combo_counts_group_hits() {
        let d = draw(&[10, 5, 1, 2, 3, 4], 7);
        // 0尾有 10，5尾有 5 → 两组各中一个，1 注中；共 4×5=20 注，每注 10。
        let b = bet("二中二", "(0尾)拖(5尾)", 200.0, 20.0);
        let e = evaluate(&b, &d, "拖", &odds());
        assert!(e.hit);
        assert_eq!(e.win_amount, 10.0 * 66.0);
    }

    #[test]
    fn custom_odds_and_rebate_are_used() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let mut o = OddsSet::default();
        if let Some(v) = o.value_mut("special_number") {
            *v = 99.0;
        }
        if let Some(v) = o.value_mut("rw_special_number") {
            *v = 10.0;
        }
        let e = evaluate(&bet("特码", "07", 100.0, 1.0), &d, "拖", &o);
        assert_eq!(e.win_amount, 9900.0);
        assert!((e.rebate - 10.0).abs() < 1e-9);
    }

    #[test]
    fn rebate_is_on_total_stake() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let e = evaluate(&bet("二中二", "01,02", 100.0, 1.0), &d, "拖", &odds());
        assert!((e.rebate - 15.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    use crate::convert::BetItem;

    fn bet(play: &str, content: &str, amount: f64, bets: f64) -> BetItem {
        BetItem {
            play: play.to_string(),
            content: content.to_string(),
            amount,
            bets,
            stake: "各组 10".to_string(),
            ..Default::default()
        }
    }

    fn draw(draw: &[u32], special: u32) -> RegionDraw {
        RegionDraw {
            draw: draw.to_vec(),
            special: Some(special),
        }
    }

    #[test]
    fn flat_special_hits_normal_draw_and_counts_each_group() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let o = OddsSet::default();
        // 01 开在平码里，不再要求必须是特码。
        let e = evaluate(&bet("平特", "01,02,03,04", 10.0, 1.0), &d, "拖", &o);
        assert!(e.hit);
        assert_eq!(e.win_stake, 10.0);
        assert_eq!(e.win_amount, 20.0);

        // 马（5 码，1.8）+ 蛇（4 码，2.0）都中：两注分别赔。
        let e = evaluate(&bet("平特", "马,蛇", 20.0, 2.0), &d, "拖", &o);
        assert!(e.hit);
        assert_eq!(e.win_stake, 20.0);
        assert!((e.win_amount - 38.0).abs() < 1e-9);
        // 回水：马 5 码 0% + 蛇 4 码 4%。
        assert!((e.rebate - 0.4).abs() < 1e-9);
        assert_eq!(e.parts.len(), 2);
    }

    #[test]
    fn lian_xiao_mixes_ma_and_normal_odds() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let e = evaluate(
            &bet("二连肖", "马,蛇,龙", 30.0, 3.0),
            &d,
            "拖",
            &OddsSet::default(),
        );
        assert!(e.hit);
        assert_eq!(e.win_stake, 30.0);
        // 马蛇 4.0 + 马龙 4.0 + 蛇龙 4.5，每注 10。
        assert!((e.win_amount - 125.0).abs() < 1e-9);
        assert!(e.parts.iter().any(|(_, s, o)| (*s - 20.0).abs() < 1e-9 && (*o - 4.0).abs() < 1e-9));
        assert!(e.parts.iter().any(|(_, s, o)| (*s - 10.0).abs() < 1e-9 && (*o - 4.5).abs() < 1e-9));
    }

    #[test]
    fn two_in_two_da_kan_settles_separately() {
        let d = draw(&[1, 2, 3, 4, 5, 6], 7);
        let mut o = OddsSet::default();
        if let Some(v) = o.value_mut("two_in_two_dai_kan") {
            *v = 100.0;
        }
        // 01/02 在平码，07 是特码：普通二中二 1 注 + 带砍 2 注。
        let e = evaluate(&bet("二中二", "01,02,07", 30.0, 3.0), &d, "拖", &o);
        assert!(e.hit);
        assert_eq!(e.win_stake, 30.0);
        assert!((e.win_amount - 2660.0).abs() < 1e-9);
        assert!(e.parts.iter().any(|(l, s, od)| {
            l == "二中二" && (*s - 10.0).abs() < 1e-9 && (*od - 66.0).abs() < 1e-9
        }));
        assert!(e.parts.iter().any(|(l, s, od)| {
            l == "二中二带砍" && (*s - 20.0).abs() < 1e-9 && (*od - 100.0).abs() < 1e-9
        }));
    }

    #[test]
    fn bu_zhong_accepts_tail_groups() {
        // 1尾 = 01,11,21,31,41 五个号；开奖号里一个都没有 → 五不中全中。
        let d = draw(&[2, 3, 4, 5, 6, 8], 9);
        let e = evaluate(
            &bet("五不中", "1尾", 10.0, 1.0),
            &d,
            "拖",
            &OddsSet::default(),
        );
        assert!(e.hit);
        assert_eq!(e.win_stake, 10.0);
        assert!((e.win_amount - 20.0).abs() < 1e-9);
    }
}

//! 「查重」：三中三注单按号码互相包含连成组，同一地区内，共享 5 个或以上
//! 号码的两行相连，连通分量即一组。判据与参考项目 `lib/duplicateCheck.ts`
//! 一致：含「拖」的行不参与，号码不足 5 个的行不参与。
//!
//! 号码都在 1..=49 之内，所以每行用一个 u64 位图表示，两行交集的个数
//! 就是 `(a & b).count_ones()`。

use std::collections::HashMap;

use crate::risk::{self, BetRow};
use crate::xiaoma_offsets::{row_key, OffsetMap};

/// 两行算「包含」的门槛：共享号码个数
const CONTAINMENT_MIN_SHARED: u32 = 5;

#[derive(Clone, Debug)]
pub struct DupEntry {
    pub region: usize,
    /// 这一行的总金额
    pub amount: f64,
    /// 每个组合的金额（总额 ÷ 注数）
    pub per: f64,
    /// 表达式原文（组里代表行显示它）
    pub expr: String,
    /// 这一行覆盖到的号码，升序
    pub numbers: Vec<u32>,
    mask: u64,
}

impl DupEntry {
    /// 这一行展开后的三中三组合数，C(n,3)。
    pub fn combo_count(&self) -> usize {
        let n = self.numbers.len();
        if n < 3 {
            0
        } else {
            n * (n - 1) * (n - 2) / 6
        }
    }
}

#[derive(Clone, Debug)]
pub struct DupGroup {
    /// 代表行的表达式：号码最多的那一行，号码数相同时取最先录入的
    pub key: String,
    /// 组内录入在全部 `DupEntry` 里的下标
    pub entries: Vec<usize>,
}

/// 把三中三注单转成查重条目。
pub fn build(bets: &[BetRow], mul_sep: &str) -> Vec<DupEntry> {
    let mut out = Vec::new();
    for b in bets {
        // 上家是抵扣方，不算实际下注覆盖。
        if b.upline {
            continue;
        }
        if b.bet.play != "三中三" {
            continue;
        }
        if !mul_sep.is_empty() && b.bet.content.contains(mul_sep) {
            continue;
        }
        let numbers: Vec<u32> = risk::covered_numbers(&b.bet.content)
            .into_iter()
            .filter(|n| (1..=49).contains(n))
            .collect();
        if numbers.len() < 5 {
            continue;
        }
        let mask = numbers.iter().fold(0u64, |m, &n| m | (1u64 << n));
        out.push(DupEntry {
            region: b.region,
            amount: b.bet.amount,
            per: b.bet.amount / b.bet.bets.max(1.0),
            expr: b.bet.content.clone(),
            numbers,
            mask,
        });
    }
    out
}

/// 某一地区的全部组（录入次数 1 的也算一组），按录入次数降序、表达式升序。
pub fn groups(entries: &[DupEntry], region: usize) -> Vec<DupGroup> {
    let idx: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].region == region)
        .collect();
    let n = idx.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in i + 1..n {
            let shared = (entries[idx[i]].mask & entries[idx[j]].mask).count_ones();
            if shared >= CONTAINMENT_MIN_SHARED {
                adj[i].push(j);
                adj[j].push(i);
            }
        }
    }

    let mut visited = vec![false; n];
    let mut out = Vec::new();
    for start in 0..n {
        if visited[start] {
            continue;
        }
        let mut comp = Vec::new();
        let mut stack = vec![start];
        while let Some(v) = stack.pop() {
            if visited[v] {
                continue;
            }
            visited[v] = true;
            comp.push(v);
            stack.extend(adj[v].iter().copied().filter(|&w| !visited[w]));
        }
        comp.sort_unstable();
        // comp 已升序，比较用严格大于，平手时保留最先的那一行
        let rep = comp.iter().copied().fold(comp[0], |best, c| {
            if entries[idx[c]].numbers.len() > entries[idx[best]].numbers.len() {
                c
            } else {
                best
            }
        });
        out.push(DupGroup {
            key: entries[idx[rep]].expr.clone(),
            entries: comp.iter().map(|&c| idx[c]).collect(),
        });
    }
    out.sort_by(|a, b| {
        b.entries
            .len()
            .cmp(&a.entries.len())
            .then_with(|| a.key.cmp(&b.key))
    });
    out
}

/// 一组录入各自承担的已减额。
///
/// 减额落在小码行上，而一行是所有覆盖它的录入合并出来的。所以行上的减额按
/// 各录入在该组合上的金额占比分摊：同号录两遍时两条各承担一半，加起来才等于
/// 这一行实际少掉的钱。只看 `group` 里的录入。
pub fn prorated_deducted(
    entries: &[DupEntry],
    group: &[usize],
    offsets: &OffsetMap,
) -> HashMap<usize, f64> {
    let mut out: HashMap<usize, f64> = HashMap::new();
    if offsets.is_empty() {
        return out;
    }
    let mut by_row: HashMap<String, Vec<(usize, f64)>> = HashMap::new();
    for &i in group {
        let entry = &entries[i];
        for combo in risk::combinations(&entry.numbers, 3) {
            let key = row_key(entry.region, "三中三", &risk::joined_numbers(&combo));
            if offsets.contains_key(&key) {
                by_row.entry(key).or_default().push((i, entry.per));
            }
        }
    }
    for (key, members) in by_row {
        let amount = offsets[&key];
        let denom: f64 = members.iter().map(|(_, per)| per).sum();
        if denom <= 0.0 {
            continue;
        }
        for (i, per) in members {
            *out.entry(i).or_insert(0.0) += amount * per / denom;
        }
    }
    out
}

//! 「合并组合」：把恰好构成某个号码集合 S 的全部 k 组合压成一行
//! `三中三 01-02-03-04-05 各-5`。移植自参考项目 `lib/mergeCombos.ts`，
//! 算法与输出文本格式都一致；输出的文本要能粘到参考项目的「组合减额」里执行。
//!
//! 合并是多轮的：每轮取剩余金额最多的组合当种子，尽量扩张号码集合 S，
//! 只有 S 的全部 C(|S|,k) 个组合都还有剩余金额时才算一组，并消耗掉这组里
//! 最低的那个剩余金额。剩下的金额留到下一轮。

use std::collections::{HashMap, HashSet};

use crate::risk::{combinations, joined_numbers};

/// 参与合并的一个组合。`total` 是这次要合并的金额（超出过滤值的部分）。
#[derive(Clone, Debug)]
pub struct MergeCombo {
    pub key: String,
    pub numbers: Vec<u32>,
    pub total: f64,
}

/// 一组合并结果：S 的全部 k 组合各被消耗了 `total / source_keys.len()`。
#[derive(Clone, Debug)]
pub struct MergeGroup {
    pub merged_key: String,
    pub source_keys: Vec<String>,
    pub total: f64,
}

#[derive(Clone, Debug, Default)]
pub struct MergeOutput {
    pub merged: Vec<MergeGroup>,
    /// 还剩金额、凑不成任何完整集合的组合
    pub unmerged: Vec<MergeCombo>,
}

/// 组合文本：号码升序、`-` 连接（与小码行的 key 一致）。
pub fn key_of(nums: &[u32]) -> String {
    let mut sorted = nums.to_vec();
    sorted.sort_unstable();
    joined_numbers(&sorted)
}

fn can_add(s: &[u32], n: u32, k: usize, active: &HashSet<String>) -> bool {
    if s.len() < k - 1 {
        return true;
    }
    combinations(s, k - 1).iter().all(|sub| {
        let mut c = sub.clone();
        c.push(n);
        active.contains(&key_of(&c))
    })
}

/// 合并主过程。顺序（种子次序、号码扩张次序）与参考实现一致，同样的输入得到同样的输出。
pub fn perform(combos: &[MergeCombo], k: usize) -> MergeOutput {
    if combos.is_empty() {
        return MergeOutput::default();
    }
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut by_key: HashMap<String, &MergeCombo> = HashMap::new();
    for c in combos {
        if seen.insert(c.key.clone()) {
            order.push(c.key.clone());
        }
        by_key.insert(c.key.clone(), c);
    }
    let mut remaining: HashMap<String, f64> =
        combos.iter().map(|c| (c.key.clone(), c.total)).collect();

    let mut num_list: Vec<u32> = Vec::new();
    for c in combos {
        for &n in &c.numbers {
            if !num_list.contains(&n) {
                num_list.push(n);
            }
        }
    }

    let mut merged: Vec<MergeGroup> = Vec::new();
    let mut changed = true;
    while changed {
        changed = false;
        let mut active: HashSet<String> = order
            .iter()
            .filter(|k| remaining.get(*k).copied().unwrap_or(0.0) > 0.0)
            .cloned()
            .collect();
        if active.len() < 2 {
            break;
        }
        let mut sorted: Vec<String> = order
            .iter()
            .filter(|k| active.contains(*k))
            .cloned()
            .collect();
        sorted.sort_by(|a, b| {
            let ra = remaining.get(a).copied().unwrap_or(0.0);
            let rb = remaining.get(b).copied().unwrap_or(0.0);
            rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
        });

        for seed_key in &sorted {
            if !active.contains(seed_key) {
                continue;
            }
            let Some(seed) = by_key.get(seed_key) else {
                continue;
            };
            let mut s: Vec<u32> = Vec::new();
            for &n in &seed.numbers {
                if !s.contains(&n) {
                    s.push(n);
                }
            }
            let mut expanded = true;
            while expanded {
                expanded = false;
                for &n in &num_list {
                    if s.contains(&n) || !can_add(&s, n, k, &active) {
                        continue;
                    }
                    let mut new_s = s.clone();
                    new_s.push(n);
                    let all_active = combinations(&new_s, k)
                        .iter()
                        .all(|c| active.contains(&key_of(c)));
                    if all_active {
                        s = new_s;
                        expanded = true;
                        break;
                    }
                }
            }

            let mut s_sorted = s.clone();
            s_sorted.sort_unstable();
            let keys: Vec<String> = combinations(&s_sorted, k)
                .iter()
                .map(|c| key_of(c))
                .collect();
            if keys.len() > 1 {
                let min_amount = keys
                    .iter()
                    .map(|key| remaining.get(key).copied().unwrap_or(0.0))
                    .fold(f64::INFINITY, f64::min);
                if min_amount > 0.0 {
                    for key in &keys {
                        let next = remaining.get(key).copied().unwrap_or(0.0) - min_amount;
                        remaining.insert(key.clone(), next);
                        if next <= 0.0 {
                            active.remove(key);
                        }
                    }
                    merged.push(MergeGroup {
                        merged_key: key_of(&s_sorted),
                        source_keys: keys.clone(),
                        total: min_amount * keys.len() as f64,
                    });
                    changed = true;
                }
            }
        }
    }

    let mut unmerged = Vec::new();
    for key in &order {
        let amount = remaining.get(key).copied().unwrap_or(0.0);
        if amount > 0.0 {
            if let Some(c) = by_key.get(key) {
                unmerged.push(MergeCombo {
                    key: key.clone(),
                    numbers: c.numbers.clone(),
                    total: amount,
                });
            }
        }
    }
    MergeOutput { merged, unmerged }
}

/// 复制用的文本：每行 `类型 组合 各-N`，与参考项目的 `mergeOutputText` 逐字一致。
pub struct MergeText {
    pub merged: String,
    pub unmerged: String,
    pub all: String,
}

pub fn text(out: &MergeOutput, label: &str) -> MergeText {
    let merged = out
        .merged
        .iter()
        .map(|g| {
            let each = (g.total / g.source_keys.len() as f64).round() as i64;
            format!("{label} {} 各-{each}", g.merged_key)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let unmerged = out
        .unmerged
        .iter()
        .map(|c| format!("{label} {} 各-{}", c.key, c.total.round() as i64))
        .collect::<Vec<_>>()
        .join("\n");
    let mut parts = Vec::new();
    if !merged.is_empty() {
        parts.push(format!("=== 已合并 ===\n{merged}"));
    }
    if !unmerged.is_empty() {
        parts.push(format!("=== 无法合并 ===\n{unmerged}"));
    }
    MergeText {
        merged,
        unmerged,
        all: parts.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(key: &str, nums: &[u32], total: f64) -> MergeCombo {
        MergeCombo {
            key: key.into(),
            numbers: nums.to_vec(),
            total,
        }
    }

    #[test]
    fn four_full_combos_merge_into_one_group() {
        let combos = vec![
            combo("01-02-03", &[1, 2, 3], 10.0),
            combo("01-02-04", &[1, 2, 4], 10.0),
            combo("01-03-04", &[1, 3, 4], 10.0),
            combo("02-03-04", &[2, 3, 4], 10.0),
        ];
        let out = perform(&combos, 3);
        assert_eq!(out.merged.len(), 1);
        assert_eq!(out.merged[0].merged_key, "01-02-03-04");
        assert_eq!(out.merged[0].total, 40.0);
        assert!(out.unmerged.is_empty());
        assert_eq!(text(&out, "三中三").merged, "三中三 01-02-03-04 各-10");
    }

    #[test]
    fn leftover_amount_stays_unmerged() {
        let combos = vec![
            combo("01-02-03", &[1, 2, 3], 10.0),
            combo("01-02-04", &[1, 2, 4], 10.0),
            combo("01-03-04", &[1, 3, 4], 10.0),
            combo("02-03-04", &[2, 3, 4], 20.0),
        ];
        let out = perform(&combos, 3);
        assert_eq!(out.merged.len(), 1);
        assert_eq!(out.unmerged.len(), 1);
        assert_eq!(out.unmerged[0].key, "02-03-04");
        assert_eq!(out.unmerged[0].total, 10.0);
    }
}

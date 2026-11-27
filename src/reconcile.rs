//! 对账：把本机出账单压成一段文本 + 每人一枚短码，对方发来同一份文本后逐人 / 逐批比对。
//!
//! 文本格式、码算法均对齐参考项目 zwy_wails_v1 的 lib/reconcile.ts（v1）。

use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 1;
/// 字段分隔符（文本里用竖线；码的规范串用 US 控制符，见 UNIT_CODE）。
const SEP: char = '|';
const UNIT_CODE: char = '\u{1f}';

fn r2(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

/// 规范金额：恒两位小数、无千分位、负零归一。
pub fn money(n: f64) -> String {
    let v = r2(if n.is_finite() { n } else { 0.0 });
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:.2}")
}

fn one_line_name(name: &str) -> String {
    let s = name.replace(['\r', '\n'], " ");
    let s = s.trim();
    if s.is_empty() {
        "未指定".to_string()
    } else {
        s.to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub no: usize,
    pub stake: f64,
    pub hk: f64,
    pub old: f64,
    pub win: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Bettor {
    pub name: String,
    pub batch_count: usize,
    pub total_stake: f64,
    pub total_win: f64,
    pub total_rebate: f64,
    pub total_net: f64,
    pub batches: Vec<Batch>,
    pub code: String,
}

impl Bettor {
    pub fn new(name: &str, batches: Vec<Batch>) -> Self {
        let mut b = Bettor {
            name: one_line_name(name),
            batch_count: batches.len(),
            batches,
            ..Default::default()
        };
        for r in &b.batches {
            b.total_stake += r.stake;
            b.total_win += r.win;
        }
        b.total_net = b.total_stake - b.total_win - b.total_rebate;
        b.code = bettor_code(&b);
        b
    }

    pub fn set_rebate(&mut self, rebate: f64) {
        self.total_rebate = r2(rebate);
        self.total_net = self.total_stake - self.total_win - self.total_rebate;
        self.code = bettor_code(self);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Draw {
    pub char: char,
    pub draw: Vec<u32>,
    pub special: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub date: String,
    pub draws: Vec<Draw>,
    pub bettors: Vec<Bettor>,
    pub total_code: String,
}

impl Model {
    pub fn new(date: String, draws: Vec<Draw>, bettors: Vec<Bettor>) -> Self {
        let total_code = total_code(&bettors);
        Model {
            date,
            draws,
            bettors,
            total_code,
        }
    }

    pub fn text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("#对账 v{}|{}", VERSION, self.date));
        lines.push(format!(
            "#开奖|{}",
            self.draws
                .iter()
                .map(draw_seg)
                .collect::<Vec<_>>()
                .join("|")
        ));
        let batch_total: usize = self.bettors.iter().map(|b| b.batch_count).sum();
        lines.push(format!(
            "#投注人|{} 位|{} 批",
            self.bettors.len(),
            batch_total
        ));
        for b in &self.bettors {
            lines.push(String::new());
            lines.push(
                [
                    b.name.clone(),
                    b.batch_count.to_string(),
                    money(b.total_stake),
                    money(b.total_win),
                    money(b.total_rebate),
                    money(b.total_net),
                    b.code.clone(),
                ]
                .join(&SEP.to_string()),
            );
            for r in &b.batches {
                lines.push(
                    [
                        r.no.to_string(),
                        money(r.stake),
                        money(r.hk),
                        money(r.old),
                        money(r.win),
                    ]
                    .join(&SEP.to_string()),
                );
            }
        }
        let (stake, win, rebate, net) = totals(&self.bettors);
        lines.push(String::new());
        lines.push(
            [
                "合计".to_string(),
                format!("{} 位", self.bettors.len()),
                format!("{} 批", batch_total),
                money(stake),
                money(win),
                money(rebate),
                money(net),
            ]
            .join(&SEP.to_string()),
        );
        lines.push(format!("#总码|{}", self.total_code));
        lines.join("\n") + "\n"
    }
}

fn draw_seg(d: &Draw) -> String {
    if d.draw.is_empty() && d.special.is_none() {
        return format!("{} 未录", d.char);
    }
    let nums = if d.draw.is_empty() {
        "平码未录".to_string()
    } else {
        d.draw
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "{} {} 特 {}",
        d.char,
        nums,
        d.special.map(|v| v.to_string()).unwrap_or_else(|| "未录".to_string())
    )
}

fn totals(bettors: &[Bettor]) -> (f64, f64, f64, f64) {
    let mut stake = 0.0;
    let mut win = 0.0;
    let mut rebate = 0.0;
    for b in bettors {
        stake += money(b.total_stake).parse::<f64>().unwrap_or(0.0);
        win += money(b.total_win).parse::<f64>().unwrap_or(0.0);
        rebate += money(b.total_rebate).parse::<f64>().unwrap_or(0.0);
    }
    let net = stake - win - rebate;
    (r2(stake), r2(win), r2(rebate), r2(net))
}

// ── 对账码 ────────────────────────────────────────────────────────────────

const B32: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn hash53(s: &str) -> f64 {
    let mut h1: u32 = 0xdead_beef;
    let mut h2: u32 = 0x41c6_ce57;
    for ch in s.encode_utf16() {
        let c = ch as u32;
        h1 = (h1 ^ c).wrapping_mul(2654435761);
        h2 = (h2 ^ c).wrapping_mul(1597334677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2246822507)
        ^ (h2 ^ (h2 >> 13)).wrapping_mul(3266489909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2246822507)
        ^ (h1 ^ (h1 >> 13)).wrapping_mul(3266489909);
    4294967296.0 * (h2 as f64 % 2097152.0) + h1 as f64
}

fn encode_code(h: f64) -> String {
    let mut x = h.abs().floor() as u64;
    let mut out = ['0'; 10];
    for i in (0..10).rev() {
        out[i] = B32[(x % 32) as usize] as char;
        x /= 32;
    }
    let s: String = out.iter().collect();
    format!("{}-{}", &s[..5], &s[5..])
}

fn canonical_batch(r: &Batch) -> String {
    format!(
        "{}{}{}{}{}{}{}{}{}",
        r.no,
        UNIT_CODE,
        money(r.stake),
        UNIT_CODE,
        money(r.hk),
        UNIT_CODE,
        money(r.old),
        UNIT_CODE,
        money(r.win)
    )
}

fn canonical_bettor(b: &Bettor) -> String {
    let name = one_line_name(&b.name);
    let mut parts: Vec<String> = vec![
        format!("v{VERSION}"),
        format!("name:{}:{name}", name.encode_utf16().count()),
        format!("n:{}", b.batch_count),
        format!("s:{}", money(b.total_stake)),
        format!("w:{}", money(b.total_win)),
        format!("r:{}", money(b.total_rebate)),
        format!("p:{}", money(b.total_net)),
    ];
    for r in &b.batches {
        parts.push(format!("b:{}", canonical_batch(r)));
    }
    parts.join(&UNIT_CODE.to_string())
}

pub fn bettor_code(b: &Bettor) -> String {
    encode_code(hash53(&canonical_bettor(b)))
}

fn cmp_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn total_code(bettors: &[Bettor]) -> String {
    let mut sorted: Vec<&Bettor> = bettors.iter().collect();
    sorted.sort_by(|a, b| cmp_utf16(&one_line_name(&a.name), &one_line_name(&b.name)));
    let mut parts: Vec<String> = vec![
        format!("v{VERSION}"),
        format!("cnt:{}", sorted.len()),
    ];
    for b in sorted {
        let name = one_line_name(&b.name);
        parts.push(format!(
            "e:{}:{name}:{}",
            name.encode_utf16().count(),
            b.code
        ));
    }
    encode_code(hash53(&parts.join(&UNIT_CODE.to_string())))
}

// ── 解析 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct ParseResult {
    pub ok: bool,
    pub model: Option<Model>,
    pub problems: Vec<String>,
    /// 文本里的码与内容重算不一致（被手改过 / 复制漏段）。
    pub self_inconsistent: bool,
}

pub fn parse(raw: &str) -> ParseResult {
    let mut problems: Vec<String> = Vec::new();
    let text = raw
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('｜', "|");
    let lines: Vec<&str> = text.split('\n').collect();

    let mut head = None;
    for (i, line) in lines.iter().take(20).enumerate() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("#") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix("对账") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('v') {
                    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(v) = digits.parse::<u32>() {
                        let date = rest[digits.len()..]
                            .trim_start_matches('|')
                            .trim()
                            .to_string();
                        head = Some((i, v, date));
                        break;
                    }
                }
            }
        }
    }
    let Some((head_idx, version, date)) = head else {
        problems.push("没找到「#对账 v1」这一行，这不像是本软件的对账文本。".into());
        return ParseResult {
            ok: false,
            model: None,
            problems,
            self_inconsistent: false,
        };
    };
    if version != VERSION {
        problems.push(format!(
            "对方软件版本不同（对方对账口径 v{version}，本机只认 v{VERSION}）。请把两台电脑升到同一版本后再对账。"
        ));
        return ParseResult {
            ok: false,
            model: None,
            problems,
            self_inconsistent: false,
        };
    }
    if head_idx > 0 {
        problems.push(format!("开头有 {head_idx} 行无法识别的内容，已跳过。"));
    }

    let mut bettors: Vec<Bettor> = Vec::new();
    let mut total_code_text = String::new();
    let mut draws: Vec<Draw> = Vec::new();

    for (idx, raw_line) in lines.iter().enumerate().skip(head_idx + 1) {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#开奖|") {
            for seg in rest.split('|') {
                let mut it = seg.split_whitespace();
                let Some(ch) = it.next().and_then(|s| s.chars().next()) else {
                    continue;
                };
                let rest: Vec<&str> = it.collect();
                let mut draw = Vec::new();
                let mut special = None;
                let mut seen_te = false;
                for tok in rest {
                    if tok == "特" {
                        seen_te = true;
                        continue;
                    }
                    if let Ok(n) = tok.parse::<u32>() {
                        if seen_te {
                            special = Some(n);
                        } else if (1..=49).contains(&n) {
                            draw.push(n);
                        }
                    }
                }
                draws.push(Draw {
                    char: ch,
                    draw,
                    special,
                });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#总码|") {
            total_code_text = rest.trim().to_string();
            continue;
        }
        if line.starts_with("#投注人|") || line.starts_with("合计|") {
            continue;
        }
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() >= 7 {
            // 名字可能自带 |：最后 6 段是定长的，其余都算名字。
            let name = fields[..fields.len() - 6].join("|");
            let tail = &fields[fields.len() - 6..];
            let parse_num = |s: &str| -> Option<f64> { s.trim().parse::<f64>().ok() };
            let count = tail[0].trim().parse::<usize>().ok();
            let stake = parse_num(tail[1]);
            let win = parse_num(tail[2]);
            let rebate = parse_num(tail[3]);
            let net = parse_num(tail[4]);
            let (Some(count), Some(stake), Some(win), Some(rebate), Some(net)) =
                (count, stake, win, rebate, net)
            else {
                problems.push(format!("第 {} 行：投注人摘要的金额写坏了。", idx + 1));
                continue;
            };
            bettors.push(Bettor {
                name: one_line_name(&name),
                batch_count: count,
                total_stake: stake,
                total_win: win,
                total_rebate: rebate,
                total_net: net,
                batches: Vec::new(),
                code: tail[5].trim().to_string(),
            });
            continue;
        }
        if fields.len() == 5 {
            let Some(b) = bettors.last_mut() else {
                problems.push(format!("第 {} 行：明细前没有投注人。", idx + 1));
                continue;
            };
            let no = fields[0].trim().parse::<usize>().ok();
            let vals: Vec<Option<f64>> = fields[1..]
                .iter()
                .map(|s| s.trim().parse::<f64>().ok())
                .collect();
            if let (Some(no), Some(stake), Some(hk), Some(old), Some(win)) =
                (no, vals[0], vals[1], vals[2], vals[3])
            {
                b.batches.push(Batch {
                    no,
                    stake,
                    hk,
                    old,
                    win,
                });
            } else {
                problems.push(format!("第 {} 行：明细的金额写坏了。", idx + 1));
            }
            continue;
        }
        problems.push(format!("第 {} 行：认不出的行，已跳过。", idx + 1));
    }

    let mut self_inconsistent = false;
    for b in &mut bettors {
        let expected = bettor_code(b);
        if !b.code.is_empty() && b.code != expected {
            self_inconsistent = true;
        }
        b.code = expected;
    }
    let computed_total = total_code(&bettors);
    if !total_code_text.is_empty() && total_code_text != computed_total {
        self_inconsistent = true;
    }

    let model = Model {
        date,
        draws,
        bettors,
        total_code: if total_code_text.is_empty() {
            computed_total
        } else {
            total_code_text
        },
    };
    ParseResult {
        ok: true,
        model: Some(model),
        problems,
        self_inconsistent,
    }
}

// ── 比对 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Same,
    Diff,
    LocalOnly,
    RemoteOnly,
}

#[derive(Clone, Default, Debug)]
pub struct FieldRow {
    pub label: &'static str,
    pub local: String,
    pub remote: String,
    pub equal: bool,
}

#[derive(Clone, Default, Debug)]
pub struct BatchRow {
    pub no: usize,
    pub local: String,
    pub remote: String,
}

#[derive(Clone, Default, Debug)]
pub struct BettorDiff {
    pub name: String,
    pub status: Status,
    pub summary: Vec<FieldRow>,
    /// 只列出有差异的批次。
    pub batches: Vec<BatchRow>,
}

impl Default for Status {
    fn default() -> Self {
        Status::Same
    }
}

#[derive(Clone, Default, Debug)]
pub struct Diff {
    pub blocking: Option<String>,
    pub same: bool,
    pub draw_note: Option<String>,
    pub bettors: Vec<BettorDiff>,
}

fn batch_text(r: Option<&Batch>) -> String {
    match r {
        Some(r) => format!(
            "金额 {} · 中奖 {}{}{}",
            money(r.stake),
            money(r.win),
            if r.hk > 0.0 {
                format!(" · 港 {}", money(r.hk))
            } else {
                String::new()
            },
            if r.old > 0.0 {
                format!(" · 老 {}", money(r.old))
            } else {
                String::new()
            }
        ),
        None => "（无）".to_string(),
    }
}

pub fn diff(local: &Model, remote: &Model) -> Diff {
    let mut out = Diff::default();
    // 对账只看五个总额（批数 / 总金额 / 总中奖 / 总回水 / 总输赢），
    // 日期与总码不参与判定。
    if !draws_equal(&local.draws, &remote.draws) {
        out.draw_note = Some("两边录入的开奖号码不同（没有命中时账目仍可能一致）。".into());
    }

    let local_map: BTreeMap<&str, &Bettor> =
        local.bettors.iter().map(|b| (b.name.as_str(), b)).collect();
    let remote_map: BTreeMap<&str, &Bettor> =
        remote.bettors.iter().map(|b| (b.name.as_str(), b)).collect();
    let names: BTreeSet<&str> = local_map.keys().copied().chain(remote_map.keys().copied()).collect();

    for name in names {
        let l = local_map.get(name).copied();
        let r = remote_map.get(name).copied();
        let (status, mut summary, batches) = match (l, r) {
            (Some(l), Some(r)) => compare_bettor(l, r),
            (Some(l), None) => (
                Status::LocalOnly,
                vec![
                    FieldRow {
                        label: "批数",
                        local: l.batch_count.to_string(),
                        remote: "—".into(),
                        equal: false,
                    },
                    FieldRow {
                        label: "总金额",
                        local: money(l.total_stake),
                        remote: "—".into(),
                        equal: false,
                    },
                    FieldRow {
                        label: "总中奖",
                        local: money(l.total_win),
                        remote: "—".into(),
                        equal: false,
                    },
                    FieldRow {
                        label: "总回水",
                        local: money(l.total_rebate),
                        remote: "—".into(),
                        equal: false,
                    },
                    FieldRow {
                        label: "总输赢",
                        local: money(l.total_net),
                        remote: "—".into(),
                        equal: false,
                    },
                ],
                Vec::new(),
            ),
            (None, Some(r)) => (
                Status::RemoteOnly,
                vec![
                    FieldRow {
                        label: "批数",
                        local: "—".into(),
                        remote: r.batch_count.to_string(),
                        equal: false,
                    },
                    FieldRow {
                        label: "总金额",
                        local: "—".into(),
                        remote: money(r.total_stake),
                        equal: false,
                    },
                    FieldRow {
                        label: "总中奖",
                        local: "—".into(),
                        remote: money(r.total_win),
                        equal: false,
                    },
                    FieldRow {
                        label: "总回水",
                        local: "—".into(),
                        remote: money(r.total_rebate),
                        equal: false,
                    },
                    FieldRow {
                        label: "总输赢",
                        local: "—".into(),
                        remote: money(r.total_net),
                        equal: false,
                    },
                ],
                Vec::new(),
            ),
            (None, None) => (Status::Same, Vec::new(), Vec::new()),
        };
        summary.shrink_to_fit();
        out.bettors.push(BettorDiff {
            name: name.to_string(),
            status,
            summary,
            batches,
        });
    }
    out.same = out.bettors.iter().all(|b| b.status == Status::Same)
        && out.blocking.is_none();
    out
}

fn compare_bettor(l: &Bettor, r: &Bettor) -> (Status, Vec<FieldRow>, Vec<BatchRow>) {
    let mut rows = vec![
        FieldRow {
            label: "批数",
            local: l.batch_count.to_string(),
            remote: r.batch_count.to_string(),
            equal: l.batch_count == r.batch_count,
        },
        FieldRow {
            label: "总金额",
            local: money(l.total_stake),
            remote: money(r.total_stake),
            equal: money(l.total_stake) == money(r.total_stake),
        },
        FieldRow {
            label: "总中奖",
            local: money(l.total_win),
            remote: money(r.total_win),
            equal: money(l.total_win) == money(r.total_win),
        },
        FieldRow {
            label: "总回水",
            local: money(l.total_rebate),
            remote: money(r.total_rebate),
            equal: money(l.total_rebate) == money(r.total_rebate),
        },
        FieldRow {
            label: "总输赢",
            local: money(l.total_net),
            remote: money(r.total_net),
            equal: money(l.total_net) == money(r.total_net),
        },
    ];
    let mut batches: Vec<BatchRow> = Vec::new();
    let n = l.batches.len().max(r.batches.len());
    for i in 0..n {
        let lb = l.batches.get(i);
        let rb = r.batches.get(i);
        let equal = match (lb, rb) {
            (Some(a), Some(b)) => {
                money(a.stake) == money(b.stake)
                    && money(a.hk) == money(b.hk)
                    && money(a.old) == money(b.old)
                    && money(a.win) == money(b.win)
            }
            _ => false,
        };
        if !equal {
            batches.push(BatchRow {
                no: lb.map(|b| b.no).or_else(|| rb.map(|b| b.no)).unwrap_or(i + 1),
                local: batch_text(lb),
                remote: batch_text(rb),
            });
        }
    }
    let same = rows.iter().all(|r| r.equal) && batches.is_empty();
    rows.shrink_to_fit();
    (if same { Status::Same } else { Status::Diff }, rows, batches)
}

fn draws_equal(a: &[Draw], b: &[Draw]) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.char == y.char && x.draw == y.draw && x.special == y.special
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bettor(name: &str) -> Bettor {
        Bettor::new(
            name,
            vec![
                Batch {
                    no: 1,
                    stake: 100.0,
                    hk: 0.0,
                    old: 0.0,
                    win: 0.0,
                },
                Batch {
                    no: 2,
                    stake: 50.0,
                    hk: 0.0,
                    old: 0.0,
                    win: 20.0,
                },
            ],
        )
    }

    #[test]
    fn text_roundtrip_and_same_diff() {
        let mut b = bettor("张三");
        b.set_rebate(10.0);
        let model = Model::new(
            "2026-11-22".into(),
            vec![],
            vec![b],
        );
        let text = model.text();
        let parsed = parse(&text);
        assert!(parsed.ok, "{:?}", parsed.problems);
        assert!(!parsed.self_inconsistent);
        let remote = parsed.model.unwrap();
        let d = diff(&model, &remote);
        assert!(d.same, "{:?}", d.bettors);
    }

    #[test]
    fn diff_detects_amount_change() {
        let model = Model::new("2026-11-22".into(), vec![], vec![bettor("张三")]);
        let mut remote = model.clone();
        remote.bettors[0].batches[1].win = 30.0;
        remote.bettors[0].total_win = 30.0;
        remote.bettors[0].total_net = 150.0 - 30.0;
        remote.bettors[0].code = bettor_code(&remote.bettors[0]);
        let d = diff(&model, &remote);
        assert!(!d.same);
        assert_eq!(d.bettors[0].batches.len(), 1);
    }

    #[test]
    fn matches_reference_codes() {
        // 与 zwy_wails_v1 的 reconcile.ts 对同一组数据算出来的码 / 文本完全一致。
        let mut b = Bettor::new(
            "张三",
            vec![
                Batch {
                    no: 1,
                    stake: 100.0,
                    hk: 0.0,
                    old: 0.0,
                    win: 0.0,
                },
                Batch {
                    no: 2,
                    stake: 50.0,
                    hk: 0.0,
                    old: 0.0,
                    win: 20.0,
                },
            ],
        );
        b.set_rebate(10.0);
        assert_eq!(b.code, "2XM7N-C1HBM");
        let model = Model::new("2026-11-22".into(), vec![], vec![b]);
        assert_eq!(model.total_code, "CAWN5-ZWAHG");
        let text = model.text();
        assert!(
            text.contains("张三|2|150.00|20.00|10.00|120.00|2XM7N-C1HBM"),
            "{text}"
        );
        assert!(text.contains("#总码|CAWN5-ZWAHG"), "{text}");
    }

    #[test]
    fn parse_rejects_other_version() {
        assert!(!parse("#对账 v2|2026-11-22\n").ok);
    }
}

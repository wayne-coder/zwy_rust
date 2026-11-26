//! 注单文本转换。
//!
//! 参考 `zwy_wails_v1/frontend/src/lib/formatConverter.ts` 的核心规则，把微信里
//! 口语化的注单整理成统一写法：`澳 特码 01,02,03 各数 100`。
//! 这里覆盖常用的「地区 / 玩法 / 号码 / 生肖 / 金额 / 区间」写法；参考项目里
//! 依赖设置表与生肖号码表的扩展玩法（各包摊分、连肖注数、五行波色等）保持原样输出，
//! 不做静默改写。

use std::collections::{BTreeMap, BTreeSet};

use crate::keywords::{Field, KeywordSettings, Section};

/// 一条转换后的注单。
#[derive(Clone, Default, Debug, PartialEq)]
pub struct BetItem {
    /// 规范化的整行文本
    pub line: String,
    /// 彩种：澳门 / 香港 / 老澳门
    pub lottery: String,
    /// 玩法：特码 / 二中二 / 三中三 …
    pub play: String,
    /// 号码、生肖等投注内容
    pub content: String,
    /// 一注金额；总额写法显示为「共 N」
    pub stake: String,
    /// 这一行实际展开的注数（组合按展开后的号码数计算）。
    pub bets: f64,
    /// 这一行的总金额：每注金额 × 注数；总额写法直接用原金额。
    pub amount: f64,
    /// 特码属性词总额写法（`特码 家禽共1000`）：按行总额 × 1.9 赔。
    pub attr_total: bool,
    /// 校验玩法 ↔ 内容关系时使用的原始内容（组合玩法展开号码之前）。
    /// `None` 表示直接使用 `content`。
    pub validate_content: Option<String>,
}

#[derive(Clone, Default, Debug)]
pub struct ConvertOutput {
    pub bets: Vec<BetItem>,
    /// 转换后的完整文本，一行一注
    pub text: String,
    /// 总金额：每行展开成实际注数后的注额合计，与参考项目批次总金额同一口径。
    pub total_stake: f64,
    /// 存疑：任一含号码的行没有同时写出「玩法 + 数字 + 金额」。
    pub doubtful: bool,
    /// 明细不满足玩法要求（如三中三不足 3 个号码）→ 失败。
    pub invalid: bool,
    /// 存疑 / 失败的具体原因，展示在详情面板。
    pub reasons: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    Macau,
    HongKong,
    OldMacau,
}

impl Region {
    fn char(self) -> &'static str {
        match self {
            Region::Macau => "澳",
            Region::HongKong => "港",
            Region::OldMacau => "老",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Region::Macau => "澳门",
            Region::HongKong => "香港",
            Region::OldMacau => "老澳门",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PlayKind {
    Special,
    FlatNumber,
    FlatSpecial,
    Combo(usize),
    LianXiao(usize),
    LianWei(usize),
    BuZhong(usize),
    ZhongYi(usize),
    LiuXiao,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StakeKind {
    PerBet,
    Total,
    PerGroup,
    Guessed,
}

#[derive(Clone, Copy, Debug)]
struct Stake {
    amount: f64,
    kind: StakeKind,
}

#[derive(Clone, Copy, Debug)]
struct StakeHit {
    /// 在原行里的字节位置（金额标记的起点）
    pos: usize,
    /// 金额（含后缀）结束后的位置
    end: usize,
    stake: Stake,
}

/// 段落结束时结算存疑：单独写了总额的段落免除金额类存疑。
fn finalize_doubt(
    out: &mut ConvertOutput,
    all_special: bool,
    doubtful: bool,
    reasons: &mut Vec<String>,
    play_no_amount: bool,
    play_reasons: &mut Vec<String>,
    standalone_total: bool,
    has_complete_line: bool,
) {
    if standalone_total {
        reasons.clear();
        play_reasons.clear();
        return;
    }
    if doubtful && !all_special {
        out.doubtful = true;
        out.reasons.append(reasons);
    } else {
        reasons.clear();
    }
    if play_no_amount {
        out.doubtful = true;
        out.reasons.append(play_reasons);
    } else {
        play_reasons.clear();
    }
    // 非特码段落：必须至少有一行同时写了玩法、号码和金额。
    if !all_special && !has_complete_line {
        out.doubtful = true;
        out.reasons
            .push("没有一行同时写出玩法、号码和金额".to_string());
    }
}

/// 段落级共享金额：段里单独一行只写了金额（`三组各8`、`共100`…）时，
/// 这一段的号码行都可以用它，即使金额写在号码行后面。
fn line_is_all_special(line: &str, settings: &KeywordSettings) -> bool {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    hits.iter().all(|h| matches!(h.kind, PlayKind::Special))
}

/// 全特码段落里，每遇到一个显式金额就切成一行。
/// `特码狗各5，虎各3，1 2 3 各10` → 三行。
fn split_after_stakes(line: &str, settings: &KeywordSettings) -> Vec<String> {
    let type_hits = find_play_hits(line, settings);
    let (hits, _) = extract_stakes(line, settings, &type_hits, false);
    if hits.len() < 2 {
        return vec![line.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0;
    for h in &hits {
        if h.end <= start || h.end > line.len() {
            continue;
        }
        let chunk = line[start..h.end]
            .trim()
            .trim_matches(|c: char| c.is_whitespace() || is_separator(c));
        if !chunk.is_empty() {
            out.push(chunk.to_string());
        }
        start = h.end;
    }
    let rest = line[start..]
        .trim()
        .trim_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// 去掉金额后缀词（`元 / 块 / 米`…）后的文本。
fn strip_stake_suffix_word<'a>(s: &'a str, settings: &KeywordSettings) -> &'a str {
    let mut best = s;
    let mut kws: Vec<&str> = settings
        .get(Field::StakeSuffix)
        .iter()
        .map(String::as_str)
        .chain(["元", "块"])
        .filter(|kw| !kw.is_empty())
        .collect();
    kws.sort_by_key(|kw| std::cmp::Reverse(kw.chars().count()));
    for kw in kws {
        if let Some(rest) = best.strip_suffix(kw) {
            best = rest.trim_end();
            break;
        }
    }
    best
}

/// 「号码 + 金额」对子的判据：左号码 1–49，右金额 > 0。
/// 斜杠（`/`、`／`）后面的数字一律是金额；点号（`.`、`．`）沿用老口径，
/// 只有大于 49 才像金额（点号本来就常用来分隔号码）。
fn is_special_amount_pair(left: &str, sep: char, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty()
        || left.chars().count() > 2
        || !left.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    let Some(pick) = left.parse::<u32>().ok() else {
        return false;
    };
    if !(1..=49).contains(&pick) {
        return false;
    }
    let mut dot = false;
    for c in right.chars() {
        if c == '.' || c == '．' {
            if dot {
                return false;
            }
            dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    let Ok(amount) = right.replace('．', ".").parse::<f64>() else {
        return false;
    };
    if amount <= 0.0 {
        return false;
    }
    matches!(sep, '/' | '／') || amount > 49.0
}

/// 一段文本（两个玩法之间 / 玩法到行尾）里如果只有一个金额（`100`、`各100`、
/// `100元`），返回这笔金额；里面还夹着号码或备注时返回 `None`。
///
/// `allow_bare_tail`：行尾的裸数字（没有「各 / 元」标记、也不大于 49）能不能
/// 当金额。没有这一行前面的号码表达式时才算金额（`三中三 10`）；前面已经写了
/// 号码时（`特10`）仍按号码处理，避免把号码吃掉。
fn pure_segment_stake(
    seg: &str,
    settings: &KeywordSettings,
    allow_bare_tail: bool,
) -> Option<Stake> {
    let seg = seg.trim_matches(|c: char| c.is_whitespace() || is_separator(c));
    if seg.is_empty() {
        return None;
    }
    let (hits, cleaned) = extract_stakes(seg, settings, &[], true);
    if hits.len() != 1 {
        return None;
    }
    let stake = hits[0].stake;
    if matches!(stake.kind, StakeKind::Guessed)
        && stake.amount <= 49.0
        && !allow_bare_tail
    {
        return None;
    }
    // 剩下的只能是「每组 / 每组各」这类被吃掉一半的前缀词，不能再有号码。
    let picks = strip_all_play_keywords(&cleaned, settings);
    if !normalize_content(&apply_variants(&picks, settings)).is_empty() {
        return None;
    }
    Some(stake)
}

/// 一行里的「玩法 + 金额」对子：金额紧跟在玩法后面、中间不夹号码。
///
/// 返回每个玩法对应的金额（没有写则为 `None`），以及去掉这些对子之后
/// 这一行还剩不剩号码（用来判断是不是只写玩法 / 玩法+金额的头行）。
fn pair_stakes_with_hits(
    text: &str,
    hits: &[PlayHit],
    settings: &KeywordSettings,
) -> (Vec<Option<Stake>>, bool) {
    if hits.is_empty() {
        return (Vec::new(), false);
    }
    // 第一个玩法前面有没有号码表达式：有的话，行尾裸数字仍可能是号码（`特10`）。
    let prefix = text.get(..hits[0].start).unwrap_or("");
    let prefix_picks = {
        let picks = strip_all_play_keywords(prefix, settings);
        !normalize_content(&apply_variants(&picks, settings)).is_empty()
    };
    let mut stakes = vec![None; hits.len()];
    let mut pieces: Vec<&str> = Vec::new();
    let mut cursor = 0;
    for (i, h) in hits.iter().enumerate() {
        let end = hits.get(i + 1).map(|n| n.start).unwrap_or(text.len());
        if h.start > cursor {
            pieces.push(&text[cursor..h.start]);
        }
        let seg = text.get(h.end..end).unwrap_or("");
        let is_last = i + 1 == hits.len();
        match pure_segment_stake(seg, settings, !is_last || !prefix_picks) {
            Some(s) => stakes[i] = Some(s),
            None => pieces.push(seg),
        }
        cursor = end;
    }
    if cursor < text.len() {
        pieces.push(&text[cursor..]);
    }
    let joined = pieces.join(" ");
    let picks = strip_all_play_keywords(&joined, settings);
    let has_picks = !normalize_content(&apply_variants(&picks, settings)).is_empty();
    (stakes, has_picks)
}

/// 整行（剥掉玩法 / 金额后缀之后）就是一对「号码/金额」。
/// 全特码段里斜杠后面的数字是这一行自己的金额，段里别处写的金额盖不掉它。
fn is_slash_amount_pair_line(line: &str, settings: &KeywordSettings) -> bool {
    let picks = strip_all_play_keywords(line, settings);
    let text = picks.trim().trim_matches(|c: char| c.is_whitespace() || is_separator(c));
    let text = strip_stake_suffix_word(text, settings);
    let Some((left, right)) = text.split_once(['/', '／']) else {
        return false;
    };
    // 只认整行就是一对，`12.36.34/20` 这种组合注不算。
    if right.contains('/') || right.contains('／') {
        return false;
    }
    is_special_amount_pair(left, '/', right)
}

/// 找出一行里并排写的「号码/金额」对子（`10/50 20/60`、`10 / 50`、`10.50 20.60`），
/// 返回每个对子的字节区间。对子必须由空白隔开（与参考项目一致）。
fn special_amount_pair_spans(
    line: &str,
    settings: &KeywordSettings,
) -> Vec<(usize, usize, usize, char)> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let suffix_words: Vec<&str> = {
        let mut kws: Vec<&str> = settings
            .get(Field::StakeSuffix)
            .iter()
            .map(String::as_str)
            .chain(["元", "块"])
            .filter(|kw| !kw.is_empty())
            .collect();
        kws.sort_by_key(|kw| std::cmp::Reverse(kw.chars().count()));
        kws
    };
    let mut spans: Vec<(usize, usize, usize, char)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let sep = chars[i].1;
        if !matches!(sep, '/' | '／' | '.' | '．') {
            i += 1;
            continue;
        }
        // 左号码：分隔符左边（隔空白）的连续数字，最多两位。
        let mut j = i;
        while j > 0 && chars[j - 1].1.is_whitespace() {
            j -= 1;
        }
        let digit_end = j;
        let mut k = j;
        while k > 0 && chars[k - 1].1.is_ascii_digit() {
            k -= 1;
        }
        if k == digit_end || digit_end - k > 2 {
            i += 1;
            continue;
        }
        // 号码前面必须是空白或行首，避免把 `12.36.34` 里的每一段都当作对子。
        if k > 0 && !chars[k - 1].1.is_whitespace() {
            i += 1;
            continue;
        }
        // 右金额：分隔符右边（隔空白）的数字，可带一个小数点。
        let mut r = i + 1;
        while r < chars.len() && chars[r].1.is_whitespace() {
            r += 1;
        }
        let amount_start = r;
        let mut dot = false;
        while r < chars.len() {
            let c = chars[r].1;
            if c.is_ascii_digit() {
                r += 1;
            } else if (c == '.' || c == '．') && !dot {
                dot = true;
                r += 1;
            } else {
                break;
            }
        }
        if r == amount_start {
            i += 1;
            continue;
        }
        let left: String = chars[k..digit_end].iter().map(|(_, c)| *c).collect();
        let right: String = chars[amount_start..r].iter().map(|(_, c)| *c).collect();
        if !is_special_amount_pair(&left, sep, &right) {
            i += 1;
            continue;
        }
        // 金额后面允许空白和最多一个金额后缀；再往后必须是空白或行尾。
        // `end` 只到金额（或后缀）为止，后面的空白留给下一组。
        let mut e = r;
        while e < chars.len() && chars[e].1.is_whitespace() {
            e += 1;
        }
        let mut end = if r < chars.len() { chars[r].0 } else { line.len() };
        if e < chars.len() {
            let rest = &line[chars[e].0..];
            if let Some(kw) = suffix_words.iter().find(|kw| rest.starts_with(**kw)) {
                end = chars[e].0 + kw.len();
            }
        }
        let after = &line[end..];
        if let Some(c) = after.chars().next() {
            if !c.is_whitespace() {
                i += 1;
                continue;
            }
        }
        spans.push((chars[k].0, end, chars[i].0, sep));
        // 跳过整个对子，别把金额里的小数点当成下一个分隔符。
        let mut next = i + 1;
        while next < chars.len() && chars[next].0 < end {
            next += 1;
        }
        i = next;
    }
    spans
}

/// 全特码段里并排写的对子按组断行：`10/50 20/60` → `10/50` ↵ `20/60`。
/// 至少两组都成立才断；单组留给单行金额逻辑。
fn split_special_amount_pairs(line: &str, settings: &KeywordSettings) -> Vec<String> {
    let spans = special_amount_pair_spans(line, settings);
    if spans.is_empty() {
        return vec![line.to_string()];
    }
    // 点号对子（`10.50`）先改成同字节长度的斜杠写法，否则后面的号码解析会把
    // 「10.50」当成小数 10.5，整注丢掉。全角点号换成全角斜杠，长度不变，
    // 下面的区间仍按原文坐标切。
    let mut rewritten = String::with_capacity(line.len());
    let mut cursor = 0;
    for &(_, _, sep_pos, sep) in &spans {
        rewritten.push_str(&line[cursor..sep_pos]);
        rewritten.push(match sep {
            '.' => '/',
            '．' => '／',
            other => other,
        });
        cursor = sep_pos + sep.len_utf8();
    }
    rewritten.push_str(&line[cursor..]);
    if spans.len() < 2 {
        return vec![rewritten];
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0;
    for (_, end, _, _) in &spans[..spans.len() - 1] {
        let piece = rewritten[start..*end].trim();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        start = *end;
    }
    let tail = rewritten[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// 这一行是否写了除「计/总」之外的金额标记。
fn has_non_summary_stake(line: &str, settings: &KeywordSettings) -> bool {
    for kw in settings.get(Field::StakePrefix) {
        if !kw.is_empty() && line.contains(kw.as_str()) {
            return true;
        }
    }
    for kw in settings.get(Field::PerGroupTotalStake) {
        if !kw.is_empty() && line.contains(kw.as_str()) {
            return true;
        }
    }
    for kw in settings.get(Field::TotalStake) {
        if kw.is_empty() || matches!(kw.as_str(), "计" | "总" | "总计" | "总共") {
            continue;
        }
        if line.contains(kw.as_str()) {
            return true;
        }
    }
    for kw in settings.get(Field::StakeSuffix) {
        if !kw.is_empty() && line.contains(kw.as_str()) {
            return true;
        }
    }
    // 内置后缀「只」：数字紧贴只（5只）。
    let chars: Vec<char> = line.chars().collect();
    chars
        .windows(2)
        .any(|w| w[0].is_ascii_digit() && w[1] == '只')
}

/// 同一行已经写了「每组各10」时，后面的「计20 / 总130」是小计，不是投注金额。
fn strip_summary_totals(line: &str, settings: &KeywordSettings) -> String {
    if !has_non_summary_stake(line, settings) {
        return line.to_string();
    }
    // 行里已经有「各 / 每组」这类每注标记时，末尾的「共N / 总共N」才是冗余汇总。
    // `家禽共1000` 没有每注标记，仍然是属性词的总额写法。
    let per_bet = settings
        .get(Field::StakePrefix)
        .iter()
        .any(|kw| !kw.is_empty() && kw != "=" && line.contains(kw.as_str()))
        || settings
            .get(Field::PerGroupTotalStake)
            .iter()
            .any(|kw| !kw.is_empty() && line.contains(kw.as_str()));
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut summary = false;
        // 「总共」是一个整体，摘掉时要连同「共」一起跳过。
        let mut skip = 0usize;
        if c == '计' {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            summary = !matches!(prev, Some('合') | Some('总'));
        } else if c == '总' {
            if per_bet && matches!(chars.get(i + 1), Some('共')) {
                summary = true;
                skip = 1;
            } else {
                summary = !matches!(chars.get(i + 1), Some('计') | Some('共') | Some('金'));
            }
        } else if c == '共' && per_bet {
            // 「总共」里的「共」由上一轮的「总」处理。
            summary = !matches!(i.checked_sub(1).and_then(|p| chars.get(p)), Some('总'));
        }
        if summary {
            let mut j = i + 1 + skip;
            while j < chars.len()
                && (chars[j].is_whitespace() || is_separator(chars[j]))
            {
                j += 1;
            }
            let num_start = j;
            if j < chars.len() && chars[j].is_ascii_digit() {
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
            } else {
                while j < chars.len() && is_cn_num_char(chars[j]) {
                    j += 1;
                }
            }
            if j > num_start {
                let mut k = j;
                while k < chars.len() && chars[k] == ' ' {
                    k += 1;
                }
                let mut end = j;
                if let Some(kw) = settings
                    .get(Field::StakeSuffix)
                    .iter()
                    .filter(|kw| !kw.is_empty())
                    .find(|kw| starts_with_chars(&chars[k..], kw))
                {
                    end = k + kw.chars().count();
                }
                out.push(' ');
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 这一行是否写了显式金额。
fn line_has_explicit_stake(line: &str, settings: &KeywordSettings) -> bool {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    let (stakes, _) = extract_stakes(&without_region, settings, &hits, false);
    !stakes.is_empty()
}

/// 整行本身就是一笔完整投注（一个玩法、一个金额、有号码）。
/// 这种行里的 `。` 是号码分隔符，不是分块符。
fn line_is_single_bet(line: &str, settings: &KeywordSettings) -> bool {
    if line.contains(BLOCK_SEP) {
        return false;
    }
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    if hits.len() != 1 {
        return false;
    }
    let (stakes, _) = extract_stakes(&without_region, settings, &hits, false);
    if stakes.len() != 1 {
        return false;
    }
    line_has_picks(line, settings)
}

/// 这一行去掉玩法和金额后是否还有号码。
fn line_has_picks(line: &str, settings: &KeywordSettings) -> bool {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    let (_, cleaned) = extract_stakes(&without_region, settings, &hits, false);
    let picks = strip_all_play_keywords(&cleaned, settings);
    !normalize_content(&apply_variants(&picks, settings)).is_empty()
}

/// 可以作为号码间隔符的符号。
const NUM_SEP_CHARS: &[char] = &[
    '.', '。', '、', '，', ',', '/', '／', ';', '；', '|', '｜', '~', '～', '-', '－', '—', '–',
    '+', '＋',
];

/// 内部占位符：把 `---` 这类明显的备注分块符先标出来，分块后自然消失。
const BLOCK_SEP: char = '\u{1f}';

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '－' | '—' | '–')
}

/// 连续 3 个及以上的横线是「两笔注单」的分隔符；单个横线仍按号码间隔处理。
fn mark_dash_blocks(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if is_dash(chars[i]) {
            let start = i;
            while i < chars.len() && is_dash(chars[i]) {
                i += 1;
            }
            if i - start >= 3 {
                out.push(BLOCK_SEP);
            } else {
                for c in &chars[start..i] {
                    out.push(*c);
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 统计某个符号夹在两个阿拉伯数字之间的次数。
fn digit_adjacent_count(s: &str, sep: char) -> usize {
    let chars: Vec<char> = s.chars().collect();
    (1..chars.len().saturating_sub(1))
        .filter(|&i| {
            chars[i] == sep && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit()
        })
        .count()
}

/// 估计这一行里「号码间隔符」大概率是哪个符号：
/// 统计每个候选符号夹在两个阿拉伯数字之间的次数，次数最多的即为号码间隔符；
/// 次数相同取更早出现的那个。空格也算一种间隔符（返回 `' '`）。
/// 同时返回该符号的出现次数，供分块时判断置信度。
fn likely_number_separator(s: &str) -> Option<(char, usize)> {
    let chars: Vec<char> = s.chars().collect();
    let mut best: Option<(char, usize, usize)> = None; // 符号、次数、首次出现位置
    let consider = |sep: char, count: usize, first: usize, best: &mut Option<(char, usize, usize)>| {
        if count == 0 {
            return;
        }
        let better = match *best {
            None => true,
            Some((_, c, f)) => count > c || (count == c && first < f),
        };
        if better {
            *best = Some((sep, count, first));
        }
    };
    for &sep in NUM_SEP_CHARS {
        let mut count = 0;
        let mut first = usize::MAX;
        for i in 1..chars.len().saturating_sub(1) {
            if chars[i] == sep
                && chars[i - 1].is_ascii_digit()
                && chars[i + 1].is_ascii_digit()
            {
                count += 1;
                first = first.min(i);
            }
        }
        consider(sep, count, first, &mut best);
    }
    // 空白：把连续空白看成一个间隔符，两侧最近的字符都是数字才算。
    let mut ws_count = 0;
    let mut ws_first = usize::MAX;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if start > 0
                && i < chars.len()
                && chars[start - 1].is_ascii_digit()
                && chars[i].is_ascii_digit()
            {
                ws_count += 1;
                ws_first = ws_first.min(start);
            }
        } else {
            i += 1;
        }
    }
    consider(' ', ws_count, ws_first, &mut best);
    best.map(|(sep, count, _)| (sep, count))
}

/// 该符号左边最近的非空白字符是否为阿拉伯数字。
fn prev_is_digit(chars: &[(usize, char)], i: usize) -> bool {
    chars[..i]
        .iter()
        .rev()
        .map(|(_, c)| *c)
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| c.is_ascii_digit())
}

/// 该符号右边最近的非空白字符是否为阿拉伯数字。
fn next_is_digit(chars: &[(usize, char)], i: usize) -> bool {
    chars[i + 1..]
        .iter()
        .map(|(_, c)| *c)
        .find(|c| !c.is_whitespace())
        .is_some_and(|c| c.is_ascii_digit())
}

/// 该符号左右最近的非空白字符是否都是阿拉伯数字。
fn separates_digits(chars: &[(usize, char)], i: usize) -> bool {
    prev_is_digit(chars, i) && next_is_digit(chars, i)
}

/// 这一段文本是否以玩法 / 地区词开头（用于判断逗号是不是备注分块符）。
fn starts_with_note_head(text: &str, settings: &KeywordSettings) -> bool {
    const REGION_FIELDS: &[Field] = &[
        Field::HongKong,
        Field::Macau,
        Field::OldMacau,
        Field::BothSides,
        Field::ThreeSides,
    ];
    let mut t = text.trim_start();
    for _ in 0..3 {
        let before = t;
        if let Some(rest) = t.strip_prefix("复式").or_else(|| t.strip_prefix("复试")) {
            t = rest.trim_start();
        }
        if let Some(rest) = REGION_FIELDS
            .iter()
            .flat_map(|f| settings.get(*f).iter())
            .filter(|k| !k.is_empty())
            .find_map(|k| t.strip_prefix(k.as_str()))
        {
            t = rest.trim_start();
        }
        if t == before {
            break;
        }
    }
    let hits = find_play_hits(t, settings);
    let Some(first) = hits.iter().min_by_key(|h| h.start) else {
        return false;
    };
    // 玩法前面可能先写了号码 / 生肖 / 尾数（`牛特碰各10`、`0尾二中二各5`），
    // 只要前缀全是号码类 token，也算一块注单的开头。
    prefix_is_picks(&t[..first.start])
}

fn prefix_is_picks(prefix: &str) -> bool {
    let mut chars = prefix.chars();
    while let Some(c) = chars.next() {
        if c.is_whitespace() || is_separator(c) {
            continue;
        }
        if is_zodiac(c) || SINGLE_ATTR_CHARS.contains(&c) {
            continue;
        }
        if c.is_ascii_digit() {
            while chars.clone().next().is_some_and(|d| d.is_ascii_digit()) {
                chars.next();
            }
            if matches!(chars.clone().next(), Some('尾') | Some('头')) {
                chars.next();
            }
            continue;
        }
        return false;
    }
    true
}

/// 按「备注分块符」把一行切成若干块。
///
/// 句号 / 分号 / 点号既可能是号码间隔符，也可能是备注分块符：
/// - 先由 `num_sep` 判断这一行的号码间隔符；
/// - `。` 两侧不是数字时一定是分块符；两侧是数字时，只有别的号码间隔符
///   明显占优（次数至少是「。」的两倍）才当分块符，否则按号码间隔处理；
/// - 点号 / 分号只有「前面是数字、后面不是数字」且前面这一块已经写了金额时
///   才当分块符，避免把 `01.02`、`15/.20`、`21.22.23.三中三1` 这种号码写法拆开；
/// - 逗号 / 顿号只在「前一块已完整（有号码且有金额）」且「后一块以玩法 / 地区开头」
///   时切分，避免把 `三中三，二中二` 这种多玩法写法拆散。
fn split_note_blocks(
    s: &str,
    num_sep: Option<char>,
    num_sep_count: usize,
    settings: &KeywordSettings,
) -> Vec<String> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut out: Vec<String> = Vec::new();
    let mut start = 0;
    for (i, &(pos, c)) in chars.iter().enumerate() {
        let split = match c {
            BLOCK_SEP => true,
            '。' => {
                if !separates_digits(&chars, i) {
                    true
                } else {
                    // 两侧都是数字：只有当别的号码间隔符明显占优时，
                    // 才认为这里的「。」是分块符；否则它也是号码间隔符。
                    match num_sep {
                        Some(sep) if sep != '。' => {
                            num_sep_count >= digit_adjacent_count(s, '。').max(1) * 2
                        }
                        _ => false,
                    }
                }
            }
            // 点号 / 分号只有在「前面是数字、后面不是数字」且前面这一块已经
            // 写了金额时才像分块符：`21.22.23.三中三1` 里的点号属于号码写法，
            // 后面跟的是同一注的玩法，不能拆。
            '.' | '；' | ';' => {
                let left = &s[start..pos];
                // 前一块已经写了金额时，后面的点号/分号就是备注分块符。
                // 金额后面可能紧跟「元 / 块」之类后缀，所以不要求前一个字符是数字。
                !next_is_digit(&chars, i) && line_has_explicit_stake(left, settings)
            }
            '，' | '、' | ',' => {
                let left = &s[start..pos];
                let right = &s[pos + c.len_utf8()..];
                !separates_digits(&chars, i)
                    && line_has_picks(left, settings)
                    && line_has_explicit_stake(left, settings)
                    && starts_with_note_head(right, settings)
            }
            _ => false,
        };
        if split {
            out.push(s[start..pos].to_string());
            start = pos + c.len_utf8();
        }
    }
    out.push(s[start..].to_string());
    out
}

/// 号码行后面连续跟着的「只写玩法」行（可带金额）是对前面号码的追加投注：
/// 并回最近的一行有号码的块，交给多玩法逻辑分别出注。
///
/// 例如：
/// ```text
/// 32 40 36
/// 三中三
/// 二中二各30
/// ```
/// 会合成 `32 40 36，三中三，二中二各30`，两个玩法共用这组号码；
/// 金额只写在其中一个玩法上时，后面的多玩法分配逻辑会让两个玩法都用它。
///
/// 如果这些玩法行后面还有带号码的行，则把它们保留为后面内容的玩法头行，不并回。
fn merge_trailing_play_lines(
    lines: Vec<String>,
    settings: &KeywordSettings,
) -> Vec<String> {
    // 判断一行是不是「只有玩法（可带金额）」：
    // - 去掉玩法关键词和紧跟玩法的金额后，不剩任何号码；
    // - 返回是否带金额。`二中二各15`、`三中三 100 二中二 50` 都算带金额。
    let play_only_info = |line: &str| -> (bool, bool) {
        let expanded = expand_ranges(line, settings);
        let (without_region, _) = strip_regions(&expanded, settings);
        let hits = find_play_hits(&without_region, settings);
        if hits.is_empty() {
            return (false, false);
        }
        let (pair_stakes, has_picks) =
            pair_stakes_with_hits(&without_region, &hits, settings);
        let has_amount = pair_stakes.iter().any(|s| s.is_some());
        (!has_picks, has_amount)
    };
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        out.push(lines[i].clone());
        if line_has_picks(&lines[i], settings) {
            // 收集紧跟其后的连续玩法行。
            let mut j = i + 1;
            let mut run_has_stake = false;
            while j < lines.len() {
                let (is_play_only, has_amount) = play_only_info(&lines[j]);
                if !is_play_only {
                    break;
                }
                run_has_stake |= has_amount;
                j += 1;
            }
            let next_has_picks =
                j < lines.len() && line_has_picks(&lines[j], settings);
            if j > i + 1 && run_has_stake && !next_has_picks {
                if let Some(prev) = out.last_mut() {
                    for line in &lines[i + 1..j] {
                        let mut line = line.clone();
                        // 裸金额（`三中三15`）并回号码行后，补一个「各」，
                        // 否则主解析会把 15 当成第 4 个号码。
                        if !line_has_explicit_stake(&line, settings) {
                            let hits = find_play_hits(&line, settings);
                            let (stakes, _) =
                                extract_stakes(&line, settings, &hits, true);
                            if stakes.len() == 1
                                && matches!(
                                    stakes[0].stake.kind,
                                    StakeKind::Guessed
                                )
                                && stakes[0].stake.amount > 0.0
                            {
                                line.insert_str(stakes[0].pos, "各");
                            }
                        }
                        prev.push('，');
                        prev.push_str(&line);
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 连肖 / 连尾关键词紧贴着写的裸数字（`四冲10`、`二连尾5`）是每注金额：
/// 补一个「各」，后面的金额识别链路就能按普通每注金额处理。
/// 号码列表（`四连肖1,2,3`）不能误判：数字后面若还是数字，不补。
fn mark_lian_stake(line: &str, settings: &KeywordSettings) -> String {
    let hits = find_play_hits(line, settings);
    let mut out = line.to_string();
    // 从右往左插，前面的命中位置不受影响。
    for h in hits.iter().rev() {
        if !matches!(h.kind, PlayKind::LianXiao(_) | PlayKind::LianWei(_)) {
            continue;
        }
        let rest = &line[h.end..];
        // 必须紧贴玩法关键词（中间没有空格）。
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace())
        {
            continue;
        }
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        let num_end = h.end + digits.len();
        let tail = &line[num_end..];
        if tail.starts_with('尾') || tail.starts_with('头') {
            continue;
        }
        let tail_ws = tail.trim_start();
        let looks_like_amount = tail_ws.is_empty()
            || stake_suffix(tail_ws, settings).is_some()
            || settings
                .get(Field::TotalStake)
                .iter()
                .any(|kw| !kw.is_empty() && tail_ws.starts_with(kw.as_str()))
            || (tail
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace() || is_separator(c))
                && !tail
                    .trim_start_matches(|c: char| c.is_whitespace() || is_separator(c))
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()));
        if looks_like_amount {
            out.insert_str(h.end, "各");
        }
    }
    out
}

/// 预先处理原文：应用替换词、组数说明、组分隔符；按推测出的备注分块符切块；
/// 全特码段落再按金额断行。
/// 一行里出现三个以上「拖」，或「恰好三个号码 + 两个拖」时，把该行所有「拖」
/// 换成「.」，并返回存疑原因。
///
/// 三个「拖」意味着至少四个对乘分组（`A拖B拖C拖D`），正常投注里极少见；三个号码
/// 用两个「拖」连起来（`01拖02拖03`）同样容易和普通三号码组合混淆。两种情况都
/// 不再按拖注展开，由人工复核。
fn replace_excess_drag(raw: &str, settings: &KeywordSettings) -> (String, Vec<String>) {
    let mut out = String::with_capacity(raw.len());
    let mut reasons = Vec::new();
    for (idx, line) in raw.split('\n').enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let drags = line.chars().filter(|c| *c == '拖').count();
        let reason = if drags >= 3 {
            Some(format!("第 {} 行：存在三个以上的对乘符号", idx + 1))
        } else if drags == 2 && count_line_numbers(line, settings) == 3 {
            Some(format!("第 {} 行：三个号码存在两个对乘符号", idx + 1))
        } else {
            None
        };
        if let Some(reason) = reason {
            out.extend(line.chars().map(|c| if c == '拖' { '.' } else { c }));
            reasons.push(reason);
        } else {
            out.push_str(line);
        }
    }
    (out, reasons)
}

/// `A10` / `a10` 这类多行金额前缀：返回去掉前缀后的行和共享金额。
fn strip_multi_line_stake(line: &str, settings: &KeywordSettings) -> (String, Option<Stake>) {
    let mut kws: Vec<&str> = settings
        .get(Field::MultiLineStakePrefix)
        .iter()
        .map(String::as_str)
        .filter(|k| !k.is_empty())
        .collect();
    kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    for kw in kws {
        let mut search_from = 0;
        while let Some(rel) = line[search_from..].find(kw) {
            let pos = search_from + rel;
            let mut i = pos + kw.len();
            while let Some(c) = line[i..].chars().next() {
                if c.is_whitespace() {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
            let start_num = i;
            let mut end = i;
            while let Some(c) = line[end..].chars().next() {
                if c.is_ascii_digit() || c == '.' {
                    end += c.len_utf8();
                } else {
                    break;
                }
            }
            if end > start_num {
                if let Ok(amount) = line[start_num..end].parse::<f64>() {
                    let mut out = String::with_capacity(line.len());
                    out.push_str(&line[..pos]);
                    out.push(' ');
                    out.push_str(&line[end..]);
                    let stake = Stake {
                        amount,
                        kind: StakeKind::PerBet,
                    };
                    return (out, Some(stake));
                }
            }
            search_from = pos + line[pos..].chars().next().map(char::len_utf8).unwrap_or(1);
        }
    }
    (line.to_string(), None)
}

/// 交集写法：`虎牛狗v双` → 两边号码集合的交集；`11.12.45.23v大` → `45`。
///
/// 分隔词取「区域 & 其他」里的交集关键词（默认 v/V），每一侧复用正常内容的
/// 号码展开（号码 / 生肖 / 尾数 / 头数 / 五行波色 / 大小单双）。任一侧展不出号码
/// 或交集为空时原样返回，不静默删掉整笔。
fn expand_intersections(text: &str, settings: &KeywordSettings) -> String {
    let mut seps: Vec<&str> = settings
        .get(Field::Intersection)
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if seps.is_empty() {
        return text.to_string();
    }
    seps.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));

    let mut segments: Vec<&str> = Vec::new();
    let mut rest = text;
    loop {
        let mut best: Option<(usize, &str)> = None;
        for sep in &seps {
            if let Some(pos) = rest.find(sep) {
                let better = match best {
                    None => true,
                    Some((bp, bs)) => pos < bp || (pos == bp && sep.len() > bs.len()),
                };
                if better {
                    best = Some((pos, sep));
                }
            }
        }
        match best {
            Some((pos, sep)) => {
                segments.push(&rest[..pos]);
                rest = &rest[pos + sep.len()..];
            }
            None => {
                segments.push(rest);
                break;
            }
        }
    }
    if segments.len() < 2 {
        return text.to_string();
    }

    let mut sets: Vec<BTreeSet<u32>> = Vec::with_capacity(segments.len());
    for seg in segments {
        let expanded = expand_ranges(seg, settings);
        let normalized = normalize_content(&apply_variants(&expanded, settings));
        let mut set: BTreeSet<u32> = BTreeSet::new();
        for tok in normalized.split(',').filter(|t| !t.is_empty()) {
            set.extend(crate::settle::token_numbers(tok));
        }
        if set.is_empty() {
            return text.to_string();
        }
        sets.push(set);
    }

    let mut intersection = sets[0].clone();
    for set in &sets[1..] {
        intersection.retain(|n| set.contains(n));
    }
    if intersection.is_empty() {
        return text.to_string();
    }
    intersection
        .into_iter()
        .map(|n| format!("{n:02}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// 这一行里独立的号码个数（1~49；尾数「3尾」和金额不算）。
fn count_line_numbers(line: &str, settings: &KeywordSettings) -> usize {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    let (_, cleaned) = extract_stakes(&without_region, settings, &hits, false);
    let picks = strip_all_play_keywords(&cleaned, settings);
    number_runs(&picks)
        .into_iter()
        .filter(|(_, end, value, _)| {
            (1.0..=49.0).contains(value)
                && !picks[*end..].starts_with('尾')
                && !picks[*end..].starts_with('头')
        })
        .count()
}

/// 一段里「多个裸表达式 + 末尾的玩法/金额声明」发散展开。
///
/// 例：
/// ```text
/// 23. 15. 26. 24. 42
/// 20. 44. 24. 36. 13
/// 复试二中二三中三各1
/// ```
/// 两行号码都要按「二中二 1」「三中三 1」各出一注。展开后每一行都变成
/// 「号码 + 玩法 + 金额」的完整写法，交给后面的逐行转换即可。
///
/// 只处理**末尾连续**的纯玩法(+金额)行；单玩法声明保持原有逻辑不动。
fn expand_trailing_play_blocks(lines: Vec<String>, settings: &KeywordSettings) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim().is_empty() {
            out.push(lines[i].clone());
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            i += 1;
        }
        out.extend(expand_paragraph_play_block(&lines[start..i], settings));
    }
    out
}

/// 一个段落内：末尾连续「纯玩法(+金额)」行 + 紧邻的「裸表达式」行。
fn expand_paragraph_play_block(para: &[String], settings: &KeywordSettings) -> Vec<String> {
    // 末尾连续的纯玩法 / 玩法+金额行。
    let mut decl_start = para.len();
    while decl_start > 0 && parse_decl_line(&para[decl_start - 1], settings).is_some() {
        decl_start -= 1;
    }
    if decl_start == para.len() {
        return para.to_vec();
    }
    // 声明块前面紧挨着的号码表达式行（没有玩法、没有金额）。
    let mut expr_start = decl_start;
    while expr_start > 0 && is_bare_expression(&para[expr_start - 1], settings) {
        expr_start -= 1;
    }
    if expr_start == decl_start {
        return para.to_vec();
    }

    let mut decls: Vec<DeclPlay> = Vec::new();
    for line in &para[decl_start..] {
        if let Some(mut plays) = parse_decl_line(line, settings) {
            decls.append(&mut plays);
        }
    }
    // 单玩法声明交给原来的「段落玩法继承」，这里不重复展开。
    if decls.len() < 2 {
        return para.to_vec();
    }
    // 某个玩法后面没写金额时，取它后面最近一个写了金额的玩法的金额：
    // `三中三，二中二各2` → 三中三也按 2；分多行写时同样向后找。
    for i in (0..decls.len()).rev() {
        if decls[i].amount.is_none() {
            if let Some(next) = decls[i + 1..].iter().find(|d| d.amount.is_some()) {
                decls[i].amount = next.amount.clone();
            }
        }
    }
    // 跨行共用一个金额（`二中二\n特碰\n三中三各1`）：
    // 整块只有一种金额写法时，没写金额的玩法也用它。
    let amounts: Vec<&String> = decls.iter().filter_map(|d| d.amount.as_ref()).collect();
    let shared = match amounts.first() {
        Some(first) if amounts.iter().all(|x| x == first) => Some((*first).clone()),
        _ => None,
    };
    for d in &mut decls {
        if d.amount.is_none() {
            d.amount = shared.clone();
        }
    }

    let mut out: Vec<String> = para[..expr_start].to_vec();
    for expr in &para[expr_start..decl_start] {
        for d in &decls {
            out.push(match &d.amount {
                Some(amount) => format!("{} {} {}", expr.trim(), d.text, amount),
                None => format!("{} {}", expr.trim(), d.text),
            });
        }
    }
    out
}

/// 一行玩法声明：只有玩法（可带金额），没有号码。
#[derive(Clone)]
struct DeclPlay {
    /// 原文里的玩法写法（如「特串」），后面重新解析时仍按用户设置识别。
    text: String,
    /// 这个玩法自己的金额写法（如「各1」）；没写就是 `None`。
    amount: Option<String>,
}

/// 解析一行纯「玩法 / 玩法+金额」声明；含号码时返回 `None`。
fn parse_decl_line(line: &str, settings: &KeywordSettings) -> Option<Vec<DeclPlay>> {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    if hits.is_empty() {
        return None;
    }
    // 纯玩法声明：去掉全部玩法与紧跟的金额后不能再有号码。
    let (_, has_picks) = pair_stakes_with_hits(&without_region, &hits, settings);
    if has_picks {
        return None;
    }
    let mut out = Vec::with_capacity(hits.len());
    for (i, h) in hits.iter().enumerate() {
        // 这个玩法后面、下一个玩法之前的片段：声明行里它只能是金额。
        let seg_end = hits
            .get(i + 1)
            .map(|n| n.start)
            .unwrap_or(without_region.len());
        let seg = without_region.get(h.end..seg_end).unwrap_or("");
        let (stakes, _) = extract_stakes(seg, settings, &[], true);
        // 从片段开头截到金额末尾：保留「每组 / 各 / 元」这类修饰词与后缀。
        let amount = stakes
            .first()
            .and_then(|s| seg.get(..s.end))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        out.push(DeclPlay {
            text: without_region[h.start..h.end].to_string(),
            amount,
        });
    }
    Some(out)
}

/// 是否是「裸表达式」：有号码，但没有玩法、也没有金额。
fn is_bare_expression(line: &str, settings: &KeywordSettings) -> bool {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    if !find_play_hits(&without_region, settings).is_empty() {
        return false;
    }
    let (stakes, _) = extract_stakes(&without_region, settings, &[], false);
    if !stakes.is_empty() {
        return false;
    }
    line_has_picks(&without_region, settings)
}

fn prepare_lines(
    raw: &str,
    settings: &KeywordSettings,
    replacements: &[crate::replacements::Rule],
) -> Vec<String> {
    let lines: Vec<&str> = raw.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i;
        while j < lines.len() && !lines[j].trim().is_empty() {
            j += 1;
        }
        let mut transformed: Vec<String> = Vec::new();
        for raw_line in &lines[i..j] {
            let replaced = crate::replacements::apply(raw_line, replacements);
            let cleaned = strip_group_counts(replaced.trim());
            if cleaned.trim().is_empty() {
                continue;
            }
            // 连肖 / 连尾紧贴的裸数字是每注金额（`四冲10共50` → `四冲各10共50`）。
            let cleaned = mark_lian_stake(&cleaned, settings);
            // `---` 是两笔注单的分隔符，先换成内部占位符，避免被当成号码间隔。
            let cleaned = mark_dash_blocks(&cleaned);
            // 整行是一笔完整投注时，`。` 等符号只是号码分隔符，不按块拆。
            if line_is_single_bet(&cleaned, settings) {
                let normalized = normalize_group_separators(&cleaned);
                transformed.push(strip_summary_totals(&normalized, settings));
                continue;
            }
            // `。`、`；`、点号既可能是号码间隔符，也可能是备注分块符：
            // 先猜这一行用哪个符号分隔号码，再据此决定在哪里切块。
            let (num_sep, num_sep_count) = match likely_number_separator(&cleaned) {
                Some((sep, count)) => (Some(sep), count),
                None => (None, 0),
            };
            // 合并只在同一行切出的块之间进行；换行是独立的一行。
            let mut line_parts: Vec<String> = Vec::new();
            for part in split_note_blocks(&cleaned, num_sep, num_sep_count, settings) {
                let cleaned = strip_group_counts(part.trim());
                let cleaned = normalize_group_separators(&cleaned);
                if cleaned.trim().is_empty() {
                    continue;
                }
                let part = strip_summary_totals(&cleaned, settings);
                let has_picks = line_has_picks(&part, settings);
                // 「。=30」/「。每组10」这种只有金额、没有号码也没有玩法的块：
                // 前一块已经写了每注金额时，它只是汇总（校验用），丢掉。
                if !has_picks
                    && line_has_explicit_stake(&part, settings)
                    && find_play_hits(&part, settings).is_empty()
                    && line_parts.last().is_some_and(|last| {
                        line_has_picks(last, settings)
                            && line_has_explicit_stake(last, settings)
                    })
                {
                    continue;
                }
                // 没有号码、但写了玩法和金额的块（`二中二，每组十块`）是对前一块
                // 号码的追加投注，并回前一块处理；纯玩法头行不合并。
                let append_to_prev = !has_picks
                    && line_has_explicit_stake(&part, settings)
                    && line_parts
                        .last()
                        .is_some_and(|last| line_has_picks(last, settings));
                if append_to_prev {
                    let last = line_parts.last_mut().unwrap();
                    last.push('，');
                    last.push_str(&part);
                } else {
                    line_parts.push(part);
                }
            }
            transformed.extend(line_parts);
        }
        // 多个裸表达式 + 末尾多个玩法/金额声明：先发散成完整的「表达式 玩法 金额」
        // 行，避免声明行被 merge_trailing_play_lines 只并到最后一个表达式上。
        let transformed = expand_trailing_play_blocks(transformed, settings);
        let transformed = merge_trailing_play_lines(transformed, settings);
        let all_special = transformed
            .iter()
            .all(|l| line_is_all_special(l, settings));
        for line in &transformed {
            // 全特码段里并排写的「号码/金额」对子（`10/50 20/60`）先按组断行，
            // 否则整行只会认最后一组的金额，前面的金额会丢。
            let pair_lines = if all_special {
                split_special_amount_pairs(line, settings)
            } else {
                vec![line.clone()]
            };
            for line in pair_lines {
                if all_special {
                    let chunks = split_after_stakes(&line, settings);
                    if chunks.len() > 1 {
                        out.extend(chunks);
                        continue;
                    }
                }
                out.push(line);
            }
        }
        i = j;
        if i < lines.len() {
            out.push(String::new());
            i += 1;
        }
    }
    out
}

/// 号码行写在玩法行前面时（如三行：尾数列表 / 生肖列表 / 「三中三各1」），
/// 取段落里第一行「只写玩法、没写号码」的玩法，作为本段内容行的默认玩法。
/// 本段里作为「玩法头」的那一行：返回 (玩法名, 类型, 是否复式, 相对行下标)。
///
/// 空行是段落边界：玩法只在**本段内**向后找，不能跨空行借用下一段的玩法。
fn find_paragraph_play(
    lines: &[&str],
    settings: &KeywordSettings,
) -> Option<(String, PlayKind, bool, usize)> {
    for (idx, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            break;
        }
        if let Some((name, kind, union)) = play_from_header_line(line, settings) {
            return Some((name, kind, union, idx));
        }
    }
    None
}

/// 一行「玩法头」的玩法：纯玩法行，或号码写在玩法前面、玩法后没有号码。
fn play_from_header_line(
    line: &str,
    settings: &KeywordSettings,
) -> Option<(String, PlayKind, bool)> {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    let h = hits.last()?;
    let (_, cleaned) = extract_stakes(&without_region, settings, &hits, false);
    let picks = strip_all_play_keywords(&cleaned, settings);
    let content = normalize_content(&apply_variants(&picks, settings));
    let union = without_region.contains("复式");
    if content.is_empty() {
        // 纯玩法行。
        return Some((h.name.to_string(), h.kind, union));
    }
    // 玩法写在号码后面、且玩法之后没有号码（`0尾/蛇 每组各30，特碰每组各5`）：
    // 这个玩法是对前面号码的追加投注，也可以给本段更前面的内容行兜底。
    let before = &without_region[..h.start];
    let after = &without_region[h.end..];
    let (_, after_cleaned) = extract_stakes(after, settings, &[], false);
    let after_picks = strip_all_play_keywords(&after_cleaned, settings);
    if line_has_picks(before, settings)
        && normalize_content(&apply_variants(&after_picks, settings)).is_empty()
    {
        return Some((h.name.to_string(), h.kind, union));
    }
    None
}

/// 行末追加玩法（号码在前、玩法与金额在后，如 `0尾/蛇 每组各30，特碰每组各5`）
/// 的那笔金额：它可以给本段其它没写金额的内容行共用。
fn trailing_play_stake(line: &str, settings: &KeywordSettings) -> Option<Stake> {
    let expanded = expand_ranges(line, settings);
    let (without_region, _) = strip_regions(&expanded, settings);
    let hits = find_play_hits(&without_region, settings);
    let h = hits.last()?;
    let before = &without_region[..h.start];
    if !line_has_picks(before, settings) {
        return None;
    }
    // 只认「号码 + 前一块，追加玩法 金额」的合并写法：玩法前面必须有逗号。
    // 普通的一行「号码 玩法 金额」由本行自己的金额处理，不能当段落共享金额。
    let before_trimmed = before.trim_end();
    if !before_trimmed.ends_with('，') && !before_trimmed.ends_with(',') {
        return None;
    }
    let after = &without_region[h.end..];
    let (stakes, cleaned) = extract_stakes(after, settings, &[], false);
    let rest = strip_all_play_keywords(&cleaned, settings);
    if !normalize_content(&apply_variants(&rest, settings)).is_empty() {
        return None;
    }
    stakes.last().map(|s| s.stake)
}

fn find_paragraph_stake(lines: &[&str], settings: &KeywordSettings) -> Option<Stake> {
    let mut found: Option<Stake> = None;
    let mut fallback: Option<Stake> = None;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            break;
        }
        // 多行金额前缀（`A10`）也是本段共享金额；它可能写在号码行之前或之后。
        if let (_, Some(stake)) = strip_multi_line_stake(line, settings) {
            found = Some(stake);
        }
        let expanded = expand_ranges(line, settings);
        let (without_region, _) = strip_regions(&expanded, settings);
        if !has_content(&without_region) {
            continue;
        }
        let hits = find_play_hits(&without_region, settings);
        let (stakes, cleaned) = extract_stakes(&without_region, settings, &hits, false);
        let picks = strip_all_play_keywords(&cleaned, settings);
        let content = normalize_content(&apply_variants(&picks, settings));
            // 行末追加玩法的金额，可作为本段无金额内容行的共用金额。
        if let Some(st) = trailing_play_stake(line, settings) {
            fallback = Some(st);
        }
        if !content.is_empty() {
            // 有号码的行：写了点法时，金额是它自己的；没写玩法时，
            // 金额可以给同段其它没写金额的号码行共用（金额写在后面也认）。
            if hits.is_empty() {
                if let Some(h) = stakes.last() {
                    fallback = Some(h.stake);
                }
            }
            continue;
        }
        // 只有金额、没有号码的行优先作为整段金额。
        // 一行里出现多个金额、或带 `+` 的算术汇总（`计:150+230=380`）是校验，
        // 不是整段共享金额，否则会把这个总数套到前面每一行上。
        if stakes.len() == 1 && !line.contains('+') {
            let next = stakes[0].stake;
            // 「各 / 每」写的是每个号码的钱；后面的「计 / 共 / 总计」是整段
            // 总额（校验用），不能反过来覆盖已经写明的每注金额。例：
            //   1-2-3-4
            //   各40
            //   计960        ← 960 = 24 个号 × 40，不是每行 960
            let keep_explicit = matches!(next.kind, StakeKind::Total)
                && found.is_some_and(|s| matches!(s.kind, StakeKind::PerBet | StakeKind::PerGroup));
            if !keep_explicit {
                found = Some(next);
            }
        }
    }
    found.or(fallback)
}

/// 「生肖 / 尾数 + 行末裸数字」的写法（`鸡猪蛇鼠猴10`、`0尾1尾10`）：
/// 前面没有任何其它裸数字，行末这个数字只能是金额。
/// 有其它号码时（`9-19-42`）不适用，交给段落金额兜底。
fn line_has_only_trailing_amount(line: &str, settings: &KeywordSettings) -> bool {
    let runs = number_runs(line);
    let Some(&(start, end, _, cn)) = runs.last() else {
        return false;
    };
    if cn {
        return false;
    }
    let hits = find_play_hits(line, settings);
    if hits.iter().any(|h| start < h.end && end > h.start) {
        return false;
    }
    let after = line[end..].trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !is_line_end(after) {
        return false;
    }
    // 行末数字之前不能有其它裸露的阿拉伯数字（尾/头记号和玩法词里的不算）。
    let others_ok = runs[..runs.len() - 1].iter().all(|(s, e, _, cn)| {
        *cn
            || hits.iter().any(|h| *s < h.end && *e > h.start)
            || line[*e..].starts_with('尾')
            || line[*e..].starts_with('头')
    });
    if !others_ok {
        return false;
    }
    // 前面必须有生肖或尾数，才算「生肖列 + 金额」。
    let prefix = strip_all_play_keywords(&line[..start], settings);
    let content = normalize_content(&apply_variants(&prefix, settings));
    content
        .split(',')
        .filter(|s| !s.is_empty())
        .any(|t| is_single_zodiac(t) || is_tail_token(t))
}

/// `15.20.25三中三30`：去掉行末数字后，号码刚好凑齐组合档位，
/// 那这个数字只能是金额（即使同段其它行写了金额，也不该把它当第 4 个号码）。
fn trailing_amount_completes_combo(line: &str, settings: &KeywordSettings) -> bool {
    let hits = find_play_hits(line, settings);
    if hits.len() != 1 {
        return false;
    }
    let size = match hits[0].kind {
        PlayKind::Combo(size) => size,
        _ => return false,
    };
    let Some((start, end, _, cn)) = number_runs(line).pop() else {
        return false;
    };
    if cn {
        return false;
    }
    // 数字必须在玩法关键词之后、并且是行尾。
    if hits.iter().any(|h| start < h.end && end > h.start) {
        return false;
    }
    if hits.iter().map(|h| h.end).max().unwrap_or(0) > start {
        return false;
    }
    let after = line[end..]
        .trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !is_line_end(after) {
        return false;
    }
    let mut rest = line.to_string();
    rest.replace_range(start..end, "");
    let (without_region, _) = strip_regions(&rest, settings);
    let rest_hits = find_play_hits(&without_region, settings);
    if rest_hits.len() != 1 {
        return false;
    }
    let (stakes, cleaned) = extract_stakes(&without_region, settings, &rest_hits, false);
    if !stakes.is_empty() {
        return false;
    }
    let picks = strip_all_play_keywords(&cleaned, settings);
    let content = normalize_content(&apply_variants(&picks, settings));
    let tokens: Vec<&str> = content
        .split(',')
        .filter(|t| !t.is_empty())
        .collect();
    tokens.len() == size
        && tokens
            .iter()
            .all(|t| t.chars().all(|c| c.is_ascii_digit()))
}

/// 段落头行写了组合玩法、本行只写号码和行末金额（`12.36.34/20`）：
/// 去掉行末数字后还剩够这个玩法的号码数时，行末数字就是本行每注金额。
///
/// 与参考项目一致：段落里没有任何显式每注金额时（`各10` 之类），号码行自己的
/// 行末数字优先于段落里的「计50 / 合计50」这类总额。
fn trailing_amount_completes_inherited_combo(
    line: &str,
    play: &PlayKind,
    settings: &KeywordSettings,
) -> bool {
    let PlayKind::Combo(size) = play else {
        return false;
    };
    // 本行自己写了玩法的交给 trailing_amount_completes_combo。
    if !find_play_hits(line, settings).is_empty() {
        return false;
    }
    let Some((start, end, _, cn)) = number_runs(line).pop() else {
        return false;
    };
    if cn {
        return false;
    }
    if line[end..].starts_with('尾') || line[end..].starts_with('头') {
        return false;
    }
    let after = line[end..]
        .trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !is_line_end(after) {
        return false;
    }
    let mut rest = line.to_string();
    rest.replace_range(start..end, "");
    let (without_region, _) = strip_regions(&rest, settings);
    let (stakes, cleaned) = extract_stakes(&without_region, settings, &[], false);
    if !stakes.is_empty() {
        return false;
    }
    let picks = strip_all_play_keywords(&cleaned, settings);
    let content = normalize_content(&apply_variants(&picks, settings));
    let mut unique: Vec<&str> = Vec::new();
    for t in content.split(',').filter(|t| !t.is_empty()) {
        if !unique.contains(&t) {
            unique.push(t);
        }
    }
    unique.len() >= *size
        && unique
            .iter()
            .all(|t| t.chars().all(|c| c.is_ascii_digit()))
}

/// 本段里有没有任何一行写了显式每注金额（各 / 每组 / 元 / A 前缀…）。
/// 总额（合计 / 计50）不算：参考项目里号码行行末自己的数字优先于段落总额。
fn paragraph_has_explicit_stake(lines: &[&str], settings: &KeywordSettings) -> bool {
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            break;
        }
        if strip_multi_line_stake(line, settings).1.is_some() {
            return true;
        }
        let expanded = expand_ranges(line, settings);
        let (without_region, _) = strip_regions(&expanded, settings);
        let hits = find_play_hits(&without_region, settings);
        let (stakes, _) = extract_stakes(&without_region, settings, &hits, false);
        if stakes
            .iter()
            .any(|h| !matches!(h.stake.kind, StakeKind::Total))
        {
            return true;
        }
    }
    false
}

/// 行末最后一个数字是否大于 49：它不可能是号码，只能是金额。
fn trailing_number_over_49(line: &str) -> bool {
    let Some((_, end, value, cn)) = number_runs(line).pop() else {
        return false;
    };
    if cn || value <= 49.0 {
        return false;
    }
    let after = line[end..].trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    is_line_end(after)
}

/// 转换一段注单原文（不带替换词，供测试使用）。
#[cfg(test)]
pub fn convert(raw: &str, settings: &KeywordSettings) -> ConvertOutput {
    convert_with_replacements(raw, settings, &[])
}

/// 带「替换词」规则的转换：先按规则替换原文，再走正常识别流程。
pub fn convert_with_replacements(
    raw: &str,
    settings: &KeywordSettings,
    replacements: &[crate::replacements::Rule],
) -> ConvertOutput {
    let mut out = ConvertOutput::default();

    let mut region = Region::Macau;
    let mut play = ("特码".to_string(), PlayKind::Special);
    let mut shared_stake: Option<Stake> = None;
    // 当前段落的玩法行是否写了「复式」（特碰/二中二不自动插拖）。
    let mut says_union = false;
    // 存疑按段落判定：整段都是特码时不存疑。
    let mut paragraph_all_special = true;
    let mut paragraph_doubtful = false;
    let mut paragraph_reasons: Vec<String> = Vec::new();
    // 段落里单独一行写了总额（计/共/总共/总计）→ 金额类存疑全部免除。
    let mut paragraph_standalone_total = false;
    let mut paragraph_play_no_amount = false;
    let mut paragraph_play_reasons: Vec<String> = Vec::new();
    // 本段至少有一行同时写了玩法、号码和金额。
    let mut paragraph_has_complete_line = false;
    // 本段是否已经出现过玩法（头行或本行），供号码行写金额时判定三要素。
    let mut paragraph_has_play = false;
    // 本段向后找到的玩法头（用于号码行在前、玩法行在后的写法）及其行下标。
    // 注意：带自己金额的号码行不向后继承这个玩法，会按默认特码处理。
    let mut paragraph_play: Option<(String, PlayKind, bool)> = None;
    let mut paragraph_play_header: Option<usize> = None;

    // 一行出现三个以上「拖」时，先按用户口径把这行的「拖」全换成「.」：
    // 三个以上的对乘符号多半是敲错了，不再自动按拖注展开，交人工复核。
    let (raw_checked, excess_drag) = replace_excess_drag(raw, settings);

    // 先做统一预处理：替换词、组数说明、组分隔符，全特码段落按金额断行。
    let prepared = prepare_lines(&raw_checked, settings, replacements);
    let prepared_refs: Vec<&str> = prepared.iter().map(String::as_str).collect();
    let mut paragraph_start = true;
    let mut paragraph_stake: Option<Stake> = None;
    // 本段有没有显式每注金额（决定号码行行末数字能否当本行金额，见
    // trailing_amount_completes_inherited_combo）。
    let mut paragraph_has_stake = false;
    // 本段的玩法头行自己写了显式每注金额（`三中三各5`）。
    let mut paragraph_header_stake = false;
    // 本段「只写玩法」的头行里列出的多个玩法（如「二中二，三中三，特碰」），
    // 后面的号码行要按每个玩法各出一注；玩法后面紧跟的金额也一起记住。
    let mut paragraph_plays: Vec<(String, PlayKind, Option<Stake>)> = Vec::new();
    // 本段是否已经出现过号码表达式（用于判断「玩法+金额」是不是落单）。
    let mut paragraph_has_expression = false;
    // 整段是否只有特码（含没写玩法的号码行、下拉默认特码）：
    // 决定「号码/金额」对子（`10/20`）里斜杠后面的数字是不是金额。
    let mut paragraph_only_special = false;

    for (line_idx, prepared_line) in prepared_refs.iter().enumerate() {
        let line_no = line_idx + 1;
        let (line_owned, multi_stake) = strip_multi_line_stake(prepared_line, settings);
        let line = line_owned.trim();
        if line.is_empty() {
            // `A10` 这种多行金额前缀单独成行：只更新共享金额，不是段落边界。
            if let Some(stake) = multi_stake {
                shared_stake = Some(stake);
                continue;
            }
            // 空行是段落边界：玩法与段落金额不跨段继承。
            finalize_doubt(
                &mut out,
                paragraph_all_special,
                paragraph_doubtful,
                &mut paragraph_reasons,
                paragraph_play_no_amount,
                &mut paragraph_play_reasons,
                paragraph_standalone_total,
                paragraph_has_complete_line,
            );
            play = ("特码".to_string(), PlayKind::Special);
            shared_stake = None;
            says_union = false;
            paragraph_all_special = true;
            paragraph_doubtful = false;
            paragraph_standalone_total = false;
            paragraph_play_no_amount = false;
            paragraph_has_complete_line = false;
            paragraph_has_play = false;
            paragraph_play = None;
            paragraph_play_header = None;
            paragraph_start = true;
            paragraph_stake = None;
            paragraph_has_stake = false;
            paragraph_header_stake = false;
            paragraph_plays.clear();
            paragraph_has_expression = false;
            paragraph_only_special = false;
            continue;
        }

        if paragraph_start {
            paragraph_stake = find_paragraph_stake(&prepared_refs[line_idx..], settings);
            paragraph_has_stake = paragraph_has_explicit_stake(&prepared_refs[line_idx..], settings);
            // 段落里只要有一行写了别的玩法，整段就不算全特码。
            paragraph_only_special = prepared_refs[line_idx..]
                .iter()
                .take_while(|l| !l.trim().is_empty())
                .all(|l| line_is_all_special(l, settings));
            // 号码（生肖 / 尾数）行写在玩法行前面时，先用段落里的玩法行兜底，
            // 让前面的内容行也能继承到「三中三 / 特碰」这类玩法。
            if let Some((name, kind, union, rel)) =
                find_paragraph_play(&prepared_refs[line_idx..], settings)
            {
                play = (name.clone(), kind);
                says_union = union;
                paragraph_has_play = true;
                paragraph_play = Some((name, kind, union));
                paragraph_play_header = Some(line_idx + rel);
            }
            paragraph_start = false;
        }

        let expanded = expand_ranges(line, settings);
        let (without_region, regions) = strip_regions(&expanded, settings);
        if let Some(r) = regions.first() {
            region = *r;
        }

        // 纯地区头行（「香港」「澳门」）只切换地区，不产出注单。
        if !has_content(&without_region) {
            continue;
        }

        let type_hits = find_play_hits(&without_region, settings);
        if let Some(h) = type_hits.last() {
            play = (h.name.to_string(), h.kind);
            says_union = without_region.contains("复式");
        }
        if !type_hits.is_empty() {
            paragraph_has_play = true;
        }

        // `A10` 这种多行金额前缀：本行和后面几行都先用它，直到遇到新的金额。
        // 要在摘金额之前记下，否则行末号码会被猜成金额。
        if let Some(stake) = multi_stake {
            shared_stake = Some(stake);
        }

        // 全特码段里的「号码/金额」对子（`特码 10/20`）：斜杠后面一律是金额，
        // 即使不大于 49，也不受段里别处写的金额影响。
        let slash_pair =
            paragraph_only_special && is_slash_amount_pair_line(&without_region, settings);
        // 段落里已经有单独的金额行（或共享金额）时，行末裸数字不再猜成金额（它们是号码）。
        let (stake_hits, stake_cleaned) = extract_stakes(
            &without_region,
            settings,
            &type_hits,
            (paragraph_stake.is_none() && shared_stake.is_none() && multi_stake.is_none())
                || trailing_number_over_49(&without_region)
                || line_has_only_trailing_amount(&without_region, settings)
                || trailing_amount_completes_combo(&without_region, settings)
                || slash_pair
                || (!paragraph_has_stake
                    && trailing_amount_completes_inherited_combo(
                        &without_region,
                        &play.1,
                        settings,
                    )),
        );
        // 本行自己写了金额、而玩法头在它后面时，不向后继承那个玩法：
        // 按默认特码处理，让「同注单混玩法」规则提示人工复核。
        // 没写金额的行仍然可以继承后面的玩法（给「号码在前、玩法在后」用）。
        let own_amount = !stake_hits.is_empty();
        if type_hits.is_empty() {
            if let Some((name, kind, union)) = &paragraph_play {
                if paragraph_play_header.is_some_and(|header| line_idx < header) {
                    if own_amount {
                        play = ("特码".to_string(), PlayKind::Special);
                        says_union = false;
                    } else {
                        play = (name.clone(), *kind);
                        says_union = *union;
                    }
                }
            }
        }
        if !matches!(play.1, PlayKind::Special) {
            paragraph_all_special = false;
        }

        // 先取「本行之前」的段落金额，再用本行自己的金额更新继承值。
        let fallback_stake = paragraph_stake.or(shared_stake);
        if let Some(h) = stake_hits.last() {
            // 对子自己的金额只属于这一行，不借给同段其它行。
            if !slash_pair {
                shared_stake = Some(h.stake);
            }
        }
        let content_text = strip_all_play_keywords(&stake_cleaned, settings);
        // 交集写法（`虎牛狗v双`）在金额摘掉之后展开成号码。
        let content_text = expand_intersections(&content_text, settings);

        // `猪拖虎拖鼠各1`：拖注但整段都没写玩法，无法确定是二中二还是三中三。
        if type_hits.is_empty() && !paragraph_has_play {
            if let Some(groups) = bare_drag_groups(&content_text, settings) {
                out.doubtful = true;
                out.reasons.push(format!(
                    "第 {line_no} 行：拖注「{}」没写玩法，请确认是「二中二」还是「三中三」",
                    groups.join("拖")
                ));
            }
        }

        // `特碰21-31，20`：逗号后面的裸数字被当成金额，但它也可能是一个号码。
        if let Some(msg) = ambiguous_trailing_amount(&without_region, &stake_hits, &type_hits, settings) {
            out.doubtful = true;
            out.reasons.push(format!("第 {line_no} 行：{msg}"));
        }

        // 转换过程中出现认不出的词（既不是号码/生肖/属性词，也不是玩法、金额、
        // 地区、复式或分隔符）→ 存疑。
        let bad_words = useless_words(&content_text, settings, replacements);
        if !bad_words.is_empty() {
            out.doubtful = true;
            out.reasons.push(format!(
                "第 {line_no} 行：无用词汇「{}」",
                bad_words.join("、")
            ));
        }

        // 「玩法 + 金额」紧挨着写的对子（`三中三 100 二中二 50`）：
        // 每个玩法各用自己后面的金额；一个表达式可以带多个这样的对子。
        let (pair_stakes, _) = if type_hits.is_empty() {
            (Vec::new(), false)
        } else {
            pair_stakes_with_hits(&without_region, &type_hits, settings)
        };

        // 金额必须写在玩法后面：出现在第一个玩法之前的金额要人工复核。
        // 带标记的金额直接看位置；裸数字只有大于 49（不可能是号码）才算金额。
        let amount_before_play = type_hits.first().is_some_and(|h| {
            stake_hits.iter().any(|s| s.pos < h.start)
                || without_region
                    .get(..h.start)
                    .is_some_and(|pre| {
                        number_runs(pre)
                            .iter()
                            .any(|(_, _, v, cn)| !cn && *v > 49.0)
                    })
        });
        if amount_before_play {
            out.doubtful = true;
            out.reasons
                .push(format!("第 {line_no} 行：金额写在了玩法前面"));
        }

        // 号码区可能写成多组（20.04.01，37.36.16，…），每组单独成注。
        let groups = split_pick_groups(&content_text, settings);
        let has_content = groups
            .iter()
            .any(|g| !normalize_content(&apply_variants(g, settings)).is_empty());
        if has_content {
            paragraph_has_expression = true;
        }
        if !has_content {
            // 单独一行只写总额（共100 / 总计100…）：这一段的金额由它统一提供，
            // 其余行缺金额不再判存疑。
            if type_hits.is_empty()
                && stake_hits
                    .iter()
                    .any(|h| matches!(h.stake.kind, StakeKind::Total))
            {
                paragraph_standalone_total = true;
            }
            // 「三中三 各5」这类只写玩法与金额的行：记住金额，供后面的号码行使用。
            // 一行列了多个玩法时全部记住（「二中二，三中三，特碰」）。
            if !type_hits.is_empty() {
                paragraph_plays = type_hits
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        (
                            h.name.to_string(),
                            h.kind,
                            pair_stakes.get(i).copied().flatten(),
                        )
                    })
                    .collect();
                let header_pair = pair_stakes.iter().any(|s| s.is_some());
                if header_pair
                    || stake_hits
                        .iter()
                        .any(|h| matches!(h.stake.kind, StakeKind::PerBet))
                {
                    paragraph_header_stake = true;
                }
                // 落单：这行的玩法+金额前面（同一行或更前面的行）没有号码，
                // 只能取后面最近的号码，交人工复核。
                if header_pair && !paragraph_has_expression {
                    let later_has_picks = prepared_refs[line_idx + 1..]
                        .iter()
                        .take_while(|l| !l.trim().is_empty())
                        .any(|l| line_has_picks(l, settings));
                    out.doubtful = true;
                    out.reasons.push(if later_has_picks {
                        format!(
                            "第 {line_no} 行：玩法+金额前面没有号码（落单），已取后面最近的号码"
                        )
                    } else {
                        format!(
                            "第 {line_no} 行：玩法+金额前面没有号码（落单），后面也没有可用的号码"
                        )
                    });
                }
            }
            continue;
        }

        // 这一行是否满足「玩法 + 号码 + 金额」三要素。玩法可以来自本段的
        // 玩法头行（`特碰\n40-45\n04-45各5`），只要号码行写了显式金额即可。
        // 玩法头行自己写了显式每注金额、号码分行写（`三中三各5` ↵ 号码行）
        // 也算「同时写出玩法、号码和金额」，不计存疑。
        if (!stake_hits.is_empty() && paragraph_has_play) || paragraph_header_stake {
            paragraph_has_complete_line = true;
        }

        // 存疑：有号码却没有金额（玩法可以沿用同一段的写法）。
        // 先记在段落上：整段都是特码时，最后不把它算成存疑。
        if stake_hits.is_empty() && fallback_stake.is_none() {
            paragraph_doubtful = true;
            paragraph_reasons.push(format!("第 {line_no} 行：缺少金额"));
        }

        // 行末裸数字猜出来的金额，有些写法可以确认它只能是金额，不用存疑：
        // - 组合玩法刚好写齐了号码个数（`1.2,3三中三10`）；
        // - 平特只写了一个生肖 / 尾数（`平特蛇200`、`平特0尾200`）。
        //   写两个以上生肖 / 尾数时（`平特蛇虎200`）分不清 200 是不是金额，
        //   仍然按「玩法后面没有写金额」存疑。
        let guessed_stake_ok = if type_hits.len() == 1
            && stake_hits.len() == 1
            && matches!(stake_hits[0].stake.kind, StakeKind::Guessed)
            && groups.len() == 1
        {
            let normalized = normalize_content(&apply_variants(&groups[0], settings));
            let picks: Vec<&str> = normalized
                .split(',')
                .filter(|s| !s.is_empty())
                .collect();
            match type_hits[0].kind {
                PlayKind::Combo(size) => {
                    let mut unique: Vec<&str> = Vec::new();
                    for t in &picks {
                        if !unique.contains(t) {
                            unique.push(t);
                        }
                    }
                    let all_numeric = unique
                        .iter()
                        .all(|t| t.chars().all(|c| c.is_ascii_digit()));
                    all_numeric && unique.len() == size
                }
                PlayKind::FlatSpecial => {
                    picks.len() == 1
                        && (is_single_zodiac(picks[0]) || is_tail_token(picks[0]))
                }
                _ => false,
            }
        } else {
            false
        };

        // 写了玩法却没有写显式金额（行末裸数字猜出来的不算）→ 存疑。
        // 全特码段的「号码/金额」对子（`特码 10/20`）例外：斜杠后面一定是金额。
        if !type_hits.is_empty()
            && !guessed_stake_ok
            && !slash_pair
            && fallback_stake.is_none()
            && !stake_hits
                .iter()
                .any(|h| !matches!(h.stake.kind, StakeKind::Guessed))
        {
            paragraph_play_no_amount = true;
            let plays: Vec<String> =
                type_hits.iter().map(|h| h.name.to_string()).collect();
            paragraph_play_reasons.push(format!(
                "第 {line_no} 行：玩法「{}」后面没有写金额",
                plays.join("、")
            ));
        }

        // 一行写了多个玩法时（三中三二中二特碰），每个玩法各出一注；
        // 金额按出现位置分配：`三中三 每组5块，二中二 每组10块` 各用各的。
        let line_stake = stake_hits.last().map(|h| h.stake);
        let types: Vec<(String, PlayKind, Option<Stake>)> = if type_hits.is_empty() {
            if paragraph_plays.is_empty() {
                vec![(play.0.clone(), play.1, line_stake.or(fallback_stake))]
            } else {
                // 前面有一行玩法头，且列了多个玩法：每个玩法都用这组号码和金额。
                paragraph_plays
                    .iter()
                    .map(|(name, kind, header_stake)| {
                        (
                            name.clone(),
                            *kind,
                            line_stake.or(*header_stake).or(fallback_stake),
                        )
                    })
                    .collect()
            }
        } else {
            // 本行自己写了玩法，后续号码行不再沿用之前的玩法头。
            paragraph_plays.clear();
            type_hits
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    // 玩法后面紧跟的金额优先（`三中三 100 二中二 50` 各用各的）。
                    let stake = pair_stakes
                        .get(i)
                        .copied()
                        .flatten()
                        .or_else(|| stake_for_type(&type_hits, &stake_hits, i))
                        .or(fallback_stake);
                    (h.name.to_string(), h.kind, stake)
                })
                .collect()
        };

        let regions_to_emit: Vec<Region> = if regions.is_empty() {
            vec![region]
        } else {
            regions.clone()
        };

        // 多个号码组只用空格隔开（`1+18     1+24     24+18`）：
        // 每组各出一注，共用本行玩法与金额。
        if !types.is_empty()
            && types
                .iter()
                .all(|(_, kind, _)| matches!(kind, PlayKind::Combo(_)))
        {
            let drag_sep = settings
                .get(Field::Multiplication)
                .iter()
                .find(|k| !k.is_empty())
                .cloned()
                .unwrap_or_else(|| "拖".to_string());
            // 已经有逗号 / 顿号等组分隔符时，交给 split_pick_groups / plan_groups；
            // 空格分组只处理「完全没有逗号类分隔符」的写法。
            let has_comma_groups = content_text
                .chars()
                .any(|c| matches!(c, '，' | '、' | '；' | ';' | '|' | '｜' | '/' | '／'));
            if !content_text.contains(&drag_sep) && !has_comma_groups {
                if let Some(space_groups) = split_space_groups(&content_text) {
                    for group in &space_groups {
                        for (name, kind, type_stake) in &types {
                            emit_bet(
                                &mut out,
                                &regions_to_emit,
                                name,
                                *kind,
                                group,
                                (*type_stake).or(fallback_stake),
                            );
                        }
                    }
                    continue;
                }
            }
        }

        // 组合玩法 + 生肖/尾数分组：自动插入「拖」，按互碰计算。
        // 一行里写了多个组合玩法时（如「二星…特碰…」），每个玩法各出一条拖注；
        // 只要有一个玩法的档位凑不齐（如三中三只有两组）、或写了「复式」，
        // 整行就不插拖，保持原来的按组输出。
        if !types.is_empty()
            && types
                .iter()
                .all(|(_, kind, _)| matches!(kind, PlayKind::Combo(_)))
        {
            let separator = settings
                .get(Field::Multiplication)
                .iter()
                .find(|k| !k.is_empty())
                .cloned()
                .unwrap_or_else(|| "拖".to_string());
            // 用户已经明确写了「拖」：按分隔符拆组，每组内部排序、补零后原样输出。
            // 组数不够交给 validate_bets 报失败，不在这里吞掉。
            if let Some(drag_groups) =
                explicit_drag_groups(&content_text, settings, &separator)
            {
                // 组数必须和每个玩法的组合大小一致，才是真正的拖注；
                // 例如 X 被替换成「拖」后得到的 4 组号码，对二中二 / 三中三 /
                // 特碰都不成立，应按普通号码列表处理，不能排序后强行展开。
                let can_drag = types.iter().all(|(_, kind, _)| {
                    matches!(kind, PlayKind::Combo(size) if *size == drag_groups.len())
                });
                if can_drag {
                    for (name, kind, type_stake) in &types {
                        emit_combo_groups(
                            &mut out,
                            &regions_to_emit,
                            name,
                            *kind,
                            &drag_groups,
                            (*type_stake).or(fallback_stake),
                            &separator,
                        );
                    }
                    continue;
                }
            }
            if let Some(drag_groups) = auto_drag_groups(&content_text) {
                // 写了「复式」（或继承了复式玩法行）时一律不插拖。
                let can_drag = !says_union
                    && types.iter().all(|(_, kind, _)| {
                        matches!(kind, PlayKind::Combo(size) if *size == drag_groups.len())
                    });
                if can_drag {
                    for (name, _, type_stake) in &types {
                        emit_drag_bet(
                            &mut out,
                            &regions_to_emit,
                            name,
                            &drag_groups,
                            (*type_stake).or(fallback_stake),
                            &separator,
                        );
                    }
                    continue;
                }
            }
        }

        let line_stake = stake_hits.last().map(|h| h.stake);
        let group_plan = if groups.len() >= 2 {
            plan_groups(&without_region, settings, line_stake, groups.len())
        } else {
            None
        };

        if let Some(plan) = group_plan {
            // 多组：每组用自己的号码与金额；玩法沿用整行识别到的类型。
            for (content, group_stake) in &plan {
                for (base_name, base_kind, type_stake) in &types {
                    // 组内没写金额时用整行/整段识别到的金额。
                    let stake = (*group_stake).or(*type_stake);
                    emit_bet(
                        &mut out,
                        &regions_to_emit,
                        base_name,
                        *base_kind,
                        content,
                        stake,
                    );
                }
            }
        } else {
            for group in &groups {
                let content = normalize_content(&apply_variants(group, settings));
                if content.is_empty() {
                    continue;
                }
                for (base_name, base_kind, type_stake) in &types {
                    emit_bet(
                        &mut out,
                        &regions_to_emit,
                        base_name,
                        *base_kind,
                        &content,
                        *type_stake,
                    );
                }
            }
        }
    }
    finalize_doubt(
        &mut out,
        paragraph_all_special,
        paragraph_doubtful,
        &mut paragraph_reasons,
        paragraph_play_no_amount,
        &mut paragraph_play_reasons,
        paragraph_standalone_total,
        paragraph_has_complete_line,
    );

    // 明细有数据时，校验玩法与内容是否匹配；不匹配按失败处理。
    if !out.bets.is_empty() {
        let problems = validate_bets(&out.bets, settings);
        if !problems.is_empty() {
            out.invalid = true;
            out.reasons.extend(problems);
        }
    }

    // 单注没有数值（金额为空 / 0）→ 直接失败，不能当作可提交的注单。
    let missing_stake = out.bets.iter().filter(|b| b.amount <= 0.0).count();
    if missing_stake > 0 {
        out.invalid = true;
        out.reasons
            .push(format!("有 {missing_stake} 注没有填写金额"));
    }

    // 同一注单里混用两类以上玩法（小码 / 特码 / 连肖 / 平特 / 平码…）→ 存疑。
    // 「小码」内部的 三中三 / 三中二 / 特碰 / 二中二 算同一类，可以同时出现。
    let mut families: Vec<&'static str> = Vec::new();
    for b in &out.bets {
        let family = play_family(&b.play);
        if !families.contains(&family) {
            families.push(family);
        }
    }
    if families.len() >= 2 {
        out.doubtful = true;
        out.reasons.push(format!(
            "同一注单出现多种玩法（{}），请人工复核",
            families.join("、")
        ));
    }

    if !excess_drag.is_empty() {
        out.doubtful = true;
        out.reasons.extend(excess_drag);
    }

    // 去重并按出现顺序保留；失败时补一条总原因。
    let mut seen: Vec<String> = Vec::new();
    for r in out.reasons.drain(..) {
        if !seen.contains(&r) {
            seen.push(r);
        }
    }
    if out.bets.is_empty() {
        seen.insert(0, "转换结果为空".to_string());
    }
    out.reasons = seen;

    out
}

/// 输出明细里的玩法类别（按规范化后的玩法名判断）。
#[derive(Clone, Copy)]
enum PlayRule {
    Combo(usize),
    Special,
    FlatNumber,
    FlatSpecial,
    LianXiao(usize),
    LianWei(usize),
    BuZhong(usize),
    ZhongYi(usize),
    LiuXiao,
}

fn play_rule(play: &str) -> Option<PlayRule> {
    match play {
        "三中三" | "三中二" => return Some(PlayRule::Combo(3)),
        "二中二" | "二全中" | "特碰" => return Some(PlayRule::Combo(2)),
        "特码" => return Some(PlayRule::Special),
        "平码" => return Some(PlayRule::FlatNumber),
        "平特" => return Some(PlayRule::FlatSpecial),
        "六肖中一" => return Some(PlayRule::LiuXiao),
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
        return Some(PlayRule::LianXiao(n));
    }
    if let Some(n) = head_size("连尾") {
        return Some(PlayRule::LianWei(n));
    }
    if let Some(n) = head_size("不中") {
        return Some(PlayRule::BuZhong(n));
    }
    if let Some(n) = head_size("中一") {
        return Some(PlayRule::ZhongYi(n));
    }
    None
}

/// 判断「混玩法」用的大类。
///
/// 用户口径：三中三 / 三中二 / 特碰 / 二中二 统称**小码**，它们同时出现不算混玩法；
/// 小码 / 特码 / 连肖 / 平特 / 平码 任意两类同时出现才需要人工复核。
fn play_family(play: &str) -> &'static str {
    match play {
        "三中三" | "三中二" | "特碰" | "二中二" | "二全中" => "小码",
        "特码" => "特码",
        "平码" => "平码",
        // 风险页把四中一 / 五中一和平特放在同一栏。
        "平特" | "四中一" | "五中一" => "平特",
        "六肖中一" => "六肖中一",
        _ if play.ends_with("连肖") => "连肖",
        _ if play.ends_with("连尾") => "连尾",
        _ if play.ends_with("不中") => "不中",
        _ if play.ends_with("中一") => "中一",
        _ => "其他",
    }
}

fn is_number_token(t: &str) -> bool {
    !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit())
        && t.parse::<u32>().is_ok_and(|n| (1..=49).contains(&n))
}

fn is_tail_token(t: &str) -> bool {
    t.strip_suffix('尾')
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

fn is_single_zodiac(t: &str) -> bool {
    let mut it = t.chars();
    it.next().is_some_and(is_zodiac) && it.next().is_none()
}

/// 校验每条明细的「玩法 ↔ 内容」关系，返回失败原因。
///
/// 规则对齐参考项目：组合玩法看号码/分组数，平特看生肖尾数或码组，
/// 连肖 / 连尾看档位数量，平码只允许数字。
fn validate_bets(bets: &[BetItem], settings: &KeywordSettings) -> Vec<String> {
    let mul = settings
        .get(Field::Multiplication)
        .iter()
        .find(|k| !k.is_empty())
        .cloned()
        .unwrap_or_else(|| "拖".to_string());
    let mut problems: Vec<String> = Vec::new();

    for bet in bets {
        let Some(rule) = play_rule(&bet.play) else {
            continue;
        };
        // 组合玩法会先把生肖展开成号码用于展示；校验仍看展开前的写法。
        let validate_text = bet.validate_content.as_deref().unwrap_or(&bet.content);
        let tokens: Vec<&str> = validate_text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let numeric = tokens.iter().filter(|t| is_number_token(t)).count();
        let zodiac = tokens.iter().filter(|t| is_single_zodiac(t)).count();
        let tail = tokens.iter().filter(|t| is_tail_token(t)).count();
        // 拖注：`(龙)拖(虎)拖(狗)` 按分组数校验。
        let drag_groups = if !mul.is_empty() && bet.content.contains(mul.as_str()) {
            bet.content
                .split(mul.as_str())
                .filter(|s| !s.trim().is_empty())
                .count()
        } else {
            0
        };
        let fail = |problems: &mut Vec<String>, reason: String| {
            let msg = format!("{}：{}", bet.play, reason);
            if !problems.contains(&msg) {
                problems.push(msg);
            }
        };

        match rule {
            PlayRule::Combo(size) => {
                // 重复号码只算一个：`01,01,02` 只有 2 个号码。
                let mut unique_tokens: Vec<&str> = Vec::new();
                for t in &tokens {
                    if !unique_tokens.contains(t) {
                        unique_tokens.push(t);
                    }
                }
                if drag_groups > 0 {
                    if drag_groups < size {
                        fail(
                            &mut problems,
                            format!("拖注至少要 {size} 个分组，当前只有 {drag_groups} 个"),
                        );
                    }
                } else if zodiac == 0 && tail == 0 && unique_tokens.len() < size {
                    // 生肖 / 尾数本身覆盖多个号码：只要写了至少一个就够，
                    // 和数字一起写也允许；纯数字则要凑够档位。
                    fail(
                        &mut problems,
                        format!(
                            "至少要 {size} 个号码或至少 1 个生肖，当前只有 {} 个号码",
                            unique_tokens.len()
                        ),
                    );
                }
            }
            PlayRule::Special => {
                if tokens.is_empty() {
                    fail(&mut problems, "至少要 1 个号码或属性词".into());
                }
            }
            PlayRule::FlatNumber => {
                if tokens.is_empty() {
                    fail(&mut problems, "至少要 1 个号码".into());
                } else if let Some(bad) = tokens.iter().find(|t| !is_number_token(t)) {
                    fail(
                        &mut problems,
                        format!("平码只能是数字（1-49），不能有「{bad}」"),
                    );
                }
            }
            PlayRule::FlatSpecial => {
                let has_zodiac = zodiac > 0;
                let has_tail = tail > 0;
                if (has_zodiac || has_tail) && numeric > 0 {
                    fail(&mut problems, "平特不支持生肖/尾数与数字混写".into());
                } else if !has_zodiac && !has_tail && numeric == 0 {
                    fail(
                        &mut problems,
                        "平特需要生肖、尾数或 4/5 个号码".into(),
                    );
                } else if !has_zodiac && !has_tail && numeric != 4 && numeric != 5 {
                    // 数字玩法只有 4 码组 / 5 码组两档，其它个数没有对应赔率。
                    fail(
                        &mut problems,
                        format!(
                            "平特数字玩法只能刚好 4 个或 5 个号码，当前有 {numeric} 个"
                        ),
                    );
                }
            }
            PlayRule::LianXiao(n) => {
                let pure_zodiac = !tokens.is_empty() && zodiac == tokens.len();
                let pure_number = !tokens.is_empty() && numeric == tokens.len();
                if !pure_zodiac && !pure_number {
                    fail(&mut problems, "连肖只能是纯生肖或纯数字".into());
                } else if zodiac < n && numeric < n {
                    fail(
                        &mut problems,
                        format!(
                            "{n}连肖至少要 {n} 个生肖或 {n} 个号码，当前只有 {zodiac} 个生肖、{numeric} 个号码"
                        ),
                    );
                }
            }
            PlayRule::LianWei(n) => {
                if tail < n && numeric < n {
                    fail(
                        &mut problems,
                        format!(
                            "{n}连尾至少要 {n} 个尾数或 {n} 个号码，当前只有 {tail} 个尾数、{numeric} 个号码"
                        ),
                    );
                }
            }
            PlayRule::BuZhong(n) => {
                // 生肖 / 尾数 / 属性词都算覆盖到的号码。
                let mut nums: Vec<u32> = Vec::new();
                for t in &tokens {
                    for num in crate::settle::token_numbers(t) {
                        if !nums.contains(&num) {
                            nums.push(num);
                        }
                    }
                }
                if nums.len() < n {
                    fail(
                        &mut problems,
                        format!(
                            "至少要 {n} 个号码，当前只有 {} 个",
                            nums.len()
                        ),
                    );
                }
            }
            PlayRule::ZhongYi(n) => {
                if n >= 4 {
                    if numeric != n {
                        fail(
                            &mut problems,
                            format!("必须刚好 {n} 个号码，当前有 {numeric} 个"),
                        );
                    }
                } else if numeric < n {
                    fail(
                        &mut problems,
                        format!("至少要 {n} 个号码，当前只有 {numeric} 个"),
                    );
                }
            }
            PlayRule::LiuXiao => {
                let distinct: BTreeSet<char> =
                    bet.content.chars().filter(|c| is_zodiac(*c)).collect();
                if distinct.len() != 6 {
                    fail(
                        &mut problems,
                        format!("必须正好 6 个不同的生肖，当前有 {} 个", distinct.len()),
                    );
                }
            }
        }
    }

    problems
}

/// 把转换后的一行拆成「内容 / 金额」两列。
/// 内容 = 转换结果去掉彩种、玩法、金额之后剩下的部分。
fn split_columns(line: &str, region_char: &str, play: &str) -> (String, String) {
    let prefix = format!("{region_char} {play} ");
    let rest = line.strip_prefix(&prefix).unwrap_or(line).trim();
    for marker in [" 各数 ", " 各组 "] {
        if let Some(pos) = rest.rfind(marker) {
            return (
                rest[..pos].trim().to_string(),
                rest[pos + 1..].trim().to_string(),
            );
        }
    }
    if let Some(pos) = rest.rfind('共') {
        return (rest[..pos].trim().to_string(), rest[pos..].trim().to_string());
    }
    (rest.to_string(), "—".to_string())
}

/// 去掉「4组」这类数量说明，避免其中的数字被当成号码。
///
/// `一组5` / `1组5` 里的「组」是金额前缀（一组=一注），不能当数量删掉：
/// 只有「组」后面不是数字或金额后缀（元/块）时才按数量说明处理。
/// 中文数字的数量说明（三组）不用管——它们本来就不会变成号码。
///
/// 但如果「N组」紧接在号码串后面（`20.44.24.36.10组三中三`），N 本身就是
/// 号码，只把「组」当分隔符删掉；只有跟在文字后面（`这样4组三中三`）才算数量。
fn strip_group_counts(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let run_start = i;
            let digits_start = out.len();
            while i < chars.len() && chars[i].is_ascii_digit() {
                out.push(chars[i]);
                i += 1;
            }
            let mut k = i;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && chars[k] == '组' {
                let mut j = k + 1;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                let followed_by_amount = j < chars.len()
                    && (chars[j].is_ascii_digit() || chars[j] == '元' || chars[j] == '块');
                if !followed_by_amount {
                    // 「20.44.24.36.10组」：前面是号码 / 号码分隔符 → 10 是号码。
                    let prev = chars[..run_start]
                        .iter()
                        .rev()
                        .copied()
                        .find(|c| !c.is_whitespace());
                    let in_pick_chain = prev
                        .is_some_and(|c| c.is_ascii_digit() || is_number_chain_sep(c));
                    if !in_pick_chain {
                        out.truncate(digits_start);
                    }
                    i = k + 1;
                    out.push(' ');
                    continue;
                }
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 号码串里常用的分隔符（点 / 逗号 / 破折号…），用来判断「N组」的 N
/// 是号码还是数量说明。空格单独处理，不算链内分隔符。
fn is_number_chain_sep(c: char) -> bool {
    matches!(
        c,
        '.' | '。' | '，' | ',' | '、' | '-' | '－' | '—' | '–' | '/' | '／' | '+' | '＋' | '~'
            | '～' | ';' | '；' | '|' | '｜'
    )
}

/// `...`（三个及以上点）、`…`、`。` 统一当作组分隔符；单个点仍是号码分隔符，
/// 所以 `02.09.13` 不会被拆开。
fn normalize_group_separators(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '.' => {
                let start = i;
                while i < chars.len() && chars[i] == '.' {
                    i += 1;
                }
                if i - start >= 3 {
                    out.push('，');
                } else {
                    for _ in start..i {
                        out.push('.');
                    }
                }
            }
            '…' => {
                while i < chars.len() && chars[i] == '…' {
                    i += 1;
                }
                out.push('，');
            }
            '。' => {
                out.push('，');
                i += 1;
            }
            // 【13-19-47】这类方括号是分组标记，统一按组分隔处理。
            '【' | '】' | '[' | ']' | '［' | '］' | '〔' | '〕' => {
                out.push('，');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// 把号码区拆成若干组。
///
/// 只有用中文顿号/逗号/分号分隔、并且**每一段都至少有两个号码**时才按多组理解：
/// `20.04.01，37.36.16` → 两组；`1，2，3，4` → 还是一组（逗号只是号码分隔符）。
fn split_pick_groups(text: &str, settings: &KeywordSettings) -> Vec<String> {
    let is_sep =
        |c: char| matches!(c, '，' | '、' | '；' | ';' | '|' | '｜' | '/' | '／');
    if !text.chars().any(is_sep) {
        return vec![text.to_string()];
    }
    let mut groups: Vec<(String, usize)> = Vec::new();
    for seg in text.split(is_sep) {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let norm = normalize_content(&apply_variants(seg, settings));
        let count = norm.split(',').filter(|t| !t.is_empty()).count();
        if count == 0 {
            continue;
        }
        groups.push((seg.to_string(), count));
    }
    if groups.len() >= 2 && groups.iter().all(|(_, n)| *n >= 2) {
        groups.into_iter().map(|(s, _)| s).collect()
    } else {
        vec![text.to_string()]
    }
}

/// 多个号码组之间隔了两个及以上空格（或制表符）时，按空格分段：
/// `1+18     1+24     24+18` 是三注，而不是一注六个号。
/// 只认「每段都是至少两个纯数字」的写法，避免把生肖列表拆开。
fn split_space_groups(content: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = content.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let ws_start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let strong = i - ws_start >= 2
                || chars[ws_start..i].iter().any(|c| *c == '\t');
            if strong {
                parts.push(chars[start..ws_start].iter().collect());
                start = i;
            }
        } else {
            i += 1;
        }
    }
    parts.push(chars[start..].iter().collect());

    let mut groups: Vec<String> = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let normalized = normalize_content(part);
        if normalized.is_empty() {
            return None;
        }
        groups.push(normalized);
    }
    if groups.len() < 2 {
        return None;
    }
    let all_numeric = groups.iter().all(|group| {
        let tokens: Vec<&str> = group
            .split(',')
            .filter(|t| !t.is_empty())
            .collect();
        tokens.len() >= 2
            && tokens
                .iter()
                .all(|t| t.chars().all(|c| c.is_ascii_digit()))
    });
    all_numeric.then_some(groups)
}

/// 按组分隔符切出原文区间（保留区间坐标，便于把金额归到某一组）。
fn split_range_segments(line: &str) -> Vec<(usize, usize)> {
    let is_sep =
        |c: char| matches!(c, '，' | '、' | '；' | ';' | '|' | '｜' | '/' | '／');
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in line.char_indices() {
        if is_sep(c) {
            out.push((start, i));
            start = i + c.len_utf8();
        }
    }
    out.push((start, line.len()));
    out
}

/// 多组时逐组解析：每组自己的号码、玩法和金额。
/// 没写金额的组沿用上一组的金额；所有组都没写时用整行金额。
fn plan_groups(
    line: &str,
    settings: &KeywordSettings,
    line_stake: Option<Stake>,
    expected_groups: usize,
) -> Option<Vec<(String, Option<Stake>)>> {
    let mut groups: Vec<(String, Option<Stake>)> = Vec::new();
    let mut ambiguous = false;
    for (a, b) in split_range_segments(line) {
        let seg = line[a..b].trim();
        if seg.is_empty() {
            continue;
        }
        let hits = find_play_hits(seg, settings);
        let (mut stakes, mut cleaned) = extract_stakes(seg, settings, &hits, false);
        // 这一组自己写了组合玩法、号码刚好够数、后面只跟一个裸数字：
        // 那个数字就是这一组的金额（如 `32-36-39三中三10`）。
        if stakes.is_empty() && hits.len() == 1 {
            let (guessed, guessed_cleaned) = extract_stakes(seg, settings, &hits, true);
            if guessed.len() == 1 && matches!(guessed[0].stake.kind, StakeKind::Guessed) {
                let picks = strip_all_play_keywords(&guessed_cleaned, settings);
                let content = normalize_content(&apply_variants(&picks, settings));
                let mut tokens: Vec<&str> = Vec::new();
                for t in content.split(',').filter(|t| !t.is_empty()) {
                    if !tokens.contains(&t) {
                        tokens.push(t);
                    }
                }
                let all_numeric = tokens
                    .iter()
                    .all(|t| t.chars().all(|c| c.is_ascii_digit()));
                let ok = match hits[0].kind {
                    PlayKind::Combo(size) => all_numeric && tokens.len() == size,
                    _ => false,
                };
                if ok {
                    stakes = guessed;
                    cleaned = guessed_cleaned;
                }
            }
        }
        let picks = strip_all_play_keywords(&cleaned, settings);
        let content = normalize_content(&apply_variants(&picks, settings));
        if content.is_empty() {
            continue;
        }
        let count = content.split(',').filter(|t| !t.is_empty()).count();
        if count < 2 {
            // 单号码段：说明这里的分隔符只是号码分隔，不是分组。
            ambiguous = true;
            continue;
        }
        groups.push((content, stakes.last().map(|s| s.stake)));
    }
    if ambiguous || groups.len() != expected_groups || groups.len() < 2 {
        return None;
    }
    let mut prev: Option<Stake> = None;
    for (_, stake) in groups.iter_mut() {
        let chosen = stake.or(prev).or(line_stake);
        *stake = chosen;
        prev = chosen;
    }
    Some(groups)
}

const MULTI_ATTR_WORDS: &[&str] = &[
    "家禽", "野兽", "合大", "合小", "合单", "合双", "大单", "小单", "大双", "小双", "红波", "蓝波",
    "绿波", "红单", "蓝单", "绿单", "红双", "蓝双", "绿双", "红大", "蓝大", "绿大", "红小", "蓝小",
    "绿小",
];
const SINGLE_ATTR_CHARS: &[char] = &['大', '小', '单', '双', '金', '木', '水', '火', '土'];

fn is_recognized_char(c: char) -> bool {
    c.is_whitespace()
        || is_separator(c)
        || "()（）[]【】「」『』{}<>《》".contains(c)
        // 多个玩法之间的连接词（三中三和三中二）
        || "和与兼反及或跟同".contains(c)
        || c.is_ascii_digit()
        || is_zodiac(c)
        || SINGLE_ATTR_CHARS.contains(&c)
        || is_cn_num_char(c)
}

/// 「关键词」设置里配置的所有词（玩法、金额、地区、乘法、区间、交集…）。
fn all_configured_keywords(settings: &KeywordSettings) -> Vec<&str> {
    let mut kws: Vec<&str> = Vec::new();
    for f in crate::keywords::ALL_FIELDS {
        kws.extend(settings.get(*f).iter().map(String::as_str));
    }
    // 固定写法同样算认识：总额写法 + 特码短写法「特」。
    kws.extend(["总共", "总计", "共计", "共", "计", "特"]);
    kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    kws
}

/// 内容里认不出的中文词（无用的词汇）。
///
/// 规则：
/// - 「关键词」设置里任何一组的词都算认识；
/// - 英文字母、特殊符号不算无用词；
/// - 只有认不出的中文词才记下来。
fn useless_words(
    text: &str,
    settings: &KeywordSettings,
    replacements: &[crate::replacements::Rule],
) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut kws = all_configured_keywords(settings);
    // 替换词里出现过的词（原词和目标词）也算认识。
    for (from, to) in replacements {
        if !from.is_empty() {
            kws.push(from.as_str());
        }
        if !to.is_empty() {
            kws.push(to.as_str());
        }
    }
    kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;

    let recognized_at = |idx: usize| -> bool {
        if idx >= chars.len() {
            return true;
        }
        let c = chars[idx];
        if is_recognized_char(c) || !is_cjk(c) || c == '复' {
            return true;
        }
        if kws
            .iter()
            .any(|k| !k.is_empty() && starts_with_chars(&chars[idx..], k))
        {
            return true;
        }
        if MULTI_ATTR_WORDS
            .iter()
            .any(|w| starts_with_chars(&chars[idx..], w))
        {
            return true;
        }
        starts_with_chars(&chars[idx..], "复式")
    };

    while i < chars.len() {
        // 设置里的关键词一律认识（可能包含英文/符号，如 A、v、=）。
        if let Some(k) = kws
            .iter()
            .find(|k| !k.is_empty() && starts_with_chars(&chars[i..], k))
        {
            i += k.chars().count();
            continue;
        }
        let c = chars[i];
        if is_recognized_char(c) {
            if c.is_ascii_digit() {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && (chars[i] == '尾' || chars[i] == '头') {
                    i += 1;
                }
                continue;
            }
            if is_cn_num_char(c) {
                while i < chars.len() && is_cn_num_char(chars[i]) {
                    i += 1;
                }
                let mut j = i;
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if j < chars.len() && (chars[j] == '组' || chars[j] == '头' || chars[j] == '尾') {
                    i = j + 1;
                }
                // 中文数字本身不算无用词（`三星19.24.46二10` 里的「二」）。
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(w) = MULTI_ATTR_WORDS
            .iter()
            .find(|w| starts_with_chars(&chars[i..], w))
        {
            i += w.chars().count();
            continue;
        }
        if starts_with_chars(&chars[i..], "复式") {
            i += 2;
            continue;
        }
        if c == '复' {
            // 「复三中三」里的「复」是复式修饰词。
            i += 1;
            continue;
        }
        if !is_cjk(c) {
            // 英文字母、特殊符号：不算无用词。
            i += 1;
            continue;
        }

        // 认不出的中文词：一直收到下一个认识的词为止。
        let mut word = String::new();
        while i < chars.len() && !recognized_at(i) {
            word.push(chars[i]);
            i += 1;
        }
        if word.is_empty() {
            i += 1;
        } else if !out.contains(&word) {
            out.push(word);
        }
    }
    out
}

/// 原文里已经写了「拖」时，按乘法分隔符拆出每个分组。
///
/// 每个分组去掉括号后走通用号码规范化（补零、去无效词），再把组内号码排序，
/// 所以 `7-43-31拖4-28-40拖11-23-47-10-34` 会得到
/// `["07,31,43", "04,28,40", "10,11,23,34,47"]`。
fn explicit_drag_groups(
    content: &str,
    settings: &KeywordSettings,
    separator: &str,
) -> Option<Vec<String>> {
    if separator.is_empty() || !content.contains(separator) {
        return None;
    }
    let mut groups = Vec::new();
    for part in content.split(separator) {
        let part = part
            .trim()
            .trim_matches(['(', ')', '（', '）', '「', '」', '[', ']', '【', '】']);
        let normalized = normalize_content(&apply_variants(part, settings));
        if normalized.is_empty() {
            return None;
        }
        groups.push(sort_group_tokens(&normalized));
    }
    if groups.len() < 2 {
        return None;
    }
    Some(groups)
}

/// 组内排序：号码按数值升序；生肖等非号码保持原来的相对顺序。
fn sort_group_tokens(s: &str) -> String {
    let mut tokens: Vec<&str> = s.split(',').filter(|t| !t.is_empty()).collect();
    tokens.sort_by_key(|t| t.parse::<u32>().ok());
    tokens.dedup();
    tokens.join(",")
}

/// 「猪拖虎拖鼠」这种没写玩法的拖注：返回 2~3 个生肖 / 尾数分组。
///
/// 这种写法既可能是二中二（两组）也可能是三中三（三组），不能默认按特码算，
/// 必须交人工确认。普通号码的拖注不在此列（`01拖02` 仍按特码理解）。
fn bare_drag_groups(content: &str, settings: &KeywordSettings) -> Option<Vec<String>> {
    let separator = settings
        .get(Field::Multiplication)
        .iter()
        .find(|k| !k.is_empty())
        .cloned()
        .unwrap_or_else(|| "拖".to_string());
    let groups = explicit_drag_groups(content, settings, &separator)?;
    if !(2..=3).contains(&groups.len()) {
        return None;
    }
    let all_zodiac_or_tail = groups.iter().all(|g| {
        g.split(',')
            .filter(|t| !t.is_empty())
            .all(|t| is_single_zodiac(t) || is_tail_token(t))
    });
    all_zodiac_or_tail.then_some(groups)
}

/// 自动插入「拖」：把相邻的生肖 / 尾数（以及属性词）各算一个分组。
///
/// 与参考项目一致：
/// - 只用于组合玩法（调用方判断）；
/// - 行里写了「复式」→ 不插；
/// - 档位所需的组数不够（如三中三只有两个生肖 / 尾数）→ 调用方不插。
fn auto_drag_groups(content: &str) -> Option<Vec<String>> {
    if content.contains("复式") {
        return None;
    }
    let chars: Vec<char> = content.chars().collect();
    let mut groups: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_zodiac(c) {
            groups.push(c.to_string());
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '尾' || chars[i] == '头') {
                groups.push(chars[start..=i].iter().collect());
                i += 1;
                continue;
            }
            // 出现普通号码：这行不是纯生肖/尾数分组，不自动插拖。
            return None;
        }
        if let Some(w) = MULTI_ATTR_WORDS
            .iter()
            .find(|w| starts_with_chars(&chars[i..], w))
        {
            groups.push((*w).to_string());
            i += w.chars().count();
            continue;
        }
        if SINGLE_ATTR_CHARS.contains(&c) {
            groups.push(c.to_string());
            i += 1;
            continue;
        }
        // 分隔符、无关文字：视为两组之间的内容。
        i += 1;
    }

    if groups.len() < 2 {
        return None;
    }
    Some(groups)
}

/// 右键「插入拖」：把一段生肖 / 属性词整理成 `「A」拖「B」…`。
///
/// 分组之外的文字（玩法、符号、号码）原样保留；两个分组之间写了什么都会被
/// 替换成分隔符。返回 `None` 表示这段文字里凑不出两组，菜单项置灰。
pub fn insert_drag_text(text: &str, settings: &KeywordSettings) -> Option<String> {
    let sep = settings
        .get(Field::Multiplication)
        .iter()
        .find(|k| !k.is_empty())
        .cloned()
        .unwrap_or_else(|| "拖".to_string());
    let mut plays: Vec<&str> = Vec::new();
    for g in crate::keywords::groups() {
        if g.section == Section::Play {
            plays.extend(settings.get(g.field).iter().map(String::as_str));
        }
    }
    plays.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));

    let mut changed = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        match insert_drag_line(line, &sep, &plays) {
            Some(s) => {
                changed = true;
                lines.push(s);
            }
            None => lines.push(line.to_string()),
        }
    }
    changed.then(|| lines.join("\n"))
}

fn insert_drag_line(line: &str, sep: &str, plays: &[&str]) -> Option<String> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        let rest = &line[pos..];
        // 玩法关键词整段跳过，避免「双星」里的「双」被当成属性词。
        if let Some(kw) = plays
            .iter()
            .find(|kw| !kw.is_empty() && rest.starts_with(**kw))
        {
            i += kw.chars().count();
            continue;
        }
        // 阿拉伯数字：只有后面紧跟 尾 / 头 时才是分组词（3尾、1头）。
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && chars[j].1.is_ascii_digit() {
                j += 1;
            }
            if j < chars.len() && (chars[j].1 == '尾' || chars[j].1 == '头') {
                spans.push((pos, chars[j].0 + chars[j].1.len_utf8()));
                i = j + 1;
            } else {
                i = j;
            }
            continue;
        }
        // 中文数字头 / 尾：三头、二尾。
        if is_cn_num_char(c) {
            let mut j = i;
            while j < chars.len() && is_cn_num_char(chars[j].1) {
                j += 1;
            }
            if j < chars.len() && (chars[j].1 == '尾' || chars[j].1 == '头') {
                spans.push((pos, chars[j].0 + chars[j].1.len_utf8()));
                i = j + 1;
            } else {
                i = j;
            }
            continue;
        }
        if let Some(w) = MULTI_ATTR_WORDS.iter().find(|w| rest.starts_with(**w)) {
            spans.push((pos, pos + w.len()));
            i += w.chars().count();
            continue;
        }
        if is_zodiac(c) {
            spans.push((pos, pos + c.len_utf8()));
            i += 1;
            continue;
        }
        if SINGLE_ATTR_CHARS.contains(&c) {
            spans.push((pos, pos + c.len_utf8()));
            i += 1;
            continue;
        }
        i += 1;
    }

    if spans.len() < 2 {
        return None;
    }
    let mut out = String::with_capacity(line.len() + spans.len() * 4);
    out.push_str(line[..spans[0].0].trim_end_matches(['「', '」']));
    for (idx, (s, e)) in spans.iter().enumerate() {
        if idx > 0 {
            out.push_str(sep);
        }
        out.push('「');
        out.push_str(&line[*s..*e]);
        out.push('」');
    }
    let tail = &line[spans.last().unwrap().1..];
    out.push_str(tail.trim_start_matches(['「', '」']));
    Some(out)
}

fn starts_with_chars(chars: &[char], word: &str) -> bool {
    let n = word.chars().count();
    chars.len() >= n && word.chars().zip(chars.iter().copied()).all(|(w, c)| w == c)
}

/// 输出一条「拖」注：括号包住每个分组，分组之间用乘法分隔符。
fn emit_drag_bet(
    out: &mut ConvertOutput,
    regions: &[Region],
    play_name: &str,
    groups: &[String],
    stake: Option<Stake>,
    separator: &str,
) {
    // 每个拖注分组内部也去重：`(01,01,02)` 按两个号码算。
    let groups: Vec<String> = groups
        .iter()
        .map(|g| dedup_combo_content(g))
        .collect();
    // 互碰组合数按实际号码交叉计算；同一组合里号码重复的跳过
    // （`特碰 马拖5尾` 里 01 同时属于两边时不产生 01-01 这一注）。
    let expanded: Vec<Vec<u32>> = groups
        .iter()
        .map(|g| crate::settle::group_numbers(g))
        .collect();
    let drag_count = crate::settle::distinct_drag_count(&expanded).max(1.0);
    let drag_count = drag_count.max(1.0);
    let display = groups
        .iter()
        .map(|g| format!("({g})"))
        .collect::<Vec<_>>()
        .join(separator);

    let stake_value = stake.unwrap_or(Stake {
        amount: 0.0,
        kind: StakeKind::Guessed,
    });
    let has_stake = stake_value.amount > 0.0;
    // 「每组总额」对拖注 / 组合玩法按参考项目的口径就是每注金额（每组各一份），
    // 不再除以组合数；只有「总金额」才需要摊。
    let total_kind = matches!(stake_value.kind, StakeKind::Total);
    let per_bet = if !has_stake {
        0.0
    } else if total_kind {
        stake_value.amount / drag_count
    } else {
        stake_value.amount
    };
    let contribution = if !has_stake {
        0.0
    } else if total_kind {
        stake_value.amount
    } else {
        stake_value.amount * drag_count
    };

    for r in regions {
        let line_text = if !has_stake {
            format!("{} {} {}", r.char(), play_name, display)
        } else {
            format!(
                "{} {} {} 各组 {}",
                r.char(),
                play_name,
                display,
                fmt_amount(per_bet)
            )
        };
        let (content_col, stake_col) = split_columns(&line_text, r.char(), play_name);
        out.bets.push(BetItem {
            line: line_text.clone(),
            lottery: r.name().to_string(),
            play: play_name.to_string(),
            content: content_col,
            stake: stake_col,
            bets: drag_count,
            amount: contribution,
            attr_total: false,
            validate_content: None,
        });
        out.text.push_str(&line_text);
        out.text.push('\n');
        out.total_stake += contribution;
    }
}

/// 组合玩法（三中三 / 二中二 / 三中二 / 特碰 / 二全中）把生肖、尾数、头数
/// 展开成具体号码，与参考项目一致：
/// `特碰 鸡` → `10,22,34,46`。
///
/// 只有全部 token 都能展开时才返回 Some；遇到属性词等无法简单展开的内容
/// 返回 None，保留原来的写法。
fn expand_combo_content(content: &str) -> Option<String> {
    let tokens: Vec<&str> = content
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    let mut out: Vec<u32> = Vec::new();
    let push = |n: u32, out: &mut Vec<u32>| {
        if (1..=49).contains(&n) && !out.contains(&n) {
            out.push(n);
        }
    };
    let tail_of = |t: &str| -> Option<u32> {
        let d = t.strip_suffix('尾')?;
        if d.chars().count() == 1 {
            d.parse::<u32>().ok().filter(|n| *n <= 9)
        } else {
            None
        }
    };
    let head_of = |t: &str| -> Option<u32> {
        let d = t.strip_suffix('头')?;
        if d.chars().count() == 1 {
            d.parse::<u32>().ok().filter(|n| *n <= 4)
        } else {
            None
        }
    };
    // 先直接号码（保持原顺序），再生肖，再尾数 / 头数。
    for t in &tokens {
        if let Ok(n) = t.parse::<u32>() {
            push(n, &mut out);
        }
    }
    for t in &tokens {
        if is_single_zodiac(t) {
            for n in crate::settle::zodiac_numbers(t)? {
                push(n, &mut out);
            }
        }
    }
    for t in &tokens {
        if let Some(d) = tail_of(t) {
            for n in crate::settle::tail_numbers(d) {
                push(n, &mut out);
            }
        }
    }
    for t in &tokens {
        if let Some(h) = head_of(t) {
            for n in crate::settle::head_group_numbers(h) {
                push(n, &mut out);
            }
        }
    }
    // 还有认不出的 token（属性词等）就不展开，保持原内容。
    for t in &tokens {
        let known = t.parse::<u32>().is_ok()
            || is_single_zodiac(t)
            || tail_of(t).is_some()
            || head_of(t).is_some();
        if !known {
            return None;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(
            out.iter()
                .map(|n| format!("{n:02}"))
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

/// 组合类玩法里重复的号码 / 生肖 / 尾数只保留一次，出现顺序不变。
fn dedup_combo_content(s: &str) -> String {
    let mut tokens: Vec<&str> = Vec::new();
    for token in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !tokens.contains(&token) {
            tokens.push(token);
        }
    }
    tokens.join(",")
}

/// 按拖注分组输出一注：分组数刚好等于玩法档位时按互碰；否则退回复式号码池。
fn emit_combo_groups(
    out: &mut ConvertOutput,
    regions: &[Region],
    name: &str,
    kind: PlayKind,
    groups: &[String],
    stake: Option<Stake>,
    separator: &str,
) {
    if let PlayKind::Combo(size) = kind {
        if groups.len() == size {
            emit_drag_bet(out, regions, name, groups, stake, separator);
            return;
        }
        if groups.len() < size {
            // 明确写了「拖」但分组不够：保留拖写法，交给校验报失败，
            // 不能悄悄变成复式。
            emit_drag_bet(out, regions, name, groups, stake, separator);
            return;
        }
        // 分组多于档位：参考项目回退成复式号码池。
    }
    let mut nums: Vec<u32> = Vec::new();
    for group in groups {
        for n in crate::settle::group_numbers(group) {
            if !nums.contains(&n) {
                nums.push(n);
            }
        }
    }
    nums.sort_unstable();
    if nums.is_empty() {
        return;
    }
    let content = nums
        .iter()
        .map(|n| format!("{n:02}"))
        .collect::<Vec<_>>()
        .join(",");
    emit_bet(out, regions, name, kind, &content, stake);
}

fn emit_bet(
    out: &mut ConvertOutput,
    regions: &[Region],
    base_name: &str,
    base_kind: PlayKind,
    content: &str,
    stake: Option<Stake>,
) {
    let stake = stake.unwrap_or(Stake {
        amount: 0.0,
        kind: StakeKind::Guessed,
    });
    let has_stake = stake.amount > 0.0;
    // 连尾只能写尾数、连肖只能写生肖：用户直接写数字时自动补齐 / 映射。
    let normalized = normalize_lian_content(content, base_kind, base_name);
    let content = normalized.as_ref();
    // 「连肖 / 连尾 / 不中」没写档位时，按内容数量补出真实档位。
    let (play_name, play_kind) = adjust_generic_play(&(base_name.to_string(), base_kind), content);
    // 三中三 / 二中二 / 三中二 / 特碰等组合玩法：生肖展开成号码，
    // 重复号码只算一个。校验仍按展开前的内容判断（生肖 / 尾数本身覆盖
    // 多个号码，单个「0尾」也算凑够二中二），所以另存一份原始内容。
    let validate_content = matches!(play_kind, PlayKind::Combo(_))
        .then(|| content.to_string());
    let deduped = matches!(play_kind, PlayKind::Combo(_)).then(|| {
        expand_combo_content(content).unwrap_or_else(|| dedup_combo_content(content))
    });
    let content = deduped.as_deref().unwrap_or(content);
    let tokens: Vec<&str> = content.split(',').filter(|s| !s.is_empty()).collect();
    // 单个属性词（`家禽各包1000`）只有一个组，每组总额就是整行总额。
    let single_attr =
        play_kind == PlayKind::Special && tokens.len() == 1 && is_special_attr(content);
    // 特码「每组总额」（`蛇猪鸡各包1000`）：每个生肖/尾数/号码组各 1000。
    let per_group = has_stake
        && play_kind == PlayKind::Special
        && matches!(stake.kind, StakeKind::PerGroup)
        && !single_attr;
    if per_group {
        let groups: Vec<Vec<u32>> = tokens
            .iter()
            .map(|t| crate::settle::token_numbers(t))
            .collect();
        if groups.iter().all(|g| !g.is_empty()) {
            emit_special_per_group(out, regions, &play_name, &tokens, &groups, stake.amount);
            return;
        }
    }
    let attr_total = play_kind == PlayKind::Special
        && ((matches!(stake.kind, StakeKind::Total | StakeKind::Guessed)
            && is_special_attr(content))
            || (matches!(stake.kind, StakeKind::PerGroup) && single_attr));
    let per_bet = if has_stake {
        per_stake(&play_kind, content, stake)
    } else {
        None
    };
    // 参考项目里只有特码/平码用「各数」，其余玩法统一用「各组」。
    let prefix = if matches!(play_kind, PlayKind::Special | PlayKind::FlatNumber) {
        "各数"
    } else {
        "各组"
    };

    // 总金额 = 每注金额 × 展开后的实际注数；总额/属性词总额写法直接用原金额。
    let contribution = if !has_stake {
        0.0
    } else if attr_total || matches!(stake.kind, StakeKind::Total) {
        stake.amount
    } else {
        stake.amount * bet_count(&play_kind, content)
    };

    for r in regions {
        out.total_stake += contribution;
        let line_text = if !has_stake {
            format!("{} {} {}", r.char(), play_name, content)
        } else if attr_total {
            format!(
                "{} {} {}共{}",
                r.char(),
                play_name,
                content,
                fmt_amount(stake.amount)
            )
        } else {
            let amount = if matches!(stake.kind, StakeKind::Total) {
                per_bet.unwrap_or(stake.amount)
            } else {
                stake.amount
            };
            format!(
                "{} {} {} {} {}",
                r.char(),
                play_name,
                content,
                prefix,
                fmt_amount(amount)
            )
        };

        // 明细列的「内容 / 一注」直接从转换结果里切出来：
        // 去掉行首「彩种 玩法」和行尾金额，剩下的就是内容。
        let (content_col, stake_col) = split_columns(&line_text, r.char(), &play_name);

        out.bets.push(BetItem {
            line: line_text.clone(),
            lottery: r.name().to_string(),
            play: play_name.clone(),
            content: content_col,
            stake: stake_col,
            bets: bet_count(&play_kind, content),
            amount: contribution,
            attr_total,
            validate_content: validate_content.clone(),
        });
        out.text.push_str(&line_text);
        out.text.push('\n');
    }
}

/// 特码「每组总额」（`蛇猪鸡各包1000`）：每个写出来的组各 1000。
///
/// 组的号码数一样时一行能写准，保留原来的生肖写法（`蛇,猪,鸡 各数 250`）；
/// 组大小不一（马 5 个号、其余 4 个）时一行写不出两个每号金额，按每号金额
/// 拆成多行、号码展开 —— 与参考项目 `formatConverter.ts` 的特码分支一致。
fn emit_special_per_group(
    out: &mut ConvertOutput,
    regions: &[Region],
    play: &str,
    tokens: &[&str],
    groups: &[Vec<u32>],
    per_group: f64,
) {
    let uniform = groups.iter().all(|g| g.len() == groups[0].len());
    if uniform {
        let size = groups[0].len().max(1) as f64;
        let per_number = round2(per_group / size);
        let contribution = per_group * groups.len() as f64;
        let content = tokens.join(",");
        let bets = groups.iter().map(Vec::len).sum::<usize>() as f64;
        for r in regions {
            out.total_stake += contribution;
            let line = format!(
                "{} {} {} 各数 {}",
                r.char(),
                play,
                content,
                fmt_amount(per_number)
            );
            let (content_col, stake_col) = split_columns(&line, r.char(), play);
            out.bets.push(BetItem {
                line: line.clone(),
                lottery: r.name().to_string(),
                play: play.to_string(),
                content: content_col,
                stake: stake_col,
                bets,
                amount: contribution,
                attr_total: false,
                validate_content: None,
            });
            out.text.push_str(&line);
            out.text.push('\n');
        }
        return;
    }

    // 组大小不一：按每号的金额分桶，同一档展开成一行号码。
    let mut buckets: BTreeMap<i64, BTreeSet<u32>> = BTreeMap::new();
    for nums in groups {
        let per_number = round2(per_group / nums.len().max(1) as f64);
        let key = (per_number * 100.0).round() as i64;
        buckets.entry(key).or_default().extend(nums.iter().copied());
    }
    for (key, nums) in buckets {
        let per_number = key as f64 / 100.0;
        let bets = nums.len() as f64;
        let contribution = round2(per_number * bets);
        let content = nums
            .iter()
            .map(|n| format!("{n:02}"))
            .collect::<Vec<_>>()
            .join(",");
        for r in regions {
            out.total_stake += contribution;
            let line = format!(
                "{} {} {} 各数 {}",
                r.char(),
                play,
                content,
                fmt_amount(per_number)
            );
            let (content_col, stake_col) = split_columns(&line, r.char(), play);
            out.bets.push(BetItem {
                line: line.clone(),
                lottery: r.name().to_string(),
                play: play.to_string(),
                content: content_col,
                stake: stake_col,
                bets,
                amount: contribution,
                attr_total: false,
                validate_content: None,
            });
            out.text.push_str(&line);
            out.text.push('\n');
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ---------------- 地区 ----------------

fn strip_regions(line: &str, settings: &KeywordSettings) -> (String, Vec<Region>) {
    let mut text = line.to_string();
    let mut found: Vec<Region> = Vec::new();
    let add = |r: Region, found: &mut Vec<Region>| {
        if !found.contains(&r) {
            found.push(r);
        }
    };

    // 「两边 / 三边」是地区合称：先摘掉，再按合称补上对应地区。
    for kw in settings.get(Field::BothSides) {
        if !kw.is_empty() && text.contains(kw.as_str()) {
            text = text.replace(kw.as_str(), " ");
            add(Region::HongKong, &mut found);
            add(Region::Macau, &mut found);
        }
    }
    for kw in settings.get(Field::ThreeSides) {
        if !kw.is_empty() && text.contains(kw.as_str()) {
            text = text.replace(kw.as_str(), " ");
            add(Region::HongKong, &mut found);
            add(Region::Macau, &mut found);
            add(Region::OldMacau, &mut found);
        }
    }

    // 最长关键词优先，避免「老澳门」被拆成「老」+「澳门」。
    let mut pairs: Vec<(&str, Region)> = Vec::new();
    for kw in settings.get(Field::OldMacau) {
        pairs.push((kw.as_str(), Region::OldMacau));
    }
    for kw in settings.get(Field::HongKong) {
        pairs.push((kw.as_str(), Region::HongKong));
    }
    for kw in settings.get(Field::Macau) {
        pairs.push((kw.as_str(), Region::Macau));
    }
    // 常见错写「奥们 / 澳们」也是澳门。
    pairs.push(("奥们", Region::Macau));
    pairs.push(("澳们", Region::Macau));
    pairs.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    for (kw, region) in pairs {
        if !kw.is_empty() && text.contains(kw) {
            text = text.replace(kw, " ");
            add(region, &mut found);
        }
    }

    // 与其他地区共用一行时，按参考项目的固定顺序输出：港、澳、老。
    found.sort_by_key(|r| match r {
        Region::HongKong => 0,
        Region::Macau => 1,
        Region::OldMacau => 2,
    });
    (text, found)
}

fn has_content(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphanumeric() || is_cjk(c))
}

fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

// ---------------- 玩法 ----------------

#[derive(Clone)]
struct PlayHit {
    start: usize,
    end: usize,
    name: &'static str,
    kind: PlayKind,
}

/// 给某个玩法挑金额：优先取它后面、下一个玩法之前的那个；
/// 没有就取它后面最近的；再没有就取它前面最近的。
fn stake_for_type(types: &[PlayHit], stakes: &[StakeHit], i: usize) -> Option<Stake> {
    if stakes.is_empty() {
        return None;
    }
    if types.len() == 1 {
        return stakes.last().map(|s| s.stake);
    }
    let h = &types[i];
    let next_start = types.get(i + 1).map(|n| n.start).unwrap_or(usize::MAX);
    if let Some(s) = stakes
        .iter()
        .find(|s| s.pos >= h.end && s.pos < next_start)
    {
        return Some(s.stake);
    }
    if let Some(s) = stakes.iter().find(|s| s.pos >= h.end) {
        return Some(s.stake);
    }
    if let Some(s) = stakes.iter().rev().find(|s| s.pos < h.start) {
        return Some(s.stake);
    }
    stakes.last().map(|s| s.stake)
}

/// 这一行是否写了可识别的玩法关键词（信息码录入用来判断要不要补默认类型）。
pub(crate) fn has_play_keyword(line: &str, settings: &KeywordSettings) -> bool {
    !find_play_hits(line, settings).is_empty()
}

fn find_play_hits(line: &str, settings: &KeywordSettings) -> Vec<PlayHit> {
    // 顺序即优先级：更具体的玩法先判，避免「三中一」被「三中三」的短词「三中」截走。
    // 关键词本身由设置页维护，组内按长度从长到短匹配。
    const ORDER: &[(Field, &str, PlayKind)] = &[
        (Field::LiuXiaoZhongYi, "六肖中一", PlayKind::LiuXiao),
        (Field::ZhongYi3, "三中一", PlayKind::ZhongYi(3)),
        (Field::ZhongYi4, "四中一", PlayKind::ZhongYi(4)),
        (Field::ZhongYi5, "五中一", PlayKind::ZhongYi(5)),
        (Field::TwoAllHit, "二全中", PlayKind::Combo(2)),
        (Field::ThreeInTwo, "三中二", PlayKind::Combo(3)),
        (Field::ThreeInThree, "三中三", PlayKind::Combo(3)),
        (Field::TwoInTwo, "二中二", PlayKind::Combo(2)),
        (Field::SpecialHit, "特碰", PlayKind::Combo(2)),
        (Field::BuZhong10, "十不中", PlayKind::BuZhong(10)),
        (Field::BuZhong9, "九不中", PlayKind::BuZhong(9)),
        (Field::BuZhong8, "八不中", PlayKind::BuZhong(8)),
        (Field::BuZhong7, "七不中", PlayKind::BuZhong(7)),
        (Field::BuZhong6, "六不中", PlayKind::BuZhong(6)),
        (Field::BuZhong5, "五不中", PlayKind::BuZhong(5)),
        // 连尾要排在连肖之前，避免「二连尾」被「二连」当成二连肖。
        (Field::LianWei4, "四连尾", PlayKind::LianWei(4)),
        (Field::LianWei3, "三连尾", PlayKind::LianWei(3)),
        (Field::LianWei2, "二连尾", PlayKind::LianWei(2)),
        (Field::LianXiao5, "五连肖", PlayKind::LianXiao(5)),
        (Field::LianXiao4, "四连肖", PlayKind::LianXiao(4)),
        (Field::LianXiao3, "三连肖", PlayKind::LianXiao(3)),
        (Field::LianXiao2, "二连肖", PlayKind::LianXiao(2)),
        (Field::FlatSpecial, "平特", PlayKind::FlatSpecial),
        (Field::FlatNumber, "平码", PlayKind::FlatNumber),
        (Field::SpecialNumber, "特码", PlayKind::Special),
    ];

    // 每个玩法类别最多取一处；同一行里写多个玩法时（三中三二中二特碰），全部保留。
    let mut hits: Vec<PlayHit> = Vec::new();
    for (field, name, kind) in ORDER {
        let mut kws: Vec<&str> = settings.get(*field).iter().map(String::as_str).collect();
        // 固定短写法「特」= 特码：用户没在设置里加也能识别。
        if *field == Field::SpecialNumber {
            kws.push("特");
        }
        kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
        let mut best: Option<(usize, usize)> = None;
        for kw in kws {
            if kw.is_empty() {
                continue;
            }
            // 同一个词可能出现在别的玩法词里面（「特碰」里的「特」）：
            // 跳过落在更长命中里的位置，继续往后找真正独立的那个。
            let mut from = 0;
            while let Some(rel) = line[from..].find(kw) {
                let pos = from + rel;
                let end = pos + kw.len();
                let inside_other = hits.iter().any(|h| {
                    h.start <= pos && end <= h.end && h.end - h.start > kw.len()
                });
                if !inside_other {
                    let better = match best {
                        None => true,
                        Some((bs, be)) => pos < bs || (pos == bs && end > be),
                    };
                    if better {
                        best = Some((pos, end));
                    }
                    break;
                }
                from = pos + line[pos..].chars().next().map(char::len_utf8).unwrap_or(1);
            }
        }
        if let Some((start, end)) = best {
            hits.push(PlayHit {
                start,
                end,
                name,
                kind: *kind,
            });
        }
    }

    // 去掉重叠命中（如「三中一」里的「三中」、「平特码」里的「平特/特码」）。
    // 先按起点、再按长度排序，后面的命中只要和已保留的重叠就丢弃；
    // 否则下面按字节区间拼接时会出现 start < last，直接 panic 闪退。
    hits.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| b.end.cmp(&a.end)));
    let mut kept: Vec<PlayHit> = Vec::with_capacity(hits.len());
    for h in hits {
        if kept.last().map_or(true, |k| h.start >= k.end) {
            kept.push(h);
        }
    }
    kept
}

/// 从内容里去掉所有玩法关键词（所有出现位置）。
/// 一行里同名玩法可能重复出现（每组写一次），不能只去掉第一处；
/// 像「3中3」这样的关键词还带数字，不摘会漏成号码。
fn strip_all_play_keywords(text: &str, settings: &KeywordSettings) -> String {
    let mut kws: Vec<&str> = Vec::new();
    for g in crate::keywords::groups() {
        if g.section == Section::Play {
            kws.extend(settings.get(g.field).iter().map(String::as_str));
        }
    }
    // 「特」是固定的特码短写法（排在最后，先摘「特碰 / 平特 / 特码」再摘它）。
    kws.push("特");
    kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    let mut out = text.to_string();
    for kw in kws {
        if !kw.is_empty() && out.contains(kw) {
            out = out.replace(kw, " ");
        }
    }
    out
}

// ---------------- 金额 ----------------

/// 裸数字紧跟在玩法词后面、且后面还有别的玩法（`三中三20特碰各5`、
/// `特码10，二中二各5`）：这是前一个玩法自己的金额，不是号码。
fn number_between_plays(
    text: &str,
    start: usize,
    end: usize,
    type_hits: &[PlayHit],
) -> bool {
    let prev = type_hits
        .iter()
        .filter(|h| h.end <= start)
        .max_by_key(|h| h.end);
    let next = type_hits
        .iter()
        .filter(|h| h.start >= end)
        .min_by_key(|h| h.start);
    let (Some(prev), Some(next)) = (prev, next) else {
        return false;
    };
    // 摘过金额之后 text 可能比原行短，玩法坐标要用 get 兜底，避免切在字符中间。
    let Some(left) = text.get(prev.end..start) else {
        return false;
    };
    // 数字必须紧贴前一个玩法（只允许空白），否则是号码列表。
    if !left.chars().all(char::is_whitespace) {
        return false;
    }
    let Some(right) = text.get(end..next.start) else {
        return false;
    };
    // 数字和下一个玩法之间只能有空白 / 分隔符，不能夹着别的号码。
    right
        .chars()
        .all(|c| c.is_whitespace() || is_separator(c))
}

/// `44+17+39+45+22特码10`：号码写在玩法前面，玩法后面只剩一个行末数字。
/// 这个数字只能是金额（号码已经在玩法前写全了）。
fn trailing_amount_after_play(
    text: &str,
    start: usize,
    end: usize,
    type_hits: &[PlayHit],
    settings: &KeywordSettings,
) -> bool {
    // 数字紧跟在某个玩法后面（中间只有空白）。一行可能有多个玩法
    // （`37-36特串2    二中二5`），取它前面最近的那个玩法。
    let Some(h) = type_hits
        .iter()
        .filter(|h| h.end <= start)
        .max_by_key(|h| h.end)
    else {
        return false;
    };
    // 数字后面必须直接到行尾（只允许空白 / 分隔符）。
    let after = text[end..].trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !is_line_end(after) {
        return false;
    }
    let Some(left) = text.get(h.end..start) else {
        return false;
    };
    if !left.chars().all(char::is_whitespace) {
        return false;
    }
    // 多玩法行里的「特10」是「特码 10」的固定短写法：10 是号码，不是金额。
    if type_hits.len() > 1 && text.get(h.start..h.end) == Some("特") {
        return false;
    }
    // 玩法前面至少写了两个号码，才排除「末尾数字也是号码」的可能；
    // `1特码10` 这种单个号码仍然交给猜金额 / 存疑流程。
    let Some(pre) = text.get(..h.start) else {
        return false;
    };
    let picks = strip_all_play_keywords(pre, settings);
    let content = normalize_content(&apply_variants(&picks, settings));
    let numbers = content.split(',').filter(|t| !t.is_empty()).count();
    numbers >= 2
}

/// 逗号后行末裸数字被猜成金额时的存疑说明。
///
/// `特碰21-31，20` 里 20 也可能是第三个号码；按金额入单的同时提示人工复核。
fn ambiguous_trailing_amount(
    line: &str,
    stakes: &[StakeHit],
    type_hits: &[PlayHit],
    settings: &KeywordSettings,
) -> Option<String> {
    let hit = stakes
        .iter()
        .find(|h| matches!(h.stake.kind, StakeKind::Guessed))?;
    if hit.stake.amount <= 0.0 || hit.stake.amount > 49.0 {
        return None;
    }
    if type_hits.len() != 1 {
        return None;
    }
    let PlayKind::Combo(size) = type_hits[0].kind else {
        return None;
    };
    // 金额必须写在行尾，且前面紧挨着逗号 / 顿号这类分组符号。
    let after = line[hit.end..].trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
    if !is_line_end(after) {
        return None;
    }
    let before = line[..hit.pos].trim_end();
    // 必须是「分组符号」而不是普通号码分隔符：`三中三 1,2,3,4` 里的 4
    // 仍属于同一组号码，不算歧义；`特碰21-31，20` 的 20 才是单独一组。
    if !before.chars().last().is_some_and(is_group_separator) {
        return None;
    }
    let picks = strip_all_play_keywords(before, settings);
    let content = normalize_content(&apply_variants(&picks, settings));
    let tokens: Vec<&str> = content.split(',').filter(|t| !t.is_empty()).collect();
    if tokens.len() != size || !tokens.iter().all(|t| t.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(format!(
        "「{},{}」末尾的{}已按金额入单，也可能是号码",
        tokens.join("."),
        fmt_amount(hit.stake.amount),
        fmt_amount(hit.stake.amount)
    ))
}

/// 摘掉一行里所有显式金额，返回每个金额的位置（用于按玩法分配）和剩余文本。
fn extract_stakes(
    line: &str,
    settings: &KeywordSettings,
    type_hits: &[PlayHit],
    allow_guess: bool,
) -> (Vec<StakeHit>, String) {
    // 显式金额标记（各/每/共/元/块…）优先，而且要全部摘掉：
    // 多组写法里每组末尾都可能写一个「各15」，只摘最后一个会留下别的 15 当号码。
    let mut text = line.to_string();
    let mut hits: Vec<StakeHit> = Vec::new();
    loop {
        let runs = number_runs(&text);
        // 每摘掉一笔金额，文本会变短；调用方给的玩法坐标已经对不上了。
        // 用当前文本重新识别一遍，避免把「二中二」里的「二」当成金额吃掉。
        let hits_now: Vec<PlayHit> = if type_hits.is_empty() {
            Vec::new()
        } else {
            find_play_hits(&text, settings)
        };
        let type_hits = if type_hits.is_empty() {
            type_hits
        } else {
            hits_now.as_slice()
        };
        let mut next: Option<String> = None;
        for (start, end, value, _cn) in runs.iter().rev() {
            // 玩法关键词里的数字（如「3中3」）不是金额。
            if type_hits.iter().any(|h| *start < h.end && *end > h.start) {
                continue;
            }
            // 号码后面的「尾 / 头」是号码记法，不是金额。
            if text[*end..].starts_with('尾') || text[*end..].starts_with('头') {
                continue;
            }
            // 金额标记与数字之间允许夹分隔符（`各-60`、`各 60`、`60-元`）。
            let before = text[..*start]
                .trim_end_matches(|c: char| c.is_whitespace() || is_separator(c));
            let after = text[*end..]
                .trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
            let suffix_len = stake_suffix(after, settings);

            let (marker_start, kind) = match stake_prefix(before, settings) {
                // 标记在 `before` 的末尾，而不是紧贴数字：`各 10` 中间有空格时
                // 起点必须用 before.len() - m，用数字起点减会切在汉字中间导致 panic。
                Some((m, k)) => (Some(before.len() - m), k),
                None if suffix_len.is_some() => (None, StakeKind::PerBet),
                // 玩法之间 / 玩法前面已有号码时的裸数字金额。
                None if number_between_plays(&text, *start, *end, type_hits)
                    || trailing_amount_after_play(&text, *start, *end, type_hits, settings) =>
                {
                    (None, StakeKind::PerBet)
                }
                None => continue,
            };
            let pos = marker_start.unwrap_or(*start);
            let remove_to = match suffix_len {
                Some(n) => *end + (text[*end..].len() - after.len()) + n,
                None => *end,
            };
            let (stake, new_text) = stake_removal(
                &text,
                *start,
                *end,
                *value,
                kind,
                marker_start,
                suffix_len,
                after,
            );
            // 从右往左摘，左边金额的坐标不受右边删除影响，可直接当原行坐标用。
            hits.push(StakeHit {
                pos,
                end: remove_to,
                stake,
            });
            next = Some(new_text);
            break;
        }
        // 金额和删除后的占位空格长度可能相同（行末一位数 `...三中三5`），
        // 不能只按长度判断是否真的改动了文本。
        match next {
            Some(t) if t != text => text = t,
            _ => break,
        }
    }
    if !hits.is_empty() {
        hits.sort_by_key(|h| h.pos);
        return (hits, text);
    }
    if !allow_guess {
        return (Vec::new(), text);
    }

    // 第二轮：没有显式标记时，才把行末裸数字猜成金额。
    // 阿拉伯数字优先；都不行时再用中文数字兜底（`三中三06.26.44五十` → 50）。
    let runs = number_runs(&text);
    for (start, end, value, cn) in runs.iter().rev() {
        if *cn {
            continue;
        }
        if type_hits.iter().any(|h| *start < h.end && *end > h.start) {
            continue;
        }
        if text[*end..].starts_with('尾') || text[*end..].starts_with('头') {
            continue;
        }
        let after = text[*end..]
            .trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
        if is_line_end(after) {
            let (stake, content) = stake_removal(
                &text,
                *start,
                *end,
                *value,
                StakeKind::Guessed,
                None,
                None,
                after,
            );
            return (
                vec![StakeHit {
                    pos: *start,
                    end: *end,
                    stake,
                }],
                content,
            );
        }
    }

    // 中文数字金额兜底：取最后一个不在玩法词里、后面不是 头/尾 的中文数字。
    // 后面必须紧跟分隔符 / 空白 / 金额后缀或行尾；紧跟其它汉字（如「三边」）时
    // 不是一个独立金额，跳过。
    for (start, end, value, cn) in runs.iter().rev() {
        if !cn || *value <= 0.0 {
            continue;
        }
        if type_hits.iter().any(|h| *start < h.end && *end > h.start) {
            continue;
        }
        if text[*end..].starts_with('尾') || text[*end..].starts_with('头') {
            continue;
        }
        let rest = &text[*end..];
        let after = rest.trim_start_matches(|c: char| c.is_whitespace() || is_separator(c));
        let suffix_len = stake_suffix(after, settings);
        let standalone = rest.is_empty()
            || rest
                .chars()
                .next()
                .is_some_and(|c| c.is_whitespace() || is_separator(c))
            || suffix_len.is_some();
        if !standalone {
            continue;
        }
        let (stake, content) = stake_removal(
            &text,
            *start,
            *end,
            *value,
            StakeKind::Guessed,
            None,
            suffix_len,
            after,
        );
        return (
            vec![StakeHit {
                pos: *start,
                end: *end,
                stake,
            }],
            content,
        );
    }

    (Vec::new(), text)
}

fn stake_removal(
    line: &str,
    start: usize,
    end: usize,
    value: f64,
    kind: StakeKind,
    marker_start: Option<usize>,
    suffix_len: Option<usize>,
    after: &str,
) -> (Stake, String) {
    let remove_from = marker_start.unwrap_or(start);
    let remove_to = match suffix_len {
        // 金额和后缀之间可能有空格（`100 元`），要跳过这些空格再删后缀。
        Some(n) => end + (line[end..].len() - after.len()) + n,
        None => end,
    };
    let mut content = String::with_capacity(line.len());
    content.push_str(line[..remove_from].trim_end());
    content.push(' ');
    content.push_str(&line[remove_to..].trim_start());
    (Stake { amount: value, kind }, content)
}

fn stake_prefix(before: &str, settings: &KeywordSettings) -> Option<(usize, StakeKind)> {
    let mut entries: Vec<(&str, StakeKind)> = Vec::new();
    for kw in settings.get(Field::TotalStake) {
        entries.push((kw.as_str(), StakeKind::Total));
    }
    for kw in settings.get(Field::PerGroupTotalStake) {
        entries.push((kw.as_str(), StakeKind::PerGroup));
    }
    for kw in settings.get(Field::StakePrefix) {
        entries.push((kw.as_str(), StakeKind::PerBet));
    }
    // 「计 / 共 / 总共 / 总计」是总额的固定写法，即使用户清空了关键词也认。
    for kw in ["总共", "总计", "共计", "共", "计"] {
        entries.push((kw, StakeKind::Total));
    }
    entries.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    for (kw, kind) in entries {
        if !kw.is_empty() && before.ends_with(kw) {
            return Some((kw.len(), kind));
        }
    }
    None
}

fn stake_suffix(after: &str, settings: &KeywordSettings) -> Option<usize> {
    let mut kws: Vec<&str> = settings
        .get(Field::StakeSuffix)
        .iter()
        .map(String::as_str)
        .collect();
    // 「只」是常见的手写金额后缀（各5只），即使用户没配置也认。
    kws.push("只");
    kws.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    kws.into_iter()
        .find(|kw| !kw.is_empty() && after.starts_with(*kw))
        .map(str::len)
}

fn is_line_end(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_whitespace() || "，。、；;,.!?！？()（）-－—–~～".contains(c)
    })
}

/// 金额候选：阿拉伯数字和中文数字（「每组五块」「每组十块」）。
/// 最后一个 bool 表示是不是中文数字；中文数字只在有显式标记时才算金额，
/// 不做行末裸数字猜测。
fn number_runs(s: &str) -> Vec<(usize, usize, f64, bool)> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c.is_ascii_digit() {
            let start = i;
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = it.peek() {
                if d.is_ascii_digit() || d == '.' {
                    end = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            let token = s[start..end].trim_end_matches('.');
            if let Ok(v) = token.parse::<f64>() {
                out.push((start, end, v, false));
            }
        } else if is_cn_num_char(c) {
            let start = i;
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = it.peek() {
                if is_cn_num_char(d) {
                    end = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            if let Some(v) = cn_to_num(&s[start..end]) {
                out.push((start, end, v, true));
            }
        }
    }
    out
}

fn is_cn_num_char(c: char) -> bool {
    "零〇一二两三四五六七八九十百千万亿".contains(c)
}

/// 解析中文整数（一 / 十五 / 二十 / 一百二十 …）。
fn cn_to_num(s: &str) -> Option<f64> {
    let digit = |c: char| -> Option<i64> {
        match c {
            '零' | '〇' => Some(0),
            '一' => Some(1),
            '二' | '两' => Some(2),
            '三' => Some(3),
            '四' => Some(4),
            '五' => Some(5),
            '六' => Some(6),
            '七' => Some(7),
            '八' => Some(8),
            '九' => Some(9),
            _ => None,
        }
    };
    let mut total = 0i64;
    let mut section = 0i64;
    let mut number = 0i64;
    let mut any = false;
    for c in s.chars() {
        if let Some(d) = digit(c) {
            number = d;
            any = true;
        } else {
            let unit = match c {
                '十' => 10,
                '百' => 100,
                '千' => 1000,
                '万' => 10000,
                _ => return None,
            };
            any = true;
            if unit == 10000 {
                section = (section + number) * unit;
                total += section;
                section = 0;
            } else {
                if number == 0 {
                    number = 1;
                }
                section += number * unit;
            }
            number = 0;
        }
    }
    if !any {
        return None;
    }
    total += section + number;
    Some(total as f64)
}

// ---------------- 内容 ----------------

fn expand_ranges(s: &str, settings: &KeywordSettings) -> String {
    let range_seps: Vec<char> = settings
        .get(Field::Range)
        .iter()
        .filter_map(|kw| {
            let mut it = kw.chars();
            let c = it.next()?;
            it.next().is_none().then_some(c)
        })
        .collect();
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let a: Option<u32> = chars[start..i].iter().collect::<String>().parse().ok();
            if let Some(a) = a {
                if i + 1 < chars.len()
                    && range_seps.contains(&chars[i])
                    && chars[i + 1].is_ascii_digit()
                {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    let b: Option<u32> = chars[i + 1..j].iter().collect::<String>().parse().ok();
                    if let Some(b) = b {
                        if a <= b && b - a <= 100 {
                            let nums: Vec<String> =
                                (a..=b).map(|n| n.to_string()).collect();
                            out.push_str(&nums.join(","));
                            i = j;
                            continue;
                        }
                    }
                }
            }
            out.extend(chars[start..i].iter());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 生肖生僻字映射（如 免→兔、候→猴）。
fn apply_variants(s: &str, settings: &KeywordSettings) -> String {
    if settings.zodiac_variants.is_empty() {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        let key: &str = c.encode_utf8(&mut buf);
        match settings.zodiac_variants.get(key).and_then(|v| v.chars().next()) {
            Some(correct) => out.push(correct),
            None => out.push(c),
        }
    }
    out
}

fn normalize_content(s: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut other = String::new();
    let mut chars = s.chars().peekable();

    let flush = |other: &mut String, tokens: &mut Vec<String>| {
        let t = other.trim();
        if let Some(tok) = normalize_word_token(t) {
            tokens.push(tok);
        }
        other.clear();
    };

    while let Some(c) = chars.next() {
        if is_zodiac(c) {
            flush(&mut other, &mut tokens);
            tokens.push(c.to_string());
        } else if c.is_ascii_digit() {
            flush(&mut other, &mut tokens);
            let mut digits = String::new();
            digits.push(c);
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            match chars.peek() {
                Some(&'尾') | Some(&'头') => {
                    let suffix = chars.next().unwrap();
                    tokens.push(format!("{digits}{suffix}"));
                }
                _ => {
                    // 号码只有 1–49；大于 49 的数字（如单号「325号」）不是金额就舍弃，
                    // 不能写进内容列。
                    if let Ok(n) = digits.parse::<u32>() {
                        if (1..=49).contains(&n) {
                            tokens.push(format!("{n:02}"));
                        }
                    }
                }
            }
        } else if (c == '头' || c == '尾')
            && other.chars().last().is_some_and(|l| cn_digit(l).is_some())
        {
            // 「三头」「二尾」：中文数字 + 头/尾 要作为一个整体收尾，
            // 否则「三头二尾」会被并成一个词。
            other.push(c);
            flush(&mut other, &mut tokens);
        } else if is_separator(c) {
            flush(&mut other, &mut tokens);
        } else {
            other.push(c);
        }
    }
    flush(&mut other, &mut tokens);

    tokens.retain(|t| !is_noise_token(t));
    tokens.join(",")
}

fn is_separator(c: char) -> bool {
    c.is_whitespace() || "，、；;：:,.|/+＋-－—–~～".contains(c)
}

/// 多组号码之间使用的「分组符号」（与 `split_pick_groups` 一致）。
fn is_group_separator(c: char) -> bool {
    matches!(c, '，' | '、' | '；' | ';' | '|' | '｜' | '/' | '／')
}

/// 非数字内容词：只保留参考项目认识的属性词，其余（「复试」「复式」这类修饰词）
/// 不算投注内容，不能进「内容」列。
fn normalize_word_token(t: &str) -> Option<String> {
    if t.is_empty() || is_noise_token(t) {
        return None;
    }
    if is_attr_token(t) {
        return Some(t.to_string());
    }
    // 中文数字的头/尾写法：「三头」→「3头」、「二尾」→「2尾」。
    let chars: Vec<char> = t.chars().collect();
    if chars.len() == 2 && (chars[1] == '头' || chars[1] == '尾') {
        if let Some(d) = cn_digit(chars[0]) {
            return Some(format!("{d}{}", chars[1]));
        }
    }
    None
}

fn is_attr_token(t: &str) -> bool {
    matches!(
        t,
        "大" | "小"
            | "单"
            | "双"
            | "金"
            | "木"
            | "水"
            | "火"
            | "土"
            | "家禽"
            | "野兽"
            | "合大"
            | "合小"
            | "合单"
            | "合双"
            | "大单"
            | "小单"
            | "大双"
            | "小双"
            | "红波"
            | "蓝波"
            | "绿波"
            | "红单"
            | "蓝单"
            | "绿单"
            | "红双"
            | "蓝双"
            | "绿双"
            | "红大"
            | "蓝大"
            | "绿大"
            | "红小"
            | "蓝小"
            | "绿小"
    )
}

fn cn_digit(c: char) -> Option<u8> {
    match c {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn is_noise_token(t: &str) -> bool {
    matches!(
        t,
        "" | "各" | "每" | "共" | "合计" | "元" | "块" | "=" | "一注" | "每注"
    )
}

fn is_zodiac(c: char) -> bool {
    "鼠牛虎兔龙蛇马羊猴鸡狗猪".contains(c)
}

fn is_special_attr(content: &str) -> bool {
    const ATTR: &[&str] = &[
        "大", "小", "单", "双", "合大", "合小", "合单", "合双", "家禽", "野兽",
    ];
    let toks: Vec<&str> = content.split(',').filter(|s| !s.is_empty()).collect();
    if toks.len() == 1 && ATTR.contains(&toks[0]) {
        return true;
    }
    if toks.len() == 6 {
        let set: BTreeSet<&str> = toks.iter().copied().collect();
        return set.len() == 6 && toks.iter().all(|t| t.chars().count() == 1 && is_zodiac(t.chars().next().unwrap()));
    }
    false
}

// ---------------- 注数 ----------------

/// 没写档位的「连肖 / 连尾 / 不中」按内容数量补出档位，和参考项目一致。
/// 连尾 / 连肖的内容归一：
/// - 连尾：纯数字自动变成尾数（`1` → `1尾`，`13` → `3尾`）；
/// - 连肖：纯数字自动换成对应生肖（`01` → `马`）；
/// - 其余内容原样保留，重复项去重（同一个尾 / 生肖写两遍没有意义）。
///
/// 只处理明确的连尾 / 连肖；其它玩法不碰。
fn normalize_lian_content<'a>(
    content: &'a str,
    kind: PlayKind,
    name: &str,
) -> std::borrow::Cow<'a, str> {
    let lian_wei = matches!(kind, PlayKind::LianWei(_)) || name == "连尾";
    let lian_xiao = matches!(kind, PlayKind::LianXiao(_)) || name == "连肖";
    if !lian_wei && !lian_xiao {
        return std::borrow::Cow::Borrowed(content);
    }
    let mut out: Vec<String> = Vec::new();
    let mut changed = false;
    for token in content.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let numeric = token.chars().all(|c| c.is_ascii_digit());
        let mapped = if numeric {
            match token.parse::<u32>() {
                Ok(n) if lian_wei => {
                    changed = true;
                    Some(format!("{}尾", n % 10))
                }
                Ok(n) => crate::settle::zodiac_of(n).map(|zodiac| {
                    changed = true;
                    zodiac.to_string()
                }),
                Err(_) => None,
            }
        } else {
            None
        };
        let token = mapped.unwrap_or_else(|| token.to_string());
        if out.contains(&token) {
            changed = true;
        } else {
            out.push(token);
        }
    }
    if changed {
        std::borrow::Cow::Owned(out.join(","))
    } else {
        std::borrow::Cow::Borrowed(content)
    }
}

fn adjust_generic_play(play: &(String, PlayKind), content: &str) -> (String, PlayKind) {
    let tokens: Vec<&str> = content.split(',').filter(|s| !s.is_empty()).collect();
    let numeric = || {
        tokens
            .iter()
            .filter(|t| t.chars().all(|c| c.is_ascii_digit()))
            .count()
    };
    match play.0.as_str() {
        "连肖" => {
            let n = tokens
                .iter()
                .filter(|t| {
                    let mut it = t.chars();
                    it.next().map(is_zodiac).unwrap_or(false) && it.next().is_none()
                })
                .count()
                .clamp(2, 5);
            (format!("{}连肖", cn(n)), PlayKind::LianXiao(n))
        }
        "连尾" => {
            let n = tokens.iter().filter(|t| t.ends_with('尾')).count().clamp(2, 4);
            (format!("{}连尾", cn(n)), PlayKind::LianWei(n))
        }
        "不中" => {
            let n = numeric().clamp(5, 10);
            (format!("{}不中", cn(n)), PlayKind::BuZhong(n))
        }
        _ => play.clone(),
    }
}

fn cn(n: usize) -> &'static str {
    match n {
        2 => "二",
        3 => "三",
        4 => "四",
        5 => "五",
        6 => "六",
        7 => "七",
        8 => "八",
        9 => "九",
        10 => "十",
        _ => "二",
    }
}

/// 一行展开成多少个实际投注（用于总金额）。
fn bet_count(kind: &PlayKind, content: &str) -> f64 {
    let picks: Vec<&str> = content.split(',').filter(|s| !s.is_empty()).collect();
    let numeric = picks
        .iter()
        .filter(|t| t.chars().all(|c| c.is_ascii_digit()))
        .count();
    let groups = picks.len();
    match kind {
        PlayKind::Special | PlayKind::FlatNumber => expanded_count(&picks),
        PlayKind::FlatSpecial => groups.max(1) as f64,
        // 复式组合按展开后的号码个数算（生肖/尾数先展开成号码）。
        // 重复号码只算一个，避免注数与金额被放大。
        PlayKind::Combo(size) => {
            let mut unique: Vec<&str> = Vec::new();
            for t in &picks {
                if !unique.contains(t) {
                    unique.push(t);
                }
            }
            comb(expanded_count(&unique) as usize, *size) as f64
        }
        PlayKind::LianXiao(size) | PlayKind::LianWei(size) => {
            comb(groups, *size) as f64
        }
        // 不中可以把生肖 / 尾数 / 属性词展开成号码后再组合。
        PlayKind::BuZhong(size) => {
            let mut nums: Vec<u32> = Vec::new();
            for t in &picks {
                for n in crate::settle::token_numbers(t) {
                    if !nums.contains(&n) {
                        nums.push(n);
                    }
                }
            }
            comb(nums.len(), *size) as f64
        }
        PlayKind::ZhongYi(3) => comb(numeric, 3) as f64,
        PlayKind::ZhongYi(_) | PlayKind::LiuXiao => 1.0,
    }
}

/// 特码/平码一个 token 覆盖的号码数（生肖、尾数、头数、属性词）。
fn expanded_count(picks: &[&str]) -> f64 {
    let n: usize = picks.iter().map(|t| token_size(t)).sum();
    n.max(1) as f64
}

/// 一个内容词覆盖的号码数。生肖与参考项目的号码表一致：马 5 个，其余 4 个。
fn token_size(t: &str) -> usize {
    if t.contains(',') {
        // 拖注的每个分组里可能写了多个号码 / 生肖：「(07,31,43)拖…」按 3 个号码算。
        // 重复的号码只算一次。
        let mut unique: Vec<&str> = Vec::new();
        for part in t.split(',').filter(|part| !part.is_empty()) {
            if !unique.contains(&part) {
                unique.push(part);
            }
        }
        return unique.into_iter().map(token_size).sum();
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        1
    } else if t.chars().count() == 1 && is_zodiac(t.chars().next().unwrap()) {
        // 生肖号码数按服务器下发的映射走（没下发时退回内置：马 5 个、其余 4 个）。
        crate::settle::zodiac_numbers(t)
            .map(|v| v.len())
            .unwrap_or(if t == "马" { 5 } else { 4 })
    } else {
        tail_size(t)
            .or_else(|| head_size(t))
            .or_else(|| crate::settle::attr_numbers(t).map(|v| v.len()))
            .or_else(|| attr_size(t))
            .unwrap_or(1)
    }
}

fn tail_size(t: &str) -> Option<usize> {
    let d = t.strip_suffix('尾')?.parse::<u32>().ok()?;
    Some(crate::settle::tail_numbers(d).len())
}

fn head_size(t: &str) -> Option<usize> {
    let d = t.strip_suffix('头')?.parse::<u32>().ok()?;
    if d > 4 {
        return None;
    }
    Some(crate::settle::head_group_numbers(d).len())
}

fn attr_size(t: &str) -> Option<usize> {
    Some(match t {
        "大" | "合大" | "家禽" | "单" | "合单" => 25,
        "小" | "合小" | "野兽" | "双" | "合双" => 24,
        "大单" => 13,
        "小单" => 11,
        "大双" => 12,
        "小双" => 12,
        "红波" => 17,
        "蓝波" | "绿波" => 16,
        "金" | "木" => 10,
        "水" | "土" => 9,
        "火" => 12,
        _ => return None,
    })
}

fn per_stake(kind: &PlayKind, content: &str, stake: Stake) -> Option<f64> {
    // 特码的「每组总额」在 emit_special_per_group 里单独处理；其余玩法的
    // 「每组总额」按参考项目就是每注金额（每个写出来的组各一份）。
    if !matches!(stake.kind, StakeKind::Total) {
        return Some(stake.amount);
    }
    let picks: Vec<&str> = content.split(',').filter(|s| !s.is_empty()).collect();
    let numeric = picks.iter().filter(|t| t.chars().all(|c| c.is_ascii_digit())).count();
    let groups = picks.len();
    // 特码 / 平码的生肖、尾数、属性词要展开成号码再摊总额：`蛇猪鸡共1000`
    // 是 12 个号各 83.33，不是 3 个 token 各 333.33。组合玩法在 emit_bet 里
    // 已经把 content 展开成号码，numeric 就是号码数。
    let expanded = expanded_count(&picks).max(1.0) as usize;
    let combos = match kind {
        PlayKind::Special | PlayKind::FlatNumber => expanded,
        PlayKind::FlatSpecial => groups.max(1),
        PlayKind::Combo(size) => comb(numeric, *size),
        PlayKind::LianXiao(size) | PlayKind::LianWei(size) | PlayKind::BuZhong(size) => {
            comb(groups, *size)
        }
        PlayKind::ZhongYi(3) => comb(numeric, 3),
        PlayKind::ZhongYi(_) | PlayKind::LiuXiao => 1,
    };
    Some(stake.amount / combos.max(1) as f64)
}

fn comb(n: usize, k: usize) -> usize {
    if n < k || k == 0 {
        return 1;
    }
    let k = k.min(n - k);
    (1..=k).fold(1usize, |acc, i| acc * (n + 1 - i) / i)
}

pub fn fmt_amount(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(raw: &str) -> ConvertOutput {
        convert(raw, &KeywordSettings::default())
    }

    fn one(raw: &str) -> BetItem {
        let out = conv(raw);
        assert_eq!(out.bets.len(), 1, "raw={raw} out={:?}", out.bets);
        out.bets[0].clone()
    }

    #[test]
    fn multiple_plays_header_applies_to_following_numbers() {
        let out = conv("二中二，三中三，特碰\n28X29X10X5=每组各3");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 28,29,10,05 各组 3",
                "澳 三中三 28,29,10,05 各组 3",
                "澳 特碰 28,29,10,05 各组 3",
            ]
        );
    }

    #[test]
    fn multiple_plays_header_with_builtin_replacements() {
        let rules = crate::replacements::default_rules();
        let out = convert_with_replacements(
            "二中二，三中三，特碰\n28X29X10X5=每组各3",
            &KeywordSettings::default(),
            &rules,
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 28,29,10,05 各组 3",
                "澳 三中三 28,29,10,05 各组 3",
                "澳 特碰 28,29,10,05 各组 3",
            ],
            "reasons={:?}",
            out.reasons
        );
    }

    #[test]
    fn multiple_groups_and_plays_cartesian_product() {
        let out = conv(
            "20.04.01，37.36.16，41.33.30，40.34.21，这样4组三中三和三中二各10",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 20,04,01 各组 10",
                "澳 三中二 20,04,01 各组 10",
                "澳 三中三 37,36,16 各组 10",
                "澳 三中二 37,36,16 各组 10",
                "澳 三中三 41,33,30 各组 10",
                "澳 三中二 41,33,30 各组 10",
                "澳 三中三 40,34,21 各组 10",
                "澳 三中二 40,34,21 各组 10",
            ]
        );
    }

    #[test]
    fn digits_inside_play_keywords_are_not_stakes() {
        let b = one("1,2,3 3中3");
        assert_eq!(b.play, "三中三");
        assert_eq!(b.content, "01,02,03");
        assert_eq!(b.stake, "—");
    }

    #[test]
    fn unparseable_text_produces_empty_result() {
        assert!(conv("这不是注单").bets.is_empty());
        assert!(!conv("三中三 1,2,3各10").bets.is_empty());
    }

    #[test]
    fn replacements_apply_before_conversion() {
        let rules = vec![
            ("复试".to_string(), "复式".to_string()),
            ("阿".to_string(), "各".to_string()),
        ];
        let out = convert_with_replacements(
            "三中三 1,2,3阿10 复试",
            &KeywordSettings::default(),
            &rules,
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert_eq!(out.bets.len(), 1);
        assert!(out.bets[0].line.ends_with("各组 10"));
    }

    #[test]
    fn doubtful_requires_play_numbers_and_stake_in_one_line() {
        assert!(!conv("三中三 1,2,3各10").doubtful);
        assert!(!conv("特码 1,2,3各1000").doubtful);
        assert!(!conv("三中三 龙虎狗 各100").doubtful);
        // 缺金额
        assert!(conv("三中三 龙虎狗").doubtful);
        // 没写玩法、整段走默认特码 → 不存疑
        assert!(!conv("1,2,3各10").doubtful);
        // 写了玩法却没有显式金额 → 存疑（行末裸数字是猜的，不算）
        assert!(conv("特码 龙虎狗").doubtful);
        assert!(conv("特码 1,2,3\n4,5,6").doubtful);
        // 玩法写在头行、号码行写了显式金额 → 三要素齐全
        assert!(!conv("三中三\n1,2,3各10").doubtful);
        // 第一行齐全，后面的号码行可以沿用玩法
        assert!(!conv("三中三 1,2,3各10\n1,2,4各10").doubtful);
        // 但仍然缺金额的行会存疑
        assert!(conv("特码 龙虎狗\n三中三 龙虎狗").doubtful);
        // 只有玩法/金额、没有号码 → 存疑（也没有可转换的注单）
        assert!(conv("三中三 各5").doubtful);
    }

    #[test]
    fn all_default_play_keywords_are_recognized() {
        let s = KeywordSettings::default();
        let cases: &[(Field, &str, &str)] = &[
            (Field::ThreeInThree, "三中三", "01,02,03"),
            (Field::TwoInTwo, "二中二", "01,02"),
            (Field::TwoAllHit, "二全中", "01,02"),
            (Field::ThreeInTwo, "三中二", "01,02,03"),
            (Field::SpecialHit, "特碰", "01,02"),
            (Field::SpecialNumber, "特码", "01"),
            (Field::FlatNumber, "平码", "01"),
            (Field::FlatSpecial, "平特", "龙,虎"),
            (Field::LianXiao2, "二连肖", "鼠,牛"),
            (Field::LianXiao3, "三连肖", "鼠,牛,虎"),
            (Field::LianXiao4, "四连肖", "鼠,牛,虎,兔"),
            (Field::LianXiao5, "五连肖", "鼠,牛,虎,兔,龙"),
            (Field::LianWei2, "二连尾", "0尾,1尾"),
            (Field::LianWei3, "三连尾", "0尾,1尾,2尾"),
            (Field::LianWei4, "四连尾", "0尾,1尾,2尾,3尾"),
            (Field::BuZhong5, "五不中", "01,02,03,04,05"),
            (Field::BuZhong6, "六不中", "01,02,03,04,05,06"),
            (Field::BuZhong7, "七不中", "01,02,03,04,05,06,07"),
            (Field::BuZhong8, "八不中", "01,02,03,04,05,06,07,08"),
            (Field::BuZhong9, "九不中", "01,02,03,04,05,06,07,08,09"),
            (Field::BuZhong10, "十不中", "01,02,03,04,05,06,07,08,09,10"),
            (Field::ZhongYi3, "三中一", "01,02,03"),
            (Field::ZhongYi4, "四中一", "01,02,03,04"),
            (Field::ZhongYi5, "五中一", "01,02,03,04,05"),
            (
                Field::LiuXiaoZhongYi,
                "六肖中一",
                "鼠,牛,虎,兔,龙,蛇",
            ),
        ];
        let mut fails = Vec::new();
        for (field, want, content) in cases {
            for kw in s.get(*field) {
                let raw = format!("{kw} {content} 各10");
                let out = convert(&raw, &s);
                let got = out.bets.first().map(|b| b.play.as_str()).unwrap_or("");
                if got != *want {
                    fails.push(format!(
                        "{:?} kw={kw:?} raw={raw:?} got={got:?} reasons={:?}",
                        field, out.reasons
                    ));
                }
            }
        }
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    #[test]
    fn custom_keywords_take_effect() {
        let mut s = KeywordSettings::default();
        s.set(Field::ThreeInThree, vec!["甲".to_string()]);
        s.set(Field::Macau, vec!["马交".to_string()]);
        s.set(Field::StakePrefix, vec!["投".to_string()]);
        let out = convert("马交 甲 01,02,03 投10", &s);
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].play, "三中三");
        assert_eq!(out.bets[0].lottery, "澳门");
        assert!((out.bets[0].amount - 10.0).abs() < 1e-6, "{:?}", out.bets);
    }

    #[test]
    fn all_default_region_and_stake_keywords_are_recognized() {
        let s = KeywordSettings::default();
        let mut fails = Vec::new();
        for (field, want) in [
            (Field::HongKong, "香港"),
            (Field::Macau, "澳门"),
            (Field::OldMacau, "老澳门"),
        ] {
            for kw in s.get(field) {
                let raw = format!("{kw} 特码 01 各10");
                let out = convert(&raw, &s);
                let got = out.bets.first().map(|b| b.lottery.as_str()).unwrap_or("");
                if got != want {
                    fails.push(format!(
                        "region {:?} kw={kw:?} raw={raw:?} got={got:?} bets={:?}",
                        field, out.bets
                    ));
                }
            }
        }
        for (field, want) in [
            // 前缀是「每注金额」，30 就是 30；总额词在只有 1 注时也是 30。
            (Field::StakePrefix, 30.0),
            (Field::TotalStake, 30.0),
            (Field::StakeSuffix, 10.0),
        ] {
            for kw in s.get(field) {
                let raw = if field == Field::StakeSuffix {
                    format!("三中三 01,02,03 各10{kw}")
                } else {
                    format!("三中三 01,02,03 {kw}30")
                };
                let out = convert(&raw, &s);
                let got = out.bets.first().map(|b| b.amount).unwrap_or(-1.0);
                // 前缀词是「每注」，30 应原样；总额词 30 应原样（只有 1 注）。
                if (got - want).abs() > 0.001 {
                    fails.push(format!(
                        "stake {:?} kw={kw:?} raw={raw:?} got={got} bets={:?}",
                        field, out.bets
                    ));
                }
            }
        }
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    #[test]
    fn per_group_total_stake_expands_each_group() {
        // 三个生肖各 1000：整行 3000，按每个号码摊成 250。
        let out = conv("特码 蛇猪鸡 各包1000");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        let b = &out.bets[0];
        assert_eq!(b.line, "澳 特码 蛇,猪,鸡 各数 250");
        assert_eq!(b.bets, 12.0);
        assert!((b.amount - 3000.0).abs() < 1e-6);
        assert!((out.total_stake - 3000.0).abs() < 1e-6);

        // 组大小不一致（马 5 个号、蛇 4 个号）→ 按每号金额拆成两行。
        let out = conv("特码 马蛇 各包1000");
        assert_eq!(out.bets.len(), 2, "{:?}", out.reasons);
        assert_eq!(out.bets[0].line, "澳 特码 01,13,25,37,49 各数 200");
        assert_eq!(out.bets[1].line, "澳 特码 02,14,26,38 各数 250");
        assert!((out.bets[0].amount - 1000.0).abs() < 1e-6);
        assert!((out.bets[1].amount - 1000.0).abs() < 1e-6);
        assert!((out.total_stake - 2000.0).abs() < 1e-6);

        // 单个属性词只有一个组：每组总额就是整行总额，仍写「共」。
        let out = conv("特码 家禽 各包1000");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].line, "澳 特码 家禽共1000");
        assert!(out.bets[0].attr_total);
        assert!((out.bets[0].amount - 1000.0).abs() < 1e-6);

        // 号码组同样按组各算：1,2,3 各包1000 = 每个号 1000，整行 3000。
        let out = conv("特码 1,2,3 各包1000");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert!((out.bets[0].amount - 3000.0).abs() < 1e-6);
        assert_eq!(out.bets[0].stake, "各数 1000");

        // 其他玩法没有「摊回号码」这一层：每个组各 1000，就是每注 1000。
        let out = conv("平特 蛇,猪,鸡 各包1000");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].line, "澳 平特 蛇,猪,鸡 各组 1000");
        assert_eq!(out.bets[0].bets, 3.0);
        assert!((out.bets[0].amount - 3000.0).abs() < 1e-6);
    }

    #[test]
    fn total_stake_with_zodiac_content_divides_by_expanded_numbers() {
        let out = conv("特码 蛇猪鸡 共1000");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].bets, 12.0);
        assert!((out.bets[0].amount - 1000.0).abs() < 1e-6);
        assert!(
            out.bets[0].stake.contains("83.33"),
            "{:?}",
            out.bets[0]
        );
    }

    #[test]
    fn multi_line_stake_prefix_sets_paragraph_stake() {
        let out = conv("A10\n三中三 01,02,03");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert!((out.bets[0].amount - 10.0).abs() < 1e-6, "{:?}", out.bets);

        let out = conv("A10\n三中三 01,02,03\n三中三 04,05,06");
        assert_eq!(out.bets.len(), 2, "{:?}", out.reasons);
        assert!(
            out.bets.iter().all(|b| (b.amount - 10.0).abs() < 1e-6),
            "{:?}",
            out.bets
        );

        // 前缀和内容写在同一行也认。
        let out = conv("A10 三中三 01,02,03");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert!((out.bets[0].amount - 10.0).abs() < 1e-6, "{:?}", out.bets);

        // 关键词可自定义。
        let mut s = KeywordSettings::default();
        s.set(Field::MultiLineStakePrefix, vec!["投".to_string()]);
        let out = convert("投20\n三中三 01,02,03", &s);
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert!((out.bets[0].amount - 20.0).abs() < 1e-6, "{:?}", out.bets);
    }

    #[test]
    fn intersection_keyword_expands_to_common_numbers() {
        let out = conv("特码 11.12.45.23v大 各10");
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].content, "45");
        assert!((out.bets[0].amount - 10.0).abs() < 1e-6, "{:?}", out.bets);

        // 交集关键词可自定义。
        let mut s = KeywordSettings::default();
        s.set(Field::Intersection, vec!["∩".to_string()]);
        let out = convert("特码 11,12,45,23∩大 各10", &s);
        assert_eq!(out.bets.len(), 1, "{:?}", out.reasons);
        assert_eq!(out.bets[0].content, "45");

        // 交集为空时原样保留，不要静默吞掉。
        let out = conv("特码 01v02 各10");
        assert!(
            out.bets.iter().any(|b| b.content.contains("01")),
            "{:?}",
            out.bets
        );
    }

    #[test]
    fn doubtful_reasons_are_reported() {
        let out = conv("三中三 1,2,3各10 这样");
        assert!(
            out.reasons.iter().any(|r| r.contains("无用词汇「这样」")),
            "{:?}",
            out.reasons
        );

        let out = conv("特码 1,2,3");
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("玩法「特码」后面没有写金额")),
            "{:?}",
            out.reasons
        );

        let out = conv("三中三\n龙虎狗");
        assert!(
            out.reasons.iter().any(|r| r.contains("缺少金额")),
            "{:?}",
            out.reasons
        );

        let out = conv("这不是注单");
        assert!(out.reasons.iter().any(|r| r == "转换结果为空"));
    }

    #[test]
    fn three_drag_symbols_become_dots_and_are_flagged() {
        let settings = KeywordSettings::default();
        let (checked, reasons) =
            replace_excess_drag("龙拖虎拖狗拖鼠 各10\n鼠拖牛拖虎 各10", &settings);
        assert_eq!(checked, "龙.虎.狗.鼠 各10\n鼠拖牛拖虎 各10");
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("三个以上的对乘符号"));

        let out = conv("三中三 龙拖虎拖狗拖鼠 各10");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("三个以上的对乘符号")),
            "{:?}",
            out.reasons
        );

        // 两个「拖」是正常拖注，不改写也不因此存疑。
        let out = conv("三中三 龙拖虎拖狗 各10");
        assert!(
            !out.reasons
                .iter()
                .any(|r| r.contains("三个以上的对乘符号")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn two_drags_with_exactly_three_numbers_are_flagged() {
        let settings = KeywordSettings::default();
        let (checked, reasons) =
            replace_excess_drag("三中三 01拖02拖03 各10\n三中三 01拖02拖03,04 各10", &settings);
        assert_eq!(
            checked,
            "三中三 01.02.03 各10\n三中三 01拖02拖03,04 各10"
        );
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("三个号码存在两个对乘符号"));

        let out = conv("三中三 01拖02拖03 各10");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("三个号码存在两个对乘符号")),
            "{:?}",
            out.reasons
        );

        // 三个号码但超过两个拖（3 个拖）走上一档原因。
        let out = conv("三中三 01拖02拖03拖04 各10");
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("三个以上的对乘符号")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn later_paragraph_stake_applies_to_earlier_number_lines() {
        let raw = "三中三\n09.24.36\n17.22.24\n12.26.49\n三组各8\n复试三中三\n07,10,14,32,36,38各3";
        let rules = vec![("复试".to_string(), "复式".to_string())];
        let out = convert_with_replacements(raw, &KeywordSettings::default(), &rules);
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 09,24,36 各组 8",
                "澳 三中三 17,22,24 各组 8",
                "澳 三中三 12,26,49 各组 8",
                "澳 三中三 07,10,14,32,36,38 各组 3",
            ]
        );
        // 玩法头行 + 号码行写出了显式金额 → 三要素齐全
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn period_as_number_separator_in_single_bet() {
        let out = conv("三中三7。32。5=200元");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].line, "澳 三中三 07,32,05 各组 200");
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn period_as_number_separator_with_multiple_bets() {
        // 号码用「。」分隔；备注之间用「；」分块。
        let out = conv("特码01。02。03各100；二中二04。05各50");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 特码 01,02,03 各数 100", "澳 二中二 04,05 各组 50"]
        );
        // 特码 + 小码混在一张注单里 → 存疑。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );

        // 号码用「。」分隔，备注之间也用「。」：`100。二` 两侧不是数字，按分块处理。
        let out = conv("特码01。02。03各100。二中二04。05各50");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 特码 01,02,03 各数 100", "澳 二中二 04,05 各组 50"]
        );
        // 特码 + 小码混在一张注单里 → 存疑。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );

        // 号码用「。」分隔，备注之间用逗号：后面紧跟玩法词，按分块处理。
        let out = conv("特码01。02。03各100，二中二04。05各50");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 特码 01,02,03 各数 100", "澳 二中二 04,05 各组 50"]
        );
    }

    #[test]
    fn mixed_period_separators_stay_in_one_bet() {
        // 号码里同时混用「.」和「。」：两边数量相当，都当号码间隔符，不拆注单。
        let out = conv("特码01.02。03各100；二中二04.05。06各50");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 特码 01,02,03 各数 100", "澳 二中二 04,05,06 各组 50"]
        );
    }

    #[test]
    fn period_blocks_when_dot_is_number_separator() {
        // 号码用「.」分隔时，「。」是备注分块符——即使两侧都是数字（`20。6`）。
        let out = conv("8.48.37三中三买20。6.19.8.48.37.9三中三一组5");
        assert_eq!(out.bets.len(), 2);
    }

    #[test]
    fn slash_groups_and_zhi_suffix() {
        let raw = "澳，三中三，42.12.15/.20.27.35/.09.10.30/.各5只，计15\n30.03.35.13.11.复式每组各5只，计50\n二中二，20.27.35/.09.10.30/.30.03.35.13/.复式每组各5只，计60\n合计，125";
        let out = conv(raw);
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 42,12,15 各组 5",
                "澳 三中三 20,27,35 各组 5",
                "澳 三中三 09,10,30 各组 5",
                "澳 三中三 30,03,35,13,11 各组 5",
                "澳 二中二 20,27,35 各组 5",
                "澳 二中二 09,10,30 各组 5",
                "澳 二中二 30,03,35,13 各组 5",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn inherited_play_and_later_amount_is_not_doubtful() {
        let out = conv("三星\n9-19-42\n6-31-32各20");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 09,19,42 各组 20",
                "澳 三中三 06,31,32 各组 20",
            ]
        );
        // 玩法在头行，号码行写了显式金额 → 三要素齐全，不存疑
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn period_blocks_and_summary_totals() {
        let raw = "奥三中三42+12+15，20+27+35每组各10，计20。复式二中二，47+26+13+28每组各10，计60。。复三中三30+03+35+13+11每组各5，计50，总130";
        let out = conv(raw);
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 42,12,15 各组 10",
                "澳 三中三 20,27,35 各组 10",
                "澳 二中二 47,26,13,28 各组 10",
                "澳 三中三 30,03,35,13,11 各组 5",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn special_paragraph_splits_at_each_amount() {
        let out = conv("特码狗各5，虎各3，1 2 3 各10");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 特码 狗 各数 5",
                "澳 特码 虎 各数 3",
                "澳 特码 01,02,03 各数 10",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 只有一个金额时不拆行
        let out = conv("特码 1，2，3，4 各10");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].content, "01,02,03,04");
    }

    #[test]
    fn bracketed_groups_share_header_and_amount() {
        let raw = "三中三\n【13-19-47】【02-06-48】【19-22-36】【03-25-46】\n【02-16-39】【02-16-41】【18-37-43】【04-13-18】\n【06-16-24】【10-27-41】【18-36-46】【07-25-46】\n【08-19-42】【04-09-21】【24-31-36】【03-20-25】\n【22-23-43】【06-35-41】\n各3";
        let out = conv(raw);
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 13,19,47 各组 3",
                "澳 三中三 02,06,48 各组 3",
                "澳 三中三 19,22,36 各组 3",
                "澳 三中三 03,25,46 各组 3",
                "澳 三中三 02,16,39 各组 3",
                "澳 三中三 02,16,41 各组 3",
                "澳 三中三 18,37,43 各组 3",
                "澳 三中三 04,13,18 各组 3",
                "澳 三中三 06,16,24 各组 3",
                "澳 三中三 10,27,41 各组 3",
                "澳 三中三 18,36,46 各组 3",
                "澳 三中三 07,25,46 各组 3",
                "澳 三中三 08,19,42 各组 3",
                "澳 三中三 04,09,21 各组 3",
                "澳 三中三 24,31,36 各组 3",
                "澳 三中三 03,20,25 各组 3",
                "澳 三中三 22,23,43 各组 3",
                "澳 三中三 06,35,41 各组 3",
            ]
        );
        // 玩法、号码、金额分在不同行，没有一行三要素齐全 → 存疑
        assert!(out.doubtful);
    }

    #[test]
    fn chinese_numeral_shorthand_is_not_useless() {
        for raw in ["三星19.24.46二10", "三中三19.24.46二10"] {
            let out = conv(raw);
            assert!(!out.doubtful, "{raw}: {:?}", out.reasons);
            assert_eq!(out.bets.len(), 1);
            assert_eq!(out.bets[0].content, "19,24,46");
            assert_eq!(out.bets[0].stake, "各组 10");
        }
    }

    #[test]
    fn standalone_total_line_suppresses_amount_doubts() {
        assert!(!conv("三中三 1,2,3\n共100").doubtful);
        assert!(!conv("三中三 1,2,3\n总计100").doubtful);
        assert!(!conv("三中三 1,2,3\n计100").doubtful);
        assert!(!conv("三中三 1,2,3\n总共100").doubtful);
        assert!(!conv("三中三 1,2,3,4\n共100").doubtful);

        // 没有单独总额行时仍然存疑
        assert!(conv("特码 1,2,3").doubtful);
        // 「各5」不是总额行，三要素仍不满足 → 存疑
        assert!(conv("三中三 1,2,3,4,5\n各5").doubtful);
    }

    #[test]
    fn multi_line_groups_inherit_play_without_doubt() {
        let raw = "3中3复试，5-15-25-35-45，每组各3\n7-17-27-37-47，每组各3\n8-18-28-38-48，每组各3";
        let rules = vec![("复试".to_string(), "复式".to_string())];
        let out = convert_with_replacements(raw, &KeywordSettings::default(), &rules);
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert_eq!(out.bets.len(), 3);
        assert_eq!(out.bets[0].line, "澳 三中三 05,15,25,35,45 各组 3");

        // 没有配置「复试→复式」时，复试是无用词，仍然存疑
        let out = convert_with_replacements(raw, &KeywordSettings::default(), &[]);
        assert!(out.doubtful);
    }

    #[test]
    fn exact_combo_picks_with_trailing_amount_is_not_doubtful() {
        assert!(!conv("1.2,3三中三10").doubtful);
        assert!(!conv("1,2二中二10").doubtful);
        assert!(!conv("1,2,3三中二10").doubtful);
        // 号码写在玩法前面时，玩法后的行末裸数字是金额（4 码组三中三）
        assert!(!conv("1.2,3,4三中三10").doubtful);
        // 特码没有这个豁免
        assert!(conv("1特码10").doubtful);
    }

    #[test]
    fn useless_words_make_doubtful() {
        assert!(conv("三中三 1,2,3各10 这样").doubtful);
        assert!(conv("特码 1,2,3各10 325号").doubtful);
        assert!(conv("三中三 1,2,3各10 复试").doubtful);
        // 英文字母、特殊符号不算无用词
        assert!(!conv("三中三 1,2,3各10 ABC").doubtful);
        assert!(!conv("三中三 1,2,3各10 ###").doubtful);
        // 关键词设置里配置过的词不算无用词
        assert!(conv("三中三 1,2,3各10 备注").doubtful);
        let mut s = KeywordSettings::default();
        s.set(Field::Intersection, vec!["v".to_string(), "备注".to_string()]);
        assert!(!convert("三中三 1,2,3各10 备注", &s).doubtful);

        // 替换词里出现过的词（原词或目标词）也不算无用词
        let rules = vec![
            ("备注".to_string(), "说明".to_string()),
            ("复试".to_string(), "复式".to_string()),
        ];
        let out = convert_with_replacements(
            "三中三 1,2,3各10 说明",
            &KeywordSettings::default(),
            &rules,
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        let out = convert_with_replacements(
            "三中三 1,2,3各10 备注",
            &KeywordSettings::default(),
            &rules,
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        // 「复式」「三组」是认识的写法，不算无用词
        assert!(!conv("三中三 1,2,3各10 复式").doubtful);
        assert!(!conv("三中三 1,2,3各10 三组").doubtful);
        // 玩法之间的「和」不是无用词
        assert!(!conv("20.04.01，37.36.16，三中三和三中二各10").doubtful);
    }

    #[test]
    fn play_without_explicit_stake_is_doubtful() {
        // 行末裸数字只是猜测，不算写了金额
        assert!(conv("特码 1,2,3").doubtful);
        assert!(conv("三中三 1,2,3").doubtful);
        assert!(!conv("特码 1,2,3各10").doubtful);
        // 玩法头行没有号码不判；下一行继承特码并写了金额 → 不存疑
        assert!(!conv("特码\n1,2,3各10").doubtful);
    }

    #[test]
    fn auto_insert_drag_for_zodiac_combos() {
        let out = conv("三中三 龙虎狗 各100");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(
            out.bets[0].line,
            "澳 三中三 (龙)拖(虎)拖(狗) 各组 100"
        );
        assert_eq!(out.bets[0].content, "(龙)拖(虎)拖(狗)");
        // 4 × 4 × 4 = 64 注 × 100
        assert_eq!(out.total_stake, 6400.0);

        let out = conv("特碰 马猴 各50");
        assert_eq!(out.bets[0].line, "澳 特碰 (马)拖(猴) 各组 50");
        assert_eq!(out.total_stake, 1000.0); // 5 × 4 × 50

        let out = conv("二全中 马猴 各50");
        assert!(out.bets[0].line.contains("拖"));
        assert_eq!(out.total_stake, 1000.0);
    }

    #[test]
    fn auto_drag_applies_to_each_combo_play_in_one_line() {
        // 一行写了两个组合玩法（二星 = 二中二 + 特碰），两个尾数各自对乘，各出一条拖注。
        let out = conv("门0尾t5尾二星各2特碰各2");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 (0尾)拖(5尾) 各组 2",
                "澳 特碰 (0尾)拖(5尾) 各组 2",
            ]
        );

        // 写了「复式」时，特碰 / 二中二不插拖。
        let out = conv("门0尾t5尾复式二星各2特碰各2");
        assert!(out.bets.iter().all(|b| !b.line.contains("拖")));
    }

    #[test]
    fn trailing_dot_before_play_does_not_split_numbers() {
        // 号码后面跟一个点号再接玩法/金额：点号不是分块符。
        let out = conv("21.22.23.三中三1\n5.25.35.三中三1\n31.1.41.三中三1\n12.32.42.三中三1");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 21,22,23 各组 1",
                "澳 三中三 05,25,35 各组 1",
                "澳 三中三 31,01,41 各组 1",
                "澳 三中三 12,32,42 各组 1",
            ]
        );
    }

    #[test]
    fn trailing_play_line_adds_bet_to_previous_content() {
        // 第二行只有「玩法 + 金额」，是对上一行生肖 / 尾数的追加投注。
        let out = conv("2中2对剩   0尾/蛇 每组各30（无坎） \n特碰每组各5");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 二中二 (0尾)拖(蛇) 各组 30", "澳 特碰 (0尾)拖(蛇) 各组 5"]
        );
    }

    #[test]
    fn validates_play_content_relationships() {
        // 组合玩法数量不足 → 失败（但明细仍有数据）
        let out = conv("二中二 1各10");
        assert!(!out.bets.is_empty());
        assert!(out.invalid, "{:?}", out.reasons);
        assert!(out.reasons.iter().any(|r| r.contains("至少要 2 个号码")));
        assert!(conv("三中三 1,2各10").invalid);
        assert!(!conv("二中二 1,2各10").invalid);
        assert!(!conv("三中三 1,2,3各10").invalid);
        // 组合玩法允许「至少一个生肖」，也允许生肖与数字一起写
        assert!(!conv("二中二 龙 各10").invalid);
        assert!(!conv("三中三 龙 各10").invalid);
        assert!(!conv("三中三 龙 1,2各10").invalid);
        // 拖注按分组数校验；单个尾数覆盖 5 个号码，够二 / 三中三
        assert!(!conv("二中二 0尾5尾 各10").invalid);
        assert!(!conv("二中二 0尾 各10").invalid);
        assert!(!conv("三中三 0尾 各10").invalid);

        // 平特：生肖 / 尾数可以；纯数字至少 4 个；生肖与数字不能混写
        assert!(!conv("平特 龙 各10").invalid);
        assert!(!conv("平特 0尾 各10").invalid);
        assert!(conv("平特 1,2,3各10").invalid);
        assert!(!conv("平特 1,2,3,4各10").invalid);
        assert!(!conv("平特 1,2,3,4,5各10").invalid);
        // 数字玩法只能 4 码组 / 5 码组，多了也失败
        assert!(conv("平特 1,2,3,4,5,6各10").invalid);
        assert!(conv(
            "平特 3.4.6.9.14.16.17.22.26.27.28.31.32.33.36.37.38.39.42.44.47.48.49各10"
        )
        .invalid);
        assert!(conv("平特 龙 1,2,3,4各10").invalid);

        // 连肖 / 连尾按档位
        assert!(conv("三连肖 鼠牛 各10").invalid);
        assert!(!conv("三连肖 鼠牛虎 各10").invalid);
        assert!(!conv("三连肖 1,2,3各10").invalid);
        assert!(conv("三连尾 1尾2尾 各10").invalid);
        assert!(!conv("三连尾 1尾2尾3尾 各10").invalid);

        // 平码只能是数字
        assert!(!conv("平码 1,2,3各10").invalid);
        assert!(conv("平码 龙 各10").invalid);
    }

    #[test]
    fn each_line_keeps_its_own_stake() {
        // 第一行是紧贴的裸金额，第二行是「各」金额；互不影响。
        let out = conv("06.21.33三中三10
06.21.33二中二各5");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 06,21,33 各组 10",
                "澳 二中二 06,21,33 各组 5",
            ]
        );
    }

    #[test]
    fn trailing_summary_after_per_bet_is_ignored() {
        let out = conv("新澳三中三，2.9.15.26.31.32各3。=30");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 三中三 02,09,15,26,31,32 各组 3"]
        );
        // 6 个号选 3 = 20 注 × 3
        assert_eq!(out.total_stake, 60.0);
    }

    #[test]
    fn standalone_total_does_not_override_per_number_stake() {
        // 六行号码每组 4 个号，后面「各40」= 每个号 40；末尾「计960」是整段
        // 总额（24 个号 × 40），不能反把每行摊成 960/4 = 240。
        let out = conv("1-2-3-4\n10-11-12-13\n19-20-21-22\n28-29-30-31\n37-38-39-40\n46-47-48-49\n各40\n计960");
        let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            got,
            vec![
                "澳 特码 01,02,03,04 各数 40",
                "澳 特码 10,11,12,13 各数 40",
                "澳 特码 19,20,21,22 各数 40",
                "澳 特码 28,29,30,31 各数 40",
                "澳 特码 37,38,39,40 各数 40",
                "澳 特码 46,47,48,49 各数 40",
            ]
        );
        assert_eq!(out.total_stake, 960.0);
    }

    #[test]
    fn multi_play_line_keeps_its_own_trailing_amount() {
        // 号码写在最前，两个玩法各自紧跟自己的金额：特串 2 / 二中二 5。
        let out = conv("37-36特串2    二中二5");
        let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            got,
            vec!["澳 特碰 37,36 各组 2", "澳 二中二 37,36 各组 5"]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert_eq!(out.total_stake, 7.0);
    }

    /// 多行裸表达式 + 末尾多个玩法/金额声明：每个表达式都要按每个玩法各出一注。
    #[test]
    fn trailing_multi_play_block_applies_to_every_expression_line() {
        let expr_lines = "23. 15. 26. 24. 42\n20. 44. 24. 36. 13\n";
        let expressions = ["23,15,26,24,42", "20,44,24,36,13"];
        let cases: &[(&str, &[(&str, &str)])] = &[
            // 一个金额管本行所有玩法。
            ("复试二中二三中三各1", &[("二中二", "1"), ("三中三", "1")]),
            // 每个玩法各写各的金额。
            ("二中二各2三中三各1", &[("二中二", "2"), ("三中三", "1")]),
            (
                "二中二特碰三中三各1",
                &[("二中二", "1"), ("特碰", "1"), ("三中三", "1")],
            ),
            (
                "二中二各1特碰各2三中三各3",
                &[("二中二", "1"), ("特碰", "2"), ("三中三", "3")],
            ),
            // 玩法分多行写；只写一次金额时由所有玩法共用。
            (
                "二中二\n特碰\n三中三各1",
                &[("二中二", "1"), ("特碰", "1"), ("三中三", "1")],
            ),
            ("二中二\n三中三各1", &[("二中二", "1"), ("三中三", "1")]),
            ("二中二各2\n三中三各2", &[("二中二", "2"), ("三中三", "2")]),
            (
                "二中二各1\n特碰各2\n三中三各3",
                &[("二中二", "1"), ("特碰", "2"), ("三中三", "3")],
            ),
        ];
        for (decl, plays) in cases {
            let raw = format!("{expr_lines}{decl}");
            let out = convert_with_replacements(
                &raw,
                &KeywordSettings::default(),
                &crate::replacements::default_rules(),
            );
            let mut expected: Vec<String> = Vec::new();
            for expr in expressions {
                for (play, amount) in *plays {
                    expected.push(format!("澳 {play} {expr} 各组 {amount}"));
                }
            }
            let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
            assert_eq!(got, expected, "decl={decl:?} reasons={:?}", out.reasons);
            assert!(!out.doubtful, "decl={decl:?} reasons={:?}", out.reasons);
        }
    }

    #[test]
    fn play_without_amount_takes_following_play_amount() {
        let expected = vec![
            "澳 三中三 20,44,24,36 各组 2",
            "澳 二中二 20,44,24,36 各组 2",
            "澳 特碰 20,44,24,36 各组 5",
        ];
        // 同一行：三中三后面空着，取后面二中二的「各2」。
        let out = conv("20.44.24.36.\n三中三，二中二各2\n特碰各5");
        let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(got, expected.clone(), "{:?}", out.reasons);
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 玩法分多行写时也能向后取金额。
        let out = conv("20.44.24.36\n三中三\n二中二各2\n特碰各5");
        let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(got, expected, "{:?}", out.reasons);
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn group_suffix_number_before_play_is_a_pick() {
        // 「N组」紧贴玩法词，但 N 在号码串里 → N 是号码，只把「组」当分隔符。
        for raw in [
            "20.44.24.36.10组三中三各2",
            "20。44。24。36。10组三中三各2",
            "20 44 24 36 10组三中三各2",
        ] {
            let out = conv(raw);
            let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
            assert_eq!(
                got,
                vec!["澳 三中三 20,44,24,36,10 各组 2"],
                "{raw} reasons={:?}",
                out.reasons
            );
            assert!(!out.doubtful, "{raw} reasons={:?}", out.reasons);
        }

        // 跟在文字后面的「这样4组」仍是数量说明，不能把 4 当号码。
        let out = conv("20.04.01，37.36.16，41.33.30，40.34.21，这样4组三中三各10");
        let got: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(got.len(), 4, "reasons={:?}", out.reasons);
        assert_eq!(got[0], "澳 三中三 20,04,01 各组 10");
    }

    /// 没写金额的单注 → 失败（不是存疑）。
    #[test]
    fn bet_without_amount_is_invalid() {
        // 号码在前、玩法在后，行末没有可猜的金额 → 单注金额为空。
        let out = conv("1,2,3三中三");
        assert!(out.invalid, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("没有填写金额")),
            "{:?}",
            out.reasons
        );

        let out = conv("三中三 1,2,3各10");
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    /// `猪拖虎拖鼠各1` 这类没写玩法的拖注 → 存疑，提示确认二中二 / 三中三。
    #[test]
    fn bare_zodiac_drag_without_play_is_doubtful() {
        for raw in ["猪x虎X鼠各1", "猪x虎各1", "0尾x1尾x2尾各1", "猪拖虎拖鼠各1"] {
            let out = convert_with_replacements(
                raw,
                &KeywordSettings::default(),
                &crate::replacements::default_rules(),
            );
            assert!(out.doubtful, "{raw} reasons={:?}", out.reasons);
            assert!(
                out.reasons.iter().any(|r| r.contains("没写玩法")
                    && r.contains("二中二")
                    && r.contains("三中三")),
                "{raw} reasons={:?}",
                out.reasons
            );
            assert!(!out.invalid, "{raw} reasons={:?}", out.reasons);
        }

        // 写了玩法就不该再报「没写玩法」。
        let out = convert_with_replacements(
            "三中三 猪拖虎拖鼠各1",
            &KeywordSettings::default(),
            &crate::replacements::default_rules(),
        );
        assert!(
            !out.reasons.iter().any(|r| r.contains("没写玩法")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn mixed_play_families_are_doubtful() {
        // 小码内部（三中三 / 三中二 / 特碰 / 二中二）混写不算混玩法。
        let out = conv("三中三 1,2,3各10\n二中二 4,5各10\n特碰 6,7各10\n三中二 8,9,10各10");
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 小码 + 特码。
        let out = conv("三中三 1,2,3各10\n特码 7 各10");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("小码") && r.contains("特码")),
            "{:?}",
            out.reasons
        );

        // 特码 + 连肖。
        let out = conv("特码 7 各10\n二连肖 鼠,牛 各10");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("特码") && r.contains("连肖")),
            "{:?}",
            out.reasons
        );

        // 平特 + 平码。
        let out = conv("平特 鼠 各10\n平码 1,2,3 各10");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("平特") && r.contains("平码")),
            "{:?}",
            out.reasons
        );

        // 同一类里的不同档位不算混玩法。
        let out = conv("二连肖 鼠,牛 各10\n三连肖 鼠,牛,虎 各10");
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn earlier_line_with_own_amount_defaults_to_special_number() {
        // 前面的号码行自己写了金额、但没写玩法：即使后面（同段或空行后）写了三中三，
        // 也不向后继承，按默认特码处理；整张注单混用特码 + 小码 → 存疑。
        for raw in [
            "6 12 27 29 37 39 41 44 各10\n6 12 27 29 37 39 41 44三中三 各3",
            "6 12 27 29 37 39 41 44 各10\n6 12 27 29 37 39 41 44 三中三 各3",
            "6 12 27 29 37 39 41 44 各10\n\n6 12 27 29 37 39 41 44三中三 各3",
        ] {
            let out = conv(raw);
            let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
            assert_eq!(
                lines,
                vec![
                    "澳 特码 06,12,27,29,37,39,41,44 各数 10",
                    "澳 三中三 06,12,27,29,37,39,41,44 各组 3",
                ],
                "raw={raw:?} reasons={:?}",
                out.reasons
            );
            assert!(out.doubtful, "raw={raw:?} reasons={:?}", out.reasons);
            assert!(
                out.reasons.iter().any(|r| r.contains("多种玩法")),
                "raw={raw:?} reasons={:?}",
                out.reasons
            );
        }
    }

    #[test]
    fn lian_plays_convert_numbers_to_tails_and_zodiacs() {
        let lines = |out: &ConvertOutput| -> Vec<String> {
            out.bets.iter().map(|b| b.line.clone()).collect()
        };

        // 连尾：数字自动补「尾」，13 取尾数 3。
        let out = conv("二连尾 1,2各10");
        assert_eq!(lines(&out), vec!["澳 二连尾 1尾,2尾 各组 10"]);
        let out = conv("三连尾 01,02,13各10");
        assert_eq!(lines(&out), vec!["澳 三连尾 1尾,2尾,3尾 各组 10"]);

        // 连肖：数字自动映射成生肖。
        let out = conv("二连肖 01,02各10");
        assert_eq!(lines(&out), vec!["澳 二连肖 马,蛇 各组 10"]);
        let out = conv("三连肖 01,02,03各10");
        assert_eq!(lines(&out), vec!["澳 三连肖 马,蛇,龙 各组 10"]);

        // 已经写的是尾数 / 生肖时保持原样。
        let out = conv("二连尾 0尾,1尾各10");
        assert_eq!(lines(&out), vec!["澳 二连尾 0尾,1尾 各组 10"]);
        let out = conv("二连肖 马,蛇各10");
        assert_eq!(lines(&out), vec!["澳 二连肖 马,蛇 各组 10"]);
    }

    #[test]
    fn line_amounts_sum_to_total_stake() {
        for raw in [
            "三中三 1,2,3,4各10",
            "二中二 0尾5尾 各10",
            "特码 龙 各100",
            "四连肖 鼠牛虎兔 各5",
        ] {
            let out = conv(raw);
            let sum: f64 = out.bets.iter().map(|b| b.amount).sum();
            assert!(
                (sum - out.total_stake).abs() < 0.001,
                "{raw}: line sum {sum} vs total {}",
                out.total_stake
            );
        }
    }

    #[test]
    fn insert_drag_wraps_zodiac_and_tail_groups() {
        let s = KeywordSettings::default();
        assert_eq!(
            insert_drag_text("鼠牛虎", &s).as_deref(),
            Some("「鼠」拖「牛」拖「虎」")
        );
        assert_eq!(
            insert_drag_text("虎x牛x鸡", &s).as_deref(),
            Some("「虎」拖「牛」拖「鸡」")
        );
        assert_eq!(
            insert_drag_text("0尾1尾", &s).as_deref(),
            Some("「0尾」拖「1尾」")
        );
        // 分组之外原样保留；重复插入不会套成「「鼠」」。
        assert_eq!(
            insert_drag_text("三中三，龙虎狗", &s).as_deref(),
            Some("三中三，「龙」拖「虎」拖「狗」")
        );
        assert_eq!(
            insert_drag_text("「鼠」拖「牛」", &s).as_deref(),
            Some("「鼠」拖「牛」")
        );
        // 一行不足两组时原样保留，换行不变。
        assert_eq!(
            insert_drag_text("五连肖\n鸡猪蛇鼠猴", &s).as_deref(),
            Some("五连肖\n「鸡」拖「猪」拖「蛇」拖「鼠」拖「猴」")
        );
        assert!(insert_drag_text("特码", &s).is_none());
    }

    #[test]
    fn lian_xiao_adjacent_stake_and_dot_split() {
        let out = conv("狗特码各10元.猪，鸡，羊，狗四冲10共50");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["澳 特码 狗 各数 10", "澳 四连肖 猪,鸡,羊,狗 各组 10"]
        );
        // 特码 + 连肖混在一张注单里 → 存疑；但内容本身是合法的。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn lian_xiao_maps_numbers_to_zodiacs() {
        // 混写生肖 + 数字：数字自动换成生肖（01 马、02 蛇），不是报错。
        let out = conv("三连肖 鼠牛 1,2各10");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(lines, vec!["澳 三连肖 鼠,牛,马,蛇 各组 10"]);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn lian_xiao_lines_with_bare_trailing_amounts() {
        // 每行「生肖 + 裸数字」：数字是该行金额，不吞进号码，也不被段落金额盖掉。
        let out = conv("门\n五连肖\n鸡猪蛇鼠猴10\n鸡猪蛇鼠马10\n特码\n鸡各100");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 五连肖 鸡,猪,蛇,鼠,猴 各组 10",
                "澳 五连肖 鸡,猪,蛇,鼠,马 各组 10",
                "澳 特码 鸡 各数 100",
            ]
        );
    }

    #[test]
    fn flat_special_single_zodiac_bare_amount() {
        // 一个生肖 + 裸数字：视为该生肖的金额，不存疑。
        let out = conv("平特蛇200");
        assert_eq!(out.bets[0].line, "澳 平特 蛇 各组 200");
        assert!(!out.doubtful, "{:?}", out.reasons);
        // 一个尾数也一样。
        assert!(!conv("平特0尾200").doubtful);
        // 超过一个生肖 / 尾数：分不清是不是金额，仍按没有写金额存疑。
        let out = conv("平特蛇虎200");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("玩法「平特」后面没有写金额")),
            "{:?}",
            out.reasons
        );
        assert!(conv("平特0尾1尾200").doubtful);
    }

    #[test]
    fn chinese_numeral_bare_amount() {
        // 「五十」没有「各」标记，也按金额 50 识别。
        let out = conv("三中三06.26.44五十，奥们");
        assert_eq!(out.bets[0].line, "澳 三中三 06,26,44 各组 50");
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert_eq!(conv("三中三06.26.44五十").bets[0].line, "澳 三中三 06,26,44 各组 50");
    }

    #[test]
    fn flat_special_with_many_numbers_is_invalid() {
        // 平特数字玩法只有 4 码组 / 5 码组两档；23 个号码必须判失败。
        let raw = "平特狗150，\n3.4.6.9.14.16.17.22.26.27.28.31.32.33.36.37.38.39.42.44.47.48.49各10，\n计:150+230=380";
        let out = conv(raw);
        assert_eq!(
            out.bets[0].line,
            "澳 平特 狗 各组 150",
            "{:?}",
            out.bets.iter().map(|b| &b.line).collect::<Vec<_>>()
        );
        assert!(out.invalid, "{:?}", out.reasons);
        assert!(
            out.reasons
                .iter()
                .any(|r| r.contains("只能刚好 4 个或 5 个号码")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn bare_te_after_other_play_is_special_number() {
        // 「特」既出现在「特碰」里，也单独出现：独立的那个要识别成特码。
        let out = conv("21-22-20-33-49三中各5
12.36.27.39.32.20三中各5特碰各5特各10");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 21,22,20,33,49 各组 5",
                "澳 三中三 12,36,27,39,32,20 各组 5",
                "澳 特碰 12,36,27,39,32,20 各组 5",
                "澳 特码 12,36,27,39,32,20 各数 10",
            ]
        );
        // 小码 + 特码混在一张注单里 → 存疑。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn bare_te_without_stake_prefix_keeps_number_as_pick() {
        // 「特10」：10 没有「各」标记，是号码；金额沿用前面的 5。
        let out = conv("21-22-20-33-49三中各5
12.36.27.39.32.20三中各5特碰各5特10");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 21,22,20,33,49 各组 5",
                "澳 三中三 12,36,27,39,32,20,10 各组 5",
                "澳 特碰 12,36,27,39,32,20,10 各组 5",
                "澳 特码 12,36,27,39,32,20,10 各数 5",
            ]
        );
        // 小码 + 特码混在一张注单里 → 存疑。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn auto_drag_inherits_play_from_later_play_line() {
        // 内容和玩法分三行写：尾数、生肖在前，玩法 + 金额在后。
        let out = conv("5尾✘2尾✘9尾\n鼠✘牛✘鸡\n三中三各1");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 (5尾)拖(2尾)拖(9尾) 各组 1",
                "澳 三中三 (鼠)拖(牛)拖(鸡) 各组 1",
            ]
        );

        // 玩法行写了「复式」→ 不插拖。
        let out = conv("5尾✘2尾✘9尾\n鼠✘牛✘鸡\n三中三复式各1");
        assert!(out.bets.iter().all(|b| !b.line.contains("拖")));
    }

    #[test]
    fn auto_drag_two_group_three_in_three_keeps_fushi() {
        let out = conv("三中三 1尾 2尾 各100");
        assert!(!out.bets[0].line.contains("拖"));
        // 1尾 + 2尾 共 10 个号选 3 = 120 注
        assert_eq!(out.total_stake, 12000.0);
    }

    #[test]
    fn auto_drag_respects_fushi() {
        let out = conv("特碰 马猴 复式 各50");
        assert!(!out.bets[0].line.contains("拖"));
        assert_eq!(out.total_stake, 1800.0); // 马+猴 9 个号选 2 = 36 注

        let out = conv("特碰 复式\n马猴 各50");
        assert!(!out.bets[0].line.contains("拖"));
        assert_eq!(out.total_stake, 1800.0);
    }

    #[test]
    fn auto_drag_inherits_combo_type() {
        let out = conv("三中三 龙虎狗\n牛猪蛇 各100");
        assert_eq!(out.bets.len(), 2);
        assert!(out.bets[0].line.contains("拖"));
        assert!(out.bets[1].line.contains("拖"));
        // 第二行写了金额，同段第一行也共享 100：64 注 × 100 × 2 行
        assert_eq!(out.total_stake, 12800.0);
    }

    #[test]
    fn auto_drag_does_not_touch_other_plays_or_numbers() {
        let out = conv("特码 龙虎狗 各10");
        assert!(!out.bets[0].line.contains("拖"));
        assert_eq!(out.total_stake, 120.0);

        let out = conv("二中二 1,2,3,4各10");
        assert!(!out.bets[0].line.contains("拖"));
        assert_eq!(out.total_stake, 60.0);
    }

    #[test]
    fn total_stake_counts_expanded_bets() {
        assert_eq!(conv("特码 1,2,3各1000").total_stake, 3000.0);
        assert_eq!(conv("二中二 1,2,3,4 共600").total_stake, 600.0);
        assert_eq!(conv("二中二 1,2,3,4各10").total_stake, 60.0);
        assert_eq!(conv("三中三 06,19,08,48,37,09 各5").total_stake, 100.0);
        assert_eq!(
            conv("8.48.37三中三买20。6.19.8.48.37.9三中三一组5").total_stake,
            120.0
        );
        assert_eq!(
            conv("20.04.01，37.36.16，41.33.30，40.34.21，这样4组三中三和三中二各10")
                .total_stake,
            80.0
        );
        assert_eq!(conv("特码 家禽共1000").total_stake, 1000.0);
        assert_eq!(conv("特码 大 各10").total_stake, 250.0);
        assert_eq!(conv("特码 龙 各10").total_stake, 40.0);
        assert_eq!(conv("特码 马 各10").total_stake, 50.0);
        assert_eq!(conv("特码 10 各100两边").total_stake, 200.0);
        assert_eq!(conv("二连肖 鼠牛虎 各10").total_stake, 30.0);
        assert_eq!(conv("五不中 1,2,3,4,5,6 各10").total_stake, 60.0);
        // 没有显式标记时，行末裸数字会被猜成金额。
        assert_eq!(conv("特码 10 20").total_stake, 20.0);
    }

    #[test]
    fn each_group_keeps_its_own_stake() {
        let out = conv("8.48.37三中三买20。6.19.8.48.37.9三中三一组5");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 08,48,37 各组 20",
                "澳 三中三 06,19,08,48,37,09 各组 5",
            ]
        );
    }

    #[test]
    fn chinese_stakes_are_assigned_per_play() {
        let out = conv("06.16.22.44.32.35三中三，每组五块。二中二，每组十块。");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 06,16,22,44,32,35 各组 5",
                "澳 二中二 06,16,22,44,32,35 各组 10",
            ]
        );
    }

    #[test]
    fn numbers_over_49_are_discarded_from_content() {
        let b = one("325号.02.13.48.14.29.34.37二中二各10");
        assert_eq!(b.content, "02,13,48,14,29,34,37");
        assert_eq!(b.line, "澳 二中二 02,13,48,14,29,34,37 各组 10");
    }

    #[test]
    fn ellipsis_groups_and_repeated_stakes() {
        let out = conv(
            "02.09.13.23.25.38.44.45二中二各15...01.03.08.10.24.26.37.46二各15....01.03.22.39.09.25.08.23二中二各15",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 02,09,13,23,25,38,44,45 各组 15",
                "澳 二中二 01,03,08,10,24,26,37,46 各组 15",
                "澳 二中二 01,03,22,39,09,25,08,23 各组 15",
            ]
        );
    }

    #[test]
    fn each_group_uses_its_own_amount() {
        let out = conv("12-36-27-39-20-32三中三各4，32-36-39三中三10，12-18-20三中三10，门");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 12,36,27,39,20,32 各组 4",
                "澳 三中三 32,36,39 各组 10",
                "澳 三中三 12,18,20 各组 10",
            ]
        );
    }

    #[test]
    fn chinese_commas_alone_are_not_group_separators() {
        let out = conv("特码 1，2，3，4 各10");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].content, "01,02,03,04");
    }

    #[test]
    fn zodiac_groups_stay_one_bet() {
        let out = conv("二连肖 鼠，牛，虎 各10");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].play, "二连肖");
        assert_eq!(out.bets[0].content, "鼠,牛,虎");
    }

    #[test]
    fn spaces_around_stake_markers_are_handled() {
        let b = one("特码 1,2,3 各 100");
        assert_eq!(b.content, "01,02,03");
        assert_eq!(b.stake, "各数 100");

        let b = one("特码 10 100 元");
        assert_eq!(b.content, "10");
        assert_eq!(b.stake, "各数 100");
    }

    #[test]
    fn fuzz_inputs_do_not_panic() {
        let charset: Vec<char> = "0123456789.,，、各每共元块特码平特碰二中三连肖尾头v拖至到港澳老新门户复试家禽野兽大小单双[] \n一二三四五六七八九十"
            .chars()
            .collect();
        let mut seed: u64 = 0x1234_5678;
        for _ in 0..20_000 {
            let mut s = String::new();
            let len = (seed % 80) as usize;
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                s.push(charset[(seed as usize) % charset.len()]);
            }
            let _ = conv(&s);
        }
    }

    #[test]
    fn overlapping_play_keywords_do_not_panic() {
        // 「平特」和「特码」在「平特码」里重叠；以前拼接文本时 start < last 会 panic。
        let b = one("平特码 10 各100");
        assert_eq!(b.play, "平特");
        assert_eq!(b.content, "10");
    }

    #[test]
    fn multiple_play_types_split_into_separate_bets() {
        let out = conv("44.8.47.18.40.13.23.22三中三二中二特碰各1");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 44,08,47,18,40,13,23,22 各组 1",
                "澳 二中二 44,08,47,18,40,13,23,22 各组 1",
                "澳 特碰 44,08,47,18,40,13,23,22 各组 1",
            ]
        );
        for b in &out.bets {
            assert_eq!(b.content, "44,08,47,18,40,13,23,22");
            assert_eq!(b.stake, "各组 1");
        }
    }

    #[test]
    fn settings_control_play_keywords() {
        let mut s = KeywordSettings::default();
        s.set(Field::SpecialNumber, vec!["单号".to_string()]);
        let out = convert("单号 10 各100", &s);
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].play, "特码");
    }

    #[test]
    fn settings_control_region_keywords() {
        let mut s = KeywordSettings::default();
        s.set(Field::HongKong, vec!["香江".to_string()]);
        let out = convert("香江 特码 10 各100", &s);
        assert_eq!(out.bets[0].lottery, "香港");
    }

    #[test]
    fn both_sides_splits_into_two_bets() {
        let b = conv("特码 10 各100两边");
        assert_eq!(b.bets.len(), 2);
        assert_eq!(b.bets[0].lottery, "香港");
        assert_eq!(b.bets[1].lottery, "澳门");
    }

    #[test]
    fn zodiac_variants_are_mapped() {
        let b = one("特码 免 各10");
        assert_eq!(b.content, "兔");
    }

    #[test]
    fn settings_control_stake_prefixes() {
        let mut s = KeywordSettings::default();
        let mut kws = s.get(Field::StakePrefix).to_vec();
        kws.push("下注".to_string());
        s.set(Field::StakePrefix, kws);
        let out = convert("特码 10 下注100", &s);
        assert_eq!(out.bets[0].stake, "各数 100");
    }

    #[test]
    fn simple_special_number() {
        let b = one("特码 10=100");
        assert_eq!(b.lottery, "澳门");
        assert_eq!(b.play, "特码");
        assert_eq!(b.content, "10");
        assert_eq!(b.stake, "各数 100");
        assert_eq!(b.line, "澳 特码 10 各数 100");
    }

    #[test]
    fn pads_numbers_and_keeps_order() {
        let b = one("特码 1,2,3各1000");
        assert_eq!(b.content, "01,02,03");
        assert_eq!(b.stake, "各数 1000");
    }

    #[test]
    fn combo_total_is_split_by_bet_count() {
        let b = one("二中二 1,2,3,4 共600");
        assert_eq!(b.play, "二中二");
        assert_eq!(b.content, "01,02,03,04");
        assert_eq!(b.stake, "各组 100");
        assert!(b.line.ends_with("各组 100"));
    }

    #[test]
    fn range_is_expanded() {
        let b = one("特码 1至5 各10");
        assert_eq!(b.content, "01,02,03,04,05");
    }

    #[test]
    fn region_prefix_and_single_char() {
        let b = one("香港 特码 10 各100");
        assert_eq!(b.lottery, "香港");
        assert!(b.line.starts_with("港 "));
        let b = one("老澳门 特码 10 各100");
        assert_eq!(b.lottery, "老澳门");
    }

    #[test]
    fn region_header_applies_to_next_line() {
        let out = conv("香港\n特码 10 各100");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].lottery, "香港");
    }

    #[test]
    fn play_inherits_inside_paragraph() {
        let out = conv("二中二 1,2,3 各10\n1,2,4 各20");
        assert_eq!(out.bets.len(), 2);
        assert_eq!(out.bets[1].play, "二中二");
        assert_eq!(out.bets[1].stake, "各组 20");
    }

    #[test]
    fn special_attr_total_keeps_total() {
        let b = one("特码 家禽共1000");
        assert_eq!(b.content, "家禽");
        assert_eq!(b.stake, "共1000");
        assert_eq!(b.line, "澳 特码 家禽共1000");
    }

    #[test]
    fn zodiac_chars_are_split() {
        let b = one("特码 龙虎狗 各10");
        assert_eq!(b.content, "龙,虎,狗");
    }

    #[test]
    fn modifier_words_are_not_part_of_content() {
        let b = one("2.6.7.30.31.32.26.44复试三中三各1门");
        assert_eq!(b.play, "三中三");
        assert_eq!(b.content, "02,06,07,30,31,32,26,44");
        assert_eq!(b.stake, "各组 1");
        assert_eq!(b.line, "澳 三中三 02,06,07,30,31,32,26,44 各组 1");
    }

    #[test]
    fn chinese_head_tail_is_kept() {
        let b = one("特码 三头二尾 各10");
        assert_eq!(b.content, "3头,2尾");
    }

    #[test]
    fn content_is_line_minus_region_play_and_stake() {
        for raw in [
            "特码 1,2,3各1000",
            "二中二 1,2,3,4 共600",
            "三中三 1,2,3 各10",
            "二连尾 0尾1尾 各10",
        ] {
            let b = one(raw);
            let prefix = format!("{} {} ", if b.lottery == "澳门" { "澳" } else { "港" }, b.play);
            assert!(b.line.starts_with(&prefix), "line={} prefix={prefix}", b.line);
            let rest = &b.line[prefix.len()..];
            let content = rest
                .rsplit_once(" 各组 ")
                .or_else(|| rest.rsplit_once(" 各数 "))
                .map(|(c, _)| c)
                .unwrap_or(rest);
            assert_eq!(b.content, content, "line={}", b.line);
            assert!(!b.content.contains(&b.play), "内容不应包含玩法: {b:?}");
        }
    }

    #[test]
    fn short_lian_xiao_keyword_is_recognised() {
        let b = one("三肖 鼠牛虎 各10");
        assert_eq!(b.play, "三连肖");
        assert_eq!(b.content, "鼠,牛,虎");
    }

    #[test]
    fn lian_wei_is_not_mistaken_for_lian_xiao() {
        let b = one("二连尾 0尾1尾 各10");
        assert_eq!(b.play, "二连尾");
        assert_eq!(b.content, "0尾,1尾");
    }

    #[test]
    fn generic_lian_xiao_infers_size() {
        let b = one("三肖 鼠牛虎 各10");
        assert_eq!(b.play, "三连肖");
        assert_eq!(b.content, "鼠,牛,虎");
        assert_eq!(b.stake, "各组 10");
    }

    #[test]
    fn tail_numbers_are_not_padded() {
        let b = one("二连尾 1尾2尾 各10");
        assert_eq!(b.play, "二连尾");
        assert_eq!(b.content, "1尾,2尾");
        assert_eq!(b.stake, "各组 10");
    }

    #[test]
    fn stake_marker_with_separator_is_recognised() {
        // `各-60`：标记和金额之间夹了分隔符，仍按显式金额处理。
        let out = conv("澳三中三 05-07-32 各-60");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].content, "05,07,32");
        assert_eq!(out.bets[0].stake, "各组 60");
        assert!(!out.doubtful);
        assert!(!out.reasons.iter().any(|r| r.contains("无用词汇")));
    }

    #[test]
    fn hyphen_amount_after_marker_is_explicit() {
        let out = conv("三中三 05-26-27-48 各-30");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].content, "05,26,27,48");
        assert_eq!(out.bets[0].stake, "各组 30");
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn triple_dash_splits_complete_bets_and_inherits_play() {
        let out = conv(
            "三中三2.7.14.33.34.35各2---14.20.22.33.38.40.48.49各1",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 02,07,14,33,34,35 各组 2",
                "澳 三中三 14,20,22,33,38,40,48,49 各组 1",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn multiline_numbers_then_multiple_play_lines_share_stake() {
        let out = conv("澳
32 40 36
三中三
二中二各30");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 32,40,36 各组 30",
                "澳 二中二 32,40,36 各组 30",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn explicit_numeric_drag_groups_are_sorted_and_padded() {
        let b = one("三中三7-43-31拖4-28-40拖11-23-47-10-34 各1");
        assert_eq!(
            b.line,
            "澳 三中三 (07,31,43)拖(04,28,40)拖(10,11,23,34,47) 各组 1"
        );
        assert_eq!(
            b.content,
            "(07,31,43)拖(04,28,40)拖(10,11,23,34,47)"
        );
        // 3 × 3 × 5 = 45 注，每注 1
        assert_eq!(b.bets, 45.0);
        assert_eq!(b.amount, 45.0);
    }

    #[test]
    fn plus_number_groups_separated_by_spaces() {
        let out = conv(
            "二中二:1+18     1+24     24+18各7
三中三:1+18+24买8",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 01,18 各组 7",
                "澳 二中二 01,24 各组 7",
                "澳 二中二 24,18 各组 7",
                "澳 三中三 01,18,24 各组 8",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn combo_trailing_amount_survives_paragraph_stake() {
        let out = conv(
            "15.20.25三中三30
14.16.19.23三中三各10，特碰各5",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 三中三 15,20,25 各组 30",
                "澳 三中三 14,16,19,23 各组 10",
                "澳 特碰 14,16,19,23 各组 5",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn bu_zhong_expands_tail_groups_for_counting() {
        let out = conv("五不中 1尾 各10");
        assert_eq!(out.bets.len(), 1);
        assert_eq!(out.bets[0].content, "1尾");
        // 1尾 = 5 个号，五不中是 C(5,5)=1 注。
        assert_eq!(out.bets[0].bets, 1.0);
        assert_eq!(out.bets[0].amount, 10.0);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn drag_cross_product_skips_duplicate_numbers() {
        // 马 {01,13,25,37,49} 与 5尾 {05,15,25,35,45} 重叠 25：
        // 5×5 − 1 = 24 注，不是 25。
        let b = one("二中二 马拖5尾 各10");
        assert_eq!(b.bets, 24.0);
        assert_eq!(b.amount, 240.0);
    }

    #[test]
    fn zodiac_special_hit_expands_to_numbers() {
        let out = conv(
            "鸡特碰各5，牛特碰各10、
6：18：46：10：26特碰各5",
        );
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 特碰 10,22,34,46 各组 5",
                "澳 特碰 06,18,30,42 各组 10",
                "澳 特碰 06,18,46,10,26 各组 5",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn trailing_bare_stake_play_line_is_merged() {
        let out = conv("21-20-35二中二各15，三中三15，");
        let lines: Vec<&str> = out.bets.iter().map(|b| b.line.as_str()).collect();
        assert_eq!(
            lines,
            vec![
                "澳 二中二 21,20,35 各组 15",
                "澳 三中三 21,20,35 各组 15",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert!(!out.invalid, "{:?}", out.reasons);
    }

    #[test]
    fn combo_duplicate_numbers_count_once() {
        let b = one("三中三 01,01,02,03 各10");
        assert_eq!(b.content, "01,02,03");
        assert_eq!(b.bets, 1.0);
        assert_eq!(b.amount, 10.0);

        let b = one("二中二 01,01,02 各10");
        assert_eq!(b.content, "01,02");
        assert_eq!(b.bets, 1.0);
        assert_eq!(b.amount, 10.0);

        let b = one("三中二 01,01,02,03 各10");
        assert_eq!(b.content, "01,02,03");
        assert_eq!(b.bets, 1.0);
        assert_eq!(b.amount, 10.0);

        let b = one("特碰 01,01,02 各10");
        assert_eq!(b.content, "01,02");
        assert_eq!(b.bets, 1.0);
        assert_eq!(b.amount, 10.0);

        // 去重后不足档位 → 失败。
        let out = conv("三中三 01,01 各10");
        assert!(out.invalid, "{:?}", out.reasons);
    }
}

/// 口语化注单的回归用例（用户反馈的真实写法）。
#[cfg(test)]
mod regression_cases {
    use super::*;
    use crate::keywords::KeywordSettings;

    fn conv(raw: &str) -> ConvertOutput {
        convert(raw, &KeywordSettings::default())
    }

    fn lines(out: &ConvertOutput) -> Vec<&str> {
        out.bets.iter().map(|b| b.line.as_str()).collect()
    }

    #[test]
    fn multiple_plays_with_own_amounts() {
        let out = conv("43.17.45三中三20特碰各5");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 43,17,45 各组 20",
                "澳 特碰 43,17,45 各组 5",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn single_tail_covers_combo_plays() {
        let out = conv("9尾二中二三中三各10");
        assert_eq!(
            lines(&out),
            vec![
                "澳 二中二 09,19,29,39,49 各组 10",
                "澳 三中三 09,19,29,39,49 各组 10",
            ]
        );
        assert!(!out.invalid, "{:?}", out.reasons);

        let out = conv("8尾二中二特碰各2");
        assert!(!out.invalid, "{:?}", out.reasons);
        assert_eq!(out.bets.len(), 2);
    }

    #[test]
    fn play_header_with_number_lines() {
        let out = conv("特碰\n40-45\n04-45各5#澳计10");
        assert_eq!(
            lines(&out),
            vec!["澳 特碰 40,45 各组 5", "澳 特碰 04,45 各组 5"]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        let out = conv("三中三\n19-29-39=5\n2-22-34=5");
        assert_eq!(
            lines(&out),
            vec!["澳 三中三 19,29,39 各组 5", "澳 三中三 02,22,34 各组 5"]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn numbers_before_play_trailing_amount_belongs_to_play() {
        let out = conv("44+17+39+45+22特码10\n二中二三中三各5");
        assert_eq!(
            lines(&out),
            vec![
                "澳 特码 44,17,39,45,22 各数 10",
                "澳 二中二 44,17,39,45,22 各组 5",
                "澳 三中三 44,17,39,45,22 各组 5",
            ]
        );
        // 特码 + 小码混在一张注单里 → 新规则要求存疑。
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("多种玩法")),
            "{:?}",
            out.reasons
        );

        let out = conv("10.22.34.46.07.19.31.43三中三5\n10.22.34.04.16.28.31.43三中三1");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 10,22,34,46,07,19,31,43 各组 5",
                "澳 三中三 10,22,34,04,16,28,31,43 各组 1",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn drag_lines_under_play_header() {
        let out = conv("三中三\n21-33-45拖11-23-35-10-34-46拖1-13-25-37 各1\n\n三中三\n21-33-45 拖7-31-43拖11-23-35-10-34-46各1");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 (21,33,45)拖(10,11,23,34,35,46)拖(01,13,25,37) 各组 1",
                "澳 三中三 (21,33,45)拖(07,31,43)拖(10,11,23,34,35,46) 各组 1",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn trailing_number_after_group_separator_is_flagged() {
        let out = conv("复式特碰21-11-12-22一组20\n特碰21-31，20");
        assert_eq!(
            lines(&out),
            vec!["澳 特碰 21,11,12,22 各组 20", "澳 特碰 21,31 各组 20"]
        );
        assert!(out.doubtful);
        assert!(
            out.reasons.iter().any(|r| r
                == "第 2 行：「21.31,20」末尾的20已按金额入单，也可能是号码"),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn hk_combo_lines_with_slash_stake() {
        let out = conv("香港
三中三
12.36.34/20
20.36.43/20
21.36.34/10
计50");
        assert_eq!(
            lines(&out),
            vec![
                "港 三中三 12,36,34 各组 20",
                "港 三中三 20,36,43 各组 20",
                "港 三中三 21,36,34 各组 10",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
        assert_eq!(out.total_stake, 50.0);
    }

    #[test]
    fn explicit_stake_wins_over_trailing_number() {
        // 段落里写了「各10」时，行末数字仍是号码，不按金额拆。
        let out = conv("三中三
1 2 3 4
5 6 7 8各10");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03,04 各组 10",
                "澳 三中三 05,06,07,08 各组 10",
            ]
        );
    }

    #[test]
    fn total_does_not_block_trailing_combo_stake() {
        // 段落总额（合计/计）不阻止号码行行末数字当本行金额。
        let out = conv("三中三
1 2 3 4
5 6 7 8
合计80");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 4",
                "澳 三中三 05,06,07 各组 8",
            ]
        );
    }

    #[test]
    fn play_header_stake_with_number_lines() {
        // 玩法+金额写在号码前面（同一行 / 上一行都没有表达式）→ 落单存疑。
        let out = conv("门。三中三各5
28-36-47,
10-32-03");
        assert_eq!(
            lines(&out),
            vec!["澳 三中三 28,36,47 各组 5", "澳 三中三 10,32,03 各组 5"]
        );
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("落单")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn expression_can_carry_multiple_play_amount_pairs() {
        // 同一行：表达式在前，后面跟多个「玩法+金额」，各用各的金额。
        let out = conv("01 02 03 三中三 100 二中二 50");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 100",
                "澳 二中二 01,02,03 各组 50",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 玩法与金额紧贴写。
        let out = conv("01 02 03 三中三100 二中二50");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 100",
                "澳 二中二 01,02,03 各组 50",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 带「各」的每注金额。
        let out = conv("01 02 03 三中三各10 二中二各20");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 10",
                "澳 二中二 01,02,03 各组 20",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn play_amount_pairs_attach_to_previous_expression_lines() {
        // 表达式在上一行：同一行的多个对子都挂到它上面。
        let out = conv("01 02 03
三中三 100 二中二 50");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 100",
                "澳 二中二 01,02,03 各组 50",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 一个玩法一行，也挂到上一行的表达式。
        let out = conv("01 02 03
三中三 100
二中二 50");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 100",
                "澳 二中二 01,02,03 各组 50",
            ]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn play_amount_pairs_with_only_later_expression_are_doubtful() {
        // 前面没有表达式，只能取下一行的号码 → 落单存疑。
        let out = conv("三中三 100
01 02 03");
        assert_eq!(lines(&out), vec!["澳 三中三 01,02,03 各组 100"]);
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("落单")),
            "{:?}",
            out.reasons
        );

        let out = conv("三中三 100 二中二 50
01 02 03");
        assert_eq!(
            lines(&out),
            vec![
                "澳 三中三 01,02,03 各组 100",
                "澳 二中二 01,02,03 各组 50",
            ]
        );
        assert!(out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn amount_before_play_is_doubtful() {
        // 金额必须写在玩法后面：写在玩法前面要存疑。
        let out = conv("01 02 03 各100 三中三");
        assert_eq!(lines(&out), vec!["澳 三中三 01,02,03 各组 100"]);
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("金额写在了玩法前面")),
            "{:?}",
            out.reasons
        );

        // 没有标记、但大于 49 的裸数字也只能是金额。
        let out = conv("100 三中三
01 02 03");
        assert!(out.doubtful, "{:?}", out.reasons);
        assert!(
            out.reasons.iter().any(|r| r.contains("金额写在了玩法前面")),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn special_slash_amount_pairs_are_split() {
        // 全特码段：斜杠后面一律是金额，即使不大于 49（`10/20` = 10 号 20 块）。
        let out = conv("特码 10/20 30/40");
        assert_eq!(
            lines(&out),
            vec!["澳 特码 10 各数 20", "澳 特码 30 各数 40"]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);

        // 没写「特码」时按默认特码处理。
        let out = conv("10/20 30/40");
        assert_eq!(
            lines(&out),
            vec!["澳 特码 10 各数 20", "澳 特码 30 各数 40"]
        );

        // 分隔符两边有空格、金额带后缀、号码有前导零。
        let out = conv("特码 05 / 50 20 / 60元");
        assert_eq!(
            lines(&out),
            vec!["澳 特码 05 各数 50", "澳 特码 20 各数 60"]
        );

        // 点号对子仍只认金额大于 49 的（点号本来就常分隔号码）。
        let out = conv("10.50 20.60");
        assert_eq!(
            lines(&out),
            vec!["澳 特码 10 各数 50", "澳 特码 20 各数 60"]
        );
    }

    #[test]
    fn special_slash_pair_keeps_its_own_amount() {
        // 单组对子。
        let out = conv("特码 10/20");
        assert_eq!(lines(&out), vec!["澳 特码 10 各数 20"]);

        // 段里别处写了金额，也不能盖掉对子自己的金额。
        let out = conv("特码 10/20
5 6 各5");
        assert_eq!(
            lines(&out),
            vec!["澳 特码 10 各数 20", "澳 特码 05,06 各数 5"]
        );
        assert!(!out.doubtful, "{:?}", out.reasons);
    }

    #[test]
    fn slash_pairs_do_not_apply_to_non_special_paragraphs() {
        // 非特码段沿用原来的「整段共享一个金额」规则，不按对子拆。
        let out = conv("三中三 10/20 30/40");
        assert_eq!(lines(&out), vec!["澳 三中三 10,20,30 各组 40"]);
    }



}

//! 出账单图片导出：不依赖窗口截图，直接按内容和字体把每个会员块画成 PNG。
//!
//! 这样导出不受窗口大小 / 滚动位置影响，图片尺寸也完全跟着内容走。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ab_glyph::{point, Font, FontArc, ScaleFont};
use image::{Rgba, RgbaImage};

const S: f32 = 2.0; // 2x 渲染，导出更清晰

pub struct BillRowImage {
    pub no: usize,
    pub amount: f64,
    /// 金额里属于香港 / 老澳门的那一份（显示在金额后面的括号里）
    pub hk: f64,
    pub old: f64,
    pub win: f64,
    pub special_win_stake: f64,
    /// (玩法标签, 注额, 赔率)，按 play 顺序展示为彩色徽章。
    pub breakdown: Vec<(String, f64, f64)>,
}

pub struct BillCategoryImage {
    pub label: String,
    pub rows: Vec<BillRowImage>,
    pub total_stake: f64,
    pub total_win: f64,
    pub total_rebate: f64,
}

pub struct BillBlockImage {
    pub name: String,
    pub rows: Vec<BillRowImage>,
    pub total_stake: f64,
    pub total_win: f64,
    pub total_rebate: f64,
    pub win_stakes: Vec<(String, f64)>,
    pub win_amounts: Vec<(String, f64)>,
    /// true = 按分类形式导出（与界面当前选择一致）
    pub category_view: bool,
    pub categories: Vec<BillCategoryImage>,
}

type Col = [u8; 4];
const TEXT: Col = [0x1B, 0x20, 0x27, 255];
const BILL_TEXT: Col = [0x33, 0x41, 0x55, 255];
const MUTED: Col = [0x61, 0x78, 0x92, 255];
const LINE: Col = [0xCB, 0xD5, 0xE1, 255];
const HEAD: Col = [0xF1, 0xF5, 0xF9, 255];
const CARD_BORDER: Col = [0xE2, 0xE8, 0xF0, 255];
const CELL_NO: Col = [0x94, 0xA3, 0xB8, 255];
const BLUE: Col = [0x25, 0x63, 0xEB, 255];
const GREEN: Col = [0x04, 0x78, 0x57, 255];
const RED: Col = [0xB9, 0x1C, 0x1C, 255];
const SPECIAL_RED: Col = [0xBB, 0x00, 0x00, 255];
const SUBTOTAL_BG: Col = [0xF8, 0xFA, 0xFC, 255];
const BAND_STAKE_BG: Col = [0xF1, 0xF5, 0xF9, 255];
const BAND_STAKE_BORDER: Col = [0xE2, 0xE8, 0xF0, 255];
const BAND_AMOUNT_BG: Col = [0xEC, 0xFD, 0xF5, 255];
const BAND_AMOUNT_BORDER: Col = [0xD1, 0xFA, 0xE5, 255];
const TOTALS_BG: Col = [0xE2, 0xE8, 0xF0, 255];
const TITLE_DARK: Col = [0x47, 0x55, 0x69, 255];
// 金额列里香港 / 老澳门拆分的小徽章（与界面同一套色值）。
const TAG_HK: Col = [0xDC, 0x26, 0x26, 255];
const TAG_HK_BORDER: Col = [0xFE, 0xE2, 0xE2, 255];
const TAG_OLD: Col = [0xD9, 0x77, 0x06, 255];
const TAG_OLD_BORDER: Col = [0xFE, 0xF3, 0xC7, 255];

struct Fonts {
    regular: FontArc,
    bold: FontArc,
}

fn fonts() -> Option<&'static Fonts> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let regular_paths = [
                r"C:\Windows\Fonts\Deng.ttf",
                r"C:\Windows\Fonts\simhei.ttf",
                r"C:\Windows\Fonts\msyh.ttc",
            ];
            let bold_paths = [
                r"C:\Windows\Fonts\Dengb.ttf",
                r"C:\Windows\Fonts\simhei.ttf",
                r"C:\Windows\Fonts\msyhbd.ttc",
            ];
            let load = |paths: &[&str]| {
                paths.iter().find_map(|p| {
                    std::fs::read(p)
                        .ok()
                        .and_then(|b| FontArc::try_from_vec(b).ok())
                })
            };
            let regular = load(&regular_paths)?;
            let bold = load(&bold_paths).unwrap_or_else(|| regular.clone());
            Some(Fonts { regular, bold })
        })
        .as_ref()
}

/// 导出图片里的金额一律四舍五入到整数，不显示小数点。
fn money(n: f64) -> String {
    let v = if n.is_finite() { n } else { 0.0 };
    format!("{:.0}", v.round())
}

fn blend(img: &mut RgbaImage, x: i32, y: i32, color: Col, cov: f32) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return;
    }
    let a = cov.clamp(0.0, 1.0) * (color[3] as f32 / 255.0);
    if a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    for i in 0..3 {
        p[i] = (color[i] as f32 * a + p[i] as f32 * (1.0 - a)).round() as u8;
    }
    p[3] = 255;
}

fn text_width(text: &str, size: f32, bold: bool) -> f32 {
    let Some(f) = fonts() else { return 0.0 };
    let font = if bold { &f.bold } else { &f.regular };
    let scaled = font.as_scaled(size);
    text.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

fn draw_text(
    img: &mut RgbaImage,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    color: Col,
    bold: bool,
) {
    let Some(f) = fonts() else { return };
    let font = if bold { &f.bold } else { &f.regular };
    let scaled = font.as_scaled(size);
    let mut pen = x;
    for ch in text.chars() {
        let gid = scaled.glyph_id(ch);
        let glyph = gid.with_scale_and_position(size, point(pen, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                blend(
                    img,
                    b.min.x as i32 + gx as i32,
                    b.min.y as i32 + gy as i32,
                    color,
                    cov,
                );
            });
        }
        pen += scaled.h_advance(gid);
    }
}

fn text_left(img: &mut RgbaImage, text: &str, x: f32, cy: f32, size: f32, color: Col, bold: bool) {
    draw_text(img, text, x, cy + size * 0.35, size, color, bold);
}

fn text_center(
    img: &mut RgbaImage,
    text: &str,
    cx: f32,
    cy: f32,
    size: f32,
    color: Col,
    bold: bool,
) {
    let w = text_width(text, size, bold);
    draw_text(img, text, cx - w / 2.0, cy + size * 0.35, size, color, bold);
}

const BADGE_SIZE: f32 = 10.5 * S;
const BADGE_PAD: f32 = 3.0 * S;
const BADGE_H: f32 = 15.0 * S;
const BADGE_GAP: f32 = 3.0 * S;

/// 金额单元格宽度：主金额 + 「(港N)」「(老N)」两段小字。
fn stake_width(amount: f64, hk: f64, old: f64, amount_font: f32, small: f32) -> f32 {
    let mut w = text_width(&money(amount), amount_font, true);
    for (ch, v) in [("港", hk), ("老", old)] {
        if v <= 0.0 {
            continue;
        }
        w += 2.0 * S
            + text_width("(", small, false)
            + text_width(ch, BADGE_SIZE, true)
            + 4.0 * S
            + text_width(&money(v), small, false)
            + text_width(")", small, false);
    }
    w
}

/// 金额单元格：`100(港50)(老20)`，括号与金额是灰色小字，港 / 老是白底彩边的小徽章。
fn draw_stake(
    img: &mut RgbaImage,
    x: f32,
    cy: f32,
    amount: f64,
    hk: f64,
    old: f64,
    amount_font: f32,
    small: f32,
) {
    let main = money(amount);
    text_left(img, &main, x, cy, amount_font, BLUE, true);
    let mut pen = x + text_width(&main, amount_font, true);
    for (ch, v, fg, border) in [
        ("港", hk, TAG_HK, TAG_HK_BORDER),
        ("老", old, TAG_OLD, TAG_OLD_BORDER),
    ] {
        if v <= 0.0 {
            continue;
        }
        pen += 2.0 * S;
        text_left(img, "(", pen, cy, small, MUTED, false);
        pen += text_width("(", small, false);
        let bw = text_width(ch, BADGE_SIZE, true) + 4.0 * S;
        let by = cy - BADGE_H / 2.0;
        boxed(img, pen, by, bw, BADGE_H, [0xFF, 0xFF, 0xFF, 255], border);
        text_center(img, ch, pen + bw / 2.0, cy, BADGE_SIZE, fg, true);
        pen += bw;
        let num = money(v);
        text_left(img, &num, pen, cy, small, MUTED, false);
        pen += text_width(&num, small, false);
        text_left(img, ")", pen, cy, small, MUTED, false);
        pen += text_width(")", small, false);
    }
}

/// 中奖列的玩法徽章：显示名、文字色、边框色。
fn badge_style(label: &str) -> (String, Col, Col) {
    let (display, fg, border) = crate::win_style::badge(label);
    let display = if display.is_empty() {
        label.to_string()
    } else {
        display.to_string()
    };
    (
        display,
        [fg[0], fg[1], fg[2], 255],
        [border[0], border[1], border[2], 255],
    )
}

fn badge_width(display: &str) -> f32 {
    text_width(display, BADGE_SIZE, true) + BADGE_PAD * 2.0 + 2.0 * S
}

fn breakdown_width(entries: &[(String, f64, f64)], small: f32) -> f32 {
    let mut w = text_width(" (", small, false);
    for (i, (label, stake, odds)) in entries.iter().enumerate() {
        if i > 0 {
            w += text_width("、", small, false);
        }
        let (display, _, _) = badge_style(label);
        w += badge_width(&display) + BADGE_GAP;
        w += text_width(
            &format!("{}×{}", money(*stake), money(*odds)),
            small,
            false,
        );
    }
    w + text_width(")", small, false)
}

fn win_content_width(
    win: f64,
    entries: &[(String, f64, f64)],
    win_size: f32,
    small: f32,
) -> f32 {
    let main = text_width(&money(win), win_size, true);
    if win <= 0.0 || entries.is_empty() {
        main
    } else {
        main + breakdown_width(entries, small)
    }
}

/// 画中奖单元格：金额 + 括号内的彩色玩法徽章与「注额×赔率」。
fn draw_win_content(
    img: &mut RgbaImage,
    x: f32,
    cy: f32,
    win: f64,
    entries: &[(String, f64, f64)],
    win_size: f32,
    small: f32,
) {
    let main = money(win);
    text_left(
        img,
        &main,
        x,
        cy,
        win_size,
        if win > 0.0 { GREEN } else { MUTED },
        true,
    );
    let mut pen = x + text_width(&main, win_size, true);
    if win <= 0.0 || entries.is_empty() {
        return;
    }
    text_left(img, " (", pen, cy, small, MUTED, false);
    pen += text_width(" (", small, false);
    for (i, (label, stake, odds)) in entries.iter().enumerate() {
        if i > 0 {
            text_left(img, "、", pen, cy, small, MUTED, false);
            pen += text_width("、", small, false);
        }
        let (display, fg, border) = badge_style(label);
        let bw = badge_width(&display);
        let by = cy - BADGE_H / 2.0;
        fill(img, pen, by, bw, BADGE_H, [0xFF, 0xFF, 0xFF, 255]);
        hline(img, pen, pen + bw, by, border);
        hline(img, pen, pen + bw, by + BADGE_H - S, border);
        vline(img, pen, by, by + BADGE_H, border);
        vline(img, pen + bw - S, by, by + BADGE_H, border);
        text_center(img, &display, pen + bw / 2.0, cy, BADGE_SIZE, fg, true);
        pen += bw + BADGE_GAP;
        let detail = format!("{}×{}", money(*stake), money(*odds));
        text_left(img, &detail, pen, cy, small, MUTED, false);
        pen += text_width(&detail, small, false);
    }
    text_left(img, ")", pen, cy, small, MUTED, false);
}

fn fill(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, color: Col) {
    let x0 = x.max(0.0) as u32;
    let y0 = y.max(0.0) as u32;
    let x1 = (x + w).max(0.0).min(img.width() as f32) as u32;
    let y1 = (y + h).max(0.0).min(img.height() as f32) as u32;
    for yy in y0..y1 {
        for xx in x0..x1 {
            img.put_pixel(xx, yy, Rgba(color));
        }
    }
}

fn hline(img: &mut RgbaImage, x0: f32, x1: f32, y: f32, color: Col) {
    fill(img, x0, y, (x1 - x0).max(0.0), S, color);
}

fn vline(img: &mut RgbaImage, x: f32, y0: f32, y1: f32, color: Col) {
    fill(img, x, y0, S, (y1 - y0).max(0.0), color);
}

fn boxed(img: &mut RgbaImage, x: f32, y: f32, w: f32, h: f32, bg: Col, border: Col) {
    fill(img, x, y, w, h, bg);
    hline(img, x, x + w, y, border);
    hline(img, x, x + w, y + h - S, border);
    vline(img, x, y, y + h, border);
    vline(img, x + w - S, y, y + h, border);
}

fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "会员".to_string()
    } else {
        s.to_string()
    }
}

/// 把一个会员块渲染成 PNG，返回保存路径。
pub fn save_block(
    block: &BillBlockImage,
    dir: &Path,
    stamp: &str,
    index: usize,
) -> Result<PathBuf, String> {
    if fonts().is_none() {
        return Err("找不到可用的中文字体".to_string());
    }
    let path = dir.join(format!(
        "出账单-{}-{}-{}.png",
        safe_name(&block.name),
        stamp,
        index
    ));
    let img = render_block(block);
    img.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn render_block(b: &BillBlockImage) -> RgbaImage {
    let pad = 12.0 * S;
    let head_font = 12.5 * S;
    let no_font = 13.0 * S;
    let amount_font = 14.0 * S;
    let win_font = 14.0 * S;
    let small = 11.5 * S;
    let row_h = 26.0 * S;
    let head_h = 26.0 * S;
    let sub_h = 26.0 * S;
    let cat_h = 22.0 * S;
    let col_gap = 12.0 * S;
    let totals_box_h = 4.0 * 22.0 * S + 8.0 * S;

    // 列宽 = 最宽内容 + 约一个中文字。
    let calc = |rows: &[BillRowImage], with_subtotal: bool, total: f64, special: f64| {
        let mut no_w = text_width("#", head_font, true);
        let mut amt_w = text_width("金额", head_font, true);
        let mut win_w = text_width("中奖", head_font, true);
        for r in rows {
            no_w = no_w.max(text_width(&r.no.to_string(), no_font, false));
            amt_w = amt_w.max(stake_width(r.amount, r.hk, r.old, amount_font, small));
            win_w = win_w.max(win_content_width(r.win, &r.breakdown, win_font, small));
        }
        if with_subtotal {
            amt_w = amt_w.max(text_width(&money(total), amount_font, true));
            let sub_special = text_width("特码 ", small, false)
                + text_width(&money(special), 13.5 * S, true);
            win_w = win_w.max(sub_special);
        }
        (
            (no_w + 22.0 * S).max(44.0 * S),
            (amt_w + 28.0 * S).max(78.0 * S),
            (win_w + 28.0 * S).max(98.0 * S),
        )
    };

    // ── 列表形式：每 30 行一竖列 ──
    struct ColLayout<'a> {
        rows: &'a [BillRowImage],
        no_w: f32,
        amt_w: f32,
        win_w: f32,
        filler: usize,
        total: f64,
        special: f64,
    }
    let multi = b.rows.len() > 30;
    let chunks: Vec<&[BillRowImage]> = if b.rows.is_empty() {
        vec![&b.rows[..]]
    } else {
        b.rows.chunks(30).collect()
    };
    let list_cols: Vec<ColLayout> = chunks
        .iter()
        .map(|rows| {
            let total = rows.iter().map(|r| r.amount).sum();
            let special = rows.iter().map(|r| r.special_win_stake).sum();
            let (no_w, amt_w, win_w) = calc(rows, true, total, special);
            ColLayout {
                rows,
                no_w,
                amt_w,
                win_w,
                filler: if multi { 30 - rows.len() } else { 0 },
                total,
                special,
            }
        })
        .collect();
    let list_table_w: f32 = list_cols
        .iter()
        .map(|c| c.no_w + c.amt_w + c.win_w)
        .sum::<f32>()
        + col_gap * (list_cols.len().saturating_sub(1)) as f32;
    let list_h = head_h + (if multi { 30 } else { b.rows.len() }) as f32 * row_h + sub_h;

    // ── 分类形式：一个类别超过 30 行时往右续列 ──
    struct CatCol<'a> {
        label: &'a str,
        rows: &'a [BillRowImage],
        no_w: f32,
        amt_w: f32,
        win_w: f32,
        show_totals: bool,
        total_stake: f64,
        total_win: f64,
        total_rebate: f64,
    }
    let mut cat_cols: Vec<CatCol> = Vec::new();
    for c in &b.categories {
        let chunks: Vec<&[BillRowImage]> = if c.rows.is_empty() {
            vec![&c.rows[..]]
        } else {
            c.rows.chunks(30).collect()
        };
        let n = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            let (no_w, amt_w, win_w) = calc(chunk, false, 0.0, 0.0);
            cat_cols.push(CatCol {
                label: &c.label,
                rows: chunk,
                no_w,
                amt_w,
                win_w,
                show_totals: i + 1 == n,
                total_stake: c.total_stake,
                total_win: c.total_win,
                total_rebate: c.total_rebate,
            });
        }
    }
    let cat_w: f32 = cat_cols
        .iter()
        .map(|c| c.no_w + c.amt_w + c.win_w)
        .sum::<f32>()
        + col_gap * (cat_cols.len().saturating_sub(1)) as f32;
    let cat_block_h = cat_cols
        .iter()
        .map(|c| {
            cat_h
                + head_h
                + c.rows.len() as f32 * row_h
                + if c.show_totals {
                    4.0 * S + totals_box_h
                } else {
                    0.0
                }
        })
        .fold(0.0, f32::max);

    let is_cat = b.category_view;
    let content_w = if is_cat { cat_w } else { list_table_w };
    let content_h = if is_cat { cat_block_h } else { list_h };

    // ── 中奖注额 / 中奖金额 / 总计宽度 ──
    let band_h = 26.0 * S;
    let band_w = |items: &[(String, f64)]| -> f32 {
        let mut w = text_width("中奖注额", 13.5 * S, true);
        for (label, v) in items {
            w += 16.0 * S
                + text_width(label, 13.0 * S, false)
                + 4.0 * S
                + text_width(&money(*v), 14.0 * S, true);
        }
        w + 20.0 * S
    };
    let stake_band_w = if b.win_stakes.is_empty() {
        0.0
    } else {
        band_w(&b.win_stakes)
    };
    let amount_band_w = if b.win_amounts.is_empty() {
        0.0
    } else {
        band_w(&b.win_amounts)
    };
    let totals_h = 32.0 * S;
    let net = b.total_stake - b.total_win - b.total_rebate;
    let value_w = [
        money(b.total_stake),
        money(b.total_win),
        money(b.total_rebate),
        money(net),
    ]
    .iter()
    .map(|v| text_width(v, 14.0 * S, true))
    .fold(0.0, f32::max);
    let totals_w =
        20.0 * S + 4.0 * (text_width("总金额", 13.5 * S, true) + 6.0 * S + value_w + 18.0 * S);
    let head_w = text_width(&b.name, 16.5 * S, true)
        + 12.0 * S
        + text_width(&format!("{} 批", b.rows.len()), 11.0 * S, false)
        + 14.0 * S;
    let inner_w = content_w
        .max(head_w)
        .max(stake_band_w)
        .max(amount_band_w)
        .max(totals_w)
        .max(300.0 * S);
    let card_w = inner_w + pad * 2.0;

    let mut card_h = pad + 24.0 * S + 8.0 * S + S + 8.0 * S + content_h;
    if !b.win_stakes.is_empty() {
        card_h += 10.0 * S + band_h;
    }
    if !b.win_amounts.is_empty() {
        card_h += 4.0 * S + band_h;
    }
    card_h += 10.0 * S + totals_h + pad;

    let w = card_w.ceil() as u32;
    let h = card_h.ceil() as u32;
    let mut img = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    hline(&mut img, 0.0, card_w, 0.0, CARD_BORDER);
    hline(&mut img, 0.0, card_w, card_h - S, CARD_BORDER);
    vline(&mut img, 0.0, 0.0, card_h, CARD_BORDER);
    vline(&mut img, card_w - S, 0.0, card_h, CARD_BORDER);

    let left = pad;
    let mut y = pad;
    text_left(&mut img, &b.name, left, y + 12.0 * S, 16.5 * S, TEXT, true);
    let cnt = format!("{} 批", b.rows.len());
    let cw = text_width(&cnt, 11.0 * S, false);
    let pill_x = left + text_width(&b.name, 16.5 * S, true) + 10.0 * S;
    fill(&mut img, pill_x, y + 2.0 * S, cw + 14.0 * S, 20.0 * S, HEAD);
    text_left(&mut img, &cnt, pill_x + 7.0 * S, y + 12.0 * S, 11.0 * S, MUTED, false);
    y += 24.0 * S + 8.0 * S;
    hline(&mut img, left, left + inner_w, y, LINE);
    y += S + 8.0 * S;

    let content_top = y;

    if is_cat {
        let mut cx = left;
        for c in &cat_cols {
            let x0 = cx;
            let x1 = x0 + c.no_w;
            let x2 = x1 + c.amt_w;
            let x3 = x2 + c.win_w;
            let col_w = c.no_w + c.amt_w + c.win_w;

            fill(&mut img, x0, content_top, col_w, cat_h, [0xE2, 0xE8, 0xF0, 255]);
            text_center(
                &mut img,
                c.label,
                (x0 + x3) / 2.0,
                content_top + cat_h / 2.0,
                13.5 * S,
                [0x17, 0x32, 0x4D, 255],
                true,
            );
            let hy = content_top + cat_h;
            fill(&mut img, x0, hy, col_w, head_h, HEAD);
            text_left(&mut img, "#", x0 + 8.0 * S, hy + head_h / 2.0, head_font, BILL_TEXT, true);
            text_left(&mut img, "金额", x1 + 8.0 * S, hy + head_h / 2.0, head_font, BILL_TEXT, true);
            text_left(&mut img, "中奖", x2 + 8.0 * S, hy + head_h / 2.0, head_font, BILL_TEXT, true);

            let mut yy = hy + head_h;
            for r in c.rows {
                text_center(&mut img, &r.no.to_string(), (x0 + x1) / 2.0, yy + row_h / 2.0, no_font, CELL_NO, false);
                draw_stake(&mut img, x1 + 8.0 * S, yy + row_h / 2.0, r.amount, r.hk, r.old, amount_font, small);
                draw_win_content(
                    &mut img,
                    x2 + 8.0 * S,
                    yy + row_h / 2.0,
                    r.win,
                    &r.breakdown,
                    win_font,
                    small,
                );
                yy += row_h;
            }
            let table_bottom = yy;
            hline(&mut img, x0, x3, content_top, LINE);
            hline(&mut img, x0, x3, content_top + cat_h, LINE);
            hline(&mut img, x0, x3, content_top + cat_h + head_h, LINE);
            for i in 0..c.rows.len() {
                hline(
                    &mut img,
                    x0,
                    x3,
                    content_top + cat_h + head_h + (i + 1) as f32 * row_h,
                    LINE,
                );
            }
            // 类别表头整格合并：内部竖线从表头下方开始，左右外框保持到底。
            vline(&mut img, x0, content_top, table_bottom, LINE);
            vline(&mut img, x1, content_top + cat_h, table_bottom, LINE);
            vline(&mut img, x2, content_top + cat_h, table_bottom, LINE);
            vline(&mut img, x3, content_top, table_bottom, LINE);

            if c.show_totals {
                let by = table_bottom + 4.0 * S;
                boxed(
                    &mut img,
                    x0,
                    by,
                    col_w,
                    totals_box_h,
                    SUBTOTAL_BG,
                    [0x94, 0xA3, 0xB8, 255],
                );
                let net = c.total_stake - c.total_win - c.total_rebate;
                let row_h2 = (totals_box_h - 8.0 * S) / 4.0;
                for (i, (label, v, color)) in [
                    ("金额", c.total_stake, BILL_TEXT),
                    ("中奖", c.total_win, GREEN),
                    ("回水", c.total_rebate, BILL_TEXT),
                    ("输赢", net, if net >= 0.0 { GREEN } else { RED }),
                ]
                .iter()
                .enumerate()
                {
                    let cy = by + 4.0 * S + row_h2 * (i as f32 + 0.5);
                    text_left(&mut img, label, x0 + 8.0 * S, cy, 13.0 * S, BILL_TEXT, false);
                    let vt = money(*v);
                    let vw = text_width(&vt, 13.5 * S, true);
                    text_left(
                        &mut img,
                        &vt,
                        x0 + col_w - 8.0 * S - vw,
                        cy,
                        13.5 * S,
                        *color,
                        true,
                    );
                }
            }
            cx += col_w + col_gap;
        }
    } else {
        let mut cx = left;
        for c in &list_cols {
            let x0 = cx;
            let x1 = x0 + c.no_w;
            let x2 = x1 + c.amt_w;
            let x3 = x2 + c.win_w;
            let col_w = c.no_w + c.amt_w + c.win_w;
            fill(&mut img, x0, content_top, col_w, head_h, HEAD);
            text_left(&mut img, "#", x0 + 8.0 * S, content_top + head_h / 2.0, head_font, BILL_TEXT, true);
            text_left(&mut img, "金额", x1 + 8.0 * S, content_top + head_h / 2.0, head_font, BILL_TEXT, true);
            text_left(&mut img, "中奖", x2 + 8.0 * S, content_top + head_h / 2.0, head_font, BILL_TEXT, true);
            let mut yy = content_top + head_h;
            for r in c.rows {
                text_center(&mut img, &r.no.to_string(), (x0 + x1) / 2.0, yy + row_h / 2.0, no_font, CELL_NO, false);
                draw_stake(&mut img, x1 + 8.0 * S, yy + row_h / 2.0, r.amount, r.hk, r.old, amount_font, small);
                draw_win_content(
                    &mut img,
                    x2 + 8.0 * S,
                    yy + row_h / 2.0,
                    r.win,
                    &r.breakdown,
                    win_font,
                    small,
                );
                yy += row_h;
            }
            yy += c.filler as f32 * row_h;
            fill(&mut img, x0, yy, col_w, sub_h, SUBTOTAL_BG);
            text_center(&mut img, "小计", (x0 + x1) / 2.0, yy + sub_h / 2.0, 13.5 * S, BILL_TEXT, true);
            text_left(&mut img, &money(c.total), x1 + 8.0 * S, yy + sub_h / 2.0, amount_font, BLUE, true);
            let cy = yy + sub_h / 2.0;
            let sw = text_width("特码 ", small, false);
            text_left(&mut img, "特码 ", x2 + 8.0 * S, cy, small, MUTED, false);
            text_left(
                &mut img,
                &money(c.special),
                x2 + 8.0 * S + sw,
                cy,
                13.5 * S,
                SPECIAL_RED,
                true,
            );
            hline(&mut img, x0, x3, content_top, LINE);
            hline(&mut img, x0, x3, content_top + head_h, LINE);
            for i in 0..if multi { 30 } else { c.rows.len() } {
                hline(
                    &mut img,
                    x0,
                    x3,
                    content_top + head_h + (i + 1) as f32 * row_h,
                    LINE,
                );
            }
            hline(&mut img, x0, x3, content_top + list_h, LINE);
            vline(&mut img, x0, content_top, content_top + list_h, LINE);
            vline(&mut img, x1, content_top, content_top + list_h, LINE);
            vline(&mut img, x2, content_top, content_top + list_h, LINE);
            vline(&mut img, x3, content_top, content_top + list_h, LINE);
            cx += col_w + col_gap;
        }
    }
    y = content_top + content_h;

    // ── 中奖注额 / 中奖金额条 ──
    if !b.win_stakes.is_empty() {
        y += 10.0 * S;
        boxed(&mut img, left, y, inner_w, band_h, BAND_STAKE_BG, BAND_STAKE_BORDER);
        let mut x = left + 10.0 * S;
        let cy = y + band_h / 2.0;
        text_left(&mut img, "中奖注额", x, cy, 13.5 * S, TITLE_DARK, true);
        x += text_width("中奖注额", 13.5 * S, true) + 16.0 * S;
        for (label, v) in &b.win_stakes {
            text_left(&mut img, label, x, cy, 13.0 * S, BILL_TEXT, false);
            x += text_width(label, 13.0 * S, false) + 4.0 * S;
            let vt = money(*v);
            text_left(&mut img, &vt, x, cy, 14.0 * S, GREEN, true);
            x += text_width(&vt, 14.0 * S, true) + 16.0 * S;
        }
        y += band_h;
    }
    if !b.win_amounts.is_empty() {
        y += 4.0 * S;
        boxed(&mut img, left, y, inner_w, band_h, BAND_AMOUNT_BG, BAND_AMOUNT_BORDER);
        let mut x = left + 10.0 * S;
        let cy = y + band_h / 2.0;
        text_left(&mut img, "中奖金额", x, cy, 13.5 * S, TITLE_DARK, true);
        x += text_width("中奖金额", 13.5 * S, true) + 16.0 * S;
        for (label, v) in &b.win_amounts {
            text_left(&mut img, label, x, cy, 13.0 * S, BILL_TEXT, false);
            x += text_width(label, 13.0 * S, false) + 4.0 * S;
            let vt = money(*v);
            text_left(&mut img, &vt, x, cy, 14.0 * S, GREEN, true);
            x += text_width(&vt, 14.0 * S, true) + 16.0 * S;
        }
        y += band_h;
    }

    // ── 总计条 ──
    y += 10.0 * S;
    boxed(&mut img, left, y, inner_w, totals_h, TOTALS_BG, LINE);
    let mut x = left + 10.0 * S;
    let cy = y + totals_h / 2.0;
    for (label, v, color) in [
        ("总金额", b.total_stake, BILL_TEXT),
        ("总中奖", b.total_win, GREEN),
        ("总回水", b.total_rebate, BILL_TEXT),
        ("总输赢", net, if net >= 0.0 { GREEN } else { RED }),
    ] {
        text_left(&mut img, label, x, cy, 13.5 * S, [0x1E, 0x29, 0x3B, 255], true);
        x += text_width(label, 13.5 * S, true) + 6.0 * S;
        let vt = money(v);
        text_left(&mut img, &vt, x, cy, 14.0 * S, color, true);
        x += text_width(&vt, 14.0 * S, true) + 18.0 * S;
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_complete_block() {
        if fonts().is_none() {
            return; // 没有中文字体的机器上跳过
        }
        let b = BillBlockImage {
            name: "张三".to_string(),
            rows: vec![
                BillRowImage {
                    no: 1,
                    amount: 100.0,
                    hk: 0.0,
                    old: 0.0,
                    win: 0.0,
                    special_win_stake: 0.0,
                    breakdown: Vec::new(),
                },
                BillRowImage {
                    no: 2,
                    amount: 50.0,
                    hk: 30.0,
                    old: 0.0,
                    win: 1500.0,
                    special_win_stake: 0.0,
                    breakdown: vec![("三中三".to_string(), 10.0, 750.0)],
                },
            ],
            total_stake: 150.0,
            total_win: 1500.0,
            total_rebate: 15.0,
            win_stakes: vec![("三中三".to_string(), 10.0)],
            win_amounts: vec![("三中三".to_string(), 7500.0)],
            category_view: false,
            categories: Vec::new(),
        };
        let img = render_block(&b);
        assert!(img.width() > 300);
        assert!(img.height() > 200);

        let dir = std::env::temp_dir().join("wechat_viewer_bill_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = save_block(&b, &dir, "test", 1).unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 1000);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}




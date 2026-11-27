//! 小码减额的三个弹窗内容：减额（分类型整列减额 / 复制 / 合并）、查重、减额记录。
//! 对应参考项目的 StakeFilterModal / MergeCombosModal / DuplicateCheckModal /
//! XiaoMaOffsetLogModal。弹窗外壳（遮罩、标题、关闭按钮）是父模块的 `xm_modal`。

use std::collections::{HashMap, HashSet};

use eframe::egui::{
    self, text::LayoutJob, text::TextFormat, Align2, Color32, CornerRadius, FontId, RichText, Stroke,
};

use super::{
    risk_region_tab_button, xm_num, XmDeduct, BILL_HEAD, BORDER, CARD, GREEN, MUTED, RED, SUB,
    SUBTOTAL_BG, TEXT,
    XM_REGION_NAMES,
};
use crate::draw::Draws;
use crate::risk::XiaoMaRow;
use crate::xiaoma_dup::{self, DupEntry};
use crate::xiaoma_merge::{self, MergeCombo};
use crate::xiaoma_offsets::{self, OffsetMap};

/// 减额弹窗的四个类型栏：标签，以及合并时的组合大小 k。
pub const XM_TYPES: [(&str, usize); 4] = [("二中二", 2), ("三中三", 3), ("特碰", 2), ("三中二", 3)];

const AMBER: Color32 = Color32::from_rgb(0xB4, 0x53, 0x09);
const AMBER_BG: Color32 = Color32::from_rgb(0xFE, 0xF3, 0xC7);
const DRAW_LINE: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);

/// 这一地区的中奖号（两位补零），含特别号。
pub fn draw_set(draws: &Draws, region: usize) -> HashSet<String> {
    let draw = draws.get(crate::risk::REGION_NAMES[region]);
    draw.draw
        .iter()
        .copied()
        .chain(draw.special)
        .map(|n| format!("{n:02}"))
        .collect()
}

/// 小按钮，宽度随文字。不可用时灰掉。返回是否被点中。
fn xm_btn(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(11.5)).min_size(egui::vec2(0.0, 24.0)),
    )
    .clicked()
}

fn region_tabs(ui: &mut egui::Ui, selected: &mut usize) {
    ui.horizontal(|ui| {
        for (r, name) in XM_REGION_NAMES.iter().enumerate() {
            if risk_region_tab_button(ui, name, *selected == r) {
                *selected = r;
            }
        }
    });
}

/// 过滤条件：空或非有限数字时为 None。
fn threshold_of(xm: &XmDeduct) -> Option<f64> {
    let text = xm.deduct_threshold.trim();
    if text.is_empty() {
        None
    } else {
        text.parse::<f64>().ok().filter(|v| v.is_finite())
    }
}

fn parse_numbers(key: &str) -> Vec<u32> {
    key.split('-').filter_map(|s| s.parse::<u32>().ok()).collect()
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | ',' | '，' | '、' | '/' | '\\')
}

/// 参考项目 `tokenizeKey`：按分隔符切开，纯汉字逐字拆，一到两位数字补零。
fn tokenize(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in expr.split(is_sep).filter(|p| !p.is_empty()) {
        if part.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            out.extend(part.chars().map(|c| c.to_string()));
        } else {
            out.push(part.to_string());
        }
    }
    out.into_iter()
        .map(|t| {
            if (1..=2).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit()) {
                format!("{t:0>2}")
            } else {
                t
            }
        })
        .collect()
}

/// 组内所有表达式共有的号码。表达式种类不足 2 种时没有共用号。
fn common_tokens(entries: &[DupEntry], group: &[usize]) -> HashSet<String> {
    let mut distinct: Vec<&str> = Vec::new();
    for &i in group {
        if !distinct.contains(&entries[i].expr.as_str()) {
            distinct.push(&entries[i].expr);
        }
    }
    if distinct.len() < 2 {
        return HashSet::new();
    }
    let mut common: HashSet<String> = tokenize(distinct[0]).into_iter().collect();
    for expr in &distinct[1..] {
        let set: HashSet<String> = tokenize(expr).into_iter().collect();
        common.retain(|t| set.contains(t));
    }
    common
}

struct Seg {
    text: String,
    dup: bool,
    draw: bool,
}

/// 参考项目 `renderSegments`：把表达式切成段，标出共用号与中奖号。
/// 中奖号只在表达式命中至少 3 个时标出（三中三才可能中）。
fn segments(expr: &str, common: &HashSet<String>, draw: &HashSet<String>) -> Vec<Seg> {
    let tokens = tokenize(expr);
    let entry_tokens: HashSet<&String> = tokens.iter().collect();
    let draw_wanted = tokens.iter().filter(|t| draw.contains(*t)).count() >= 3;
    let mut out = Vec::new();
    let mut rest = expr;
    for token in &tokens {
        let Some(idx) = rest.find(token.as_str()) else {
            continue;
        };
        if idx > 0 {
            out.push(Seg {
                text: rest[..idx].to_string(),
                dup: false,
                draw: false,
            });
        }
        out.push(Seg {
            text: token.clone(),
            dup: common.contains(token) && entry_tokens.contains(token),
            draw: draw_wanted && draw.contains(token),
        });
        rest = &rest[idx + token.len()..];
    }
    if !rest.is_empty() {
        out.push(Seg {
            text: rest.to_string(),
            dup: false,
            draw: false,
        });
    }
    if out.is_empty() {
        out.push(Seg {
            text: expr.to_string(),
            dup: false,
            draw: false,
        });
    }
    out
}

fn expr_job(segs: &[Seg]) -> LayoutJob {
    let mut job = LayoutJob::default();
    for seg in segs {
        let mut fmt = TextFormat {
            font_id: FontId::proportional(11.0),
            color: TEXT,
            ..Default::default()
        };
        if seg.dup {
            fmt.background = AMBER_BG;
            fmt.color = AMBER;
        }
        if seg.draw {
            fmt.underline = Stroke::new(2.0, DRAW_LINE);
        }
        job.append(&seg.text, 0.0, fmt);
    }
    job
}

/// 「减额」弹窗：过滤条件、复制全部、四个类型栏（每栏：整列减额 / 复制 / 合并，每行 −）。
pub fn deduct_body(xm: &mut XmDeduct, rows: &[XiaoMaRow], ui: &mut egui::Ui) {
    region_tabs(ui, &mut xm.deduct_region);
    let region = xm.deduct_region;
    let threshold = threshold_of(xm);
    let columns: Vec<Vec<XiaoMaRow>> = XM_TYPES
        .iter()
        .map(|(label, _)| match threshold {
            Some(t) => xiaoma_offsets::column_rows(rows, region, label, t),
            None => Vec::new(),
        })
        .collect();

    ui.add_space(6.0);
    let mut copy_all = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("过滤条件").size(12.0).color(SUB));
        let mut resp = ui.add(
            egui::TextEdit::singleline(&mut xm.deduct_threshold)
                .desired_width(120.0)
                .hint_text("例如：10"),
        );
        super::text_edit_context_menu(ui.ctx(), resp.id, &mut xm.deduct_threshold, &mut resp);
        if xm.deduct_focus_pending {
            resp.request_focus();
            xm.deduct_focus_pending = false;
        }
        copy_all = xm_btn(ui, "复制全部", threshold.is_some());
        if let Some(hint) = &xm.deduct_hint {
            ui.label(RichText::new(hint).size(11.5).color(MUTED));
        }
    });
    if copy_all {
        if let Some(t) = threshold {
            let lines: Vec<String> = XM_TYPES
                .iter()
                .zip(&columns)
                .flat_map(|((label, _), col)| copy_lines(label, col, t))
                .collect();
            let count = lines.len();
            if count == 0 {
                xm.deduct_hint = Some("无可复制内容".to_string());
            } else {
                ui.ctx().copy_text(lines.join("\n"));
                xm.deduct_hint = Some(format!("已复制 {count} 条"));
            }
        }
    }
    ui.add_space(8.0);

    // 类型栏铺满正文剩余高度：弹窗高度由 xm_modal 的 body_ratio 决定，
    // 这里跟着它走，行数多时在栏内滚动。
    let col_h = (ui.available_height() - 6.0).max(240.0);
    ui.columns(XM_TYPES.len(), |cols| {
        for (i, (label, _)) in XM_TYPES.iter().enumerate() {
            deduct_column(xm, &mut cols[i], i, label, region, &columns[i], threshold, col_h);
        }
    });
}

/// 一个类型栏：标题、三个按钮、表格。
fn deduct_column(
    xm: &mut XmDeduct,
    ui: &mut egui::Ui,
    idx: usize,
    label: &str,
    region: usize,
    rows: &[XiaoMaRow],
    threshold: Option<f64>,
    col_h: f32,
) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong().size(13.0).color(TEXT));
            if threshold.is_some() {
                ui.label(RichText::new(format!("{} 条", rows.len())).size(11.5).color(MUTED));
            }
        });
        let has_rows = threshold.is_some() && !rows.is_empty();
        ui.horizontal(|ui| {
            if xm_btn(ui, "整列减额", has_rows) {
                if let Some(t) = threshold {
                    batch_deduct(xm, label, region, rows, t);
                }
            }
            if xm_btn(ui, "复制", has_rows) {
                if let Some(t) = threshold {
                    let lines = copy_lines(label, rows, t);
                    ui.ctx().copy_text(lines.join("\n"));
                    xm.deduct_hint = Some(format!("已复制 {} 条{label}", lines.len()));
                }
            }
            if xm_btn(ui, "合并", has_rows) {
                if let Some(t) = threshold {
                    // 合并窗口不再提供选择：打开即对本类型栏的全部组合执行合并。
                    let combos: Vec<MergeCombo> = rows
                        .iter()
                        .map(|row| MergeCombo {
                            key: row.key.clone(),
                            numbers: parse_numbers(&row.key),
                            total: xiaoma_offsets::excess_of(row.stake, t),
                        })
                        .collect();
                    xm.merge_open = Some(idx);
                    xm.merge_result = Some(xiaoma_merge::perform(&combos, XM_TYPES[idx].1));
                    xm.merge_hint = None;
                }
            }
        });
        ui.add_space(4.0);

        match threshold {
            None => {
                ui.label(RichText::new("请输入过滤值").size(12.0).color(MUTED));
            }
            Some(_) if rows.is_empty() => {
                ui.label(RichText::new("无符合条件记录").size(12.0).color(MUTED));
            }
            Some(t) => {
                // 表头放在滚动区外面，滚动时固定不动。
                let total = (ui.available_width() - scrollbar_width(ui)).max(200.0);
                // 组合列不再吃掉全部剩余宽度：留一点空白，表格更紧凑。
                let flex = (total - 24.0 - 52.0 - 30.0).clamp(60.0, 110.0);
                let widths = [24.0, flex, 52.0, 30.0];
                let cols = GridCols::new(ui.cursor().min.x, &widths);
                grid_header_plain(ui, &cols, &["#", "组合", "投注额", "操作"]);
                egui::ScrollArea::vertical()
                    .id_salt(("xm_col_scroll", idx))
                    .max_height((col_h - 60.0).max(120.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (n, row) in rows.iter().enumerate() {
                            let bg = if n % 2 == 1 { SUBTOTAL_BG } else { CARD };
                            let (_, p, rect) =
                                cols.row_plain(ui, 22.0, bg, egui::Sense::hover());
                            let (x0, x1) = cols.col(0);
                            p.text(
                                egui::pos2((x0 + x1) / 2.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                format!("{}", n + 1),
                                FontId::proportional(10.0),
                                SUB,
                            );
                            let (x0, x1) = cols.col(1);
                            cell_text(
                                &p,
                                rect,
                                x0,
                                x1,
                                &row.key,
                                FontId::proportional(11.0),
                                TEXT,
                            );
                            let excess = xiaoma_offsets::excess_of(row.stake, t);
                            let (x0, x1) = cols.col(2);
                            cell_text_center(
                                &p,
                                rect,
                                x0,
                                x1,
                                &format!("+{}", xm_num(excess)),
                                FontId::proportional(11.0),
                                RED,
                            );
                            let (x0, x1) = cols.col(3);
                            // 自绘的小按钮：尺寸完全可控，并在行内垂直居中。
                            let btn_rect = egui::Rect::from_center_size(
                                egui::pos2((x0 + x1) / 2.0, rect.center().y),
                                egui::vec2(16.0, 12.0),
                            );
                            let btn = ui.interact(
                                btn_rect,
                                ui.make_persistent_id(("xm_minus", idx, n)),
                                egui::Sense::click(),
                            );
                            let bp = ui.painter();
                            bp.rect_filled(
                                btn_rect,
                                CornerRadius::same(3),
                                if btn.hovered() {
                                    Color32::from_rgb(0xEF, 0xF3, 0xF8)
                                } else {
                                    Color32::WHITE
                                },
                            );
                            bp.rect_stroke(
                                btn_rect,
                                CornerRadius::same(3),
                                Stroke::new(1.0, BORDER),
                                egui::StrokeKind::Inside,
                            );
                            bp.text(
                                btn_rect.center(),
                                Align2::CENTER_CENTER,
                                "−",
                                FontId::proportional(9.5),
                                TEXT,
                            );
                            if btn.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if btn.on_hover_text("减到过滤条件这个数").clicked() {
                                let mut delta = OffsetMap::new();
                                delta.insert(xiaoma_offsets::key_of(row), excess);
                                xm.store.apply(
                                    delta,
                                    "减额",
                                    format!(
                                        "{} {} {} 单行 -{}",
                                        XM_REGION_NAMES[region],
                                        label,
                                        row.key,
                                        xm_num(excess)
                                    ),
                                );
                                xm.deduct_hint =
                                    Some(format!("已减额 1 条{label} -{}", xm_num(excess)));
                            }
                        }
                    });
            }
        }
    });
}

/// 整列减额：每行减掉自己超出过滤值的那个数。
fn batch_deduct(xm: &mut XmDeduct, label: &str, region: usize, rows: &[XiaoMaRow], t: f64) {
    let delta = xiaoma_offsets::excess_delta(rows, region, t);
    let count = delta.len();
    if count == 0 {
        xm.deduct_hint = Some("没有可减额的行".to_string());
        return;
    }
    let total: f64 = delta.values().sum();
    xm.store.apply(
        delta,
        "减额",
        format!(
            "{} {} 整列减额 过滤值 {}，{} 行",
            XM_REGION_NAMES[region],
            label,
            xm_num(t),
            count
        ),
    );
    xm.deduct_hint = Some(format!("已减额 {count} 条{label}，合计 -{}", xm_num(total)));
}

/// 复制用的文本：`三中三 01-02-03 各-5`，与参考项目逐字一致（粘到「组合减额」执行）。
fn copy_lines(label: &str, rows: &[XiaoMaRow], t: f64) -> Vec<String> {
    rows.iter()
        .map(|row| {
            format!(
                "{label} {} 各-{}",
                row.key,
                xm_num(xiaoma_offsets::excess_of(row.stake, t))
            )
        })
        .collect()
}

/// 垂直滚动条占用的宽度。表头画在滚动区外面，需要预留这部分宽度才能和行对齐。
fn scrollbar_width(ui: &egui::Ui) -> f32 {
    let scroll = &ui.spacing().scroll;
    if scroll.floating {
        0.0
    } else {
        scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin
    }
}

/// 带线表格的列边界：xs[0] 是左缘，xs[i + 1] 是第 i 列的右缘。
struct GridCols {
    xs: Vec<f32>,
}

impl GridCols {
    fn new(x0: f32, widths: &[f32]) -> Self {
        let mut xs = Vec::with_capacity(widths.len() + 1);
        xs.push(x0);
        let mut x = x0;
        for w in widths {
            x += *w;
            xs.push(x);
        }
        Self { xs }
    }

    fn col(&self, i: usize) -> (f32, f32) {
        (self.xs[i], self.xs[i + 1])
    }

    /// 分配一行并画底色、列线与底线。
    fn row(
        &self,
        ui: &mut egui::Ui,
        h: f32,
        bg: Color32,
        sense: egui::Sense,
    ) -> (egui::Response, egui::Painter, egui::Rect) {
        let w = self.xs[self.xs.len() - 1] - self.xs[0];
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), sense);
        let p = ui.painter().with_clip_rect(rect);
        p.rect_filled(rect, CornerRadius::ZERO, bg);
        for &x in &self.xs {
            p.vline(x, rect.y_range(), Stroke::new(1.0, BORDER));
        }
        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, BORDER));
        (resp, p, rect)
    }
}

impl GridCols {
    /// 无表格线的一行（只铺底色）：减额窗口的类型栏用它，保持原来的列表样式。
    fn row_plain(
        &self,
        ui: &mut egui::Ui,
        h: f32,
        bg: Color32,
        sense: egui::Sense,
    ) -> (egui::Response, egui::Painter, egui::Rect) {
        let w = self.xs[self.xs.len() - 1] - self.xs[0];
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), sense);
        let p = ui.painter().with_clip_rect(rect);
        p.rect_filled(rect, CornerRadius::ZERO, bg);
        (resp, p, rect)
    }
}

/// 无表格线的表头。
fn grid_header_plain(ui: &mut egui::Ui, cols: &GridCols, labels: &[&str]) {
    let (_, p, rect) = cols.row_plain(ui, 22.0, BILL_HEAD, egui::Sense::hover());
    for (i, label) in labels.iter().enumerate() {
        let (x0, x1) = cols.col(i);
        cell_text(&p, rect, x0, x1, label, FontId::proportional(10.5), MUTED);
    }
}

fn grid_header(ui: &mut egui::Ui, cols: &GridCols, labels: &[&str]) {
    let (_, p, rect) = cols.row(ui, 24.0, BILL_HEAD, egui::Sense::hover());
    for (i, label) in labels.iter().enumerate() {
        let (x0, x1) = cols.col(i);
        cell_text(&p, rect, x0, x1, label, FontId::proportional(10.5), MUTED);
    }
}

fn cell_text(
    p: &egui::Painter,
    rect: egui::Rect,
    x0: f32,
    x1: f32,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, rect.top()),
        egui::pos2(x1, rect.bottom()),
    ));
    clip.text(
        egui::pos2(x0 + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
}

fn cell_text_center(
    p: &egui::Painter,
    rect: egui::Rect,
    x0: f32,
    x1: f32,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, rect.top()),
        egui::pos2(x1, rect.bottom()),
    ));
    clip.text(
        egui::pos2((x0 + x1) / 2.0, rect.center().y),
        Align2::CENTER_CENTER,
        text,
        font,
        color,
    );
}

fn cell_text_right(
    p: &egui::Painter,
    rect: egui::Rect,
    x0: f32,
    x1: f32,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, rect.top()),
        egui::pos2(x1, rect.bottom()),
    ));
    clip.text(
        egui::pos2(x1 - 8.0, rect.center().y),
        Align2::RIGHT_CENTER,
        text,
        font,
        color,
    );
}

/// 「合并组合」弹窗：勾选一个类型栏里的组合，执行合并，复制结果。
pub fn merge_body(xm: &mut XmDeduct, ui: &mut egui::Ui) {
    let Some(ti) = xm.merge_open else {
        return;
    };
    let label = XM_TYPES[ti].0;
    // 组合在按下「合并」时就已全部自动合并好（见 deduct_column），这里只展示结果。

    let Some(result) = xm.merge_result.clone() else {
        ui.label(RichText::new("没有可合并的组合").size(12.0).color(MUTED));
        return;
    };
    if result.merged.is_empty() && result.unmerged.is_empty() {
        ui.label(
            RichText::new("这一列的组合凑不成任何完整的号码集合，无法合并。")
                .size(12.0)
                .color(MUTED),
        );
        return;
    }
    let text = xiaoma_merge::text(&result, label);
    ui.add_space(6.0);

    if !result.merged.is_empty() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("已合并 ({} 组)", result.merged.len())).strong().size(12.5));
            if xm_btn(ui, "复制", true) {
                ui.ctx().copy_text(text.merged.clone());
                xm.merge_hint = Some(format!("已复制 {} 条", result.merged.len()));
            }
        });
        // 表头在滚动区外，滚动时固定不动；宽度预留滚动条，保证表头与行对齐。
        let total = (ui.available_width() - scrollbar_width(ui)).max(200.0);
        let widths = [(total - 80.0 - 96.0).max(120.0), 80.0, 96.0];
        let cols = GridCols::new(ui.cursor().min.x, &widths);
        grid_header(ui, &cols, &["合并结果", "每组金额", "总投注额"]);
        egui::ScrollArea::vertical()
            .id_salt("xm_merge_done_scroll")
            .max_height(320.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for g in &result.merged {
                    let each = (g.total / g.source_keys.len() as f64).round();
                    let (_, p, rect) = cols.row(ui, 24.0, CARD, egui::Sense::hover());
                    let (x0, x1) = cols.col(0);
                    cell_text(&p, rect, x0, x1, &g.merged_key, FontId::proportional(11.5), TEXT);
                    let (x0, x1) = cols.col(1);
                    cell_text_right(
                        &p,
                        rect,
                        x0,
                        x1,
                        &xm_num(each),
                        FontId::proportional(11.5),
                        TEXT,
                    );
                    let (x0, x1) = cols.col(2);
                    cell_text_right(
                        &p,
                        rect,
                        x0,
                        x1,
                        &xm_num(g.total.round()),
                        FontId::proportional(11.5),
                        TEXT,
                    );
                }
            });
    }

    if !result.unmerged.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("无法合并 ({} 条)", result.unmerged.len())).strong().size(12.5));
            if xm_btn(ui, "复制", true) {
                ui.ctx().copy_text(text.unmerged.clone());
                xm.merge_hint = Some(format!("已复制 {} 条", result.unmerged.len()));
            }
        });
        // 表头在滚动区外，滚动时固定不动。
        let total = (ui.available_width() - scrollbar_width(ui)).max(200.0);
        let widths = [(total - 96.0).max(120.0), 96.0];
        let cols = GridCols::new(ui.cursor().min.x, &widths);
        grid_header(ui, &cols, &["组合", "投注额"]);
        egui::ScrollArea::vertical()
            .id_salt("xm_merge_left_scroll")
            .max_height(240.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for c in &result.unmerged {
                    let (_, p, rect) = cols.row(ui, 24.0, CARD, egui::Sense::hover());
                    let (x0, x1) = cols.col(0);
                    cell_text(&p, rect, x0, x1, &c.key, FontId::proportional(11.5), TEXT);
                    let (x0, x1) = cols.col(1);
                    cell_text_right(
                        &p,
                        rect,
                        x0,
                        x1,
                        &xm_num(c.total.round()),
                        FontId::proportional(11.5),
                        TEXT,
                    );
                }
            });
    }

    if !text.all.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if xm_btn(ui, "复制合并结果", true) {
                ui.ctx().copy_text(text.all.clone());
                xm.merge_hint = Some("已复制合并结果".to_string());
            }
            if let Some(hint) = &xm.merge_hint {
                ui.label(RichText::new(hint).size(11.5).color(MUTED));
            }
        });
    }
}

/// 这一组里每条录入分到的「已减」。只算展开中的组。
fn group_deducted(
    xm: &XmDeduct,
    region: usize,
    gi: usize,
    group: &xiaoma_dup::DupGroup,
) -> HashMap<usize, f64> {
    if xm.dup_expanded.contains(&(region, gi)) {
        xiaoma_dup::prorated_deducted(&xm.dup_entries, &group.entries, &xm.store.offsets)
    } else {
        HashMap::new()
    }
}

/// 查重地区页签：比其它弹窗的页签小一号，只是切换地区。
fn dup_region_tabs(ui: &mut egui::Ui, selected: &mut usize) {
    ui.horizontal(|ui| {
        for (r, name) in XM_REGION_NAMES.iter().enumerate() {
            if ui
                .selectable_label(*selected == r, RichText::new(*name).size(10.5))
                .clicked()
            {
                *selected = r;
            }
        }
    });
}

/// 顶部统计：一行小字，不占卡片的空间。
fn stat_chip(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.label(RichText::new(label).size(10.0).color(MUTED));
    ui.label(RichText::new(value).size(10.0).strong().color(color));
    ui.add_space(8.0);
}

/// 「查重」弹窗：只看，不减额。顶部一行统计，下面是按录入次数排的组。
pub fn dup_body(xm: &mut XmDeduct, draw_sets: &[HashSet<String>; 3], ui: &mut egui::Ui) {
    dup_region_tabs(ui, &mut xm.dup_region);
    let region = xm.dup_region;
    let groups = xm.dup_groups[region].clone();
    let draw = &draw_sets[region];

    let deducted: Vec<HashMap<usize, f64>> = groups
        .iter()
        .enumerate()
        .map(|(gi, g)| group_deducted(xm, region, gi, g))
        .collect();
    let mut total = 0.0;
    for (group, d) in groups.iter().zip(&deducted) {
        for &ei in &group.entries {
            total += xm.dup_entries[ei].amount - d.get(&ei).copied().unwrap_or(0.0);
        }
    }
    let dup_count = groups.iter().filter(|g| g.entries.len() >= 2).count();

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        stat_chip(ui, "数字表达式", &format!("{} 组", groups.len()), TEXT);
        let dup_color = if dup_count > 0 { AMBER } else { TEXT };
        stat_chip(ui, "重复组", &format!("{dup_count} 组"), dup_color);
        stat_chip(ui, "总投注额", &xm_num(total), TEXT);
    });
    ui.add_space(4.0);

    if groups.is_empty() {
        ui.label(
            RichText::new("这一地区当前没有符合条件的三中三（需 ≥5 个数字且不含「拖」）")
                .size(11.0)
                .color(MUTED),
        );
        return;
    }

    // 有线表格：表头 + 每组一行（可点开看录入明细），行列都画线。
    egui::ScrollArea::vertical()
        .id_salt("dup_table_scroll")
        .max_height(600.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let cols = DupCols::new(ui);
            dup_table_header(ui, &cols);
            for (gi, group) in groups.iter().enumerate() {
                let expandable = group.entries.len() >= 2;
                let expanded = xm.dup_expanded.contains(&(region, gi));
                let mut distinct: Vec<&str> = Vec::new();
                for &ei in &group.entries {
                    if !distinct.contains(&xm.dup_entries[ei].expr.as_str()) {
                        distinct.push(&xm.dup_entries[ei].expr);
                    }
                }
                let distinct = distinct.len();
                let raw_total: f64 =
                    group.entries.iter().map(|&ei| xm.dup_entries[ei].amount).sum();
                let group_total: f64 = group
                    .entries
                    .iter()
                    .map(|&ei| {
                        xm.dup_entries[ei].amount
                            - deducted[gi].get(&ei).copied().unwrap_or(0.0)
                    })
                    .sum();
                let reduced = (group_total - raw_total).abs() > 1e-9;
                let bg = if expandable { AMBER_BG } else { CARD };
                let sense = if expandable {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (resp, p, rect) = cols.row(ui, 26.0, bg, sense);
                let cy = rect.center().y;

                p.text(
                    egui::pos2((cols.x0 + cols.x1) / 2.0, cy),
                    Align2::CENTER_CENTER,
                    format!("{}", gi + 1),
                    FontId::proportional(10.0),
                    SUB,
                );

                // 表达式：可展开的组前面带箭头，后面注明「含 N 个表达式 / 重复 N 次」。
                let mut job = LayoutJob::default();
                if expandable {
                    let arrow = if expanded { "▲ " } else { "▼ " };
                    job.append(
                        arrow,
                        0.0,
                        TextFormat {
                            font_id: FontId::proportional(9.0),
                            color: MUTED,
                            ..Default::default()
                        },
                    );
                }
                job.append(
                    &group.key,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(11.0),
                        color: TEXT,
                        ..Default::default()
                    },
                );
                if expandable {
                    let note = if distinct > 1 {
                        format!("   含 {distinct} 个表达式")
                    } else {
                        format!("   重复 {} 次", group.entries.len())
                    };
                    job.append(
                        &note,
                        0.0,
                        TextFormat {
                            font_id: FontId::proportional(9.5),
                            color: MUTED,
                            ..Default::default()
                        },
                    );
                }
                let galley = p.layout_job(job);
                let pc = cols.cell_clip(&p, rect, cols.x1 + 5.0, cols.x2 - 4.0);
                pc.galley(
                    egui::pos2(cols.x1 + 5.0, cy - galley.size().y / 2.0),
                    galley,
                    TEXT,
                );

                let count_color = if expandable { AMBER } else { TEXT };
                p.text(
                    egui::pos2(cols.x2 + 5.0, cy),
                    Align2::LEFT_CENTER,
                    format!("{} 条", group.entries.len()),
                    FontId::proportional(10.0),
                    count_color,
                );
                p.text(
                    egui::pos2(cols.x3 + 5.0, cy),
                    Align2::LEFT_CENTER,
                    xm_num(group_total),
                    FontId::proportional(10.0),
                    if reduced { AMBER } else { TEXT },
                );

                if expandable {
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if resp.clicked() {
                        if expanded {
                            xm.dup_expanded.remove(&(region, gi));
                        } else {
                            xm.dup_expanded.insert((region, gi));
                        }
                    }
                }

                if expanded && expandable {
                    dup_table_members(xm, ui, &cols, group, &deducted[gi], draw);
                }
            }
        });
}

/// 查重表格的列边界：左缘 / # 后 / 表达式后 / 次数后 / 右缘。
struct DupCols {
    x0: f32,
    x1: f32,
    x2: f32,
    x3: f32,
    x4: f32,
}

impl DupCols {
    fn new(ui: &egui::Ui) -> Self {
        let x0 = ui.cursor().min.x;
        let x4 = x0 + ui.available_width();
        let x1 = x0 + 38.0;
        let x3 = x4 - 86.0;
        let x2 = x3 - 80.0;
        Self { x0, x1, x2, x3, x4 }
    }

    /// 分配一行并画底色与列线；返回响应、裁剪到这一行的画笔、行矩形。
    fn row(
        &self,
        ui: &mut egui::Ui,
        h: f32,
        bg: Color32,
        sense: egui::Sense,
    ) -> (egui::Response, egui::Painter, egui::Rect) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(self.x4 - self.x0, h), sense);
        let p = ui.painter().with_clip_rect(rect);
        p.rect_filled(rect, CornerRadius::ZERO, bg);
        for x in [self.x0, self.x1, self.x2, self.x3, self.x4] {
            p.vline(x, rect.y_range(), Stroke::new(1.0, BORDER));
        }
        p.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, BORDER));
        (resp, p, rect)
    }

    fn cell_clip(&self, p: &egui::Painter, rect: egui::Rect, x0: f32, x1: f32) -> egui::Painter {
        p.with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(x0, rect.top()),
            egui::pos2(x1, rect.bottom()),
        ))
    }
}

fn dup_table_header(ui: &mut egui::Ui, cols: &DupCols) {
    let (_, p, rect) = cols.row(ui, 24.0, BILL_HEAD, egui::Sense::hover());
    let cy = rect.center().y;
    let font = FontId::proportional(10.5);
    for (x, t) in [
        ((cols.x0 + cols.x1) / 2.0, "#"),
        (cols.x1 + 5.0, "数字表达式"),
        (cols.x2 + 5.0, "录入次数"),
        (cols.x3 + 5.0, "总投注额"),
    ] {
        let align = if t == "#" {
            Align2::CENTER_CENTER
        } else {
            Align2::LEFT_CENTER
        };
        p.text(egui::pos2(x, cy), align, t, font.clone(), MUTED);
    }
}

/// 展开后的录入行：表达式（共用号黄底、已开号红下划线）、各、减后金额。
fn dup_table_members(
    xm: &XmDeduct,
    ui: &mut egui::Ui,
    cols: &DupCols,
    group: &xiaoma_dup::DupGroup,
    deducted: &HashMap<usize, f64>,
    draw: &HashSet<String>,
) {
    let common = common_tokens(&xm.dup_entries, &group.entries);
    for &ei in &group.entries {
        let entry = &xm.dup_entries[ei];
        let d = deducted.get(&ei).copied().unwrap_or(0.0);
        let combos = entry.combo_count().max(1) as f64;
        let per = if d > 0.0 { entry.per - d / combos } else { entry.per };
        let total = entry.amount - d;

        let (resp, p, rect) = cols.row(ui, 22.0, CARD, egui::Sense::hover());
        let cy = rect.center().y;

        let mut job = expr_job(&segments(&entry.expr, &common, draw));
        if d > 0.0 {
            job.append(
                &format!("   已减 {}", xm_num(d)),
                0.0,
                TextFormat {
                    font_id: FontId::proportional(9.5),
                    color: AMBER,
                    ..Default::default()
                },
            );
        }
        let galley = p.layout_job(job);
        let pc = cols.cell_clip(&p, rect, cols.x1 + 13.0, cols.x2 - 4.0);
        pc.galley(
            egui::pos2(cols.x1 + 13.0, cy - galley.size().y / 2.0),
            galley,
            TEXT,
        );

        p.text(
            egui::pos2(cols.x2 + 5.0, cy),
            Align2::LEFT_CENTER,
            format!("各{}", xm_num(per)),
            FontId::proportional(10.0),
            SUB,
        );
        p.text(
            egui::pos2(cols.x3 + 5.0, cy),
            Align2::LEFT_CENTER,
            xm_num(total),
            FontId::proportional(10.5),
            if d > 0.0 { AMBER } else { TEXT },
        );

        let mut hint = format!("各{} · {} 组合", xm_num(per), entry.combo_count());
        if d > 0.0 {
            hint.push_str(&format!(" · 已减 {}", xm_num(d)));
        }
        resp.on_hover_text(hint);
    }
}

const LINE_PREVIEW_MAX: usize = 3;

/// 「减额记录」弹窗：每笔一张卡片，可多选撤回。明细超过 3 行时折起来。
pub fn log_body(xm: &mut XmDeduct, ui: &mut egui::Ui) {
    let len = xm.store.log.len();
    let undoable: Vec<usize> = (0..len).filter(|&i| !xm.store.log[i].undone).collect();

    ui.horizontal(|ui| {
        if xm_btn(ui, "全选可撤回", !undoable.is_empty()) {
            xm.log_checked = undoable.iter().copied().collect();
        }
        if xm_btn(ui, "清空选择", !xm.log_checked.is_empty()) {
            xm.log_checked.clear();
        }
        let picked = xm.log_checked.len();
        if xm_btn(ui, &format!("撤回所选（{picked}）"), picked > 0) {
            let mut indices: Vec<usize> = xm.log_checked.iter().copied().collect();
            indices.sort_unstable();
            for i in indices {
                xm.store.undo(i);
            }
            xm.log_checked.clear();
            xm.log_hint = Some(format!("已撤回 {picked} 笔"));
        }
        if let Some(hint) = &xm.log_hint {
            ui.label(RichText::new(hint).size(11.5).color(MUTED));
        }
    });
    ui.label(
        RichText::new(format!("共 {len} 笔，其中可撤回 {} 笔", undoable.len()))
            .size(12.0)
            .color(MUTED),
    );
    ui.add_space(6.0);
    if len == 0 {
        ui.label(RichText::new("还没有任何减额记录").size(12.0).color(MUTED));
        return;
    }

    egui::ScrollArea::vertical()
        .max_height(460.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for i in (0..len).rev() {
                let entry = xm.store.log[i].clone();
                egui::Frame::new()
                    .fill(if entry.undone { CARD } else { AMBER_BG })
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let mut checked = xm.log_checked.contains(&i);
                            let resp = ui.add_enabled(
                                !entry.undone,
                                egui::Checkbox::without_text(&mut checked),
                            );
                            if resp.changed() {
                                if checked {
                                    xm.log_checked.insert(i);
                                } else {
                                    xm.log_checked.remove(&i);
                                }
                            }
                            ui.label(RichText::new(&entry.kind).strong().size(12.5).color(TEXT));
                            ui.label(RichText::new(totals_text(&entry.delta)).size(12.0).color(TEXT));
                            ui.label(
                                RichText::new(format!("{} 行", entry.delta.len())).size(11.5).color(SUB),
                            );
                            if entry.undone {
                                ui.label(RichText::new("已撤回").size(11.5).color(AMBER));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(&entry.at).size(11.0).color(SUB));
                            });
                        });
                        if !entry.desc.is_empty() {
                            ui.label(RichText::new(&entry.desc).size(11.5).color(MUTED));
                        }

                        let details = detail_rows(&entry.delta);
                        let expanded = xm.log_expanded.contains(&i);
                        let shown = if expanded { details.len() } else { details.len().min(LINE_PREVIEW_MAX) };
                        for (name, amount) in details.iter().take(shown) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).size(12.0).color(TEXT));
                                let color = if *amount < 0.0 { GREEN } else { RED };
                                ui.label(RichText::new(amount_text(*amount)).size(12.0).color(color));
                            });
                        }
                        if details.len() > LINE_PREVIEW_MAX {
                            let label = if expanded {
                                "收起 ▴".to_string()
                            } else {
                                format!("展开全部 {} 行 ▾", details.len())
                            };
                            if ui.add(egui::Button::new(RichText::new(label).size(11.5).color(MUTED)).frame(false)).clicked() {
                                if expanded {
                                    xm.log_expanded.remove(&i);
                                } else {
                                    xm.log_expanded.insert(i);
                                }
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
}

/// 减 / 加 合计：正 = 减额，负 = 加额。
fn totals_text(delta: &OffsetMap) -> String {
    let deduct: f64 = delta.values().filter(|v| **v > 0.0).sum();
    let add: f64 = delta.values().filter(|v| **v < 0.0).map(|v| -v).sum();
    let mut parts = Vec::new();
    if deduct > 0.0 {
        parts.push(format!("减 {}", xm_num(deduct.round())));
    }
    if add > 0.0 {
        parts.push(format!("加 {}", xm_num(add.round())));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" / ")
    }
}

/// 每一行：`三中三 01-02-03` 与金额。键是 `地区|玩法|组合`。
fn detail_rows(delta: &OffsetMap) -> Vec<(String, f64)> {
    delta
        .iter()
        .map(|(key, amount)| {
            let mut parts = key.splitn(3, '|');
            let _region = parts.next();
            let play = parts.next().unwrap_or("");
            let combo = parts.next().unwrap_or("");
            let label = if play == "二全中" { "二中二" } else { play };
            let name = if combo.is_empty() {
                key.clone()
            } else {
                format!("{label} {combo}")
            };
            (name, *amount)
        })
        .collect()
}

/// 参考项目 `amountText`：减额写成 `-5`，加额写成 `+5`。
fn amount_text(amount: f64) -> String {
    let n = (amount * 100.0).round() / 100.0;
    if n > 0.0 {
        format!("-{}", xm_num(n))
    } else {
        format!("+{}", xm_num(-n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xiaoma_offsets::{row_key, LogEntry, Store};
    use egui_kittest::{kittest::Queryable, Harness};

    fn log_state() -> XmDeduct {
        let mut xm = XmDeduct::default();
        xm.store = Store {
            offsets: OffsetMap::new(),
            log: vec![LogEntry {
                at: "2026-11-24 10:00:00".into(),
                kind: "减额".into(),
                desc: "澳门 整列减额".into(),
                undone: false,
                delta: OffsetMap::from([(row_key(0, "三中三", "01-02-03"), 5.0)]),
            }],
        };
        xm.log_open = true;
        xm
    }

    #[test]
    fn log_window_works_with_other_modals_open() {
        let mut state = log_state();
        state.deduct_open = true;
        state.dup_open = true;
        let mut harness = Harness::builder().with_accessibility_check(false).build_ui_state(
            |ui, xm: &mut XmDeduct| {
                super::super::install_fonts(ui.ctx());
                let ctx = ui.ctx().clone();
                if xm.deduct_open {
                    let close = super::super::xm_modal(&ctx, "xm_deduct", "减额", "t", 860.0, None, 0.9, |ui| {
                        deduct_body(xm, &[], ui)
                    });
                    if close {
                        xm.deduct_open = false;
                    }
                }
                if xm.dup_open {
                    let draws = [HashSet::new(), HashSet::new(), HashSet::new()];
                    let close = super::super::xm_modal(&ctx, "xm_dup", "查重", "t", 820.0, None, 0.9, |ui| {
                        dup_body(xm, &draws, ui)
                    });
                    if close {
                        xm.dup_open = false;
                    }
                }
                if xm.log_open {
                    let close = super::super::xm_modal(&ctx, "xm_log", "减额记录", "t", 680.0, None, 0.9, |ui| {
                        log_body(xm, ui)
                    });
                    if close {
                        xm.log_open = false;
                    }
                }
            },
            state,
        );
        harness.run();
        harness.get_by_label("全选可撤回").click();
        harness.run();
        assert_eq!(harness.state().log_checked.len(), 1, "other modals block the log window");
        assert!(harness.state().log_open, "log window closed by a backdrop click");
    }

    #[test]
    fn log_window_checkbox_toggles() {
        let mut harness = Harness::builder().with_accessibility_check(false).build_ui_state(
            |ui, xm: &mut XmDeduct| {
                super::super::install_fonts(ui.ctx());
                if xm.log_open {
                    let ctx = ui.ctx().clone();
                    let close = super::super::xm_modal(&ctx, "xm_log", "减额记录", "t", 680.0, None, 0.9, |ui| {
                        log_body(xm, ui)
                    });
                    if close {
                        xm.log_open = false;
                    }
                }
            },
            log_state(),
        );
        harness.run();
        harness
            .get_all_by_role(egui::accesskit::Role::CheckBox)
            .next()
            .expect("no checkbox found")
            .click();
        harness.run();
        assert_eq!(harness.state().log_checked.len(), 1, "勾选框没有生效");
    }

    #[test]
    fn log_window_select_all_button_responds() {
        let mut harness = Harness::builder().with_accessibility_check(false).build_ui_state(
            |ui, xm: &mut XmDeduct| {
                super::super::install_fonts(ui.ctx());
                if xm.log_open {
                    let close = super::super::xm_modal(
                        ui.ctx(),
                        "xm_log",
                        "减额记录",
                        "test",
                        680.0,
                        None,
                        0.9,
                        |ui| log_body(xm, ui),
                    );
                    if close {
                        xm.log_open = false;
                    }
                }
            },
            log_state(),
        );
        harness.run();
        harness.get_by_label("全选可撤回").click();
        harness.run();
        assert_eq!(harness.state().log_checked.len(), 1, "全选可撤回 没有生效");
    }
}

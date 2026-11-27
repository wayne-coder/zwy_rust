//! 小掌柜界面：
//! - 主页：顶部按钮分组（注单 / 对奖 / 设置），左侧成员竖排 Tab，中间常驻注单消息列表，右侧注单详情面板；
//! - 选定会员页：联系人 / 群聊两个 Tab，群聊树形展开成员，全部可勾选；
//! - 导入注单：弹窗设置日期与时段，导入后在所选时段内实时追加新消息。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveTime, TimeZone, Timelike};
use jiff::civil::Date as JDate;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke,
    Vec2,
};

use crate::bill_image;
use crate::convert::{self, BetItem, ConvertOutput};
use crate::draw;
use crate::edits::{self, EditKey};
use crate::groups;
use crate::keywords::{self, Field, KeywordSettings, Section};
use crate::loader::{self, Msg};
use crate::messages::{self, ImportMsg, ImportRequest, MonitorMsg};
use crate::model::{
    AppData, Avatar, Group, ImportResult, MemberGroup, MsgRow, Person, QuoteRef,
};
use crate::doubt;
use crate::reconcile;
use crate::replacements::{self, Rule};
use crate::settle;
use crate::times;

mod xiaoma_modals;

const GREEN: Color32 = Color32::from_rgb(0x0A, 0x8F, 0x52);
const GREEN_DEEP: Color32 = Color32::from_rgb(0x08, 0x73, 0x43);
const GREEN_TINT: Color32 = Color32::from_rgb(0xE7, 0xF5, 0xEC);
const BG: Color32 = Color32::from_rgb(0xF1, 0xF3, 0xF4);
const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const RAIL_BG: Color32 = Color32::from_rgb(0xF7, 0xF8, 0xFA);
const TEXT: Color32 = Color32::from_rgb(0x1B, 0x20, 0x27);
const SUB: Color32 = Color32::from_rgb(0x7C, 0x85, 0x8F);
const HOVER: Color32 = Color32::from_rgb(0xEC, 0xEF, 0xF1);
const BORDER: Color32 = Color32::from_rgb(0xE1, 0xE5, 0xE9);
// 顶部工具栏：深色底，按钮比底色亮并带细边，禁用按钮无填充。文字统一白色。
const TOOLBAR_BG: Color32 = Color32::from_rgb(0x2B, 0x31, 0x38);
const TOOLBAR_BTN: Color32 = Color32::from_rgb(0x43, 0x4B, 0x54);
const TOOLBAR_BTN_HOVER: Color32 = Color32::from_rgb(0x52, 0x5B, 0x65);
const TOOLBAR_BTN_ACTIVE: Color32 = Color32::from_rgb(0x3A, 0x41, 0x49);
const TOOLBAR_BTN_EDGE: Color32 = Color32::from_rgb(0x6B, 0x74, 0x7D);
const TOOLBAR_BTN_EDGE_HOVER: Color32 = Color32::from_rgb(0x8A, 0x93, 0x9C);
const TOOLBAR_OFF_EDGE: Color32 = Color32::from_rgb(0x4F, 0x58, 0x60);
const TOOLBAR_OFF_TEXT: Color32 = Color32::from_rgb(0x8A, 0x93, 0x9C);
const TOOLBAR_SUB: Color32 = Color32::from_rgb(0xD3, 0xD8, 0xDD);
const HEAD_BG: Color32 = Color32::from_rgb(0xF7, 0xF8, 0xFA);
const STRIPE: Color32 = Color32::from_rgb(0xFA, 0xFB, 0xFC);
const GOLD_FG: Color32 = Color32::from_rgb(0xA8, 0x6A, 0x00);
const GOLD_BG: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xD0);
// 信息码标签：对齐参考项目「当前记录」里的红底小胶囊。
const INFO_CHIP_BG: Color32 = Color32::from_rgb(0xFE, 0xF2, 0xF2);
const INFO_CHIP_BORDER: Color32 = Color32::from_rgb(0xFE, 0xCA, 0xCA);
const INFO_CHIP_TEXT: Color32 = Color32::from_rgb(0xB9, 0x1C, 0x1C);
const INFO_CHIP_AMOUNT: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const RED: Color32 = Color32::from_rgb(0xC0, 0x39, 0x2B);
const ORANGE: Color32 = Color32::from_rgb(0xC8, 0x6A, 0x00);
const ORANGE_BG: Color32 = Color32::from_rgb(0xFF, 0xF6, 0xEC);
const RED_BG: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xF0);
const RED_HOVER: Color32 = Color32::from_rgb(0xFF, 0xE4, 0xE1);
const BLUE_FG: Color32 = Color32::from_rgb(0x2C, 0x6C, 0xB8);
const BLUE_BG: Color32 = Color32::from_rgb(0xE8, 0xF1, 0xFB);
const BLUE_HOVER: Color32 = Color32::from_rgb(0xD8, 0xE6, 0xF8);

// 按钮悬浮态：浅绿底 + 淡绿描边；按下时再深一档。
const BTN_HOVER_BG: Color32 = Color32::from_rgb(0xF2, 0xF8, 0xF5);
const BTN_ACTIVE_BG: Color32 = Color32::from_rgb(0xE3, 0xF1, 0xE9);
const BTN_HOVER_BORDER: Color32 = Color32::from_rgb(0x9D, 0xD6, 0xB8);
const GREEN_HOVER: Color32 = Color32::from_rgb(0x0B, 0x7D, 0x48);
const GREEN_ACTIVE: Color32 = Color32::from_rgb(0x07, 0x62, 0x38);
const BLUE_SOLID_HOVER: Color32 = Color32::from_rgb(0x1D, 0x4E, 0xD8);
const BLUE_SOLID_ACTIVE: Color32 = Color32::from_rgb(0x1E, 0x40, 0xAF);
const RED_SOLID: Color32 = Color32::from_rgb(0xD9, 0x3A, 0x3A);
const RED_SOLID_HOVER: Color32 = Color32::from_rgb(0xC5, 0x30, 0x30);
const RED_SOLID_ACTIVE: Color32 = Color32::from_rgb(0x9B, 0x2C, 0x2C);

// 出账单配色，对齐 zwy_wails_v1 的 SettlementBillView。
const BILL_LINE: Color32 = Color32::from_rgb(0xCB, 0xD5, 0xE1);
const BILL_HEAD: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);
const BILL_TEXT: Color32 = Color32::from_rgb(0x33, 0x41, 0x55);
const MUTED: Color32 = Color32::from_rgb(0x61, 0x78, 0x92);
const MONEY_BLUE: Color32 = Color32::from_rgb(0x25, 0x63, 0xEB);
const MONEY_GREEN: Color32 = Color32::from_rgb(0x04, 0x78, 0x57);
const MONEY_RED: Color32 = Color32::from_rgb(0xB9, 0x1C, 0x1C);
const SPECIAL_RED: Color32 = Color32::from_rgb(0xBB, 0x00, 0x00);
const CELL_NO: Color32 = Color32::from_rgb(0x94, 0xA3, 0xB8);
const SUBTOTAL_BG: Color32 = Color32::from_rgb(0xF8, 0xFA, 0xFC);
// 出账单「金额」列里香港 / 老澳门拆分的小字徽章（对齐 SettlementBillView 的
// .hk-tag / .old-tag：白底、浅色边框、红 / 琥珀文字，小字部分用灰色）。
const TAG_HK: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const TAG_HK_BORDER: Color32 = Color32::from_rgb(0xFE, 0xE2, 0xE2);
const TAG_OLD: Color32 = Color32::from_rgb(0xD9, 0x77, 0x06);
const TAG_OLD_BORDER: Color32 = Color32::from_rgb(0xFE, 0xF3, 0xC7);

// 风险页对齐 zwy_wails_v1 右列面板用的那一套色值。
const REF_HEAD_BG: Color32 = Color32::from_rgb(0xF5, 0xF8, 0xFC);
const REF_HEAD_TINT: Color32 = Color32::from_rgb(0xF4, 0xF6, 0xF8);
const REF_LINE: Color32 = Color32::from_rgb(0xE9, 0xED, 0xF2);
const REF_NAVY: Color32 = Color32::from_rgb(0x13, 0x20, 0x33);
const REF_ZERO: Color32 = Color32::from_rgb(0xB6, 0xC2, 0xD1);
const REF_ID: Color32 = Color32::from_rgb(0x8B, 0x9B, 0xB0);
const REF_HIT_BG: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xF2);
const REF_RISK_RED: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const REF_RISK_GREEN: Color32 = Color32::from_rgb(0x16, 0xA3, 0x4A);
const BAND_STAKE_BG: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);
const BAND_STAKE_BORDER: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
const BAND_AMOUNT_BG: Color32 = Color32::from_rgb(0xEC, 0xFD, 0xF5);
const BAND_AMOUNT_BORDER: Color32 = Color32::from_rgb(0xD1, 0xFA, 0xE5);
const TOTALS_BG: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF0);
const MONITOR_INTERVAL_MS: u64 = 3000;

/// 单条录入的默认投注类型下拉项。
const SINGLE_PLAY_OPTIONS: &[&str] = &[
    "自动识别",
    "三中三",
    "三中二",
    "二中二",
    "二全中",
    "特碰",
    "特码",
    "平特",
    "平码",
    "二连肖",
    "三连肖",
    "四连肖",
    "五连肖",
    "二连尾",
    "三连尾",
    "四连尾",
    "五不中",
    "六不中",
    "七不中",
    "八不中",
    "九不中",
    "十不中",
    "三中一",
    "四中一",
    "五中一",
    "六肖中一",
];
const SINGLE_REGION_OPTIONS: &[&str] = &["澳门", "香港", "老澳门"];

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // 全软件统一等线体，并且正文也直接用加粗字重（Dengb）——需求是「所有字体加粗」。
    let candidates = [
        r"C:\Windows\Fonts\Dengb.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\Deng.ttf",
    ];
    let mut regular: Option<std::sync::Arc<egui::FontData>> = None;
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            regular = Some(std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            break;
        }
    }
    if let Some(data) = &regular {
        fonts.font_data.insert("cjk".into(), data.clone());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".into());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("cjk".into());
    }
    // 出账单 / 总账里的数字和标题需要更粗的字形：优先找系统里的粗体，
    // 没有就退回正文字体（至少字号加大会更醒目）。
    let bold_candidates = [
        r"C:\Windows\Fonts\Dengb.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
    ];
    let bold = bold_candidates
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .map(|b| std::sync::Arc::new(egui::FontData::from_owned(b)))
        .or_else(|| regular.clone());
    let bold_family = egui::FontFamily::Name("cjk-bold".into());
    match bold {
        Some(data) => {
            fonts.font_data.insert("cjk_bold".into(), data);
            fonts
                .families
                .insert(bold_family, vec!["cjk_bold".into()]);
        }
        None => {
            let fallback = fonts
                .families
                .get(&egui::FontFamily::Proportional)
                .cloned()
                .unwrap_or_default();
            fonts.families.insert(bold_family, fallback);
        }
    }
    ctx.set_fonts(fonts);
}

fn bold_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("cjk-bold".into()))
}

pub fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = CARD;
        style.visuals.extreme_bg_color = CARD;
        style.visuals.faint_bg_color = Color32::from_rgb(0xFA, 0xFB, 0xFC);
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = GREEN_TINT;
        style.visuals.selection.stroke = Stroke::new(1.0, GREEN);
        style.visuals.hyperlink_color = GREEN_DEEP;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.bg_fill = CARD;
        style.visuals.widgets.inactive.weak_bg_fill = CARD;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, SUB);
        style.visuals.widgets.hovered.bg_fill = BTN_HOVER_BG;
        style.visuals.widgets.hovered.weak_bg_fill = BTN_HOVER_BG;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BTN_HOVER_BORDER);
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, GREEN_DEEP);
        style.visuals.widgets.hovered.expansion = 1.0;
        style.visuals.widgets.active.bg_fill = BTN_ACTIVE_BG;
        style.visuals.widgets.active.weak_bg_fill = BTN_ACTIVE_BG;
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, GREEN);
        style.visuals.widgets.active.expansion = 0.5;
        style.visuals.widgets.open.bg_fill = HOVER;
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        style.animation_time = 0.12;
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        // 弹窗标题栏：左右留 6px，上下只留 4px，整体压低。
        style.spacing.window_margin = Margin::symmetric(6, 4);
        style.spacing.menu_margin = Margin::same(3);
        style.text_styles = [
            (egui::TextStyle::Heading, FontId::proportional(16.0)),
            (egui::TextStyle::Body, FontId::proportional(15.5)),
            (egui::TextStyle::Button, FontId::proportional(15.0)),
            (egui::TextStyle::Small, FontId::proportional(12.5)),
            (egui::TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
    });
    // 界面整体放大一档，表格 / 按钮 / 正文一起变大，而不是只放大默认字号。
    ctx.set_zoom_factor(1.1);
}

enum State {
    Loading,
    Ready(Box<AppData>),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Select,
    /// 总账（出账单）：按会员汇总当前注单消息的投注金额。
    Ledger,
    /// 风险：复刻参考项目右列面板（小码 / 特码 / 连肖 / 平特 / 平码）。
    Risk,
}

/// 「注单消息」面板的三个页签。
#[derive(Clone, Copy, PartialEq, Eq)]
enum OrderTab {
    /// 正常注单（已撤回的不在这里）。
    Active,
    /// 引用类型消息。
    References,
    /// 已撤回的原消息（本地持久化记录）。
    Revoked,
}

/// 小码的减额相关状态：减额表（`store`）与查重 / 减额 / 减额记录三个弹窗。
#[derive(Default)]
struct XmDeduct {
    store: crate::xiaoma_offsets::Store,
    dup_open: bool,
    dup_region: usize,
    dup_entries: Vec<crate::xiaoma_dup::DupEntry>,
    /// 每个地区的重复组（录入 ≥ 2 条）
    dup_groups: [Vec<crate::xiaoma_dup::DupGroup>; 3],
    /// (地区, 组下标) 是否展开
    dup_expanded: HashSet<(usize, usize)>,
    /// 勾选的录入，下标是 `dup_entries` 的
    deduct_open: bool,
    deduct_region: usize,
    deduct_threshold: String,
    /// 打开「减额」弹窗时把焦点放到过滤条件输入框
    deduct_focus_pending: bool,
    deduct_hint: Option<String>,
    /// 合并组合弹窗打开的是哪一个类型栏（下标见 xiaoma_modals::XM_TYPES）
    merge_open: Option<usize>,
    merge_result: Option<crate::xiaoma_merge::MergeOutput>,
    merge_hint: Option<String>,
    log_open: bool,
    /// 勾选的减额记录，下标是 `store.log` 的
    log_checked: HashSet<usize>,
    /// 展开了全部明细的减额记录，下标是 `store.log` 的
    log_expanded: HashSet<usize>,
    log_hint: Option<String>,
}

/// 「风险」页面的五个页签。
#[derive(Clone, Copy, PartialEq, Eq)]
enum RiskTab {
    XiaoMa,
    Special,
    LianXiao,
    FlatSpecial,
    FlatNumber,
}

/// 特码「吃码上报」的两种口径。
#[derive(Clone, Copy, PartialEq, Eq)]
enum CodeReportMode {
    /// 风险下限：把低于阈值的号吃回阈值之上。
    Risk,
    /// 下注额上限：把超过上限的号吃回上限以内。
    Amount,
}

impl RiskTab {
    fn label(self) -> &'static str {
        match self {
            RiskTab::XiaoMa => "小码",
            RiskTab::Special => "特码",
            RiskTab::LianXiao => "连肖",
            RiskTab::FlatSpecial => "平特",
            RiskTab::FlatNumber => "平码",
        }
    }

    fn panel_list(self) -> Option<crate::risk::PanelList> {
        match self {
            RiskTab::XiaoMa => None,
            RiskTab::Special => Some(crate::risk::PanelList::Special),
            RiskTab::LianXiao => Some(crate::risk::PanelList::LianXiao),
            RiskTab::FlatSpecial => Some(crate::risk::PanelList::FlatSpecial),
            RiskTab::FlatNumber => Some(crate::risk::PanelList::FlatNumber),
        }
    }
}

/// 小码表可点表头排序的三列。
#[derive(Clone, Copy, PartialEq, Eq)]
enum RiskSort {
    Stake,
    Risk,
    Profit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Contacts,
    Groups,
}

/// 注单状态：转换结果为空 = 失败；否则有存疑行 = 存疑；全部齐全 = 成功。
#[derive(Clone, Copy, PartialEq, Eq)]
enum OrderStatus {
    Success,
    Doubtful,
    Failed,
    /// 踩中信息码：状态显示「信息」，整行黄色。
    Info,
}

fn order_status(conv: Option<&ConvertOutput>) -> OrderStatus {
    match conv {
        Some(c) if c.bets.is_empty() || c.invalid => OrderStatus::Failed,
        Some(c) if c.doubtful => OrderStatus::Doubtful,
        Some(_) => OrderStatus::Success,
        None => OrderStatus::Failed,
    }
}

/// 「原文」文本框右键菜单的动作。
#[derive(Clone, Copy)]
enum SourceMenuAction {
    Copy,
    Paste,
    Cut,
    SelectAll,
    InsertDrag,
    BatchNewline,
    BatchReplace,
}

enum OrdersState {
    Idle,
    Loading(String),
    Ready,
    Failed(String),
}

pub struct ViewerApp {
    state: State,
    status: String,
    rx: Option<Receiver<Msg>>,

    page: Page,
    tab: Tab,
    search: String,
    textures: HashMap<String, Option<egui::TextureHandle>>,
    expanded: HashSet<String>,

    // 会员选择
    selected_contacts: HashSet<String>,
    selected_groups: HashSet<String>,
    /// (群 username, 成员 username)
    selected_members: HashSet<(String, String)>,

    // 会员分组（本地持久化）
    groups: Vec<MemberGroup>,
    group_dialog_open: bool,
    group_name: String,
    group_error: Option<String>,
    /// 正在改名的分组原名
    group_rename: Option<String>,
    group_rename_input: String,
    group_rename_error: Option<String>,
    /// 正在管理成员的分组名
    group_manage: Option<String>,
    /// 待确认删除的分组下标
    group_delete: Option<usize>,

    // 导入弹窗
    import_date: JDate,
    import_start: NaiveTime,
    import_end: NaiveTime,
    dialog_open: bool,
    dialog_error: Option<String>,

    // 注单列表与实时监控
    orders: Vec<MsgRow>,
    /// (sort, 会话 username, 昵称, 内容)：同一成员在不同群的消息互不去重
    order_keys: HashSet<(i64, String, String, String)>,
    /// 注单列表按哪个会员分组过滤；None = 全部
    order_filter: Option<String>,
    orders_state: OrdersState,
    /// 注单消息面板当前页签。
    order_tab: OrderTab,
    /// 已撤回消息（持久化）：重新导入时据此过滤原消息。
    revoked: HashMap<crate::revoked::RevokeKey, crate::revoked::RevokedRow>,
    last_update: String,
    import_rx: Option<Receiver<ImportMsg>>,

    // 实时监控（选定会员后常驻）
    selection_dirty: bool,
    monitor_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    monitor_rx: Option<Receiver<MonitorMsg>>,
    /// 当前监控绑定的导入时段；时段改了要重启监控。
    monitor_range: Option<(i64, i64)>,

    // 清空注单弹窗
    clear_open: bool,
    clear_groups: HashSet<String>,
    clear_members: HashSet<(String, String)>,

    // 主页成员条与详情面板
    /// 左侧竖排选中的成员（只筛左侧注单列表，不再占用右侧详情面板）
    member_focus: Option<MemberKey>,
    /// 点击的注单（详情面板展示该条消息）
    selected_order_key: Option<(i64, String, String, String)>,
    selected_order_row: Option<MsgRow>,
    /// 用「上一条 / 下一条」切换后，把选中行滚动到可见区域。
    scroll_to_order: bool,
    /// 当前注单点「提交」后的转换结果
    converted: Option<ConvertOutput>,
    /// 每条注单自动转换的结果（新消息入列时生成，选中时直接展示）
    auto_converted: HashMap<EditKey, ConvertOutput>,
    /// 保存过的注单原文编辑：(sort, 会话, 发送者) → 文本
    order_edits: HashMap<EditKey, String>,
    /// 保存过的注单时间编辑：(sort, 会话, 发送者) → 时间
    order_times: HashMap<EditKey, String>,
    /// 已手动「取消存疑」的注单
    doubt_cleared: HashSet<EditKey>,

    // 原文右键「批量替换」
    batch_replace_open: bool,
    batch_replace_find: String,
    batch_replace_to: String,
    /// 批量替换针对的注单（弹窗开着时切换注单就不要误改另一条）
    batch_replace_target: Option<EditKey>,

    // 原文「识别图片」：剪贴板截图 → 系统 OCR
    /// 识别线程的结果（Ok = 文本，Err = 错误说明）
    ocr_rx: Option<Receiver<Result<String, String>>>,
    /// 发起识别时选中的注单；结果回来前已切换就丢弃，避免写进另一条原文
    ocr_target: Option<(i64, String, String, String)>,
    /// 识别状态：(提示文字, 是否错误)
    ocr_msg: Option<(String, bool)>,

    // 开奖
    draws: draw::Draws,
    draw_open: bool,
    /// 出账单展现形式：false = 列表，true = 分类
    ledger_category: bool,
    /// 「导出图片」的后台结果（回调线程写入，界面线程读取）
    ledger_msg: Option<String>,

    // 对账
    reconcile_open: bool,
    reconcile_pasted: String,
    reconcile_only_diff: bool,
    reconcile_msg: Option<String>,
    /// 当前出账单每个会员块的屏幕矩形（导出图片用）
    /// 导出图片：可选投注人 / 已选投注人 / 保存目录
    bill_export_open: bool,
    bill_export_candidates: Vec<String>,
    bill_export_selected: BTreeSet<String>,
    bill_export_dir: String,
    /// 澳门 / 香港 / 老澳门，各 7 个输入框（前 6 平码 + 特码）
    draw_inputs: [[String; 7]; 3],
    /// 每个地区的「整期粘贴」输入框与解析错误
    draw_paste: [String; 3],
    draw_paste_error: [Option<String>; 3],
    draw_error: Option<String>,

    // 引用列表（独立窗口）：引用类型消息
    references: Vec<MsgRow>,
    reference_keys: HashSet<(i64, String, String, String)>,
    reference_open: bool,
    /// 有新引用进来时把窗口弹到最前（每帧消费一次）
    /// 新引用到达后剩余几帧强制把窗口恢复 / 置前（新建视口第一帧可能收不到命令）。
    reference_popup: u8,
    /// 有新数据时把列表滚回顶部
    reference_scroll_top: bool,
    reference_filter: crate::reference::Filter,
    reference_ignore_edit: String,
    reference_search: String,
    reference_msg: Option<String>,
    /// 引用窗口提示是否为错误（红色显示）
    reference_msg_error: bool,
    /// 注单列表里找不到被引用消息时，从微信库补读的后台结果。
    quote_fetch_rx: Option<Receiver<Result<Vec<MsgRow>, String>>>,
    /// 发起补读的那条引用消息（回来后再按正文 / 发送人匹配）。
    quote_fetch_row: Option<MsgRow>,
    /// 这次补读是为了删除被引用消息（而不是定位）。
    quote_fetch_delete: bool,
    /// 补读到、但不在当前注单列表里的被引用原文（弹窗展示，不参与记账）。
    quote_original: Option<MsgRow>,
    /// 详情面板里正在编辑的原文
    edit_text: String,
    /// 详情面板里正在编辑的注单时间
    edit_time: String,
    /// 原文右键菜单打开期间保留的选区。
    ///
    /// egui 在文本框失去焦点（例如菜单打开）时会把选区折叠成一个光标，
    /// 菜单里的复制 / 剪切 / 插入拖就失效了。这里记下右键前的选区，
    /// 菜单存续期间每帧恢复。
    source_sel_snapshot: Option<(egui::Id, egui::text::CCursorRange)>,
    /// 右键菜单点中的动作。用持久状态而不是局部变量，避免多 pass 时丢事件。
    pending_source_action:
        Option<(egui::Id, SourceMenuAction, Option<egui::text::CCursorRange>)>,

    // 关键词设置
    keywords: KeywordSettings,
    keywords_open: bool,
    keyword_section: Section,
    /// 每个关键词组各自的输入框内容
    keyword_inputs: HashMap<Field, String>,
    /// 生肖生僻字映射的新条目
    keyword_variant: String,
    keyword_correct: String,

    // 替换词
    replacements: Vec<Rule>,
    replacements_open: bool,
    replacement_from: String,
    replacement_to: String,

    // 单条录入
    single_open: bool,
    single_raw: String,
    /// 默认投注类型（原文没写玩法时使用）；「自动识别」表示不加默认玩法。
    single_play: String,
    single_region: String,
    single_preview: Option<ConvertOutput>,
    /// 单条录入选中的会员（None = 不指定）。
    single_member: Option<MemberKey>,
    single_members: Vec<SingleMemberChoice>,

    // 赔率与回水（默认 / 分组 / 会员覆盖）
    odds: crate::odds::OddsConfig,
    odds_open: bool,
    odds_draft: crate::odds::OddsConfig,
    odds_scope: OddsScope,
    odds_region: usize,
    odds_new_member: String,

    // 风险页面（复刻参考项目右列面板）
    risk_tab: RiskTab,
    /// 小码表的地区筛选：None = 全部
    risk_xm_region: Option<usize>,
    risk_xm_sort: RiskSort,
    /// true = 降序（默认风险降序）
    risk_xm_desc: bool,
    risk_xm_page: usize,
    risk_xm_page_size: usize,
    risk_xm_jump: String,
    /// 小码表的组合搜索词（参考项目 xiaoMaSearch）
    risk_xm_search: String,
    /// 「概率」明细弹窗
    risk_prob_open: bool,
    /// 小码减额：减额表与查重 / 减额 / 减额记录弹窗
    xm: XmDeduct,
    /// 连肖子页签规格（2/3/4/5），默认二连肖
    risk_lian_size: usize,
    /// 连肖每一栏各自的页码（1 起）
    risk_lian_pages: [usize; 3],
    /// 特码每一栏各自的风险排序方向：false = 升序（默认，赔得最狠的在前）
    risk_panel_desc: [bool; 3],
    /// 三栏折叠状态：老澳门默认收起
    risk_collapsed: [bool; 3],

    // 特码「吃码上报」
    code_report_open: bool,
    code_report_region: usize,
    code_report_mode: CodeReportMode,
    code_report_threshold: String,
    code_report_limit: String,
    /// 当前地区的号码 → 吃码金额输入（字符串，提交时再解析 / 夹取）
    code_report_reduce: HashMap<u32, String>,
    /// 复制格式的反馈文字
    code_report_msg: Option<String>,

    // 信息码
    info_codes: Vec<crate::info_code::InfoCodeEntry>,
    info_code_open: bool,
    info_code_draft: String,
    info_code_default_region: usize,
    info_code_default_play: String,
    info_code_msg: Option<String>,
    info_code_confirm_clear: bool,
    /// 信息码弹窗右表只看命中项（默认勾选）。
    info_code_only_hit: bool,
    /// 信息码录入框右键菜单打开期间保留的选区。
    info_sel_snapshot: Option<(egui::Id, egui::text::CCursorRange)>,
    /// 信息码录入框右键菜单点中的动作。
    pending_info_action:
        Option<(egui::Id, SourceMenuAction, Option<egui::text::CCursorRange>)>,
    info_code_lines: Vec<crate::info_code::InfoCodeLine>,
    info_code_keys: HashSet<crate::info_code::CodeKey>,
    info_hit_cache: HashMap<EditKey, (u64, Option<crate::info_code::OrderInfoHits>)>,
    info_rev: u64,

    // 在线鉴权（与 zwy_wails_v1 共用服务器）
    auth: crate::auth::Auth,
    login_user: String,
    login_pass: String,
    login_remember: bool,
    /// (是否错误, 文案)
    login_msg: Option<(bool, String)>,
    login_rx: Option<Receiver<crate::auth::LoginOutcome>>,
    auth_open: bool,
    pwd_old: String,
    pwd_new: String,
    pwd_confirm: String,
    pwd_msg: Option<(bool, String)>,
    pwd_rx: Option<Receiver<crate::auth::ChangeOutcome>>,
    auth_refresh_rx: Option<Receiver<()>>,
    /// 当前窗口标题：版本号 + 账号 + 剩余时间；变化时才下发，避免每帧重设。
    window_title: String,
    /// 上一帧是否已登录：用来在被心跳踢下线时给出「登录已失效」的提示。
    was_authenticated: bool,
    /// 登录页微信状态：上次检测 Weixin.exe 的时间（进程快照要节流）。
    wx_proc_checked: Option<Instant>,
    /// 最近一次检测是否发现 Weixin.exe 在运行。
    wx_proc_running: bool,
    /// 微信从「未运行」变成「运行」后，隔几秒自动重检一次（等它自动登录完成）。
    wx_auto_check_at: Option<Instant>,
}

/// 赔率设置对话框当前编辑的对象。
#[derive(Clone, PartialEq, Eq)]
enum OddsScope {
    Default,
    Group(String),
    Member(String),
}

/// 总账里的一条注单（一行）。
/// 分类视图里一个类别在一行里的金额 / 中奖切片。
#[derive(Default)]
struct BillSlice {
    amount: f64,
    /// 该类别里属于香港 / 老澳门的金额
    hk: f64,
    old: f64,
    win: f64,
    win_stake: f64,
    rebate: f64,
    breakdown: Vec<(String, f64, f64)>,
}

/// 出账单的一行（一批注单）。
struct LedgerBillRow {
    no: usize,
    /// 来源注单在 `self.orders` 里的下标（序号格点击跳转用）。
    idx: usize,
    amount: f64,
    /// 金额里属于香港 / 老澳门的那一份（显示在金额后面的括号里）
    hk: f64,
    old: f64,
    win: f64,
    rebate: f64,
    /// 这一行里特码的中奖注额（列表视图「小计」行用）。
    special_win_stake: f64,
    breakdown: Vec<(String, f64, f64)>,
    /// 按出账类别拆开的切片（分类视图用）。
    cats: BTreeMap<&'static str, BillSlice>,
}

/// 出账单里的一个会员块。
struct LedgerBillBlock {
    name: String,
    rows: Vec<LedgerBillRow>,
    amount: f64,
    win: f64,
    rebate: f64,
    win_stakes: BTreeMap<String, f64>,
    win_amounts: BTreeMap<String, f64>,
}

/// 单条录入里可选的会员。
#[derive(Clone)]
struct SingleMemberChoice {
    key: MemberKey,
    /// 下拉框显示名（群成员带群名，便于区分）。
    label: String,
    /// 注单列表昵称列显示名。
    nick: String,
}

/// 成员维度：联系人会话，或某个群聊里的成员。
#[derive(Clone, PartialEq, Eq, Hash)]
enum MemberKey {
    /// 联系人 username
    Contact(String),
    /// (群 username, 成员 username)
    Member(String, String),
}

impl MemberKey {
    fn matches_row(&self, row: &MsgRow) -> bool {
        match self {
            MemberKey::Contact(c) => row.chat == *c,
            MemberKey::Member(chat, m) => row.chat == *chat && row.sender == *m,
        }
    }
}

struct ClearGroupNode {
    chat: String,
    name: String,
    members: Vec<(String, String)>,
}

/// 导入 / 实时监控的数据来源。
struct ImportSource {
    /// true = 收集全部分组下的全部成员；false = 没有分组，沿用会员选择
    from_groups: bool,
    contacts: Vec<String>,
    /// 群 username → 选中的成员（非空；空群不会出现）
    groups: HashMap<String, Vec<String>>,
}

impl ViewerApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        loader::start(ctx.clone(), tx);
        let draws = draw::load();
        let draw_inputs = draw_inputs_from(&draws);
        let reference_filter = crate::reference::load();
        let auth = crate::auth::Auth::new();
        let (login_user, login_pass, login_remember) = auth.remembered();
        let mut app = Self {
            state: State::Loading,
            status: "正在启动…".to_string(),
            rx: Some(rx),
            page: Page::Home,
            tab: Tab::Contacts,
            search: String::new(),
            textures: HashMap::new(),
            expanded: HashSet::new(),
            selected_contacts: HashSet::new(),
            selected_groups: HashSet::new(),
            selected_members: HashSet::new(),
            groups: groups::load(),
            group_dialog_open: false,
            group_name: String::new(),
            group_error: None,
            group_rename: None,
            group_rename_input: String::new(),
            group_rename_error: None,
            group_manage: None,
            group_delete: None,
            import_date: jiff::Zoned::now().date(),
            import_start: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            import_end: NaiveTime::from_hms_opt(21, 30, 0).unwrap(),
            dialog_open: false,
            dialog_error: None,
            orders: Vec::new(),
            order_keys: HashSet::new(),
            order_filter: None,
            orders_state: OrdersState::Idle,
            order_tab: OrderTab::Active,
            revoked: crate::revoked::load(),
            last_update: String::new(),
            import_rx: None,
            selection_dirty: true,
            monitor_stop: None,
            monitor_rx: None,
            monitor_range: None,
            clear_open: false,
            clear_groups: HashSet::new(),
            clear_members: HashSet::new(),
            member_focus: None,
            selected_order_key: None,
            selected_order_row: None,
            scroll_to_order: false,
            converted: None,
            auto_converted: HashMap::new(),
            order_edits: edits::load(),
            order_times: times::load(),
            doubt_cleared: doubt::load(),
            batch_replace_open: false,
            batch_replace_find: String::new(),
            batch_replace_to: String::new(),
            batch_replace_target: None,
            draws,
            draw_open: false,
            ledger_category: false,
            ledger_msg: None,
            reconcile_open: false,
            reconcile_pasted: String::new(),
            reconcile_only_diff: true,
            reconcile_msg: None,
            bill_export_open: false,
            bill_export_candidates: Vec::new(),
            bill_export_selected: BTreeSet::new(),
            bill_export_dir: export_dir().display().to_string(),
            draw_inputs,
            draw_paste: Default::default(),
            draw_paste_error: Default::default(),
            draw_error: None,
            references: Vec::new(),
            reference_keys: HashSet::new(),
            reference_open: false,
            reference_popup: 0,
            reference_scroll_top: false,
            reference_ignore_edit: reference_filter.to_lines(),
            reference_filter,
            reference_search: String::new(),
            reference_msg: None,
            reference_msg_error: false,
            quote_fetch_rx: None,
            quote_fetch_row: None,
            quote_fetch_delete: false,
            quote_original: None,
            edit_text: String::new(),
            edit_time: String::new(),
            ocr_rx: None,
            ocr_target: None,
            ocr_msg: None,
            source_sel_snapshot: None,
            pending_source_action: None,
            keywords: keywords::load(),
            keywords_open: false,
            keyword_section: Section::Play,
            keyword_inputs: HashMap::new(),
            keyword_variant: String::new(),
            keyword_correct: String::new(),
            replacements: replacements::load(),
            replacements_open: false,
            replacement_from: String::new(),
            replacement_to: String::new(),
            single_open: false,
            single_raw: String::new(),
            single_play: "三中三".to_string(),
            single_region: "澳门".to_string(),
            single_preview: None,
            single_member: None,
            single_members: Vec::new(),
            odds: crate::odds::load(),
            odds_open: false,
            odds_draft: crate::odds::OddsConfig::default(),
            odds_scope: OddsScope::Default,
            odds_region: 0,
            odds_new_member: String::new(),
            risk_tab: RiskTab::XiaoMa,
            risk_xm_region: None,
            risk_xm_sort: RiskSort::Risk,
            risk_xm_desc: true,
            risk_xm_page: 1,
            risk_xm_page_size: 20,
            risk_xm_jump: String::new(),
            risk_xm_search: String::new(),
            risk_prob_open: false,
            xm: XmDeduct {
                store: crate::xiaoma_offsets::Store::load(),
                ..Default::default()
            },
            risk_lian_size: 2,
            risk_lian_pages: [1, 1, 1],
            risk_panel_desc: [false, false, false],
            risk_collapsed: [false, false, true],
            code_report_open: false,
            code_report_region: 0,
            code_report_mode: CodeReportMode::Risk,
            code_report_threshold: "-10000".to_string(),
            code_report_limit: "5000".to_string(),
            code_report_reduce: HashMap::new(),
            code_report_msg: None,
            info_codes: crate::info_code::load(),
            info_code_open: false,
            info_code_draft: String::new(),
            info_code_default_region: 0,
            info_code_default_play: String::new(),
            info_code_msg: None,
            info_code_confirm_clear: false,
            info_code_only_hit: true,
            info_sel_snapshot: None,
            pending_info_action: None,
            info_code_lines: Vec::new(),
            info_code_keys: HashSet::new(),
            info_hit_cache: HashMap::new(),
            info_rev: 0,
            auth,
            login_user,
            login_pass,
            login_remember,
            login_msg: None,
            login_rx: None,
            auth_open: false,
            pwd_old: String::new(),
            pwd_new: String::new(),
            pwd_confirm: String::new(),
            pwd_msg: None,
            pwd_rx: None,
            auth_refresh_rx: None,
            window_title: format!("小掌柜 v{}", env!("CARGO_PKG_VERSION")),
            was_authenticated: false,
            wx_proc_checked: None,
            // 初值先取一次，避免「启动时微信本来就在运行」被当成刚启动而自动重检。
            wx_proc_running: !crate::keyextract::weixin_pids().is_empty(),
            wx_auto_check_at: None,
        };
        app.rebuild_info_codes();
        app
    }

    fn info_mul_sep(&self) -> String {
        self.keywords
            .get(crate::keywords::Field::Multiplication)
            .iter()
            .find(|k| !k.is_empty())
            .cloned()
            .unwrap_or_else(|| "拖".to_string())
    }

    fn rebuild_info_codes(&mut self) {
        let sep = self.info_mul_sep();
        let (lines, keys) = crate::info_code::build_lines(
            &self.info_codes,
            &self.keywords,
            &sep,
        );
        self.info_code_lines = lines;
        self.info_code_keys = keys;
        self.info_hit_cache.clear();
        self.info_rev = self.info_rev.wrapping_add(1);
    }

    /// 这一条注单是否踩中信息码；结果按 (注单, 信息码版本) 缓存。
    fn info_hits_for(
        &mut self,
        row: &MsgRow,
        conv: &ConvertOutput,
    ) -> Option<crate::info_code::OrderInfoHits> {
        if self.info_code_keys.is_empty() {
            return None;
        }
        let key = edit_key(row);
        if let Some((rev, hits)) = self.info_hit_cache.get(&key) {
            if *rev == self.info_rev {
                return hits.clone();
            }
        }
        let sep = self.info_mul_sep();
        let hits = crate::info_code::order_hits(conv, &self.info_code_keys, &sep);
        self.info_hit_cache
            .insert(key, (self.info_rev, hits.clone()));
        hits
    }

    /// 一条注单命中的信息码，按「信息码原文」合并金额：同一码撞中本单多个组合只列一次。
    fn info_hit_groups(
        &self,
        hit: &crate::info_code::OrderInfoHits,
    ) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        for line in &self.info_code_lines {
            let amount: f64 = line
                .keys
                .iter()
                .map(|k| {
                    hit.combos
                        .iter()
                        .filter(|(hk, _)| hk == k)
                        .map(|(_, a)| *a)
                        .sum::<f64>()
                })
                .sum();
            if amount > 0.0 {
                out.push((line.text.clone(), amount));
            }
        }
        out
    }

    /// 弹窗右列：所有信息码组合 + 当前注单的命中金额/单数。
    fn info_window_rows(
        &mut self,
    ) -> Vec<(crate::info_code::CodeKey, f64, usize)> {
        let mut agg: std::collections::BTreeMap<
            crate::info_code::CodeKey,
            (f64, usize),
        > = std::collections::BTreeMap::new();
        let orders = self.orders.clone();
        for row in orders {
            let key = edit_key(&row);
            let conv = self
                .auto_converted
                .get(&key)
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = safe_convert(
                        &row.content,
                        &self.keywords,
                        &self.replacements,
                    );
                    self.apply_doubt_override(&key, &mut c);
                    c
                });
            if let Some(hit) = self.info_hits_for(&row, &conv) {
                for (k, amount) in hit.combos {
                    let e = agg.entry(k).or_insert((0.0, 0));
                    e.0 += amount;
                    e.1 += 1;
                }
            }
        }
        let mut rows: std::collections::BTreeMap<
            crate::info_code::CodeKey,
            (f64, usize),
        > = std::collections::BTreeMap::new();
        for line in &self.info_code_lines {
            for key in &line.keys {
                rows.entry(key.clone()).or_insert((0.0, 0));
            }
        }
        for (k, v) in agg {
            rows.insert(k, v);
        }
        rows.into_iter()
            .map(|(k, (a, n))| (k, a, n))
            .collect()
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.stop_monitor();
        self.state = State::Loading;
        self.status = "正在重新读取微信数据…".to_string();
        self.textures.clear();
        self.expanded.clear();
        self.selected_contacts.clear();
        self.selected_groups.clear();
        self.selected_members.clear();
        // 分组定义保留（引用的是 username，重新读取后仍可对应）。
        self.group_dialog_open = false;
        self.group_name.clear();
        self.group_error = None;
        self.group_rename = None;
        self.group_rename_input.clear();
        self.group_rename_error = None;
        self.group_manage = None;
        self.group_delete = None;
        self.dialog_open = false;
        self.dialog_error = None;
        self.orders.clear();
        self.order_keys.clear();
        self.info_hit_cache.clear();
        self.order_filter = None;
        self.orders_state = OrdersState::Idle;
        self.last_update.clear();
        self.import_rx = None;
        self.selection_dirty = true;
        self.clear_open = false;
        self.clear_groups.clear();
        self.clear_members.clear();
        self.member_focus = None;
        self.selected_order_key = None;
        self.selected_order_row = None;
        self.scroll_to_order = false;
        self.converted = None;
        self.auto_converted.clear();
        self.source_sel_snapshot = None;
        self.pending_source_action = None;
        self.edit_text.clear();
        self.keywords_open = false;
        self.replacements_open = false;
        self.odds_open = false;
        self.single_open = false;
        self.single_raw.clear();
        self.single_preview = None;
        self.single_member = None;
        self.single_members.clear();
        self.batch_replace_open = false;
        self.batch_replace_find.clear();
        self.batch_replace_to.clear();
        self.batch_replace_target = None;
        self.draw_open = false;
        self.draw_error = None;
        self.draw_inputs = draw_inputs_from(&self.draws);
        self.draw_paste = Default::default();
        self.draw_paste_error = Default::default();
        self.references.clear();
        self.reference_keys.clear();
        self.reference_search.clear();
        self.reference_popup = 0;
        self.reference_scroll_top = false;
        self.reference_msg = None;
        self.reference_msg_error = false;
        self.reconcile_open = false;
        self.reconcile_pasted.clear();
        self.reconcile_only_diff = true;
        self.reconcile_msg = None;
        let (tx, rx) = channel();
        self.rx = Some(rx);
        loader::start(ctx.clone(), tx);
    }

    // ---------------- 消息轮询 ----------------

    fn poll(&mut self) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for m in msgs {
            match m {
                Msg::Status(s) => self.status = s,
                Msg::Done(d) => {
                    self.state = State::Ready(d);
                    self.rx = None;
                }
                Msg::Failed(e) => {
                    self.state = State::Failed(e);
                    self.rx = None;
                }
            }
        }
        if disconnected {
            self.rx = None;
        }
    }

    /// 「识别图片」的词库：关键词设置里的全部词 + 生肖 + 地区 / 金额 / 组合标记。
    fn ocr_vocab(&self) -> std::collections::HashSet<String> {
        crate::ocr::vocabulary(&self.keywords)
    }

    /// 「识别图片」后台线程的结果：成功就替换原文，失败只显示错误。
    fn poll_ocr(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.ocr_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(text)) => {
                // 识别期间切换了注单：结果属于上一条，不能写进当前原文。
                if self.ocr_target.as_ref() == self.selected_order_key.as_ref() {
                    let vocab = self.ocr_vocab();
                    let (text, fixed) = crate::ocr::correct_text(&text, &vocab);
                    let count = text.chars().filter(|c| !c.is_whitespace()).count();
                    self.edit_text = text;
                    let msg = if fixed > 0 {
                        format!("已识别 {count} 字，按词库纠正 {fixed} 处，检查后点「提交」保存")
                    } else {
                        format!("已识别 {count} 字，检查后点「提交」保存")
                    };
                    self.ocr_msg = Some((msg, false));
                }
                self.ocr_target = None;
                self.ocr_rx = None;
            }
            Ok(Err(e)) => {
                self.ocr_msg = Some((e, true));
                self.ocr_target = None;
                self.ocr_rx = None;
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(TryRecvError::Disconnected) => {
                self.ocr_msg = Some(("识别线程异常退出".to_string(), true));
                self.ocr_target = None;
                self.ocr_rx = None;
            }
        }
    }

    fn poll_import(&mut self) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.import_rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for m in msgs {
            match m {
                ImportMsg::Status(s) => self.orders_state = OrdersState::Loading(s),
                ImportMsg::Done(res) => {
                    let ImportResult { rows, .. } = *res;
                    self.orders.clear();
                    self.order_keys.clear();
                    self.auto_converted.clear();
                    self.info_hit_cache.clear();
                    self.references.clear();
                    self.reference_keys.clear();
                    self.fill_orders(rows);
                    self.orders_state = OrdersState::Ready;
                    self.last_update = now_hms();
                    self.import_rx = None;
                }
                ImportMsg::Failed(e) => {
                    self.orders_state = OrdersState::Failed(e);
                    self.import_rx = None;
                }
            }
        }
        if disconnected {
            self.import_rx = None;
        }
    }

    fn poll_monitor(&mut self) {
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.monitor_rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => msgs.push(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for m in msgs {
            match m {
                MonitorMsg::Rows(rows) => {
                    self.fill_orders(rows);
                    self.last_update = now_hms();
                }
                // 监控状态不再在界面上展示；读取一下 payload，避免字段被判为未使用。
                MonitorMsg::Status(status) => {
                    let _ = status;
                }
                MonitorMsg::Stopped(status) => {
                    let _ = status;
                    self.monitor_rx = None;
                    self.monitor_stop = None;
                }
            }
        }
        if disconnected {
            self.monitor_rx = None;
            self.monitor_stop = None;
        }
    }

    fn fill_orders(&mut self, rows: Vec<MsgRow>) {
        let filter = self.reference_filter.clone();
        let mut new_reference = false;
        // 保存过编辑内容的，用编辑后的文本覆盖微信原文；撤回提示也按编辑后的
        // 文本识别。先统一处理，再建立撤回匹配候选池。
        let mut rows = rows;
        for r in &mut rows {
            if let Some(edited) = self.order_edits.get(&edit_key(r)) {
                r.content = edited.clone();
            }
            if let Some(t) = self.order_times.get(&edit_key(r)) {
                r.time = t.clone();
            }
        }
        // 本批里所有原消息（含被占位 / 链接规则过滤掉的、以及已撤回的）都是
        // 撤回提示的匹配候选；撤回提示本身除外。已撤回的原消息虽然不再显示，
        // 也必须留在候选池里，否则重新导入时会误把更早的同发送人消息当成
        // 撤回目标，导致「已撤回」越导入越多。
        let batch_seen = revoke_candidates(&rows);
        // 本次导入之前已经在列表里的消息（监控/上一次导入）。
        // 撤回目标的「按名字猜」只允许在这些旧消息里找：本批新读到的消息如果
        // 是撤回目标，它已经被微信替换成撤回提示、不会还留在库里；还留在
        // 库里的上一条并不是被撤回的那条，不能拿来顶替。
        let prior_orders: Vec<MsgRow> = self.orders.clone();
        let (self_wxid, name_to_usr) = match &self.state {
            State::Ready(d) => {
                // 撤回提示里的名字可能是备注名，也可能是群昵称：两种都映射到 username。
                let mut map = HashMap::new();
                let mut add = |usr: &str, name: &str| {
                    if !usr.is_empty() && !name.is_empty() {
                        map.entry(name.to_string()).or_insert_with(|| usr.to_string());
                    }
                };
                for p in &d.contacts {
                    add(&p.username, &p.display_name);
                    add(&p.username, &p.nick_name);
                }
                for g in &d.groups {
                    for m in &g.members {
                        add(&m.username, &m.display_name);
                        add(&m.username, &m.nick_name);
                    }
                }
                add(&d.self_wxid, &d.self_name);
                (d.self_wxid.clone(), map)
            }
            _ => (String::new(), HashMap::new()),
        };
        for r in rows {
            // 撤回提示不进任何列表：先找出被撤回的原消息并记入「已撤回」，
            // 同时把它从当前注单列表里移除。
            if r.kind == "撤回" {
                self.handle_revoke(
                    &r,
                    &prior_orders,
                    &batch_seen,
                    &self_wxid,
                    &name_to_usr,
                );
                continue;
            }
            // 已撤回名单里的原消息：重新导入也不再出现。
            if self.revoked.contains_key(&revoke_key_of(&r)) {
                continue;
            }
            // 引用不参与注单转换，进「引用列表」独立窗口；优先于下面的占位过滤：
            // 正文就算是「1」也仍然保留。
            if r.kind == "引用" {
                let key = (
                    r.sort,
                    r.chat.clone(),
                    r.nick.clone(),
                    r.content.clone(),
                );
                if self.reference_keys.insert(key) {
                    self.references.push(r);
                    new_reference = true;
                }
                continue;
            }
            // 可配置的占位消息（默认「1」「2」）直接丢弃，哪个列表都不进。
            if filter.is_ignored(&r.content) {
                continue;
            }
            // 普通链接同样不进注单列表；引用列表只展示「引用」类型。
            if r.kind == "链接" {
                continue;
            }
            // 带上会话 username：同一成员在不同群聊里的消息必须各自保留。
            let key = (
                r.sort,
                r.chat.clone(),
                r.nick.clone(),
                r.content.clone(),
            );
            if self.order_keys.insert(key) {
                // 新增的注单自动跑一次「提交」转换，选中时直接显示明细。
                let mut conv = safe_convert(&r.content, &self.keywords, &self.replacements);
                self.apply_doubt_override(&edit_key(&r), &mut conv);
                self.auto_converted.insert(edit_key(&r), conv);
                self.orders.push(r);
                self.info_hit_cache.clear();
            }
        }
        self.orders.sort_by_key(|r| r.sort);
        self.references.sort_by_key(|r| r.sort);
        if new_reference {
            // 每来一条新引用都弹到最前，并让列表回到最新一条。
            self.reference_open = true;
            self.reference_popup = 8;
            self.reference_scroll_top = true;
            // 清掉旧的搜索条件，保证新引用不会被过滤掉。
            self.reference_search.clear();
            self.reference_msg = None;
            self.reference_msg_error = false;
        }
    }

    /// 处理一条撤回提示：找出被撤回的原消息，写入「已撤回」名单并从注单 /
    /// 引用列表移除；写入后重新导入也会被过滤掉。
    fn handle_revoke(
        &mut self,
        revoke: &MsgRow,
        prior_orders: &[MsgRow],
        batch_seen: &[MsgRow],
        self_wxid: &str,
        name_to_usr: &HashMap<String, String>,
    ) {
        let Some(target) =
            find_revoke_target(revoke, prior_orders, batch_seen, self_wxid, name_to_usr)
        else {
            return;
        };
        // 只处理当前导入时段内的原消息：撤回提示在时段内，但原消息可能是
        // 上一次不同时段遗留在 self.orders 里的，不能顺手记成已撤回。
        if let Some((start_ts, end_ts)) = self.import_range_ts() {
            let start = start_ts.saturating_mul(1000);
            let end = end_ts.saturating_mul(1000).saturating_add(999);
            if target.sort < start || target.sort > end {
                return;
            }
        }
        let key = revoke_key_of(&target);
        let record = crate::revoked::RevokedRow {
            sort: target.sort,
            chat: target.chat.clone(),
            sender: target.sender.clone(),
            time: target.time.clone(),
            nick: target.nick.clone(),
            kind: target.kind.clone(),
            content: target.content.clone(),
        };
        if self.revoked.get(&key) != Some(&record) {
            self.revoked.insert(key.clone(), record);
            crate::revoked::save(&self.revoked);
        }

        // 从注单 / 引用里移除（被撤回的可能本来就不在列表里）。
        let row_key = (
            target.sort,
            target.chat.clone(),
            target.nick.clone(),
            target.content.clone(),
        );
        self.orders.retain(|o| revoke_key_of(o) != key);
        self.order_keys.remove(&row_key);
        self.auto_converted.remove(&edit_key(&target));
        self.info_hit_cache.clear();
        self.references.retain(|r| revoke_key_of(r) != key);
        self.reference_keys.remove(&row_key);
        if self
            .selected_order_row
            .as_ref()
            .is_some_and(|s| revoke_key_of(s) == key)
        {
            self.selected_order_row = None;
            self.selected_order_key = None;
            self.converted = None;
            self.edit_text.clear();
            self.edit_time.clear();
            self.ocr_msg = None;
        }
    }

    /// 导入与监控的数据来源：
    /// - 已有会员分组时：**全部分组下的全部联系人 / 群成员**。每次导入都按当前
    ///   分组重新组装来源，新增的分组或新分配的会员会立即生效，并覆盖上一次的注单；
    ///   成员按 `(群, 成员)` 保留，同一成员在不同群里分别读取；
    /// - 没有分组时：沿用「选定会员」的原有选择（含整群）。
    fn import_source(&self) -> ImportSource {
        if self.groups.is_empty() {
            return ImportSource {
                from_groups: false,
                contacts: self.selected_contacts.iter().cloned().collect(),
                groups: group_filters(&self.selected_groups, &self.selected_members),
            };
        }
        source_from_groups(&self.groups, &HashSet::new(), &HashSet::new())
    }

    /// 当前导入日期 / 时段对应的 Unix 秒区间。
    fn import_range_ts(&self) -> Option<(i64, i64)> {
        let date = chrono::NaiveDate::from_ymd_opt(
            self.import_date.year() as i32,
            self.import_date.month() as u32,
            self.import_date.day() as u32,
        )?;
        messages::range_to_ts(date, self.import_start, self.import_end).ok()
    }

    /// 选定会员 / 分组后常驻监控：有可读来源就运行；无来源则停止。
    ///
    /// 监控也受「导入注单」里选的日期 / 时段限制：时段外的消息不会追加进来
    /// （以前是只追加从现在开始的新消息，会把第二天的新消息也带进昨天的注单）。
    fn sync_monitor(&mut self, ctx: &egui::Context, data: &AppData) {
        let source = self.import_source();
        let has_source = !source.contacts.is_empty() || !source.groups.is_empty();
        if !has_source {
            if self.monitor_rx.is_some() || self.monitor_stop.is_some() {
                self.stop_monitor();
            }
            self.selection_dirty = false;
            return;
        }

        let Some((start_ts, end_ts)) = self.import_range_ts() else {
            self.stop_monitor();
            self.selection_dirty = false;
            return;
        };
        let range = (start_ts, end_ts);
        let range_changed = self.monitor_range != Some(range);

        // 导入时段已经结束：不再启动监控，否则时段外的消息会被追加。
        if Local::now().timestamp() > end_ts {
            if self.monitor_rx.is_some() || self.monitor_stop.is_some() {
                self.stop_monitor();
            }
            self.monitor_range = Some(range);
            self.selection_dirty = false;
            return;
        }

        if self.monitor_rx.is_none() || self.selection_dirty || range_changed {
            let req = ImportRequest {
                account_dir: data.account_path.clone(),
                workdir: data.workdir.clone(),
                keys: data.keys.clone(),
                contacts: source.contacts,
                groups: source.groups.into_iter().collect(),
                name_of: build_name_map(data),
                start_ts,
                end_ts,
            };
            self.stop_monitor();
            let since = Local::now().timestamp_millis();
            let (stop, rx) = messages::start_monitor(
                req,
                since,
                Duration::from_millis(MONITOR_INTERVAL_MS),
                ctx.clone(),
            );
            self.monitor_stop = Some(stop);
            self.monitor_rx = Some(rx);
            self.monitor_range = Some(range);
        }
        self.selection_dirty = false;
    }

    fn stop_monitor(&mut self) {
        if let Some(flag) = self.monitor_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.monitor_rx = None;
    }

    /// 关键词 / 替换词变化后，重新转换全部注单（状态、总金额同步刷新）。
    fn recompute_all_conversions(&mut self) {
        self.auto_converted.clear();
        let rows: Vec<(EditKey, String)> = self
            .orders
            .iter()
            .map(|r| (edit_key(r), r.content.clone()))
            .collect();
        for (key, content) in rows {
            let mut conv = safe_convert(&content, &self.keywords, &self.replacements);
            self.apply_doubt_override(&key, &mut conv);
            self.auto_converted.insert(key, conv);
        }
        if let Some(row) = self.selected_order_row.clone() {
            self.converted = self.auto_converted.get(&edit_key(&row)).cloned();
        }
        self.info_hit_cache.clear();
    }

    /// 保存详情面板里改过的原文，并同步当前列表行与选中键。
    fn save_order_edit(&mut self, row: &MsgRow, text: String) {
        self.order_edits.insert(edit_key(row), text.clone());
        edits::save(&self.order_edits);

        let Some(old) = self.selected_order_key.clone() else {
            return;
        };
        let Some(i) = self.orders.iter().position(|r| {
            r.sort == old.0 && r.chat == old.1 && r.nick == old.2 && r.content == old.3
        }) else {
            return;
        };
        self.order_keys.remove(&old);
        self.orders[i].content = text.clone();
        let new_key = (
            self.orders[i].sort,
            self.orders[i].chat.clone(),
            self.orders[i].nick.clone(),
            text,
        );
        self.order_keys.insert(new_key.clone());
        self.selected_order_key = Some(new_key);
        self.selected_order_row = Some(self.orders[i].clone());
        self.info_hit_cache.clear();
    }

    /// 保存详情面板里改过的时间，并同步列表行。
    fn save_order_time(&mut self) {
        let Some(row) = self.selected_order_row.clone() else {
            return;
        };
        let time = self.edit_time.trim().to_string();
        if time.is_empty() {
            return;
        }
        let key = edit_key(&row);
        self.order_times.insert(key.clone(), time.clone());
        times::save(&self.order_times);
        if let Some(r) = self.orders.iter_mut().find(|r| edit_key(r) == key) {
            r.time = time.clone();
        }
        if let Some(sel) = &mut self.selected_order_row {
            sel.time = time;
        }
    }

    /// 已手动取消存疑的注单：转换结果不再标存疑。
    fn apply_doubt_override(&self, key: &EditKey, conv: &mut ConvertOutput) {
        // 只清存疑；校验失败（invalid）的原因要保留。
        if !conv.invalid && self.doubt_cleared.contains(key) {
            conv.doubtful = false;
            conv.reasons.clear();
        }
    }

    /// 「取消存疑」：把当前注单标记为已核对，并保存到本地。
    fn clear_doubt(&mut self, row: &MsgRow) {
        let key = edit_key(row);
        self.doubt_cleared.insert(key.clone());
        doubt::save(&self.doubt_cleared);
        if let Some(conv) = self.auto_converted.get_mut(&key) {
            conv.doubtful = false;
            conv.reasons.clear();
        }
        if let Some(conv) = &mut self.converted {
            conv.doubtful = false;
            conv.reasons.clear();
        }
    }

    /// 从当前注单列表删除一条（只影响本程序，不改微信数据；重新导入会恢复）。
    fn delete_order(&mut self, row: &MsgRow) {
        let key = edit_key(row);
        self.orders.retain(|r| edit_key(r) != key);
        self.order_keys.retain(|k| {
            !(k.0 == row.sort
                && k.1 == row.chat
                && k.2 == row.nick
                && k.3 == row.content)
        });
        self.auto_converted.remove(&key);
        self.info_hit_cache.remove(&key);
        let selected = self
            .selected_order_row
            .as_ref()
            .is_some_and(|r| edit_key(r) == key);
        if selected {
            self.selected_order_row = None;
            self.selected_order_key = None;
            self.converted = None;
            self.scroll_to_order = false;
            self.edit_text.clear();
            self.edit_time.clear();
        }
    }

    /// 执行「原文」右键菜单的动作。文本改动后同步回 TextEdit 的光标 / 选区。
    fn source_menu_action(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        action: SourceMenuAction,
        fallback_range: Option<egui::text::CCursorRange>,
    ) {
        use egui::text::{CCursor, CCursorRange};

        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        // 优先用 TextEdit 里的选区；菜单打开期间它可能被 egui 折叠，
        // 这时用右键时记下的选区，保证每个菜单项都能拿到选中内容。
        let range = state.cursor.char_range().or(fallback_range);
        let char_range = range.map(|r| {
            let r = r.as_sorted_char_range();
            (r.start.0, r.end.0)
        });
        let selected = range
            .map(|r| r.slice_str(&self.edit_text).to_string())
            .unwrap_or_default();
        let cursor = range
            .and_then(|r| r.single())
            .map(|c| c.index.0)
            .unwrap_or_else(|| self.edit_text.chars().count());
        let (start, end) = char_range.unwrap_or((cursor, cursor));
        let byte_at = |text: &str, ci: usize| {
            text.char_indices()
                .nth(ci)
                .map(|(b, _)| b)
                .unwrap_or(text.len())
        };
        let bs = byte_at(&self.edit_text, start);
        let be = byte_at(&self.edit_text, end);

        let mut new_sel: Option<(usize, usize)> = None;
        match action {
            SourceMenuAction::Copy => {
                if !selected.is_empty() {
                    ctx.copy_text(selected);
                }
            }
            SourceMenuAction::Paste => {
                if let Some(text) = crate::clipboard::read_text() {
                    self.edit_text.replace_range(bs..be, &text);
                    let at = start + text.chars().count();
                    new_sel = Some((at, at));
                }
            }
            SourceMenuAction::Cut => {
                if !selected.is_empty() {
                    ctx.copy_text(selected);
                    self.edit_text.replace_range(bs..be, "");
                    new_sel = Some((start, start));
                }
            }
            SourceMenuAction::SelectAll => {
                new_sel = Some((0, self.edit_text.chars().count()));
            }
            SourceMenuAction::InsertDrag => {
                if let Some(next) = convert::insert_drag_text(&selected, &self.keywords) {
                    self.edit_text.replace_range(bs..be, &next);
                    let at = start + next.chars().count();
                    new_sel = Some((start, at));
                }
            }
            SourceMenuAction::BatchNewline => {
                if !selected.is_empty() && !selected.contains('\n') {
                    self.edit_text = self.edit_text.replace(&selected, "\n");
                    new_sel = Some((0, 0));
                }
            }
            SourceMenuAction::BatchReplace => {
                if !selected.is_empty() {
                    self.batch_replace_find = selected;
                    self.batch_replace_to.clear();
                    self.batch_replace_target =
                        self.selected_order_row.as_ref().map(edit_key);
                    self.batch_replace_open = true;
                }
                ctx.memory_mut(|m| m.request_focus(id));
                return;
            }
        }

        if let Some((s, e)) = new_sel {
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(s),
                CCursor::new(e),
            )));
            state.store(ctx, id);
        }
        ctx.memory_mut(|m| m.request_focus(id));
    }

    /// 信息码录入框的右键菜单动作（复制 / 粘贴 / 剪切 / 全选 / 插入拖 / 批量回车）。
    fn info_menu_action(
        &mut self,
        ctx: &egui::Context,
        id: egui::Id,
        action: SourceMenuAction,
        fallback_range: Option<egui::text::CCursorRange>,
    ) {
        use egui::text::{CCursor, CCursorRange};

        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        let range = state.cursor.char_range().or(fallback_range);
        let char_range = range.map(|r| {
            let r = r.as_sorted_char_range();
            (r.start.0, r.end.0)
        });
        let selected = range
            .map(|r| r.slice_str(&self.info_code_draft).to_string())
            .unwrap_or_default();
        let cursor = range
            .and_then(|r| r.single())
            .map(|c| c.index.0)
            .unwrap_or_else(|| self.info_code_draft.chars().count());
        let (start, end) = char_range.unwrap_or((cursor, cursor));
        let byte_at = |text: &str, ci: usize| {
            text.char_indices()
                .nth(ci)
                .map(|(b, _)| b)
                .unwrap_or(text.len())
        };
        let bs = byte_at(&self.info_code_draft, start);
        let be = byte_at(&self.info_code_draft, end);

        let mut new_sel: Option<(usize, usize)> = None;
        match action {
            SourceMenuAction::Copy => {
                if !selected.is_empty() {
                    ctx.copy_text(selected);
                }
            }
            SourceMenuAction::Paste => {
                if let Some(text) = crate::clipboard::read_text() {
                    self.info_code_draft.replace_range(bs..be, &text);
                    let at = start + text.chars().count();
                    new_sel = Some((at, at));
                }
            }
            SourceMenuAction::Cut => {
                if !selected.is_empty() {
                    ctx.copy_text(selected);
                    self.info_code_draft.replace_range(bs..be, "");
                    new_sel = Some((start, start));
                }
            }
            SourceMenuAction::SelectAll => {
                new_sel = Some((0, self.info_code_draft.chars().count()));
            }
            SourceMenuAction::InsertDrag => {
                if let Some(next) = convert::insert_drag_text(&selected, &self.keywords) {
                    self.info_code_draft.replace_range(bs..be, &next);
                    let at = start + next.chars().count();
                    new_sel = Some((start, at));
                }
            }
            SourceMenuAction::BatchNewline => {
                if !selected.is_empty() && !selected.contains('\n') {
                    self.info_code_draft = self.info_code_draft.replace(&selected, "\n");
                    new_sel = Some((0, 0));
                }
            }
            SourceMenuAction::BatchReplace => {}
        }

        if let Some((s, e)) = new_sel {
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(s),
                CCursor::new(e),
            )));
            state.store(ctx, id);
        }
        ctx.memory_mut(|m| m.request_focus(id));
    }

    /// 「开奖」弹窗：卡片式录入三个地区，带生肖、实时校验、整期粘贴与自动跳格。
    fn draw_dialog(&mut self, ctx: &egui::Context) {
        if !self.draw_open {
            return;
        }
        const NAMES: [&str; 3] = ["澳门", "香港", "老澳门"];
        const ACCENTS: [Color32; 3] = [
            Color32::from_rgb(0x05, 0x96, 0x69),
            Color32::from_rgb(0xDC, 0x26, 0x26),
            Color32::from_rgb(0xD9, 0x77, 0x06),
        ];
        let mut open = true;
        let mut confirm = false;
        let mut clear_all = false;
        let mut close = false;
        let mut focus_after: Option<egui::Id> = None;

        egui::Window::new("开奖号码录入")
            .collapsible(false)
            .resizable(false)
            .default_width(680.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "每个地区填 6 个平码 + 1 个特码（1-49，互不重复）；留空的地区不会录入。",
                    )
                    .size(11.5)
                    .color(SUB),
                );
                draw_summary_ui(ui, &self.draws, "当前已保存：");
                ui.add_space(8.0);
                // 加了 1-49 号码盘之后内容会变高：正文限高滚动，底部的确认按钮始终可见。
                let body_h = (ctx.content_rect().height() * 0.72).max(320.0);
                egui::ScrollArea::vertical()
                    .id_salt("draw_dialog_scroll")
                    .max_height(body_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {

                for i in 0..3 {
                    egui::Frame::new()
                        .fill(CARD)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(CornerRadius::same(10))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                egui::Frame::new()
                                    .fill(ACCENTS[i])
                                    .corner_radius(CornerRadius::same(5))
                                    .inner_margin(Margin::symmetric(9, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(NAMES[i])
                                                .size(12.0)
                                                .strong()
                                                .color(Color32::WHITE),
                                        );
                                    });
                                ui.add_space(4.0);
                                let filled = self.draw_inputs[i]
                                    .iter()
                                    .filter(|s| !s.trim().is_empty())
                                    .count();
                                ui.label(
                                    RichText::new(if filled == 0 {
                                        "未录入".to_string()
                                    } else {
                                        format!("{filled}/7")
                                    })
                                    .size(10.5)
                                    .color(SUB),
                                );
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        if ui.add(tiny_button("清空本区", SUB)).clicked()
                                        {
                                            for s in &mut self.draw_inputs[i] {
                                                s.clear();
                                            }
                                            self.draw_paste[i].clear();
                                            self.draw_paste_error[i] = None;
                                        }
                                    },
                                );
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                for j in 0..7 {
                                    let special = j == 6;
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(if special {
                                                "特".to_string()
                                            } else {
                                                format!("{}", j + 1)
                                            })
                                            .size(9.5)
                                            .color(if special { SPECIAL_RED } else { SUB }),
                                        );
                                        let id =
                                            egui::Id::unique(("draw_num", i, j));
                                        let mut resp = ui.add(
                                            egui::TextEdit::singleline(
                                                &mut self.draw_inputs[i][j],
                                            )
                                            .id(id)
                                            .desired_width(52.0)
                                            .min_size(Vec2::new(52.0, 30.0))
                                            .margin(Margin::symmetric(4, 3))
                                            .char_limit(2)
                                            .align(Align2::CENTER_CENTER)
                                            .font(bold_font(17.0))
                                            .text_color(if special {
                                                SPECIAL_RED
                                            } else {
                                                TEXT
                                            }),
                                        );
                                        text_edit_context_menu(
                                            ui.ctx(),
                                            resp.id,
                                            &mut self.draw_inputs[i][j],
                                            &mut resp,
                                        );
                                        if resp.changed() {
                                            let cleaned: String = self.draw_inputs[i][j]
                                                .chars()
                                                .filter(|c| c.is_ascii_digit())
                                                .take(2)
                                                .collect();
                                            if cleaned != self.draw_inputs[i][j] {
                                                self.draw_inputs[i][j] = cleaned;
                                            }
                                            self.draw_error = None;
                                            self.draw_paste_error[i] = None;
                                            let value = self.draw_inputs[i][j]
                                                .trim()
                                                .parse::<u32>()
                                                .ok();
                                            if let Some(v) = value {
                                                if (1..=49).contains(&v)
                                                    && unique_in_draw_row(
                                                        &self.draw_inputs[i],
                                                        j,
                                                        v,
                                                    )
                                                    && j < 6
                                                {
                                                    focus_after = Some(egui::Id::unique((
                                                        "draw_num",
                                                        i,
                                                        j + 1,
                                                    )));
                                                }
                                            }
                                        }
                                        if resp.has_focus()
                                            && ui.input(|inp| {
                                                inp.key_pressed(egui::Key::Enter)
                                            })
                                        {
                                            focus_after = Some(if j < 6 {
                                                egui::Id::unique(("draw_num", i, j + 1))
                                            } else if i < 2 {
                                                egui::Id::unique(("draw_num", i + 1, 0))
                                            } else {
                                                egui::Id::unique(("draw_num", 0, 0))
                                            });
                                        }
                                        let z = self.draw_inputs[i][j]
                                            .trim()
                                            .parse::<u32>()
                                            .ok()
                                            .and_then(crate::settle::zodiac_of)
                                            .unwrap_or(" ");
                                        ui.label(
                                            RichText::new(z).size(10.0).color(
                                                if special {
                                                    Color32::from_rgb(0xBB, 0x00, 0x00)
                                                } else {
                                                    SUB
                                                },
                                            ),
                                        );
                                    });
                                    if j == 5 {
                                        ui.add_space(10.0);
                                    }
                                }
                            });

                            // 实时校验：不用点确认就能看到缺几个号 / 哪个号重复。
                            let check = check_draw_row(&self.draw_inputs[i]);
                            if let Some(err) = &check.error {
                                ui.label(RichText::new(err).size(10.5).color(RED));
                            } else if check.filled == 7 {
                                ui.label(
                                    RichText::new("已录满 7/7").size(10.5).color(GREEN_DEEP),
                                );
                            } else if check.filled == 0 {
                                ui.label(
                                    RichText::new("可留空，也可以在下面整期粘贴后填入。")
                                        .size(10.5)
                                        .color(SUB),
                                );
                            }

                            ui.horizontal(|ui| {
                                ui.label(RichText::new("整期粘贴").size(10.5).color(SUB));
                                let mut resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.draw_paste[i])
                                        .hint_text("01 02 03 04 05 06 07")
                                        .desired_width(220.0)
                                        .font(FontId::proportional(11.5)),
                                );
                                text_edit_context_menu(
                                    ui.ctx(),
                                    resp.id,
                                    &mut self.draw_paste[i],
                                    &mut resp,
                                );
                                if ui.add(tiny_button("填入本区", TEXT)).clicked() {
                                    match parse_draw_paste(&self.draw_paste[i]) {
                                        Ok(vals) => {
                                            self.draw_inputs[i] = vals;
                                            self.draw_paste[i].clear();
                                            self.draw_paste_error[i] = None;
                                        }
                                        Err(e) => {
                                            self.draw_paste_error[i] = Some(e);
                                        }
                                    }
                                }
                            });
                            if let Some(err) = &self.draw_paste_error[i] {
                                ui.label(RichText::new(err).size(10.5).color(RED));
                            }
                            // 1-49 号码盘：点一下把号码填进本区第一个空输入框；
                            // 已经录进本区的号码变淡、点不动。
                            ui.add_space(4.0);
                            ui.horizontal_top(|ui| {
                                ui.label(RichText::new("号码盘").size(10.0).color(SUB));
                                ui.add_space(6.0);
                                let used: Vec<u32> = self.draw_inputs[i]
                                    .iter()
                                    .filter_map(|s| s.trim().parse::<u32>().ok())
                                    .collect();
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    for r in 0..7 {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 1.0;
                                            for c in 0..7 {
                                                let n = (r * 7 + c + 1) as u32;
                                                if pick_ball(ui, n, used.contains(&n), 18.0) {
                                                    if let Some(slot) = self.draw_inputs[i]
                                                        .iter_mut()
                                                        .find(|s| s.trim().is_empty())
                                                    {
                                                        *slot = format!("{n:02}");
                                                        self.draw_error = None;
                                                        self.draw_paste_error[i] = None;
                                                    }
                                                }
                                            }
                                        });
                                    }
                                });
                            });
                        });
                    ui.add_space(6.0);
                }
                    });

                if let Some(e) = &self.draw_error {
                    ui.label(RichText::new(e).size(11.5).color(RED));
                }
                ui.add_space(4.0);

                let checks: Vec<DrawRowCheck> =
                    self.draw_inputs.iter().map(|r| check_draw_row(r)).collect();
                let can_confirm = checks.iter().all(|c| c.error.is_none());
                let any_filled = checks.iter().any(|c| c.filled > 0);
                let summary: Vec<&str> = NAMES
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| checks[*i].filled > 0)
                    .map(|(_, n)| *n)
                    .collect();

                ui.horizontal(|ui| {
                    let label = if any_filled {
                        "确认对奖"
                    } else {
                        "清空开奖并保存"
                    };
                    let fill = if can_confirm {
                        GREEN
                    } else {
                        Color32::from_rgb(0x94, 0xA3, 0xB8)
                    };
                    let hover = if can_confirm { GREEN_HOVER } else { fill };
                    let active = if can_confirm { GREEN_ACTIVE } else { fill };
                    if styled_button_enabled(
                        ui,
                        RichText::new(label).size(13.0).color(Color32::WHITE),
                        can_confirm,
                        fill,
                        hover,
                        active,
                        Stroke::NONE,
                        Stroke::NONE,
                        CornerRadius::same(6),
                        Vec2::new(112.0, 30.0),
                    )
                    .clicked()
                    {
                        confirm = true;
                    }
                    if ui.add(tiny_button("清空全部", RED)).clicked() {
                        clear_all = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(tiny_button("关闭", SUB)).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if can_confirm {
                        if any_filled {
                            ui.label(
                                RichText::new(format!("将保存：{}", summary.join("、")))
                                    .size(10.5)
                                    .color(SUB),
                            );
                        } else {
                            ui.label(
                                RichText::new("将清空已保存的开奖号码。")
                                    .size(10.5)
                                    .color(SUB),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("有地区还没填完整或号码有误，修正后才能确认。")
                                .size(10.5)
                                .color(RED),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("两位数字自动跳格，回车下一格，Ctrl+Enter 确认。")
                            .size(10.0)
                            .color(SUB),
                    );
                });
            });

        if let Some(id) = focus_after {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if clear_all {
            self.draw_inputs = Default::default();
            self.draw_paste = Default::default();
            self.draw_paste_error = Default::default();
            self.draw_error = None;
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }
        if confirm {
            let checks: Vec<DrawRowCheck> =
                self.draw_inputs.iter().map(|r| check_draw_row(r)).collect();
            if checks.iter().any(|c| c.error.is_some()) {
                self.draw_error = Some("还有地区没填完整或号码有误，请先修正。".to_string());
            } else {
                match parse_draw_inputs(&self.draw_inputs) {
                    Ok(d) => {
                        self.draws = d;
                        draw::save(&self.draws);
                        self.draw_open = false;
                        self.draw_error = None;
                    }
                    Err(e) => self.draw_error = Some(e),
                }
            }
        }
        if close || !open {
            self.draw_open = false;
        }
    }

    /// 独立窗口「引用列表」：只展示引用类型消息，引用不进入注单列表。
    /// 「信息码」弹窗：左侧录入，右侧看所有信息码组合被当前注单踩中的金额。
    fn info_code_dialog(&mut self, ctx: &egui::Context) {
        if !self.info_code_open {
            return;
        }
        let sep = self.info_mul_sep();
        let parsed = crate::info_code::parse_input(
            &self.info_code_draft,
            &self.keywords,
            self.info_code_default_region,
            &self.info_code_default_play,
            &sep,
        );
        let can_add = !parsed.is_empty() && parsed.iter().all(|l| l.ok);
        let rows = self.info_window_rows();
        let hit_count = rows.iter().filter(|(_, a, _)| *a > 0.0).count();
        let total_count = rows.len();
        let hit_amount: f64 = rows.iter().map(|(_, a, _)| *a).sum();
        let only_hit = self.info_code_only_hit;
        let visible_rows: Vec<(crate::info_code::CodeKey, f64, usize)> = rows
            .iter()
            .filter(|(_, amount, _)| !only_hit || *amount > 0.0)
            .cloned()
            .collect();

        let mut open = true;
        let mut close = false;
        let mut add = false;
        let mut clear = false;

        // 没有标题栏也要能拖动：默认位置放在屏幕中央，之后由用户拖走。
        let default_pos =
            ctx.content_rect().center() - Vec2::new(470.0, 310.0);
        let window_frame = egui::Frame::window(&ctx.global_style())
            // 左面板左缘 / 右面板右缘距窗口边缘各 3px。
            .inner_margin(Margin::symmetric(3, 3));
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        egui::Window::new("信息码")
            .open(&mut open)
            .frame(window_frame)
            .default_pos(default_pos)
            .collapsible(false)
            .resizable(false)
            .fixed_size([600.0, 700.0])
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "把信息码粘到左边；注单踩中后会在消息列表标黄，并列出命中的组合和金额。",
                    )
                    .size(11.5)
                    .color(SUB),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "命中 {hit_count} / 共 {total_count} 条组合"
                        ))
                        .size(10.5)
                        .color(if hit_count > 0 { GOLD_FG } else { MUTED }),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("命中金额 {}", fmt_money(hit_amount)))
                                .size(10.5)
                                .color(if hit_amount > 0.0 { GOLD_FG } else { MUTED }),
                        );
                    });
                });
                ui.add_space(8.0);

                ui.horizontal_top(|ui| {
                    // 面板之间的间距完全由 add_space 控制，避免默认 8px 叠在 3px 上。
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // 左：录入
                    let panel_h = ui.available_height();
                    ui.allocate_ui_with_layout(
                        Vec2::new(272.0, panel_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                          egui::Frame::new()
                            .fill(STRIPE)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                            let inner_h = ui.available_height();
                            ui.set_width(ui.available_width());
                            ui.set_min_height(inner_h);
                            ui.label(
                                RichText::new("我录的信息码").size(12.0).strong().color(TEXT),
                            );
                            ui.label(
                                RichText::new(format!("已录 {} 条", self.info_codes.len()))
                                    .size(10.5)
                                    .color(SUB),
                            );
                            ui.add_space(6.0);
                            // 下拉框按钮压到 20 高，但展开后的候选列表给足高度，
                            // 否则（尤其「默认类型」）只能看到一两项。
                            ui.scope(|ui| {
                                ui.spacing_mut().interact_size.y = 20.0;
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.label(
                                        RichText::new("默认地区").size(10.5).color(SUB),
                                    );
                                    egui::ComboBox::from_id_salt("info_code_region")
                                        .width(70.0)
                                        .height(240.0)
                                        .selected_text(
                                            RichText::new(
                                                crate::odds::REGION_LABELS
                                                    [self.info_code_default_region.min(2)],
                                            )
                                            .size(10.5),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (i, name) in
                                                crate::odds::REGION_LABELS.iter().enumerate()
                                            {
                                                ui.selectable_value(
                                                    &mut self.info_code_default_region,
                                                    i,
                                                    RichText::new(*name).size(10.5),
                                                );
                                            }
                                        });
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("默认类型").size(10.5).color(SUB),
                                    );
                                    let play_text = if self.info_code_default_play.is_empty() {
                                        "不指定".to_string()
                                    } else {
                                        self.info_code_default_play.clone()
                                    };
                                    egui::ComboBox::from_id_salt("info_code_play")
                                        .width(70.0)
                                        .height(240.0)
                                        .selected_text(
                                            RichText::new(play_text).size(10.5),
                                        )
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.info_code_default_play,
                                                String::new(),
                                                RichText::new("不指定").size(10.5),
                                            );
                                            for p in [
                                                "特码", "三中三", "二中二", "三中二",
                                                "特碰", "平特", "平码",
                                            ] {
                                                ui.selectable_value(
                                                    &mut self.info_code_default_play,
                                                    p.to_string(),
                                                    RichText::new(p).size(10.5),
                                                );
                                            }
                                        });
                                });
                            });
                            ui.add_space(6.0);
                            // 文本框限高并在内部滚动：内容再多也不会把窗口撑高，
                            // 下面的「添加 / 清空信息码」按钮始终可见。
                            // 预留 210px 给提示、错误行列表、按钮和提示语。
                            let box_h = (ui.available_height() - 210.0).clamp(120.0, 460.0);
                            let rows = ((box_h / 14.5) as usize).max(6);
                            let draft_id = egui::Id::unique("info_code_draft");
                            egui::ScrollArea::vertical()
                                .id_salt("info_code_draft_scroll")
                                .max_height(box_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            // 和主页原文一样：右键会折叠选区，先把选区记下来，
                            // 菜单存续期间再恢复，复制 / 剪切 / 插入拖才有内容可用。
                            let prev_selection =
                                egui::text_edit::TextEditState::load(ui.ctx(), draft_id)
                                    .and_then(|s| s.cursor.char_range())
                                    .filter(|r| !r.is_empty());
                            if ui.input(|i| i.pointer.secondary_pressed()) {
                                self.info_sel_snapshot =
                                    prev_selection.map(|r| (draft_id, r));
                            }
                            let mut edit_out = egui::TextEdit::multiline(
                                &mut self.info_code_draft,
                            )
                            .id(draft_id)
                            .desired_rows(rows)
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::proportional(12.0))
                            .hint_text(
                                RichText::new(
                                    "一行一条，可一次粘贴多条，例如：\n三中三 01 02 03 04\n特码 12",
                                )
                                .size(12.0),
                            )
                            .show(ui);
                            let saved_selection = self
                                .info_sel_snapshot
                                .as_ref()
                                .filter(|(id, _)| *id == draft_id)
                                .map(|(_, range)| *range);
                            if let Some(sel) = saved_selection {
                                if edit_out
                                    .state
                                    .cursor
                                    .char_range()
                                    .is_none_or(|r| r.is_empty())
                                {
                                    edit_out.state.cursor.set_char_range(Some(sel));
                                    edit_out.state.clone().store(ui.ctx(), draft_id);
                                }
                            }
                            let sel_range = edit_out
                                .state
                                .cursor
                                .char_range()
                                .or_else(|| {
                                    self.info_sel_snapshot
                                        .as_ref()
                                        .filter(|(id, _)| *id == draft_id)
                                        .map(|(_, range)| *range)
                                });
                            let sel_text = sel_range
                                .map(|r| r.slice_str(&self.info_code_draft).to_string())
                                .unwrap_or_default();
                            let has_sel = !sel_text.trim().is_empty();
                            let has_any_sel =
                                sel_range.is_some_and(|r| !r.is_empty());
                            let can_drag = has_sel
                                && convert::insert_drag_text(
                                    &sel_text,
                                    &self.keywords,
                                )
                                .is_some();
                            let mul_sep = self.info_mul_sep();
                            let menu_was_open =
                                edit_out.response.context_menu_opened();
                            edit_out.response.context_menu(|ui| {
                                compact_context_menu(ui);
                                if ui
                                    .add_enabled(
                                        has_sel,
                                        egui::Button::new("复制")
                                            .shortcut_text("Ctrl+C"),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::Copy,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                                if ui
                                    .add(
                                        egui::Button::new("粘贴")
                                            .shortcut_text("Ctrl+V"),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::Paste,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        has_sel,
                                        egui::Button::new("剪切")
                                            .shortcut_text("Ctrl+X"),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::Cut,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .add(
                                        egui::Button::new("全选")
                                            .shortcut_text("Ctrl+A"),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::SelectAll,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .add_enabled(
                                        can_drag,
                                        egui::Button::new(format!("插入{mul_sep}")),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::InsertDrag,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        has_any_sel && !sel_text.contains('\n'),
                                        egui::Button::new("批量回车"),
                                    )
                                    .clicked()
                                {
                                    self.pending_info_action = Some((
                                        draft_id,
                                        SourceMenuAction::BatchNewline,
                                        sel_range,
                                    ));
                                    ui.close();
                                }
                            });
                            if let Some((id, action, range)) =
                                self.pending_info_action.take()
                            {
                                self.info_menu_action(ui.ctx(), id, action, range);
                            }
                            if ui.input(|i| i.pointer.primary_pressed())
                                && !menu_was_open
                                && !ui.input(|i| i.pointer.secondary_pressed())
                            {
                                self.info_sel_snapshot = None;
                            }
                                });
                            ui.label(
                                RichText::new(
                                    "不用写金额。没写地区的按左边选的地区算，「默认类型」只补给没写类型的行。",
                                )
                                .size(10.5)
                                .color(MUTED),
                            );
                            ui.add_space(4.0);
                            if !parsed.is_empty() {
                                egui::ScrollArea::vertical()
                                    .id_salt("info_code_input_lines")
                                    .max_height(110.0)
                                    .show(ui, |ui| {
                                        for l in &parsed {
                                            let color = if l.ok { GREEN_DEEP } else { RED };
                                            let text = if l.ok {
                                                format!(
                                                    "第 {} 行  ✓ {} 组合",
                                                    l.line, l.combos
                                                )
                                            } else {
                                                format!(
                                                    "第 {} 行  ✗ {}",
                                                    l.line, l.reason
                                                )
                                            };
                                            ui.label(
                                                RichText::new(text).size(10.5).color(color),
                                            );
                                        }
                                    });
                            }
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if primary_button(
                                    ui,
                                    RichText::new("添加")
                                        .size(10.5)
                                        .color(Color32::WHITE),
                                    can_add,
                                    CornerRadius::same(6),
                                    Vec2::new(64.0, 24.0),
                                )
                                .clicked()
                                {
                                    add = true;
                                }
                                ui.add_space(6.0);
                                if self.info_code_confirm_clear {
                                    if info_small_button(ui, "确认清空", RED).clicked() {
                                        clear = true;
                                        self.info_code_confirm_clear = false;
                                    }
                                    if info_small_button(ui, "取消", SUB).clicked() {
                                        self.info_code_confirm_clear = false;
                                    }
                                } else if info_small_button(ui, "清空信息码", RED).clicked()
                                {
                                    self.info_code_confirm_clear = true;
                                }
                            });
                            if let Some(msg) = &self.info_code_msg {
                                ui.label(RichText::new(msg).size(10.5).color(GOLD_FG));
                            }
                            });
                        },
                    );
                    // 左右面板间 3px。
                    ui.add_space(3.0);
                    // 右：合并后的命中表
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), panel_h),
                        Layout::top_down(Align::Min),
                        |ui| {
                          egui::Frame::new()
                            .fill(CARD)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                            let inner_h = ui.available_height();
                            ui.set_width(ui.available_width());
                            ui.set_min_height(inner_h);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("命中详情（所有信息码合并）")
                                        .size(12.0)
                                        .strong()
                                        .color(TEXT),
                                );
                                ui.add_space(8.0);
                                ui.scope(|ui| {
                                    let mut style = (**ui.style()).clone();
                                    style.text_styles.insert(
                                        egui::TextStyle::Body,
                                        FontId::proportional(10.5),
                                    );
                                    style.text_styles.insert(
                                        egui::TextStyle::Button,
                                        FontId::proportional(10.5),
                                    );
                                    ui.set_style(style);
                                    ui.checkbox(
                                        &mut self.info_code_only_hit,
                                        "只查看命中项",
                                    );
                                });
                            });
                            ui.add_space(4.0);
                            egui::ScrollArea::both()
                                .id_salt("info_code_table")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    egui::Grid::new("info_code_grid")
                                        .num_columns(5)
                                        .spacing([10.0, 4.0])
                                        .striped(true)
                                        .show(ui, |ui| {
                                            for (i, h) in [
                                                "地区", "玩法", "组合", "命中金额",
                                                "命中单数",
                                            ]
                                            .into_iter()
                                            .enumerate()
                                            {
                                                let text = RichText::new(h)
                                                    .size(10.5)
                                                    .strong()
                                                    .color(REF_NAVY);
                                                if i >= 3 {
                                                    // 固定列宽，避免右对齐布局把表格撑到可视区外，
                                                    // 导致「命中单数」这一列看着像不在表里。
                                                    // 「命中金额」「命中单数」四个字都占
                                                    // 约 52px，列宽必须够，否则文字会
                                                    // 溢出到相邻列，看起来像不在表里。
                                                    ui.allocate_ui_with_layout(
                                                        Vec2::new(56.0, 18.0),
                                                        Layout::right_to_left(
                                                            Align::Center,
                                                        ),
                                                        |ui| {
                                                            ui.label(text);
                                                        },
                                                    );
                                                } else {
                                                    ui.label(text);
                                                }
                                            }
                                            ui.end_row();
                                            for (key, amount, n) in &visible_rows {
                                                let color = if *amount > 0.0 {
                                                    GOLD_FG
                                                } else {
                                                    MUTED
                                                };
                                                ui.label(
                                                    RichText::new(key.region_char())
                                                        .size(10.5)
                                                        .color(color),
                                                );
                                                ui.label(
                                                    RichText::new(&key.play)
                                                        .size(10.5)
                                                        .color(color),
                                                );
                                                ui.label(
                                                    RichText::new(&key.key)
                                                        .size(10.5)
                                                        .color(color),
                                                );
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(56.0, 18.0),
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        let mut text = RichText::new(
                                                            fmt_money(*amount),
                                                        )
                                                        .size(10.5)
                                                        .color(color);
                                                        if *amount > 0.0 {
                                                            text = text.strong();
                                                        }
                                                        ui.label(text);
                                                    },
                                                );
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(56.0, 18.0),
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        ui.label(
                                                            RichText::new(n.to_string())
                                                                .size(10.5)
                                                                .color(color),
                                                        );
                                                    },
                                                );
                                                ui.end_row();
                                            }
                                        });
                                    if visible_rows.is_empty() {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(if rows.is_empty() {
                                                "还没有信息码"
                                            } else {
                                                "暂无命中项"
                                            })
                                            .size(11.0)
                                            .color(MUTED),
                                        );
                                    }
                                });
                            });
                        },
                    );
                });
            });

        if add {
            let now = chrono::Local::now().timestamp_millis();
            let mut added = 0;
            for l in parsed.iter().filter(|l| l.ok) {
                self.info_codes.push(crate::info_code::InfoCodeEntry {
                    text: l.text.clone(),
                    at: now,
                });
                added += 1;
            }
            crate::info_code::save(&self.info_codes);
            self.rebuild_info_codes();
            self.info_code_draft.clear();
            self.info_code_msg = Some(format!("已添加 {added} 条信息码"));
        }
        if clear {
            self.info_codes.clear();
            crate::info_code::save(&self.info_codes);
            self.rebuild_info_codes();
            self.info_code_msg = Some("已清空信息码".to_string());
        }
        if close || !open {
            self.info_code_open = false;
        }
    }

    fn reference_window(&mut self, ctx: &egui::Context) {
        if !self.reference_open {
            return;
        }
        let popup_frames = self.reference_popup;
        let popup = popup_frames > 0;
        if popup {
            self.reference_popup = popup_frames - 1;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("引用列表")
            .with_inner_size([420.0, 360.0])
            .with_min_inner_size([420.0, 360.0]);
        let mut keep_open = true;
        let uid = egui::ViewportId::from_hash_of("reference_list");
        ctx.show_viewport_immediate(uid, builder, |ui, _class| {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(BG)
                        .inner_margin(Margin::symmetric(12, 10)),
                )
                .show(ui, |ui| self.reference_ui(ui));
            if popup {
                // 新引用到达：即使窗口被最小化 / 被别的窗口盖住，也恢复到前台。
                // 连续发几帧，覆盖「视口刚创建、第一帧命令还没生效」的情况。
                let ctx = ui.ctx();
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                if popup_frames == 8 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                ctx.request_repaint();
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                keep_open = false;
            }
        });
        if !keep_open {
            self.reference_open = false;
            self.reference_popup = 0;
        }
    }

    fn reference_ui(&mut self, ui: &mut egui::Ui) {
      
        ui.label(
            RichText::new(
                "引用类型的消息不会进入注单列表；这里保留引用内容与被引用原文，可一键定位回注单列表。",
            )
            .size(11.0)
            .color(SUB),
        );
        ui.add_space(6.0);

        let mut clear = false;
        ui.horizontal(|ui| {
            let mut resp = ui.add(
                egui::TextEdit::singleline(&mut self.reference_search)
                    .font(FontId::proportional(11.0))
                    .margin(Margin::symmetric(4, 1))
                    .align(Align2::LEFT_CENTER)
                    .hint_text(RichText::new("搜索引用 / 被引用原文 / 发送人").size(9.0))
                    .desired_width(260.0),
            );
            text_edit_context_menu(ui.ctx(), resp.id, &mut self.reference_search, &mut resp);
            if !self.reference_search.is_empty()
                && ui.add(tiny_button("清除搜索", SUB)).clicked()
            {
                self.reference_search.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(tiny_button("清空引用列表", RED))
                    .on_hover_text("只清空当前窗口里的引用，不影响微信消息")
                    .clicked()
                {
                    clear = true;
                }
            });
        });
        if clear {
            self.references.clear();
            self.reference_keys.clear();
        }

        if let Some(msg) = &self.reference_msg {
            ui.label(
                RichText::new(msg)
                    .size(10.5)
                    .color(if self.reference_msg_error { RED } else { MUTED }),
            );
        }

        ui.add_space(1.0);
        egui::CollapsingHeader::new(RichText::new("过滤设置").size(11.0))
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "原文（去掉首尾空白后）完全等于下面任意一行时，不导入注单列表；引用类型消息不受影响。默认忽略「1」「2」。",
                    )
                    .size(10.5)
                    .color(SUB),
                );
                let mut resp = ui.add(
                    egui::TextEdit::multiline(&mut self.reference_ignore_edit)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("每行一个，例如：1\n2"),
                );
                text_edit_context_menu(
                    ui.ctx(),
                    resp.id,
                    &mut self.reference_ignore_edit,
                    &mut resp,
                );
                ui.horizontal(|ui| {
                    if ui.add(tiny_button("保存规则", TEXT)).clicked() {
                        self.reference_filter =
                            crate::reference::Filter::from_lines(&self.reference_ignore_edit);
                        crate::reference::save(&self.reference_filter);
                        self.reference_msg =
                            Some("已保存；重新导入后按新规则过滤。".to_string());
                        self.reference_msg_error = false;
                    }
                    if ui.add(tiny_button("恢复默认 1 / 2", SUB)).clicked() {
                        self.reference_filter = crate::reference::Filter::default();
                        self.reference_ignore_edit = self.reference_filter.to_lines();
                        crate::reference::save(&self.reference_filter);
                        self.reference_msg =
                            Some("已恢复默认；重新导入后生效。".to_string());
                        self.reference_msg_error = false;
                    }
                });
                ui.label(
                    RichText::new(
                        "过滤规则保存后对下一次导入生效；引用类型消息固定只进入本列表。",
                    )
                    .size(10.0)
                    .color(SUB),
                );
            });

        ui.add_space(1.0);
        hairline(ui);
        ui.add_space(6.0);

        let query = self.reference_search.trim().to_lowercase();
        let rows: Vec<MsgRow> = self
            .references
            .iter()
            .rev()
            .filter(|r| {
                query.is_empty()
                    || r.content.to_lowercase().contains(&query)
                    || r.nick.to_lowercase().contains(&query)
                    || r.quote.as_ref().is_some_and(|q| {
                        q.content.to_lowercase().contains(&query)
                            || q.from.to_lowercase().contains(&query)
                    })
            })
            .cloned()
            .collect();

        if rows.is_empty() {
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if self.references.is_empty() {
                        "暂无引用消息"
                    } else {
                        "没有匹配的引用"
                    })
                    .size(12.0)
                    .color(SUB),
                );
                if self.references.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("导入注单后，引用消息会自动出现在这里。")
                            .size(10.5)
                            .color(MUTED),
                    );
                }
            });
            return;
        }

        let to_top = std::mem::take(&mut self.reference_scroll_top);
        let mut scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("reference_list_scroll");
        if to_top {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        let mut locate: Option<MsgRow> = None;
        let mut delete: Option<MsgRow> = None;
        scroll.show(ui, |ui| {
            for row in &rows {
                let action = reference_card(ui, row);
                if action.locate {
                    locate = Some(row.clone());
                }
                if action.delete {
                    delete = Some(row.clone());
                }
                ui.add_space(6.0);
            }
        });
        if let Some(row) = delete {
            if self.delete_quoted_message(&row, ui.ctx()) {
                // 按需求：删除成功后，连同引用列表里的这条引用一起移除；
                // 后台补读删除时等 poll_quote_fetch 成功后再移除。
                self.remove_reference(&row);
            }
        }
        if let Some(row) = locate {
            self.locate_quoted_message(&row, ui.ctx());
        }
    }

    /// 在注单列表里找出最像「被引用消息」的那一行。
    fn find_quoted_order(&self, row: &MsgRow) -> Option<usize> {
        let quote = row.quote.as_ref()?;
        if quote.msg_type == 3 {
            return find_quoted_image_index(&self.orders, row, quote);
        }
        if compact_text(quote.content.trim()).is_empty() {
            return None;
        }
        let quote_time = if quote.create_time > 0 {
            chrono::Local
                .timestamp_opt(quote.create_time, 0)
                .single()
                .map(|t| format!("{}:{:02}", t.hour(), t.minute()))
        } else {
            None
        };
        let mut best: Option<(i32, usize)> = None;
        for (i, order) in self.orders.iter().enumerate() {
            let Some(score) =
                quote_match_score(order, &row.chat, quote, quote_time.as_deref())
            else {
                continue;
            };
            if best.is_none_or(|(best_score, _)| score > best_score) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// 删除被引用的那条注单消息（只影响当前列表，重新导入会恢复）。
    ///
    /// 返回 `true` 表示已经删掉；返回 `false` 表示没删（找不到，或正在后台补读，
    /// 调用方不要急着把引用从列表里移除）。
    fn delete_quoted_message(&mut self, row: &MsgRow, ctx: &egui::Context) -> bool {
        if let Some(index) = self.find_quoted_order(row) {
            let target = self.orders[index].clone();
            self.delete_order(&target);
            self.reference_msg = Some(format!(
                "已删除被引用消息（{} {}）",
                target.time, target.nick
            ));
            self.reference_msg_error = false;
            return true;
        }
        // 和定位一样：列表里匹配不上时回微信库补读再删（原文可能被编辑过，
        // 或者正文被引用截断，只看正文会漏掉）。
        self.start_quote_fetch(row, true, ctx);
        false
    }

    /// 把一条引用从引用列表里移除。
    fn remove_reference(&mut self, row: &MsgRow) {
        let key = (
            row.sort,
            row.chat.clone(),
            row.nick.clone(),
            row.content.clone(),
        );
        self.references.retain(|r| {
            !(r.sort == key.0
                && r.chat == key.1
                && r.nick == key.2
                && r.content == key.3)
        });
        self.reference_keys.remove(&key);
    }

    /// 从微信消息库补读被引用原文（定位 / 删除共用）。
    ///
    /// 返回 `false` 表示没启动后台读取（没有时间 / 数据未加载 / 已有任务在跑），
    /// 已经在 `reference_msg` 里给出原因。
    fn start_quote_fetch(
        &mut self,
        row: &MsgRow,
        for_delete: bool,
        ctx: &egui::Context,
    ) -> bool {
        if self.quote_fetch_rx.is_some() {
            self.reference_msg = Some("正在读取上一条被引用消息，请稍候…".to_string());
            self.reference_msg_error = false;
            return false;
        }
        let Some(quote) = row.quote.clone() else {
            self.reference_msg = Some("这条引用没有可定位的原文。".to_string());
            self.reference_msg_error = true;
            return false;
        };
        let State::Ready(data) = &self.state else {
            self.reference_msg = Some("数据尚未加载完成，无法读取被引用原文".to_string());
            self.reference_msg_error = true;
            return false;
        };
        if quote.create_time <= 0 {
            self.reference_msg = Some(format!(
                "未在注单列表中找到被引用消息：{}",
                truncate_display(quote.content.trim(), 24)
            ));
            self.reference_msg_error = true;
            return false;
        }
        // 注单列表只包含本次导入的会员 / 时段；被引用原文可能是链接、占位消息、
        // 其它群成员或更早的消息，本来就不在列表里。直接回微信消息库补读。
        let chat = row.chat.clone();
        let is_group = chat.ends_with("@chatroom");
        let req = ImportRequest {
            account_dir: data.account_path.clone(),
            workdir: data.workdir.clone(),
            keys: data.keys.clone(),
            contacts: if is_group { Vec::new() } else { vec![chat.clone()] },
            groups: if is_group {
                vec![(chat.clone(), Vec::new())]
            } else {
                Vec::new()
            },
            name_of: build_name_map(data),
            start_ts: quote.create_time.saturating_sub(3),
            end_ts: quote.create_time.saturating_add(3),
        };
        let (tx, rx) = channel();
        self.quote_fetch_rx = Some(rx);
        self.quote_fetch_row = Some(row.clone());
        self.quote_fetch_delete = for_delete;
        self.reference_msg = Some("正在从微信读取被引用消息…".to_string());
        self.reference_msg_error = false;
        let ctx_repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = crate::messages::run(&req, &|_| {}).map(|r| r.rows);
            let _ = tx.send(result);
            // 补读是在引用窗口（子视口）里发起的，结果由主窗口轮询。
            ctx_repaint.request_repaint_of(egui::ViewportId::ROOT);
        });
        true
    }

    /// 在注单消息列表里找到并选中被引用的那条消息。
    ///
    /// 匹配顺序：正文完全一致 → 去掉空白后一致 → 正文包含；
    /// 再按同一会话、被引用发送人、时间做加权，避免群里同一句话多个人说过时选错。
    /// 注单列表里没有时，回微信消息库按被引用时间补读一次。
    fn locate_quoted_message(&mut self, row: &MsgRow, ctx: &egui::Context) {
        if let Some(index) = self.find_quoted_order(row) {
            self.select_order_index(index, ctx);
            return;
        }
        // 注单列表只包含本次导入的会员 / 时段；被引用原文可能是链接、占位消息、
        // 其它群成员或更早的消息，本来就不在列表里。直接回微信消息库补读。
        self.start_quote_fetch(row, false, ctx);
    }

    /// 选中注单列表里的第 `index` 条，并同步详情面板。
    fn select_order_index(&mut self, index: usize, ctx: &egui::Context) {
        let target_row = self.orders[index].clone();
        // 清掉成员筛选，保证目标行一定在可见列表里；分组过滤不要直接切回
        // 「全部分组」——当前分组已经包含目标行时保持不变；不包含时切到目标行
        // 所属的分组，只有它不属于任何分组（例如没有分组、或来源是「选定会员」）
        // 时才退回全部分组。
        self.member_focus = None;
        let keep_group = match self.order_filter.as_deref() {
            None => true,
            Some(name) => self
                .groups
                .iter()
                .find(|g| g.name == name)
                .is_some_and(|g| g.matches(&target_row)),
        };
        if !keep_group {
            self.order_filter = self
                .groups
                .iter()
                .find(|g| g.matches(&target_row))
                .map(|g| g.name.clone());
        }
        self.page = Page::Home;
        // 从「引用」页签定位回来时切回注单列表，用户才看得到选中行。
        self.order_tab = OrderTab::Active;
        let key = (
            target_row.sort,
            target_row.chat.clone(),
            target_row.nick.clone(),
            target_row.content.clone(),
        );
        let conv = self
            .auto_converted
            .get(&edit_key(&target_row))
            .cloned()
            .unwrap_or_else(|| {
                let mut conv = safe_convert(
                    &target_row.content,
                    &self.keywords,
                    &self.replacements,
                );
                self.apply_doubt_override(&edit_key(&target_row), &mut conv);
                conv
            });
        self.selected_order_key = Some(key);
        self.edit_time = target_row.time.clone();
        self.edit_text = target_row.content.clone();
        self.selected_order_row = Some(target_row.clone());
        self.converted = Some(conv);
        self.ocr_msg = None;
        self.scroll_to_order = true;
        self.reference_msg = Some(format!(
            "已定位到被引用消息（{} {}）",
            target_row.time, target_row.nick
        ));
        self.reference_msg_error = false;
        // 把主窗口拉到最前，用户能立刻看到选中行。
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Minimized(false),
        );
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Visible(true),
        );
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Focus,
        );
    }

    /// 补读结果：匹配到原文后选中注单行；原文不在列表里则弹窗展示。
    fn poll_quote_fetch(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.quote_fetch_rx else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(150));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("补读线程异常退出".to_string()),
        };
        self.quote_fetch_rx = None;
        let for_delete = std::mem::take(&mut self.quote_fetch_delete);
        let Some(row) = self.quote_fetch_row.take() else {
            return;
        };
        let Some(quote) = row.quote.clone() else {
            return;
        };
        let rows = match result {
            Ok(rows) => rows,
            Err(e) => {
                self.reference_msg = Some(format!("读取被引用消息失败：{e}"));
                self.reference_msg_error = true;
                return;
            }
        };
        let Some(msg) = find_quote_in_rows(&rows, &row, &quote) else {
            self.reference_msg = Some(format!(
                "未在微信消息库中找到被引用消息：{}",
                truncate_display(quote.content.trim(), 24)
            ));
            self.reference_msg_error = true;
            return;
        };
        // 补读的消息如果本来就在注单列表里（只是正文被改过等原因没匹配上），
        // 按 sort + 会话 + 发送人认回那一行。
        let order_index = self
            .orders
            .iter()
            .position(|o| o.sort == msg.sort && o.chat == msg.chat && o.sender == msg.sender);
        if let Some(index) = order_index {
            if for_delete {
                let target = self.orders[index].clone();
                self.delete_order(&target);
                self.remove_reference(&row);
                self.reference_msg = Some(format!(
                    "已删除被引用消息（{} {}）",
                    target.time, target.nick
                ));
                self.reference_msg_error = false;
            } else {
                self.select_order_index(index, ctx);
            }
            return;
        }
        if for_delete {
            self.reference_msg = Some(format!(
                "被引用原文不在当前注单列表，无法删除：{}",
                truncate_display(quote.content.trim(), 24)
            ));
            self.reference_msg_error = true;
            return;
        }
        self.reference_msg = Some(format!(
            "已找到被引用原文（{} {}），它不在当前注单列表，已在弹窗中显示",
            msg.time, msg.nick
        ));
        self.reference_msg_error = false;
        self.quote_original = Some(msg);
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Minimized(false),
        );
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Visible(true),
        );
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Focus,
        );
    }

    /// 补读到、但不在注单列表里的被引用原文。
    fn quote_original_dialog(&mut self, ctx: &egui::Context) {
        let Some(row) = self.quote_original.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("被引用原文")
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} {}", row.time, row.nick))
                            .size(11.5)
                            .color(SUB),
                    );
                    ui.label(RichText::new(&row.kind).size(11.0).color(MUTED));
                });
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(STRIPE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_min_width(380.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&row.content).size(12.5).color(TEXT),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "该消息不在当前注单列表（可能是链接、占位消息，或不在本次导入的时段 / 会员范围内）。",
                    )
                    .size(10.5)
                    .color(MUTED),
                );
            });
        if !open {
            self.quote_original = None;
        }
    }

    /// 「批量替换」弹窗：把原文里的查找串全部换成输入的内容。
    fn batch_replace_dialog(&mut self, ctx: &egui::Context) {
        if !self.batch_replace_open {
            return;
        }
        // 弹窗开着时切换了注单：不要误改另一条，直接关掉。
        if let Some(target) = &self.batch_replace_target {
            let same = self.selected_order_row.as_ref().is_some_and(|r| {
                let key = edit_key(r);
                key.0 == target.0 && key.1 == target.1 && key.2 == target.2
            });
            if !same {
                self.batch_replace_open = false;
                self.batch_replace_find.clear();
                self.batch_replace_to.clear();
                self.batch_replace_target = None;
                return;
            }
        }
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let count = if self.batch_replace_find.is_empty() {
            0
        } else {
            self.edit_text.matches(&self.batch_replace_find).count()
        };

        egui::Window::new("批量替换")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && count > 0 {
                    confirm = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                ui.label(RichText::new("查找内容").size(12.0).color(SUB));
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xF5, 0xF6, 0xF7))
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.batch_replace_find)
                                    .size(13.0)
                                    .color(TEXT),
                            )
                            .wrap()
                            .selectable(false),
                        );
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("替换为（全文共 {count} 处）"))
                        .size(12.0)
                        .color(SUB),
                );
                let input_id = egui::Id::unique("batch_replace_input");
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.batch_replace_to)
                        .id(input_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("留空表示删除"),
                );
                text_edit_context_menu(
                    ui.ctx(),
                    resp.id,
                    &mut self.batch_replace_to,
                    &mut resp,
                );
                if ui.memory(|m| m.focused()).is_none() {
                    ui.memory_mut(|m| m.request_focus(input_id));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(
                            ui,
                            RichText::new("替换").size(13.0).color(Color32::WHITE),
                            count > 0,
                            CornerRadius::same(14),
                            Vec2::new(80.0, 30.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(12), Vec2::new(60.0, 28.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });

        if confirm {
            let find = self.batch_replace_find.clone();
            let to = self.batch_replace_to.clone();
            if !find.is_empty() {
                self.edit_text = self.edit_text.replace(&find, &to);
                // 替换后把光标放到末尾，避免 TextEdit 用旧的字符下标。
                if let Some(row) = &self.selected_order_row {
                    let id = egui::Id::unique((
                        "order_source_edit",
                        row.sort,
                        row.chat.as_str(),
                        row.sender.as_str(),
                    ));
                    let mut state =
                        egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
                    state.cursor.set_char_range(Some(
                        egui::text::CCursorRange::one(egui::text::CCursor::new(
                            self.edit_text.chars().count(),
                        )),
                    ));
                    state.store(ctx, id);
                    ctx.memory_mut(|m| m.request_focus(id));
                }
            }
            self.batch_replace_open = false;
            self.batch_replace_find.clear();
            self.batch_replace_to.clear();
            self.batch_replace_target = None;
            self.source_sel_snapshot = None;
        } else if cancel || !open {
            self.batch_replace_open = false;
            self.batch_replace_find.clear();
            self.batch_replace_to.clear();
            self.batch_replace_target = None;
        }
    }

    // ---------------- 顶部栏 ----------------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let n_c = self.selected_contacts.len();
        let n_g = self.selected_groups.len();
        let n_m = self.selected_members.len();

        ui.horizontal(|ui| {
            match self.page {
                Page::Home => {
                    // 按功能分组，从左到右依次是：注单操作 → 对奖查看 → 规则设置。
                    top_group_label(ui, "注单", "选择会员、导入 / 清空注单与查看引用");
                    if top_button(ui, "选定会员", 70.0).clicked() {
                        self.page = Page::Select;
                    }
                    ui.add_space(2.0);
                    let ready = matches!(self.state, State::Ready(_));
                    if top_button_tone(ui, "导入注单", 70.0, ready, TOOLBAR_TONE_GREEN)
                        .on_hover_text("选择日期与时间段后导入消息")
                        .clicked()
                    {
                        self.dialog_error = None;
                        self.dialog_open = true;
                        self.clear_open = false;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                    ui.add_space(2.0);
                    if top_button_tone(ui, "清空注单", 70.0, ready, TOOLBAR_TONE_RED)
                        .on_hover_text("按会员清空注单；不勾选会员则清空全部")
                        .clicked()
                    {
                        self.dialog_open = false;
                        self.clear_open = true;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                    ui.add_space(2.0);
                    let ref_label = if self.references.is_empty() {
                        "引用列表".to_string()
                    } else {
                        format!("引用列表 {}", self.references.len())
                    };
                    if top_button(ui, &ref_label, 76.0)
                        .on_hover_text("在独立窗口查看引用类型消息")
                        .clicked()
                    {
                        self.reference_open = true;
                        self.reference_scroll_top = true;
                        self.reference_msg = None;
                        self.reference_msg_error = false;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }

                    top_group_gap(ui);
                    top_group_label(ui, "对奖", "录入开奖后查看总账与风险");
                    if top_button(ui, "开奖", 48.0)
                        .on_hover_text("录入开奖号码，用于总账对奖")
                        .clicked()
                    {
                        self.draw_inputs = draw_inputs_from(&self.draws);
                        self.draw_paste = Default::default();
                        self.draw_paste_error = Default::default();
                        self.draw_error = None;
                        self.draw_open = true;
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "总账", 48.0)
                        .on_hover_text("出账单：按会员汇总投注金额、中奖、回水和输赢")
                        .clicked()
                    {
                        self.page = Page::Ledger;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "风险", 48.0)
                        .on_hover_text("按号码 / 组合查看整区风险敞口")
                        .clicked()
                    {
                        self.page = Page::Risk;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }

                    top_group_gap(ui);
                    top_group_label(ui, "设置", "玩法关键词、替换词、赔率与信息码");
                    if top_button(ui, "关键词", 56.0)
                        .on_hover_text("玩法关键词 / 金额识别 / 区域 & 其他")
                        .clicked()
                    {
                        self.keywords_open = true;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "替换词", 56.0)
                        .on_hover_text("转换前把 A 词替换为 B 词")
                        .clicked()
                    {
                        self.replacements_open = true;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "赔率", 48.0)
                        .on_hover_text("设置默认 / 分组 / 会员的赔率与回水")
                        .clicked()
                    {
                        self.odds_draft = self.odds.clone();
                        self.odds_scope = OddsScope::Default;
                        self.odds_region = 0;
                        self.odds_new_member.clear();
                        self.odds_open = true;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "信息码", 56.0)
                        .on_hover_text("录入分析师给的组合；注单踩中时会标黄提醒")
                        .clicked()
                    {
                        // 弹窗已去掉右上角关闭按钮，工具栏按钮兼作开关。
                        let opening = !self.info_code_open;
                        self.info_code_open = opening;
                        if opening {
                            self.info_code_confirm_clear = false;
                            self.info_code_msg = None;
                            self.dialog_open = false;
                            self.clear_open = false;
                            self.keywords_open = false;
                            self.replacements_open = false;
                            self.reference_open = false;
                        }
                    }
                    ui.add_space(2.0);
                    if top_button(ui, "权限", 48.0)
                        .on_hover_text("查看当前账号、到期时间，修改密码 / 退出登录")
                        .clicked()
                    {
                        self.toggle_auth_dialog();
                    }
                }
                Page::Ledger => {
                    if top_button(ui, "←  返回主页", 84.0).clicked() {
                        self.page = Page::Home;
                    }
                    ui.add_space(10.0);
                    if top_button(ui, "开奖", 52.0).clicked() {
                        self.draw_inputs = draw_inputs_from(&self.draws);
                        self.draw_paste = Default::default();
                        self.draw_paste_error = Default::default();
                        self.draw_error = None;
                        self.draw_open = true;
                    }
                    ui.add_space(8.0);
                    if top_button(ui, "赔率", 52.0).clicked() {
                        self.odds_draft = self.odds.clone();
                        self.odds_scope = OddsScope::Default;
                        self.odds_region = 0;
                        self.odds_new_member.clear();
                        self.odds_open = true;
                    }
                    ui.add_space(8.0);
                    let ref_label = if self.references.is_empty() {
                        "引用列表".to_string()
                    } else {
                        format!("引用 {}", self.references.len())
                    };
                    if top_button(ui, &ref_label, 88.0).clicked() {
                        self.reference_open = true;
                        self.reference_scroll_top = true;
                        self.reference_msg = None;
                        self.reference_msg_error = false;
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("总账").size(14.0).strong().color(Color32::WHITE));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("按会员汇总当前注单，开奖后自动计算中奖 / 回水 / 输赢")
                            .size(11.5)
                            .color(TOOLBAR_SUB),
                    );
                }
                Page::Risk => {
                    if top_button(ui, "←  返回主页", 84.0).clicked() {
                        self.page = Page::Home;
                    }
                    ui.add_space(10.0);
                    if top_button(ui, "开奖", 52.0).clicked() {
                        self.draw_inputs = draw_inputs_from(&self.draws);
                        self.draw_paste = Default::default();
                        self.draw_paste_error = Default::default();
                        self.draw_error = None;
                        self.draw_open = true;
                    }
                    ui.add_space(8.0);
                    if top_button(ui, "赔率", 52.0).clicked() {
                        self.odds_draft = self.odds.clone();
                        self.odds_scope = OddsScope::Default;
                        self.odds_region = 0;
                        self.odds_new_member.clear();
                        self.odds_open = true;
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("风险").size(14.0).strong().color(Color32::WHITE));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("按号码 / 组合汇总整区投注、回水与风险敞口")
                            .size(11.5)
                            .color(TOOLBAR_SUB),
                    );
                }
                Page::Select => {
                    if top_button(ui, "←  返回主页", 84.0).clicked() {
                        self.page = Page::Home;
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!("已选 {n_c} 联系人  {n_g} 群聊  {n_m} 成员"))
                            .size(12.0)
                            .color(TOOLBAR_SUB),
                    );
                    ui.add_space(4.0);
                    if top_button_enabled(ui, "清空", 48.0, n_c + n_g + n_m > 0)
                        .clicked()
                    {
                        self.selected_contacts.clear();
                        self.selected_groups.clear();
                        self.selected_members.clear();
                        self.selection_dirty = true;
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // 账号 / 剩余时间已改到窗口标题（版本号后面），顶栏只保留读取状态。
                match &self.state {
                    State::Ready(_) => {}
                    State::Loading => {
                        ui.add_space(6.0);
                        ui.label(RichText::new(self.status.as_str()).size(11.0).color(TOOLBAR_SUB));
                    }
                    State::Failed(_) => {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("数据读取失败")
                                .size(11.0)
                                .color(Color32::from_rgb(0xFF, 0xB4, 0xAB)),
                        );
                    }
                }
            });
        });
    }

    // ---------------- 主页 ----------------

    fn home_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let h = ui.available_height();
        let total = ui.available_width();
        // 左侧成员条（≤6 个中文字符宽），注单面板，右侧详情面板；面板之间用细线隔开。
        let rail_w = 72.0;
        let line_w = 1.0;
        // 注单消息列表固定宽度：不随窗口缩放，时间列始终在同一条竖线上。
        let orders_w = 268.0;
        let detail_w = (total - rail_w - orders_w - line_w * 2.0).max(200.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                Vec2::new(rail_w, h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(rail_w, h));
                    self.member_rail(ui, data);
                },
            );
            separator_line(ui, h);
            ui.allocate_ui_with_layout(
                Vec2::new(orders_w, h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(orders_w, h));
                    self.orders_panel(ui, data);
                },
            );
            separator_line(ui, h);
            ui.allocate_ui_with_layout(
                Vec2::new(detail_w, h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(detail_w, h));
                    self.detail_panel(ui);
                },
            );
        });
    }

    /// 该会话 / 成员命中的、配置过赔率的分组（按分组创建顺序取第一个）。
    fn group_with_odds_for(&self, chat: &str, sender: &str) -> Option<String> {
        self.groups
            .iter()
            .find(|g| {
                self.odds.has_group(&g.name)
                    && (g.contacts.contains(chat)
                        || g.members.contains(&(chat.to_string(), sender.to_string())))
            })
            .map(|g| g.name.clone())
    }

    fn group_with_odds_for_member(&self, key: &MemberKey) -> Option<String> {
        self.groups
            .iter()
            .find(|g| {
                self.odds.has_group(&g.name)
                    && match key {
                        MemberKey::Contact(chat) => g.contacts.contains(chat),
                        MemberKey::Member(chat, member) => {
                            g.members.contains(&(chat.clone(), member.clone()))
                        }
                    }
            })
            .map(|g| g.name.clone())
    }

    /// 某条注单在指定地区的赔率：会员覆盖（来源身份 → 旧昵称）→ 分组 → 默认。
    fn odds_for_row(&self, row: &MsgRow, lottery: &str) -> &crate::odds::OddsSet {
        let group = self.group_with_odds_for(&row.chat, &row.sender);
        let identity = member_identity(&row.chat, &row.sender);
        self.odds.resolve_member(
            &identity,
            &row.nick,
            group.as_deref(),
            crate::odds::region_index(lottery),
        )
    }

    /// 详情面板正在展示的会员（注单详情优先，其次单条录入选中的会员）。
    fn detail_odds(&self, lottery: &str) -> &crate::odds::OddsSet {
        if let Some(row) = &self.selected_order_row {
            let group = self.group_with_odds_for(&row.chat, &row.sender);
            let identity = member_identity(&row.chat, &row.sender);
            return self
                .odds
                .resolve_member(
                    &identity,
                    &row.nick,
                    group.as_deref(),
                    crate::odds::region_index(lottery),
                );
        }
        if let Some(key) = &self.single_member {
            if let Some(choice) = self.single_members.iter().find(|c| &c.key == key) {
                let (chat, member) = match key {
                    MemberKey::Contact(c) => (c.as_str(), c.as_str()),
                    MemberKey::Member(chat, member) => (chat.as_str(), member.as_str()),
                };
                let identity = member_identity(chat, member);
                let group = self.group_with_odds_for_member(key);
                return self.odds.resolve_member(
                    &identity,
                    &choice.nick,
                    group.as_deref(),
                    crate::odds::region_index(lottery),
                );
            }
        }
        self.odds
            .resolve_member("", "", None, crate::odds::region_index(lottery))
    }

    /// 总账（出账单）：按会员汇总投注，并用「开奖」录入的号码自动对奖，算出
    /// 中奖 / 回水 / 输赢（赔率与回水按会员 / 分组 / 默认逐级取用）。
    /// 出账单：布局与 zwy_wails_v1 的 SettlementBillView 对齐。
    /// 按当前过滤结果构建出账单的会员块（总账 / 对账共用）。
    fn bill_blocks(&self, visible: &[usize]) -> Vec<LedgerBillBlock> {
        let mul_sep = self
            .keywords
            .get(crate::keywords::Field::Multiplication)
            .iter()
            .find(|k| !k.is_empty())
            .cloned()
            .unwrap_or_else(|| "拖".to_string());

        let mut blocks: BTreeMap<(String, String, String), LedgerBillBlock> = BTreeMap::new();
        for &i in visible {
            let row = &self.orders[i];
            let conv = self
                .auto_converted
                .get(&edit_key(row))
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = safe_convert(&row.content, &self.keywords, &self.replacements);
                    self.apply_doubt_override(&edit_key(row), &mut c);
                    c
                });
            let key = (row.nick.clone(), row.chat.clone(), row.sender.clone());
            let block = blocks.entry(key).or_insert_with(|| LedgerBillBlock {
                name: row.nick.clone(),
                rows: Vec::new(),
                amount: 0.0,
                win: 0.0,
                rebate: 0.0,
                win_stakes: BTreeMap::new(),
                win_amounts: BTreeMap::new(),
            });
            let no = block.rows.len() + 1;
            let mut br = LedgerBillRow {
                no,
                idx: i,
                amount: conv.total_stake,
                hk: 0.0,
                old: 0.0,
                win: 0.0,
                rebate: 0.0,
                special_win_stake: 0.0,
                breakdown: Vec::new(),
                cats: BTreeMap::new(),
            };
            let mut bd: BTreeMap<(String, u64), f64> = BTreeMap::new();
            let mut ws: BTreeMap<String, f64> = BTreeMap::new();
            let mut wa: BTreeMap<String, f64> = BTreeMap::new();
            for bet in &conv.bets {
                let ev = settle::evaluate(
                    bet,
                    self.draws.get(&bet.lottery),
                    &mul_sep,
                    self.odds_for_row(row, &bet.lottery),
                );
                let cat = bill_category(&bet.play);
                let slice = br.cats.entry(cat).or_default();
                slice.amount += bet.amount;
                // 金额按彩种拆出香港 / 老澳门的部分（澳门是合计的隐含余量）。
                match bet.lottery.as_str() {
                    "香港" => {
                        br.hk += bet.amount;
                        slice.hk += bet.amount;
                    }
                    "老澳门" => {
                        br.old += bet.amount;
                        slice.old += bet.amount;
                    }
                    _ => {}
                }
                slice.rebate += ev.rebate;
                if ev.hit {
                    slice.win += ev.win_amount;
                    slice.win_stake += ev.win_stake;
                    // 一注里可能拆出不同标签 / 赔率（三中二中2/中3、连肖含马、
                    // 平特 4/5 码、二中二带砍），逐段归集。
                    let parts: Vec<(String, f64, f64)> = if ev.parts.is_empty() {
                        vec![(ev.label.clone(), ev.win_stake, ev.odds)]
                    } else {
                        ev.parts.clone()
                    };
                    for (label, stake, odds) in &parts {
                        if *stake <= 0.0 {
                            continue;
                        }
                        slice.breakdown.push((
                            label.clone(),
                            *stake,
                            *odds,
                        ));
                        *ws.entry(label.clone()).or_default() += *stake;
                        *wa.entry(label.clone()).or_default() += stake * odds;
                        *bd.entry((label.clone(), (odds * 100.0).round() as u64))
                            .or_default() += *stake;
                        if label == "特码" {
                            br.special_win_stake += *stake;
                        }
                    }
                }
                br.win += ev.win_amount;
                br.rebate += ev.rebate;
            }
            for ((label, odds100), stake) in bd {
                br.breakdown.push((label, stake, odds100 as f64 / 100.0));
            }
            br.breakdown.sort_by_key(|(label, _, _)| play_order(label));
            block.amount += br.amount;
            block.win += br.win;
            block.rebate += br.rebate;
            for (k, v) in ws {
                *block.win_stakes.entry(k).or_default() += v;
            }
            for (k, v) in wa {
                *block.win_amounts.entry(k).or_default() += v;
            }
            block.rows.push(br);
        }
        let mut block_list: Vec<LedgerBillBlock> = blocks.into_values().collect();
        block_list.sort_by(|a, b| a.name.cmp(&b.name));

        block_list
    }

    /// 「导出图片」弹窗：选择要导出的投注人。
    fn bill_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.bill_export_open {
            return;
        }
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let mut select_all = false;
        let mut clear_all = false;
        let mut selected = self.bill_export_selected.clone();
        let candidates = self.bill_export_candidates.clone();

        egui::Window::new("导出图片")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(440.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("保存到").size(12.0).color(MUTED));
                    let mut resp = ui.add(
                        egui::TextEdit::singleline(&mut self.bill_export_dir)
                            .desired_width(320.0)
                            .hint_text("文件夹路径"),
                    );
                    text_edit_context_menu(
                        ui.ctx(),
                        resp.id,
                        &mut self.bill_export_dir,
                        &mut resp,
                    );
                });
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    let home = std::env::var_os("USERPROFILE")
                        .map(std::path::PathBuf::from);
                    if ui.add(tiny_button("图片", TEXT)).clicked() {
                        self.bill_export_dir = export_dir().display().to_string();
                    }
                    if let Some(h) = &home {
                        if ui.add(tiny_button("桌面", TEXT)).clicked() {
                            self.bill_export_dir =
                                h.join("Desktop").display().to_string();
                        }
                        if ui.add(tiny_button("文档", TEXT)).clicked() {
                            self.bill_export_dir =
                                h.join("Documents").display().to_string();
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "已选 {} / {} 个投注人",
                            selected.len(),
                            candidates.len()
                        ))
                        .size(12.0)
                        .color(MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(tiny_button("全不选", SUB)).clicked() {
                            clear_all = true;
                        }
                        if ui.add(tiny_button("全选", GREEN)).clicked() {
                            select_all = true;
                        }
                    });
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .id_salt("bill_export_list")
                    .show(ui, |ui| {
                        for name in &candidates {
                            let mut on = selected.contains(name);
                            if ui.checkbox(&mut on, name).changed() {
                                if on {
                                    selected.insert(name.clone());
                                } else {
                                    selected.remove(name);
                                }
                            }
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if blue_button(
                            ui,
                            RichText::new("导出图片")
                                .size(13.0)
                                .strong()
                                .color(Color32::WHITE),
                            !selected.is_empty(),
                            CornerRadius::same(10),
                            Vec2::new(96.0, 30.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ui.add(tiny_button("取消", SUB)).clicked() {
                            cancel = true;
                        }
                    });
                });
            });

        if select_all {
            selected = candidates.iter().cloned().collect();
        }
        if clear_all {
            selected.clear();
        }
        self.bill_export_selected = selected;
        if confirm && !self.bill_export_selected.is_empty() {
            let names = self.bill_export_selected.clone();
            let raw = self.bill_export_dir.trim().to_string();
            let dir = if raw.is_empty() {
                export_dir()
            } else {
                std::path::PathBuf::from(raw)
            };
            self.export_bill_images(&names, &dir);
            self.bill_export_open = false;
        } else if cancel || !open {
            self.bill_export_open = false;
        }
    }

    /// 用软件渲染把选中的会员块画成 PNG（不依赖窗口截图）。
    fn export_bill_images(
        &mut self,
        names: &BTreeSet<String>,
        dir: &std::path::Path,
    ) {
        let visible = self.visible_order_indices();
        let blocks = self.bill_blocks(&visible);
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.ledger_msg = Some(format!("导出失败：{e}"));
            return;
        }
        let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
        let mut saved = 0usize;
        let mut failed = 0usize;
        let mut last_err = String::new();
        for (idx, block) in blocks.iter().enumerate() {
            if !names.contains(&block.name) {
                continue;
            }
            let mut win_stakes: Vec<(String, f64)> = block
                .win_stakes
                .iter()
                .filter(|(_, v)| **v > 0.0)
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            win_stakes.sort_by_key(|(k, _)| play_order(k));
            let mut win_amounts: Vec<(String, f64)> = block
                .win_amounts
                .iter()
                .filter(|(_, v)| **v > 0.0)
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            win_amounts.sort_by_key(|(k, _)| play_order(k));
            let mut categories: Vec<bill_image::BillCategoryImage> = Vec::new();
            if self.ledger_category {
                let mut labels: Vec<&'static str> = block
                    .rows
                    .iter()
                    .flat_map(|r| r.cats.keys().copied())
                    .collect();
                labels.sort_by_key(|c| category_order(c));
                labels.dedup();
                for label in labels {
                    let mut rows: Vec<bill_image::BillRowImage> = Vec::new();
                    let mut total_stake = 0.0;
                    let mut total_win = 0.0;
                    let mut total_rebate = 0.0;
                    for row in &block.rows {
                        let Some(slice) = row.cats.get(label) else {
                            continue;
                        };
                        if slice.amount <= 0.0 && slice.win <= 0.0 {
                            continue;
                        }
                        rows.push(bill_image::BillRowImage {
                            no: row.no,
                            amount: slice.amount,
                            hk: slice.hk,
                            old: slice.old,
                            win: slice.win,
                            special_win_stake: 0.0,
                            breakdown: sorted_breakdown(&slice.breakdown),
                        });
                        total_stake += slice.amount;
                        total_win += slice.win;
                        total_rebate += slice.rebate;
                    }
                    if !rows.is_empty() {
                        categories.push(bill_image::BillCategoryImage {
                            label: label.to_string(),
                            rows,
                            total_stake,
                            total_win,
                            total_rebate,
                        });
                    }
                }
            }
            let data = bill_image::BillBlockImage {
                name: block.name.clone(),
                rows: block
                    .rows
                    .iter()
                    .map(|r| bill_image::BillRowImage {
                        no: r.no,
                        amount: r.amount,
                        hk: r.hk,
                        old: r.old,
                        win: r.win,
                        special_win_stake: r.special_win_stake,
                        breakdown: sorted_breakdown(&r.breakdown),
                    })
                    .collect(),
                total_stake: block.amount,
                total_win: block.win,
                total_rebate: block.rebate,
                win_stakes,
                win_amounts,
                category_view: self.ledger_category,
                categories,
            };
            match bill_image::save_block(&data, dir, &stamp, idx + 1) {
                Ok(_) => saved += 1,
                Err(e) => {
                    failed += 1;
                    last_err = e;
                }
            }
        }
        self.ledger_msg = Some(if failed == 0 {
            format!("已导出 {saved} 张图片到 {}", dir.display())
        } else {
            format!("已导出 {saved} 张，{failed} 张失败：{last_err}")
        });
    }

    /// 本机对账模型：当前出账单过滤范围 + 当前开奖号码。
    fn reconcile_model(&self) -> reconcile::Model {
        let visible = self.visible_order_indices();
        let blocks = self.bill_blocks(&visible);
        let bettors = blocks
            .iter()
            .map(|b| {
                let batches = b
                    .rows
                    .iter()
                    .map(|r| reconcile::Batch {
                        no: r.no,
                        stake: r.amount,
                        hk: r.hk,
                        old: r.old,
                        win: r.win,
                    })
                    .collect();
                let mut bettor = reconcile::Bettor::new(&b.name, batches);
                bettor.set_rebate(b.rebate);
                bettor
            })
            .collect();
        let draws = vec![
            reconcile::Draw {
                char: '澳',
                draw: self.draws.macau.draw.clone(),
                special: self.draws.macau.special,
            },
            reconcile::Draw {
                char: '港',
                draw: self.draws.hongkong.draw.clone(),
                special: self.draws.hongkong.special,
            },
            reconcile::Draw {
                char: '老',
                draw: self.draws.old_macau.draw.clone(),
                special: self.draws.old_macau.special,
            },
        ];
        reconcile::Model::new(
            Local::now().format("%Y-%m-%d").to_string(),
            draws,
            bettors,
        )
    }

    /// 对账弹窗：本机文本 + 对方粘贴 + 逐人 / 逐批比对。
    fn reconcile_dialog(&mut self, ctx: &egui::Context) {
        if !self.reconcile_open {
            return;
        }
        let local = self.reconcile_model();
        let local_text = local.text();
        let parsed = reconcile::parse(&self.reconcile_pasted);
        let mut open = true;

        egui::Window::new("对账")
            .collapsible(false)
            .resizable(true)
            .default_size([880.0, 720.0])
            .min_width(620.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("reconcile_scroll")
                    .show(ui, |ui| {
                        let batch_total: usize =
                            local.bettors.iter().map(|b| b.batch_count).sum();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("对账").size(18.0).strong().color(TEXT),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} 位投注人 · {} 批",
                                    local.bettors.len(),
                                    batch_total
                                ))
                                .size(12.0)
                                .color(MUTED),
                            );
                        });
                        ui.add_space(8.0);

                        ui.label(
                            RichText::new("① 本机的账")
                                .size(13.0)
                                .strong()
                                .color(BILL_TEXT),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "投注人 {} 位",
                                    local.bettors.len()
                                ))
                                .size(11.0)
                                .color(MUTED),
                            );
                        });
                        egui::Frame::new()
                            .fill(SUBTOTAL_BG)
                            .stroke(Stroke::new(1.0, BORDER))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(140.0)
                                    .id_salt("reconcile_local_text")
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&local_text)
                                                    .size(11.0)
                                                    .monospace()
                                                    .color(BILL_TEXT),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    });
                            });
                        ui.horizontal(|ui| {
                            if ui.add(tiny_button("复制对账文本", GREEN)).clicked() {
                                ctx.copy_text(local_text.clone());
                                self.reconcile_msg = Some("已复制对账文本".into());
                            }
                            if ui.add(tiny_button("保存为 txt", GREEN)).clicked() {
                                let path = reconcile_export_path();
                                self.reconcile_msg = Some(
                                    match std::fs::write(&path, &local_text) {
                                        Ok(()) => {
                                            format!("已保存：{}", path.display())
                                        }
                                        Err(e) => format!("保存失败：{e}"),
                                    },
                                );
                            }
                            if let Some(msg) = &self.reconcile_msg {
                                ui.label(
                                    RichText::new(msg).size(11.0).color(MONEY_GREEN),
                                );
                            }
                        });
                        ui.add_space(10.0);

                        ui.label(
                            RichText::new("② 对方发来的文本")
                                .size(13.0)
                                .strong()
                                .color(BILL_TEXT),
                        );
                        ui.add_space(4.0);
                        // 粘贴框最高 80px，超出在框内滚动，避免把下面的比对结果挤走。
                        let mut resp = egui::ScrollArea::vertical()
                            .max_height(80.0)
                            .id_salt("reconcile_pasted_scroll")
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.reconcile_pasted)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(3)
                                        .hint_text(
                                            "把对方发来的对账文本整段粘到这里（不用改一个字）",
                                        ),
                                )
                            })
                            .inner;
                        text_edit_context_menu(
                            ui.ctx(),
                            resp.id,
                            &mut self.reconcile_pasted,
                            &mut resp,
                        );
                        ui.add_space(10.0);

                        ui.label(
                            RichText::new("③ 比对结果")
                                .size(13.0)
                                .strong()
                                .color(BILL_TEXT),
                        );
                        ui.add_space(4.0);
                        if self.reconcile_pasted.trim().is_empty() {
                            ui.label(
                                RichText::new(
                                    "粘上对方的文本后这里会立刻出结果：逐个投注人比对批数、总金额、总中奖、总回水和总输赢，有差异再往下看是第几批。",
                                )
                                .size(11.5)
                                .color(MUTED),
                            );
                        } else if !parsed.ok || parsed.model.is_none() {
                            for p in &parsed.problems {
                                ui.label(RichText::new(p).size(11.5).color(RED));
                            }
                        } else {
                            let remote = parsed.model.as_ref().unwrap();
                            let d = reconcile::diff(&local, remote);
                            for p in &parsed.problems {
                                ui.label(RichText::new(p).size(11.0).color(ORANGE));
                            }
                            if parsed.self_inconsistent {
                                ui.label(
                                    RichText::new(
                                        "对方文本里的码与内容对不上——可能被手工改过，或复制时漏了一段。下面的比对按内容进行。",
                                    )
                                    .size(11.0)
                                    .color(ORANGE),
                                );
                            }
                            if let Some(block) = &d.blocking {
                                ui.label(RichText::new(block).size(12.0).color(RED));
                            } else {
                                let (label, color, bg, border) = if d.same {
                                    (
                                        "✅ 一致",
                                        MONEY_GREEN,
                                        BAND_AMOUNT_BG,
                                        BAND_AMOUNT_BORDER,
                                    )
                                } else {
                                    ("⚠ 有差异", MONEY_RED, RED_BG, RED)
                                };
                                egui::Frame::new()
                                    .fill(bg)
                                    .stroke(Stroke::new(1.0, border))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(Margin::symmetric(12, 7))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(label)
                                                .size(15.0)
                                                .strong()
                                                .color(color),
                                        );
                                    });
                                ui.add_space(4.0);
                                if let Some(note) = &d.draw_note {
                                    ui.label(
                                        RichText::new(note).size(11.0).color(ORANGE),
                                    );
                                }
                                ui.add_space(4.0);
                                ui.checkbox(
                                    &mut self.reconcile_only_diff,
                                    "只看差异",
                                );
                                ui.add_space(4.0);
                                let shown: Vec<&reconcile::BettorDiff> = d
                                    .bettors
                                    .iter()
                                    .filter(|b| {
                                        !self.reconcile_only_diff
                                            || b.status
                                                != reconcile::Status::Same
                                    })
                                    .collect();
                                if shown.is_empty() {
                                    ui.label(
                                        RichText::new("没有差异。")
                                            .size(11.5)
                                            .color(MUTED),
                                    );
                                }
                                for b in shown {
                                    reconcile_bettor_card(ui, b);
                                    ui.add_space(6.0);
                                }
                            }
                        }
                    });
            });

        if !open {
            self.reconcile_open = false;
        }
    }

    fn ledger_ui(&mut self, ui: &mut egui::Ui) {
        let visible = self.visible_order_indices();
        let block_list = self.bill_blocks(&visible);
        // 序号格点击后要跳到主页的注单消息：等这一帧画完再切页。
        let mut jump: Option<usize> = None;

        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut category_view = self.ledger_category;

        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                // ── 固定顶部：标题 / 按钮 / 开奖号码，不随账单滚动 ──
                ui.horizontal(|ui| {
                    ui.label(RichText::new("出账单").size(20.0).strong().color(TEXT));
                    ui.add_space(10.0);
                    ui.label(RichText::new(&today).size(15.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if styled_button(
                            ui,
                            RichText::new("对账")
                                .size(13.0)
                                .strong()
                                .color(Color32::from_rgb(0x17, 0x32, 0x4D)),
                            CARD,
                            BTN_HOVER_BG,
                            BTN_ACTIVE_BG,
                            Stroke::new(1.0, BILL_LINE),
                            Stroke::new(1.0, BTN_HOVER_BORDER),
                            CornerRadius::same(10),
                            Vec2::new(72.0, 30.0),
                        )
                        .on_hover_text("和对方发来的对账文本逐人、逐批比对")
                        .clicked()
                        {
                            self.reconcile_msg = None;
                            self.reconcile_open = true;
                        }
                        ui.add_space(8.0);
                        if blue_button(
                            ui,
                            RichText::new("导出图片")
                                .size(13.0)
                                .strong()
                                .color(Color32::WHITE),
                            true,
                            CornerRadius::same(10),
                            Vec2::new(96.0, 30.0),
                        )
                        .on_hover_text("按会员分别导出出账单图片到「图片」文件夹")
                        .clicked()
                        {
                            self.bill_export_candidates = block_list
                                .iter()
                                .map(|b| b.name.clone())
                                .collect();
                            self.bill_export_selected = self
                                .bill_export_candidates
                                .iter()
                                .cloned()
                                .collect();
                            self.bill_export_open = true;
                        }
                        ui.add_space(8.0);
                        egui::Frame::new()
                            .stroke(Stroke::new(
                                1.0,
                                Color32::from_rgb(0xC6, 0xCB, 0xD2),
                            ))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::ZERO)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if view_tab(ui, "分类", category_view) {
                                    category_view = true;
                                }
                                if view_tab(ui, "列表", !category_view) {
                                    category_view = false;
                                }
                            });
                    });
                });
                self.ledger_category = category_view;
                ui.add_space(4.0);
                draw_summary_ui(ui, &self.draws, "开奖：");
                ui.add_space(6.0);
                hairline(ui);
                ui.add_space(6.0);
                // ── 账单内容：只有这里滚动 ──
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("bill_scroll")
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "共 {} 个投注人 · 每竖列最多 30 行，超出往右另起一列 · 每竖列末尾一行小计：# 写「小计」、金额列是本列金额合计、中奖列是本列特码的中奖注额（只统计特码）· 每个投注人底部的总输赢为账房净得（总金额−总中奖−总回水）· 总计上方两行按玩法汇总中奖注额 / 中奖金额。",
                                    block_list.len()
                                ))
                                .size(12.0)
                                .color(MUTED),
                            )
                            .wrap(),
                        );
                        if category_view {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        "分类形式：同一批记录按注单类别拆成小表，一个类别独占一列；# 沿用列表形式里的序号，金额/中奖各是这一类的那一份；没有数据的类别不出表；小计按类别统计。",
                                    )
                                    .size(11.5)
                                    .color(MUTED),
                                )
                                .wrap(),
                            );
                        }
                        if let Some(msg) = &self.ledger_msg {
                            let color = if msg.starts_with("导出失败") {
                                MONEY_RED
                            } else {
                                MONEY_GREEN
                            };
                            ui.label(RichText::new(msg).size(11.0).color(color));
                        }
                        ui.add_space(14.0);

                        if block_list.is_empty() {
                            ui.add_space(32.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("当日暂无投注记录。")
                                        .size(13.0)
                                        .color(MUTED),
                                );
                            });
                            return;
                        }

                        for (bi, block) in block_list.iter().enumerate() {
                            let (_, hit) = bill_block_ui(ui, block, category_view, bi);
                            if hit.is_some() {
                                jump = hit;
                            }
                            ui.add_space(18.0);
                        }
                    });
            });
        if let Some(i) = jump {
            self.jump_to_order(i);
        }
    }

    /// 跳到主页的注单消息：选中这一条并滚动到它。
    ///
    /// 出账单序号格只带 `self.orders` 的下标；这里清掉成员筛选，保证这一条在
    /// 主页列表里可见（分组筛选保留——它本来就在当前分组里）。
    fn jump_to_order(&mut self, index: usize) -> bool {
        let Some(row) = self.orders.get(index).cloned() else {
            return false;
        };
        self.member_focus = None;
        self.page = Page::Home;
        let key = (
            row.sort,
            row.chat.clone(),
            row.nick.clone(),
            row.content.clone(),
        );
        let conv = self
            .auto_converted
            .get(&edit_key(&row))
            .cloned()
            .unwrap_or_else(|| {
                let mut conv = safe_convert(&row.content, &self.keywords, &self.replacements);
                self.apply_doubt_override(&edit_key(&row), &mut conv);
                conv
            });
        self.selected_order_key = Some(key);
        self.edit_time = row.time.clone();
        self.edit_text = row.content.clone();
        self.selected_order_row = Some(row);
        self.converted = Some(conv);
        self.scroll_to_order = true;
        true
    }

    // ---------------- 风险页面 ----------------

    /// 当前范围里的分组：选中某个分组时只看它，否则看全部。
    fn upline_groups(&self) -> impl Iterator<Item = &MemberGroup> {
        self.groups
            .iter()
            .filter(|g| self.order_filter.as_deref().is_none_or(|n| g.name == n))
    }

    /// 这条注单消息是不是上家发的（上家标记在分组管理里设置）。
    fn is_upline_msg(&self, row: &MsgRow) -> bool {
        self.upline_groups()
            .any(|g| g.is_upline(&row.chat, &row.sender))
    }

    /// 成员栏里的这个人是不是上家（联系人按 username，群成员按 (群, 成员)）。
    fn is_upline_key(&self, key: &MemberKey) -> bool {
        self.upline_groups().any(|g| match key {
            MemberKey::Contact(c) => g.upline_contacts.contains(c),
            MemberKey::Member(c, m) => g.upline_members.iter().any(|(gc, gm)| gc == c && gm == m),
        })
    }

    /// 当前注单转换出的全部注（忽略投注人），供风险面板汇总。
    /// 风险页要统计的注单：跟随主页的「分组」下拉框过滤。
    fn risk_bets(&self) -> Vec<crate::risk::BetRow> {
        let filter_group = self
            .order_filter
            .as_deref()
            .and_then(|name| self.groups.iter().find(|g| g.name == name));
        let mut out = Vec::new();
        for row in &self.orders {
            if filter_group.is_some_and(|g| !g.matches(row)) {
                continue;
            }
            let conv = self
                .auto_converted
                .get(&edit_key(row))
                .cloned()
                .unwrap_or_else(|| {
                    let mut c = safe_convert(&row.content, &self.keywords, &self.replacements);
                    self.apply_doubt_override(&edit_key(row), &mut c);
                    c
                });
            // 上家的注单在风险页按减额（负数）汇总。
            let upline = self.is_upline_msg(row);
            for bet in conv.bets {
                out.push(
                    crate::risk::BetRow::new(bet, &row.time).with_upline(upline),
                );
            }
        }
        out
    }

    fn risk_ui(&mut self, ui: &mut egui::Ui) {
        let bets = self.risk_bets();
        // 选了具体分组：所有统计统一用该分组的赔率 / 回水；
        // 全部分组：用默认（未匹配分组 / 会员）的赔率 / 回水。
        let odds_group = self
            .order_filter
            .clone()
            .filter(|name| self.groups.iter().any(|g| &g.name == name));
        let mul_sep = self
            .keywords
            .get(crate::keywords::Field::Multiplication)
            .iter()
            .find(|k| !k.is_empty())
            .cloned()
            .unwrap_or_else(|| "拖".to_string());

        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 6))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::symmetric(2, 2))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = 1.0;
                            for tab in [
                                RiskTab::XiaoMa,
                                RiskTab::Special,
                                RiskTab::LianXiao,
                                RiskTab::FlatSpecial,
                                RiskTab::FlatNumber,
                            ] {
                                if risk_tab_button(ui, tab.label(), self.risk_tab == tab) {
                                    self.risk_tab = tab;
                                }
                            }
                        });
                    // 开奖富样式不能放进 right_to_left 布局：里面的横向组会跟着
                    // 从右往左排，整行顺序反过来（最后一个「特」跑到最前面）。
                    // 先按宽度留白，再正常从左往右画，效果就是右对齐。
                    let summary_w = draw_summary_width(ui.painter(), &self.draws, 17.0);
                    ui.add_space((ui.available_width() - summary_w).max(0.0));
                    draw_summary_ui(ui, &self.draws, "");
                });
                ui.add_space(4.0);
                match self.risk_tab {
                    RiskTab::XiaoMa => {
                        self.risk_xiaoma_ui(ui, &bets, &mul_sep, odds_group.as_deref())
                    }
                    tab => {
                        if let Some(list) = tab.panel_list() {
                            self.risk_bet_ui(ui, list, &bets, odds_group.as_deref());
                        }
                    }
                }
            });

        if self.risk_prob_open {
            self.risk_probability_window(ui.ctx(), &bets, &mul_sep);
        }
        if self.xm.dup_open
            || self.xm.deduct_open
            || self.xm.log_open
            || self.xm.merge_open.is_some()
        {
            let base = crate::risk::xiaoma(
                &bets,
                &self.draws,
                &self.odds,
                odds_group.as_deref(),
                &mul_sep,
            );
            let rows = crate::xiaoma_offsets::apply_to_rows(base.clone(), &self.xm.store.offsets);
            let draw_sets = [0, 1, 2].map(|r| xiaoma_modals::draw_set(&self.draws, r));
            self.xm_modals(ui.ctx(), &rows, &draw_sets);
        }
        if self.code_report_open {
            self.code_report_dialog(ui.ctx(), &bets, odds_group.as_deref());
        }
    }

    /// 特码「吃码上报」：按号码把投注额减掉，写进减额表（风险页立即生效）。
    fn code_report_dialog(
        &mut self,
        ctx: &egui::Context,
        bets: &[crate::risk::BetRow],
        odds_group: Option<&str>,
    ) {
        let mut open = self.code_report_open;
        let mut close = false;
        let mut apply = false;
        let mut copy_clicked = false;
        let mut fill_suggested = false;

        egui::Window::new("吃码上报")
            .collapsible(false)
            .resizable(true)
            .default_size([800.0, 580.0])
            .min_width(640.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "一个号码一行：按当前特码风险 / 下注额算出要吃多少，确认后写进减额表；风险页立即生效，总账不受影响。",
                    )
                    .size(11.0)
                    .color(SUB),
                );
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("地区").size(11.5).color(SUB));
                    for r in 0..3 {
                        if ui
                            .selectable_label(
                                self.code_report_region == r,
                                RichText::new(crate::risk::REGION_NAMES[r]).size(12.0),
                            )
                            .clicked()
                            && self.code_report_region != r
                        {
                            self.code_report_region = r;
                            self.code_report_reduce.clear();
                            self.code_report_msg = None;
                        }
                    }
                    ui.separator();
                    if ui
                        .selectable_label(
                            self.code_report_mode == CodeReportMode::Risk,
                            RichText::new("风险下限").size(12.0),
                        )
                        .clicked()
                    {
                        self.code_report_mode = CodeReportMode::Risk;
                    }
                    if ui
                        .selectable_label(
                            self.code_report_mode == CodeReportMode::Amount,
                            RichText::new("下注额上限").size(12.0),
                        )
                        .clicked()
                    {
                        self.code_report_mode = CodeReportMode::Amount;
                    }
                    match self.code_report_mode {
                        CodeReportMode::Risk => {
                            let mut resp = ui.add(
                                egui::TextEdit::singleline(&mut self.code_report_threshold)
                                    .desired_width(90.0),
                            );
                            text_edit_context_menu(
                                ui.ctx(),
                                resp.id,
                                &mut self.code_report_threshold,
                                &mut resp,
                            );
                        }
                        CodeReportMode::Amount => {
                            let mut resp = ui.add(
                                egui::TextEdit::singleline(&mut self.code_report_limit)
                                    .desired_width(90.0),
                            );
                            text_edit_context_menu(
                                ui.ctx(),
                                resp.id,
                                &mut self.code_report_limit,
                                &mut resp,
                            );
                        }
                    }
                    if ui.add(tiny_button("一键建议", TEXT)).clicked() {
                        fill_suggested = true;
                    }
                    if ui.add(tiny_button("复制格式", TEXT)).clicked() {
                        copy_clicked = true;
                    }
                    if let Some(msg) = &self.code_report_msg {
                        ui.label(RichText::new(msg).size(10.5).color(MUTED));
                    }
                });
                ui.add_space(6.0);

                let region = self.code_report_region.min(2);
                let columns = self.special_panel_columns(bets, odds_group);
                let col = &columns[region];
                let set = self.odds.resolve("", odds_group, region);
                let odds = set.value("special_number");
                let rebate = set.value("rw_special_number");
                let threshold = self
                    .code_report_threshold
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(-10000.0);
                let limit = self
                    .code_report_limit
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(5000.0);
                let denom = odds + rebate / 100.0 - 1.0;
                let total = col.total_stake;

                struct RowView {
                    num: u32,
                    stake: f64,
                    risk: f64,
                    need: f64,
                }
                let mut row_views: Vec<RowView> = col
                    .rows
                    .iter()
                    .filter_map(|row| {
                        let num = row.num?;
                        let stake = row.stake;
                        let risk = row.risk;
                        let need = match self.code_report_mode {
                            CodeReportMode::Risk => {
                                if stake > 0.0 && risk < threshold && denom > 0.0 {
                                    ((threshold - risk) / denom).ceil().min(stake)
                                } else {
                                    0.0
                                }
                            }
                            CodeReportMode::Amount => {
                                if stake > limit {
                                    stake - limit
                                } else {
                                    0.0
                                }
                            }
                        };
                        Some(RowView {
                            num,
                            stake,
                            risk,
                            need,
                        })
                    })
                    .collect();
                if self.code_report_mode == CodeReportMode::Risk {
                    row_views.sort_by(|a, b| {
                        a.risk
                            .partial_cmp(&b.risk)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.num.cmp(&b.num))
                    });
                } else {
                    row_views.sort_by(|a, b| {
                        b.stake
                            .partial_cmp(&a.stake)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| a.num.cmp(&b.num))
                    });
                }

                if fill_suggested {
                    self.code_report_reduce.clear();
                    for row in &row_views {
                        if row.need > 0.0 {
                            self.code_report_reduce
                                .insert(row.num, crate::convert::fmt_amount(row.need));
                        }
                    }
                    self.code_report_msg = Some("已填入建议额".to_string());
                }

                let mut reduce_now: Vec<(u32, f64)> = Vec::new();
                for row in &row_views {
                    let v = code_report_reduce_of(&self.code_report_reduce, row.num, row.stake);
                    if v > 0.0 {
                        reduce_now.push((row.num, v));
                    }
                }
                let total_reduce: f64 = reduce_now.iter().map(|(_, v)| *v).sum();
                let new_total = total - total_reduce;
                let new_base = new_total - new_total * rebate / 100.0;
                let report_text = code_report_text(region, &reduce_now);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "总金额 {}",
                            crate::convert::fmt_amount(total)
                        ))
                        .size(12.0)
                        .strong()
                        .color(TEXT),
                    );
                    if total_reduce > 0.0 {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(format!(
                                "吃码后 {}",
                                crate::convert::fmt_amount(new_total)
                            ))
                            .size(11.5)
                            .color(GREEN_DEEP),
                        );
                    }
                });
                ui.add_space(4.0);

                if copy_clicked && !report_text.is_empty() {
                    ui.ctx().copy_text(report_text.clone());
                    self.code_report_msg = Some("已复制".to_string());
                }
                if !report_text.is_empty() {
                    egui::Frame::new()
                        .fill(STRIPE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&report_text)
                                        .monospace()
                                        .size(11.5)
                                        .color(TEXT),
                                )
                                .wrap(),
                            );
                        });
                    ui.add_space(6.0);
                }

                let table_h = (ui.available_height() - 76.0).max(180.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("code_report_scroll")
                    .max_height(table_h)
                    .show(ui, |ui| {
                        egui::Grid::new("code_report_grid")
                            .num_columns(6)
                            .spacing([12.0, 2.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for h in [
                                    "号码",
                                    "生肖",
                                    "下注额",
                                    if self.code_report_mode == CodeReportMode::Risk {
                                        "风险"
                                    } else {
                                        "超出"
                                    },
                                    "吃码金额",
                                    "吃码后风险",
                                ] {
                                    ui.label(RichText::new(h).size(11.0).strong().color(SUB));
                                }
                                ui.end_row();
                                for row in &row_views {
                                    ui.label(
                                        RichText::new(format!("{:02}", row.num))
                                            .size(12.0)
                                            .color(TEXT),
                                    );
                                    ui.label(
                                        RichText::new(
                                            crate::settle::zodiac_of(row.num).unwrap_or(""),
                                        )
                                        .size(12.0)
                                        .color(SUB),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(if row.stake == 0.0 {
                                                    "-".to_string()
                                                } else {
                                                    crate::convert::fmt_amount(row.stake)
                                                })
                                                .size(12.0)
                                                .color(if row.stake == 0.0 {
                                                    MUTED
                                                } else {
                                                    TEXT
                                                }),
                                            );
                                        },
                                    );
                                    let (metric, danger) =
                                        if self.code_report_mode == CodeReportMode::Risk {
                                            (
                                                if row.stake == 0.0 {
                                                    "-".to_string()
                                                } else {
                                                    crate::convert::fmt_amount(row.risk)
                                                },
                                                row.stake > 0.0 && row.risk < threshold,
                                            )
                                        } else {
                                            let over = (row.stake - limit).max(0.0);
                                            (
                                                if over > 0.0 {
                                                    crate::convert::fmt_amount(over)
                                                } else {
                                                    "-".to_string()
                                                },
                                                over > 0.0,
                                            )
                                        };
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(metric)
                                                    .size(12.0)
                                                    .color(if danger { MONEY_RED } else { TEXT }),
                                            );
                                        },
                                    );
                                    let entry =
                                        self.code_report_reduce.entry(row.num).or_default();
                                    let hint = if row.need > 0.0 {
                                        crate::convert::fmt_amount(row.need)
                                    } else {
                                        String::new()
                                    };
                                    let mut resp = ui.add(
                                        egui::TextEdit::singleline(entry)
                                            .desired_width(64.0)
                                            .align(Align2::RIGHT_CENTER)
                                            .hint_text(hint),
                                    );
                                    text_edit_context_menu(
                                        ui.ctx(),
                                        resp.id,
                                        entry,
                                        &mut resp,
                                    );
                                    let reduce = code_report_reduce_of(
                                        &self.code_report_reduce,
                                        row.num,
                                        row.stake,
                                    );
                                    if reduce > 0.0 {
                                        let after =
                                            new_base - (row.stake - reduce) * odds;
                                        ui.label(
                                            RichText::new(crate::convert::fmt_amount(after))
                                                .size(12.0)
                                                .color(if after >= threshold {
                                                    MONEY_GREEN
                                                } else {
                                                    MONEY_RED
                                                }),
                                        );
                                    } else {
                                        ui.label(RichText::new("-").size(12.0).color(MUTED));
                                    }
                                    ui.end_row();
                                }
                            });
                    });

                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "按 {} 倍 / 回水 {}% 算",
                            crate::convert::fmt_amount(odds),
                            crate::convert::fmt_amount(rebate)
                        ))
                        .size(10.5)
                        .color(SUB),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut final_reduce: Vec<(u32, f64)> = Vec::new();
                        for row in &row_views {
                            let v = code_report_reduce_of(
                                &self.code_report_reduce,
                                row.num,
                                row.stake,
                            );
                            if v > 0.0 {
                                final_reduce.push((row.num, v));
                            }
                        }
                        let enabled = !final_reduce.is_empty();
                        if primary_button(
                            ui,
                            RichText::new("确认上报")
                                .size(12.5)
                                .color(if enabled { Color32::WHITE } else { SUB }),
                            enabled,
                            CornerRadius::same(6),
                            Vec2::new(96.0, 28.0),
                        )
                        .clicked()
                        {
                            apply = true;
                        }
                        if ui.add(tiny_button("取消", SUB)).clicked() {
                            close = true;
                        }
                    });
                });
            });

        if apply {
            let region = self.code_report_region.min(2);
            let columns = self.special_panel_columns(bets, odds_group);
            let col = &columns[region];
            let mut reduce_now: Vec<(u32, f64)> = Vec::new();
            let mut delta: BTreeMap<String, f64> = BTreeMap::new();
            for row in &col.rows {
                let Some(num) = row.num else {
                    continue;
                };
                let v = code_report_reduce_of(&self.code_report_reduce, num, row.stake);
                if v > 0.0 {
                    reduce_now.push((num, v));
                    delta.insert(
                        crate::xiaoma_offsets::row_key(region, "特码", &format!("{num:02}")),
                        v,
                    );
                }
            }
            if !delta.is_empty() {
                let detail = code_report_detail(&reduce_now);
                let desc = format!(
                    "{} {}：{}",
                    crate::risk::REGION_NAMES[region],
                    "特码",
                    detail.replace('\n', "；")
                );
                self.xm.store.apply(delta, "吃码上报", desc);
                self.code_report_reduce.clear();
                self.code_report_msg = None;
                close = true;
            }
        }
        if close || !open {
            self.code_report_open = false;
        }
    }

    /// 小码减额的弹窗（减额 / 合并 / 查重 / 减额记录）。
    /// `rows` 是减额后的小码行；`base` 是没有减额的小码行（「将影响 N 行」用）。
    fn xm_modals(
        &mut self,
        ctx: &egui::Context,
        rows: &[crate::risk::XiaoMaRow],
        draw_sets: &[HashSet<String>; 3],
    ) {
        if self.xm.deduct_open {
            let xm = &mut self.xm;
            let close = xm_modal(
                ctx,
                "xm_deduct",
                "投注额过滤 · 减额",
                "只作用于小码面板，总账不受影响",
                860.0,
                // 高度固定为软件窗口高度的 95%。
                Some(ctx.content_rect().height() * 0.95),
                0.9,
                |ui| xiaoma_modals::deduct_body(xm, rows, ui),
            );
            if close {
                self.xm.deduct_open = false;
            }
        }
        if let Some(ti) = self.xm.merge_open {
            let title = format!("合并组合 - {}", xiaoma_modals::XM_TYPES[ti].0);
            let subtitle = format!(
                "已自动合并{}该类型栏的全部组合（恰好构成某个号码集合时压成一行）；复制出来的文本可直接粘到「组合减额」执行。",
                XM_REGION_NAMES[self.xm.deduct_region]
            );
            let xm = &mut self.xm;
            let close = xm_modal(
                ctx,
                "xm_merge",
                &title,
                &subtitle,
                640.0,
                // 与「减额」弹窗一致：固定为软件窗口高度的 95%。
                Some(ctx.content_rect().height() * 0.95),
                0.9,
                |ui| xiaoma_modals::merge_body(xm, ui),
            );
            if close {
                self.xm.merge_open = None;
            }
        }
        if self.xm.dup_open {
            let xm = &mut self.xm;
            let close = xm_modal(
                ctx,
                "xm_dup",
                "查重 · 三中三",
                "只看，不减额。减额请用「减额」按钮",
                760.0,
                None,
                0.9,
                |ui| xiaoma_modals::dup_body(xm, draw_sets, ui),
            );
            if close {
                self.xm.dup_open = false;
            }
        }
        if self.xm.log_open {
            let xm = &mut self.xm;
            let close = xm_modal(
                ctx,
                "xm_log",
                "减额记录",
                "只作用于小码面板；撤回会把这一笔的金额原样加回去",
                680.0,
                None,
                0.9,
                |ui| xiaoma_modals::log_body(xm, ui),
            );
            if close {
                self.xm.log_open = false;
            }
        }
    }

    /// 「概率」明细弹窗：四种组合玩法的去重组合数 / 全部组合数 / 总注数 / 覆盖率。
    fn risk_probability_window(
        &mut self,
        ctx: &egui::Context,
        bets: &[crate::risk::BetRow],
        mul_sep: &str,
    ) {
        let stats = crate::risk::play_stats(bets, self.risk_xm_region, mul_sep);
        let region_label = match self.risk_xm_region {
            None => "全部地区".to_string(),
            Some(0) => "澳门".to_string(),
            Some(1) => "香港".to_string(),
            _ => "老澳门".to_string(),
        };

        let mut close = false;

        // 参考项目是带遮罩的模态框：遮罩也把底层页面挡住，点空白处关闭。
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("risk_prob_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let resp = ui.allocate_rect(
                    egui::Rect::from_min_size(screen.min, screen.size()),
                    Sense::click(),
                );
                ui.painter()
                    .rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(70));
                if resp.clicked() {
                    close = true;
                }
            });

        egui::Window::new("中奖概率")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(640.0)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_min_width(600.0);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("中奖概率").size(16.0).strong().color(TEXT),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "统计当前日期「{region_label}」收集到的组合，按已收集组合占全部组合的比例计算"
                                    ))
                                    .size(11.5)
                                    .color(MUTED),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if secondary_small_button(ui, "关闭").clicked() {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(8.0);
                        hairline(ui);
                        ui.add_space(8.0);

                        egui::Grid::new("risk_prob_grid")
                            .num_columns(5)
                            .spacing([18.0, 6.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for h in
                                    ["玩法", "去重组合数", "全部组合数", "总注数", "中奖概率"]
                                {
                                    ui.label(
                                        RichText::new(h).size(12.0).strong().color(REF_NAVY),
                                    );
                                }
                                ui.end_row();
                                for i in 0..4 {
                                    ui.label(
                                        RichText::new(crate::risk::PLAY_LABELS[i])
                                            .size(12.0)
                                            .strong()
                                            .color(TEXT),
                                    );
                                    risk_num_label(
                                        ui,
                                        &stats[i].distinct.to_string(),
                                        TEXT,
                                        false,
                                    );
                                    risk_num_label(
                                        ui,
                                        &(crate::risk::TOTAL_COMBOS[i] as u64).to_string(),
                                        TEXT,
                                        false,
                                    );
                                    risk_num_label(
                                        ui,
                                        &stats[i].total_bets.to_string(),
                                        TEXT,
                                        false,
                                    );
                                    let text = if stats[i].distinct == 0 {
                                        "0%".to_string()
                                    } else {
                                        format_probability(
                                            stats[i].distinct as f64
                                                / crate::risk::TOTAL_COMBOS[i],
                                        )
                                    };
                                    risk_num_label(ui, &text, MONEY_BLUE, true);
                                    ui.end_row();
                                }
                            });

                        ui.add_space(10.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "中奖概率 = 去重组合数 ÷ 全部组合数。三中三 / 三中二的全部组合是 C(49,3)=18424，\
                                     二中二 / 特碰是 C(49,2)=1176。去重组合数：同一组号码（同玩法）只算一次，\
                                     跨记录、跨投注人的重复合并；总注数：展开后的实际注数。重复投注不会提高概率，\
                                     只有覆盖到新的不同组合才会。",
                                )
                                .size(12.0)
                                .color(MUTED),
                            )
                            .wrap(),
                        );
                    });
            });

        if close {
            self.risk_prob_open = false;
        }
    }

    /// 四个投注页签（特码 / 连肖 / 平特 / 平码）共用的三栏表。
    /// 当前减额表里属于「特码」的吃码金额（按地区拆开，号码 → 减额）。
    fn special_offsets_by_region(&self) -> [BTreeMap<u32, f64>; 3] {
        let mut out: [BTreeMap<u32, f64>; 3] = Default::default();
        for (key, amount) in &self.xm.store.offsets {
            let mut parts = key.splitn(3, '|');
            let Some(region) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                continue;
            };
            if region >= 3 || parts.next() != Some("特码") {
                continue;
            }
            let Some(combo) = parts.next() else {
                continue;
            };
            if let Ok(num) = combo.parse::<u32>() {
                out[region].insert(num, *amount);
            }
        }
        out
    }

    /// 特码三栏（已应用吃码减额）。
    fn special_panel_columns(
        &self,
        bets: &[crate::risk::BetRow],
        odds_group: Option<&str>,
    ) -> Vec<crate::risk::PanelColumn> {
        let mut columns = crate::risk::panel(
            crate::risk::PanelList::Special,
            bets,
            &self.draws,
            &self.odds,
            odds_group,
            self.risk_lian_size,
        );
        let offsets = self.special_offsets_by_region();
        for (r, col) in columns.iter_mut().enumerate() {
            crate::risk::apply_special_offsets(col, r, &offsets[r], &self.odds, odds_group);
        }
        columns
    }

    fn risk_bet_ui(
        &mut self,
        ui: &mut egui::Ui,
        list: crate::risk::PanelList,
        bets: &[crate::risk::BetRow],
        odds_group: Option<&str>,
    ) {
        if list == crate::risk::PanelList::Special {
            ui.horizontal(|ui| {
                if secondary_small_button(ui, "吃码上报")
                    .on_hover_text("按号码吃码，直接反映到下面的特码金额 / 风险（只影响风险页）")
                    .clicked()
                {
                    self.code_report_open = true;
                    self.code_report_reduce.clear();
                    self.code_report_msg = None;
                }
                if ui
                    .add(tiny_button("减额记录", SUB))
                    .on_hover_text("查看 / 撤回吃码记录（与小码减额共用一张表）")
                    .clicked()
                {
                    self.xm.log_open = true;
                }
                let count: usize = self.special_offsets_by_region().iter().map(|m| m.len()).sum();
                if count > 0 {
                    ui.label(
                        RichText::new(format!("已吃码 {count} 个号"))
                            .size(11.0)
                            .color(MUTED),
                    );
                }
            });
            ui.add_space(6.0);
        }
        if list == crate::risk::PanelList::LianXiao {
            ui.horizontal(|ui| {
                ui.label(RichText::new("规格").size(12.0).color(MUTED));
                ui.add_space(4.0);
                egui::Frame::new()
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(2, 2))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for size in 2..=5usize {
                            if risk_tab_button(ui, lian_size_label(size), self.risk_lian_size == size)
                            {
                                if self.risk_lian_size != size {
                                    self.risk_lian_size = size;
                                    self.risk_lian_pages = [1, 1, 1];
                                }
                            }
                        }
                    });
            });
            ui.add_space(8.0);
        }

        let columns = if list == crate::risk::PanelList::Special {
            self.special_panel_columns(bets, odds_group)
        } else {
            crate::risk::panel(
                list,
                bets,
                &self.draws,
                &self.odds,
                odds_group,
                self.risk_lian_size,
            )
        };
        let with_risk = list == crate::risk::PanelList::Special;

        struct ColView {
            rows: Vec<crate::risk::PanelRow>,
            total_rows: usize,
            page: usize,
            total_pages: usize,
            total_stake: f64,
            total_water: f64,
        }

        let mut views: Vec<ColView> = Vec::with_capacity(3);
        for r in 0..3 {
            let mut rows = columns[r].rows.clone();
            match list {
                crate::risk::PanelList::Special => {
                    let desc = self.risk_panel_desc[r];
                    rows.sort_by(|a, b| {
                        let ord = a
                            .risk
                            .partial_cmp(&b.risk)
                            .unwrap_or(std::cmp::Ordering::Equal);
                        let ord = if desc { ord.reverse() } else { ord };
                        ord.then_with(|| a.num.cmp(&b.num))
                    });
                }
                _ => rows.sort_by(|a, b| {
                    b.stake
                        .partial_cmp(&a.stake)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.key.cmp(&b.key))
                }),
            }

            let total_rows = rows.len();
            let mut page = 1usize;
            let mut total_pages = 1usize;
            let rows = if list == crate::risk::PanelList::LianXiao {
                total_pages = total_rows.div_ceil(200).max(1);
                let p = self.risk_lian_pages[r].clamp(1, total_pages);
                self.risk_lian_pages[r] = p;
                page = p;
                let start = (p - 1) * 200;
                rows[start..(start + 200).min(total_rows)].to_vec()
            } else {
                rows
            };

            views.push(ColView {
                rows,
                total_rows,
                page,
                total_pages,
                total_stake: columns[r].total_stake,
                total_water: columns[r].total_water,
            });
        }

        let spacing = 8.0;
        let narrow = 52.0;
        let collapsed_count = self.risk_collapsed.iter().filter(|c| **c).count();
        let expanded = 3 - collapsed_count;
        let avail_w = ui.available_width();
        let exp_w = if expanded > 0 {
            ((avail_w - narrow * collapsed_count as f32 - spacing * 2.0 - 2.0)
                / expanded as f32)
                .max(140.0)
        } else {
            0.0
        };
        let head_h = 28.0;
        let thead_h = 26.0;
        // 连肖只有某一栏超过一页时才需要底部翻页行；一页都没有时不占高度。
        let pager_h = if list == crate::risk::PanelList::LianXiao
            && views.iter().any(|v| v.total_pages > 1)
        {
            30.0
        } else {
            0.0
        };

        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                let scroll_h =
                    (ui.available_height() - head_h - thead_h - pager_h - 10.0).max(90.0);

                // 区头：固定不滚，三栏与表体、翻页共用同一套列宽。
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for r in 0..3 {
                        if r > 0 {
                            ui.add_space(spacing);
                        }
                        let w = if self.risk_collapsed[r] { narrow } else { exp_w };
                        ui.allocate_ui(Vec2::new(w, head_h), |ui| {
                            self.risk_region_head(
                                ui,
                                r,
                                w,
                                views[r].total_stake,
                                views[r].total_water,
                            );
                        });
                    }
                });

                // 列标题：参考项目是 sticky thead，这里固定在滚动区上方。
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for r in 0..3 {
                        if r > 0 {
                            ui.add_space(spacing);
                        }
                        let w = if self.risk_collapsed[r] { narrow } else { exp_w };
                        ui.allocate_ui(Vec2::new(w, thead_h), |ui| {
                            if self.risk_collapsed[r] {
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::new(w, thead_h), Sense::hover());
                                ui.painter().rect_filled(rect, CornerRadius::ZERO, REF_HEAD_BG);
                                ui.painter().hline(
                                    rect.x_range(),
                                    rect.max.y - 0.5,
                                    Stroke::new(1.0, REF_LINE),
                                );
                            } else {
                                let widths = risk_col_widths(w, list);
                                let headings: &[&str] = match list {
                                    crate::risk::PanelList::Special => {
                                        &["号码", "金额", "风险", "ID"]
                                    }
                                    crate::risk::PanelList::FlatNumber => &["号码", "金额", "ID"],
                                    crate::risk::PanelList::LianXiao => &["组合", "金额", "ID"],
                                    crate::risk::PanelList::FlatSpecial => &["项目", "金额", "ID"],
                                };
                                if risk_header_row(
                                    ui,
                                    &widths,
                                    headings,
                                    with_risk,
                                    r,
                                    self.risk_panel_desc[r],
                                ) {
                                    self.risk_panel_desc[r] = !self.risk_panel_desc[r];
                                }
                            }
                        });
                    }
                });

                // 三栏共用一个滚动容器：滚动条只有一条，三栏一起走。
                egui::ScrollArea::vertical()
                    .id_salt(("risk_shared", list as u8))
                    .auto_shrink([false, false])
                    .max_height(scroll_h)
                    .min_scrolled_height(scroll_h)
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            for r in 0..3 {
                                if r > 0 {
                                    ui.add_space(spacing);
                                }
                                let w = if self.risk_collapsed[r] { narrow } else { exp_w };
                                if self.risk_collapsed[r] {
                                    let (rect, _) = ui.allocate_exact_size(
                                        Vec2::new(w, scroll_h),
                                        Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius::same(8),
                                        REF_HEAD_TINT,
                                    );
                                } else {
                                    let widths = risk_col_widths(w, list);
                                    let row_h = if list == crate::risk::PanelList::LianXiao
                                        || views[r].rows.is_empty()
                                    {
                                        21.0
                                    } else {
                                        ((scroll_h - 2.0) / views[r].rows.len() as f32)
                                            .max(14.0)
                                    };
                                    let body_h = if views[r].rows.is_empty() {
                                        64.0
                                    } else {
                                        views[r].rows.len() as f32 * row_h
                                    };
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(w, body_h),
                                        Layout::top_down(Align::Min),
                                        |ui| {
                                            ui.set_min_width(w);
                                            risk_column_body(
                                                ui,
                                                &widths,
                                                list,
                                                &views[r].rows,
                                                row_h,
                                                with_risk,
                                            );
                                        },
                                    );
                                }
                            }
                        });
                    });

                // 连肖各栏的翻页：与区头 / 表体同一套列宽。
                if list == crate::risk::PanelList::LianXiao && pager_h > 0.0 {
                    ui.add_space(2.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for r in 0..3 {
                            if r > 0 {
                                ui.add_space(spacing);
                            }
                            let w = if self.risk_collapsed[r] { narrow } else { exp_w };
                            ui.allocate_ui(Vec2::new(w, 24.0), |ui| {
                                if !self.risk_collapsed[r] && views[r].total_pages > 1 {
                                    ui.horizontal(|ui| {
                                        if ghost_button_sized(
                                            ui,
                                            RichText::new("上一页").size(11.0).color(SUB),
                                            CornerRadius::same(6),
                                            Vec2::new(52.0, 22.0),
                                        )
                                        .clicked()
                                            && views[r].page > 1
                                        {
                                            self.risk_lian_pages[r] = views[r].page - 1;
                                        }
                                        ui.label(
                                            RichText::new(format!(
                                                "{} / {} 页（{} 行）",
                                                views[r].page,
                                                views[r].total_pages,
                                                views[r].total_rows
                                            ))
                                            .size(11.0)
                                            .color(MUTED),
                                        );
                                        if ghost_button_sized(
                                            ui,
                                            RichText::new("下一页").size(11.0).color(SUB),
                                            CornerRadius::same(6),
                                            Vec2::new(52.0, 22.0),
                                        )
                                        .clicked()
                                            && views[r].page < views[r].total_pages
                                        {
                                            self.risk_lian_pages[r] = views[r].page + 1;
                                        }
                                    });
                                }
                            });
                        }
                    });
                }
            });
    }

    fn risk_region_head(
        &mut self,
        ui: &mut egui::Ui,
        region: usize,
        width: f32,
        total_stake: f64,
        total_water: f64,
    ) {
        let collapsed = self.risk_collapsed[region];
        egui::Frame::new()
            .fill(REF_HEAD_TINT)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width((width - 16.0).max(8.0));
                ui.set_min_height(20.0);
                if collapsed {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(crate::risk::REGION_CHARS[region])
                                .size(13.0)
                                .strong()
                                .color(region_color(region)),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("▶").size(10.0).color(SUB))
                                        .frame(false),
                                )
                                .on_hover_text(format!(
                                    "展开{}",
                                    crate::risk::REGION_NAMES[region]
                                ))
                                .clicked()
                            {
                                self.risk_collapsed[region] = false;
                            }
                        });
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(crate::risk::REGION_NAMES[region])
                                .size(12.0)
                                .strong()
                                .color(region_color(region)),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new("总投注").size(11.0).color(MUTED));
                        ui.label(
                            RichText::new(fmt_money(total_stake))
                                .size(11.0)
                                .strong()
                                .color(REF_NAVY),
                        );
                        ui.add_space(4.0);
                        ui.label(RichText::new("回水").size(11.0).color(MUTED));
                        ui.label(
                            RichText::new(fmt_money(total_water))
                                .size(11.0)
                                .strong()
                                .color(REF_NAVY),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("◀").size(10.0).color(SUB))
                                        .frame(false),
                                )
                                .on_hover_text(format!(
                                    "收起{}",
                                    crate::risk::REGION_NAMES[region]
                                ))
                                .clicked()
                            {
                                self.risk_collapsed[region] = true;
                            }
                        });
                    });
                }
            });
    }

    /// 小码页签：地区页签 + 三张统计卡 + 四张概率卡 + 组合表 + 翻页。
    fn risk_xiaoma_ui(
        &mut self,
        ui: &mut egui::Ui,
        bets: &[crate::risk::BetRow],
        mul_sep: &str,
        odds_group: Option<&str>,
    ) {
        ui.horizontal(|ui| {
            for (label, region) in [
                ("全部", None),
                ("澳门", Some(0)),
                ("香港", Some(1)),
                ("老澳门", Some(2)),
            ] {
                if risk_region_tab_button(ui, label, self.risk_xm_region == region) {
                    if self.risk_xm_region != region {
                        self.risk_xm_region = region;
                        self.risk_xm_page = 1;
                    }
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.risk_xm_search)
                        .desired_width(130.0)
                        .hint_text("搜索组合")
                        .font(egui::TextStyle::Small),
                );
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.risk_xm_search, &mut resp);
                if resp.changed() {
                    self.risk_xm_page = 1;
                }
                if secondary_small_button(ui, "概率").clicked() {
                    self.risk_prob_open = true;
                }
                if secondary_small_button(ui, "减额").clicked() {
                    self.xm.deduct_open = true;
                    self.xm.deduct_focus_pending = true;
                    self.xm.deduct_hint = None;
                }
                if secondary_small_button(ui, "查重").clicked() {
                    let entries = crate::xiaoma_dup::build(bets, mul_sep);
                    let groups = [0, 1, 2].map(|r| crate::xiaoma_dup::groups(&entries, r));
                    self.xm.dup_entries = entries;
                    self.xm.dup_groups = groups;
                    self.xm.dup_expanded.clear();
                    self.xm.dup_open = true;
                }
                if secondary_small_button(ui, "减额记录").clicked() {
                    self.xm.log_open = true;
                    self.xm.log_hint = None;
                }
            });
        });
        ui.add_space(8.0);

        let all_rows = crate::xiaoma_offsets::apply_to_rows(
            crate::risk::xiaoma(bets, &self.draws, &self.odds, odds_group, mul_sep),
            &self.xm.store.offsets,
        );
        let has_any = !all_rows.is_empty();
        let query = self.risk_xm_search.trim().to_string();
        let region = self.risk_xm_region;
        let mut rows: Vec<crate::risk::XiaoMaRow> = all_rows
            .into_iter()
            .filter(|r| region.is_none_or(|reg| r.region == reg))
            .filter(|r| query.is_empty() || r.key.contains(&query))
            .collect();
        let sort = self.risk_xm_sort;
        let desc = self.risk_xm_desc;
        rows.sort_by(|a, b| match (a.hit, b.hit) {
            (true, true) => a
                .profit
                .partial_cmp(&b.profit)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.key.cmp(&b.key)),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => {
                let va = match sort {
                    RiskSort::Stake => a.stake,
                    RiskSort::Risk => a.risk,
                    RiskSort::Profit => a.profit,
                };
                let vb = match sort {
                    RiskSort::Stake => b.stake,
                    RiskSort::Risk => b.risk,
                    RiskSort::Profit => b.profit,
                };
                let ord = va
                    .partial_cmp(&vb)
                    .unwrap_or(std::cmp::Ordering::Equal);
                let ord = if desc { ord.reverse() } else { ord };
                ord.then_with(|| a.key.cmp(&b.key))
            }
        });

        let total_stake: f64 = rows.iter().map(|r| r.stake).sum();
        let total_water: f64 = rows.iter().map(|r| r.water).sum();
        let total_profit: f64 = rows.iter().map(|r| r.profit).sum();

        let card_w = ((ui.available_width() - 20.0) / 3.0).max(120.0);
        ui.horizontal(|ui| {
            risk_stat_card(ui, "总投注", &fmt_money(total_stake), TEXT, card_w);
            risk_stat_card(ui, "总回水", &fmt_money(total_water), TEXT, card_w);
            risk_stat_card(ui, "总盈亏", &fmt_money(total_profit), TEXT, card_w);
        });
        ui.add_space(8.0);

        let stats = crate::risk::play_stats(bets, self.risk_xm_region, mul_sep);
        let prob_w = ((ui.available_width() - 30.0) / 4.0).max(100.0);
        ui.horizontal(|ui| {
            for (i, label) in crate::risk::PLAY_LABELS.iter().enumerate() {
                let distinct = stats[i].distinct;
                let text = if distinct == 0 {
                    "—".to_string()
                } else {
                    format_probability(distinct as f64 / crate::risk::TOTAL_COMBOS[i])
                };
                let hint = format!(
                    "{}：已收集 {} 个组合 / 全部 {} 个",
                    label, distinct, crate::risk::TOTAL_COMBOS[i] as u64
                );
                let _ = risk_prob_card(ui, label, &text, prob_w).on_hover_text(hint);
            }
        });
        ui.add_space(10.0);

        let page_size = self.risk_xm_page_size.max(1);
        let total_pages = rows.len().div_ceil(page_size).max(1);
        let page = self.risk_xm_page.clamp(1, total_pages);
        self.risk_xm_page = page;
        let start = (page - 1) * page_size;
        let page_rows = &rows[start..(start + page_size).min(rows.len())];

        // 多预留一些底部空间，分页不要贴着窗口底边。
        let table_h = (ui.available_height() - 60.0).max(120.0);
        let table_w = (ui.available_width() - 2.0).max(200.0);
        let widths = risk_xiaoma_widths(table_w);
        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .show(ui, |ui| {
                ui.set_min_width(table_w);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                if let Some(clicked) = risk_xiaoma_header_row(
                    ui,
                    &widths,
                    self.risk_xm_sort,
                    self.risk_xm_desc,
                ) {
                    if self.risk_xm_sort == clicked {
                        self.risk_xm_desc = !self.risk_xm_desc;
                    } else {
                        self.risk_xm_sort = clicked;
                        self.risk_xm_desc = true;
                    }
                    self.risk_xm_page = 1;
                }

                if rows.is_empty() {
                    let text = if has_any {
                        "没有匹配的组合"
                    } else {
                        "这一天还没有小码记录"
                    };
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(widths.iter().sum::<f32>(), 70.0),
                        Sense::hover(),
                    );
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        text,
                        FontId::proportional(12.5),
                        MUTED,
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("risk_xiaoma_table")
                        .auto_shrink([false, false])
                        .max_height((table_h - 27.0).max(80.0))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            for (i, row) in page_rows.iter().enumerate() {
                                risk_xiaoma_row(ui, &widths, row, start + i + 1);
                            }
                        });
                }
            });

        ui.add_space(8.0);
        let (line_rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 1.0),
            Sense::hover(),
        );
        ui.painter().hline(
            line_rect.x_range(),
            line_rect.center().y,
            Stroke::new(1.0, REF_LINE),
        );
        ui.add_space(8.0);

        // 分页整体作为一组居中放在列表下方，所有文字 / 控件统一 11.5px。
        let page_label = format!("{page} / {total_pages}");
        let font = FontId::proportional(11.5);
        let text_w = |ui: &egui::Ui, s: &str| {
            ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(s.to_owned(), font.clone(), MUTED)
                    .size()
                    .x
            })
        };
        // 每个控件宽度 + 11 个 8px 间距；按钮按最小宽度 56 / 48 计。
        let group_w = text_w(ui, "每页")
            + 56.0
            + text_w(ui, "条")
            + 12.0
            + 56.0
            + text_w(ui, &page_label)
            + 56.0
            + 12.0
            + text_w(ui, "跳至")
            + 48.0
            + text_w(ui, "页")
            + 48.0
            + 11.0 * 8.0;
        // 估算宽度比实际略大，补回半个误差让整组正好落在列表中线。
        let leading = ((ui.available_width() - group_w) / 2.0 + 10.5).max(0.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().interact_size.y = 20.0;
            ui.add_space(leading);
            ui.label(RichText::new("每页").size(11.5).color(MUTED));
            egui::ComboBox::from_id_salt("risk_xm_page_size")
                .width(56.0)
                .selected_text(
                    RichText::new(self.risk_xm_page_size.to_string()).size(11.5),
                )
                .show_ui(ui, |ui| {
                    for n in [10usize, 20, 30, 40, 50, 100] {
                        if ui
                            .selectable_value(
                                &mut self.risk_xm_page_size,
                                n,
                                RichText::new(n.to_string()).size(11.5),
                            )
                            .clicked()
                        {
                            self.risk_xm_page = 1;
                        }
                    }
                });
            ui.label(RichText::new("条").size(11.5).color(MUTED));
            ui.add_space(12.0);
            if ghost_button_sized(
                ui,
                RichText::new("上一页").size(11.5).color(SUB),
                CornerRadius::same(8),
                Vec2::new(56.0, 22.0),
            )
            .clicked()
                && page > 1
            {
                self.risk_xm_page = page - 1;
            }
            ui.label(RichText::new(page_label).size(11.5).color(MUTED));
            if ghost_button_sized(
                ui,
                RichText::new("下一页").size(11.5).color(SUB),
                CornerRadius::same(8),
                Vec2::new(56.0, 22.0),
            )
            .clicked()
                && page < total_pages
            {
                self.risk_xm_page = page + 1;
            }
            ui.add_space(12.0);
            ui.label(RichText::new("跳至").size(11.5).color(MUTED));
            let mut resp = ui.add(
                egui::TextEdit::singleline(&mut self.risk_xm_jump)
                    .desired_width(48.0)
                    .font(font.clone())
                    .hint_text(RichText::new(page.to_string()).size(11.5)),
            );
            text_edit_context_menu(ui.ctx(), resp.id, &mut self.risk_xm_jump, &mut resp);
            ui.label(RichText::new("页").size(11.5).color(MUTED));
            if ghost_button_sized(
                ui,
                RichText::new("跳转").size(11.5).color(SUB),
                CornerRadius::same(8),
                Vec2::new(48.0, 22.0),
            )
            .clicked()
            {
                if let Ok(p) = self.risk_xm_jump.trim().parse::<usize>() {
                    self.risk_xm_page = p.clamp(1, total_pages);
                }
            }
        });
        // 与窗口底边再留一点呼吸空间。
        ui.add_space(10.0);
    }

    /// 当前分组过滤下，成员条要显示的联系人 / 群成员集合。
    fn rail_member_sets(&self) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
        let mut contacts: BTreeSet<String> = BTreeSet::new();
        let mut members: BTreeSet<(String, String)> = BTreeSet::new();
        match &self.order_filter {
            None => {
                for g in &self.groups {
                    contacts.extend(g.contacts.iter().cloned());
                    members.extend(g.members.iter().cloned());
                }
            }
            Some(name) => {
                if let Some(g) = self.groups.iter().find(|g| &g.name == name) {
                    contacts.extend(g.contacts.iter().cloned());
                    members.extend(g.members.iter().cloned());
                }
            }
        }
        (contacts, members)
    }

    /// 成员条当前的成员数（联系人 + 群成员）。
    fn rail_member_count(&self) -> usize {
        let (contacts, members) = self.rail_member_sets();
        contacts.len() + members.len()
    }

    /// 左侧竖排成员 Tab：全部分组 = 所有联系人 / 群成员；选定某分组 = 该分组成员。
    fn member_rail(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let filter_before = self.order_filter.clone();
        let (contacts, members) = self.rail_member_sets();
        let name_of = |k: &MemberKey| match k {
            MemberKey::Contact(c) => display_of_contact(data, c),
            MemberKey::Member(chat, m) => data
                .groups
                .iter()
                .find(|g| g.username == *chat)
                .and_then(|g| g.members.iter().find(|p| p.username == *m))
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| m.clone()),
        };
        let mut keys: Vec<MemberKey> = Vec::with_capacity(contacts.len() + members.len());
        keys.extend(contacts.into_iter().map(MemberKey::Contact));
        keys.extend(members.into_iter().map(|(c, m)| MemberKey::Member(c, m)));
        keys.sort_by(|a, b| name_of(a).cmp(&name_of(b)));
        // 上家固定排在最上面，与普通会员之间画一条红线隔开。
        let up_count = keys.iter().filter(|k| self.is_upline_key(k)).count();
        let mut ordered: Vec<MemberKey> = Vec::with_capacity(keys.len());
        ordered.extend(keys.iter().filter(|k| self.is_upline_key(k)).cloned());
        ordered.extend(keys.iter().filter(|k| !self.is_upline_key(k)).cloned());

        let group_names: Vec<String> = self.groups.iter().map(|g| g.name.clone()).collect();
        egui::Frame::new()
            .fill(RAIL_BG)
            // 全部成员面板左右留白固定 2px。
            .inner_margin(Margin::symmetric(2, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                // 分组下拉放在成员列表标题的位置：选中分组后，下面只列该组成员。
                ui.horizontal(|ui| {
                    if group_names.is_empty() {
                        ui.label(RichText::new("全部成员").size(10.5).strong().color(TEXT));
                    } else {
                        let selected = self
                            .order_filter
                            .clone()
                            .unwrap_or_else(|| "全部分组".to_string());
                        // 成员条很窄，只显示前四个字；完整分组名放在悬停提示里。
                        let short: String = selected.chars().take(4).collect();
                        egui::ComboBox::from_id_salt("member_rail_group_filter")
                            .width(ui.available_width())
                            .selected_text(RichText::new(short).size(10.5))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.order_filter, None, "全部分组");
                                for name in &group_names {
                                    ui.selectable_value(
                                        &mut self.order_filter,
                                        Some(name.clone()),
                                        name,
                                    );
                                }
                            })
                            .response
                            .on_hover_text(format!("按分组过滤成员与注单（当前：{selected}）"));
                        // 切换分组后，原来选中的成员 / 注单可能已不在本组，清除选中。
                        if self.order_filter != filter_before {
                            self.member_focus = None;
                            self.selected_order_row = None;
                            self.selected_order_key = None;
                            self.converted = None;
                            self.edit_text.clear();
                            self.edit_time.clear();
                            self.ocr_msg = None;
                        }
                    }
                });
                ui.add_space(4.0);
                hairline(ui);
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("member_rail_scroll")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (ki, key) in ordered.iter().enumerate() {
                            if up_count > 0 && ki == up_count && up_count < ordered.len() {
                                ui.add_space(2.0);
                                let (line, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 1.0),
                                    Sense::hover(),
                                );
                                ui.painter().hline(
                                    line.x_range(),
                                    line.center().y,
                                    Stroke::new(1.5, RED),
                                );
                                ui.add_space(2.0);
                            }
                            let label = truncate_display(&name_of(key), 4);
                            let selected = self.member_focus.as_ref() == Some(key);
                            if rail_item(ui, &label, selected) {
                                // 点成员只筛左侧注单；右侧面板不再显示整段成员消息。
                                // 再点一次同一个成员 = 取消筛选。
                                self.member_focus = if selected {
                                    None
                                } else {
                                    Some(key.clone())
                                };
                                self.selected_order_row = None;
                                self.selected_order_key = None;
                                self.converted = None;
                                self.edit_text.clear();
                                self.edit_time.clear();
                                self.ocr_msg = None;
                            }
                        }
                        if keys.is_empty() {
                            ui.add_space(10.0);
                            ui.label(RichText::new("还没有成员").size(11.5).color(SUB));
                        }
                    });
            });
    }

    /// 右侧详情面板：左侧为注单原文，右侧为「彩种 / 玩法 / 内容 / 注数 / 单注 / 小计」表格。
    fn detail_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(CARD)
            // 左侧留白 2px；明细表格右缘距窗口 5px。
            .inner_margin(Margin {
                left: 2,
                right: 5,
                top: 6,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                // 右键菜单的动作如果因为 egui 多 pass 没能在当帧执行，
                // 下一帧在这里补上。
                if let Some((id, action, range)) = self.pending_source_action.take() {
                    self.source_menu_action(ui.ctx(), id, action, range);
                }
                if let Some(row) = self.selected_order_row.clone() {
                    let reasons = self
                        .converted
                        .as_ref()
                        .map(|c| c.reasons.clone())
                        .unwrap_or_default();
                    let status = order_status(self.converted.as_ref());
                    let show_reasons = !reasons.is_empty();
                    let total_w = ui.available_width();
                    let body_h = ui.available_height();
                    // 明细列始终占满「原文」右侧的整块宽度：存疑原因改成明细上方
                    // 的横幅（见下），不再挤占表格的宽度。
                    // 原文列要放得下「识别图片 / 删除 / 取消存疑 / 提交」整排按钮，
                    // 太窄时 right_to_left 会往左溢出、盖住左边的注单消息面板；
                    // 宁可牺牲一点明细宽度（最小窗口已在 main.rs 相应调大）。
                    let left_w = 260.0;
                    let mid_w = (total_w - left_w - 3.0).max(160.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(left_w, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(left_w);
                                ui.set_min_height(body_h);
                                // 原文标题 / 时间 / 操作按钮都放在文本框面板上方。
                                let mut time_changed = false;
                                ui.horizontal(|ui| {
                                    section_label(ui, "原文");
                                    // 注数放在标题旁边，把整行宽度留给下面的操作按钮。
                                    if let Some(out) = &self.converted {
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(format!("{} 注", out.bets.len()))
                                                .size(10.5)
                                                .color(SUB),
                                        );
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(3.0);
                                        let mut resp = ui
                                            .add(
                                                egui::TextEdit::singleline(&mut self.edit_time)
                                                    .desired_width(68.0)
                                                    .font(egui::TextStyle::Small)
                                                    .hint_text("时间"),
                                            )
                                            .on_hover_text("可修改这条注单的时间");
                                        text_edit_context_menu(
                                            ui.ctx(),
                                            resp.id,
                                            &mut self.edit_time,
                                            &mut resp,
                                        );
                                        time_changed = resp.changed();
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("时间").size(10.5).color(SUB),
                                        );
                                    });
                                });
                                if time_changed {
                                    self.save_order_time();
                                }
                                ui.horizontal(|ui| {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(3.0);
                                        // 「识别图片」在删除按钮右边：识别剪贴板截图，
                                        // 把文字填进下面的原文编辑框。
                                        if self.ocr_rx.is_some() {
                                            styled_button_enabled(
                                                ui,
                                                RichText::new("识别中…").size(10.5).color(MUTED),
                                                false,
                                                BLUE_BG,
                                                BLUE_HOVER,
                                                Color32::from_rgb(0xC6, 0xDB, 0xF5),
                                                Stroke::new(1.0, BLUE_FG),
                                                Stroke::new(1.0, BLUE_FG),
                                                CornerRadius::same(5),
                                                Vec2::new(56.0, 22.0),
                                            );
                                        } else if styled_button(
                                            ui,
                                            RichText::new("识别图片").size(10.5).color(BLUE_FG),
                                            BLUE_BG,
                                            BLUE_HOVER,
                                            Color32::from_rgb(0xC6, 0xDB, 0xF5),
                                            Stroke::new(1.0, BLUE_FG),
                                            Stroke::new(1.0, BLUE_FG),
                                            CornerRadius::same(5),
                                            Vec2::new(56.0, 22.0),
                                        )
                                        .on_hover_text("识别剪贴板里的截图，把文字填入原文")
                                        .clicked()
                                        {
                                            let (tx, rx) = channel();
                                            self.ocr_rx = Some(rx);
                                            self.ocr_target = self.selected_order_key.clone();
                                            self.ocr_msg = None;
                                            ui.ctx()
                                                .request_repaint_after(Duration::from_millis(100));
                                            std::thread::spawn(move || {
                                                let _ = tx.send(crate::ocr::recognize_clipboard());
                                            });
                                        }
                                        ui.add_space(4.0);
                                        if styled_button(
                                            ui,
                                            RichText::new("删除").size(10.5).color(RED),
                                            RED_BG,
                                            RED_HOVER,
                                            Color32::from_rgb(0xFF, 0xD5, 0xCF),
                                            Stroke::new(1.0, RED),
                                            Stroke::new(1.0, RED),
                                            CornerRadius::same(5),
                                            Vec2::new(40.0, 22.0),
                                        )
                                        .on_hover_text("从当前注单列表删除（重新导入会恢复）")
                                        .clicked()
                                        {
                                            self.delete_order(&row);
                                        }
                                        ui.add_space(4.0);
                                        if status == OrderStatus::Doubtful {
                                            if styled_button(
                                                ui,
                                                RichText::new("取消存疑")
                                                    .size(10.5)
                                                    .color(BLUE_FG),
                                                BLUE_BG,
                                                BLUE_HOVER,
                                                Color32::from_rgb(0xC6, 0xDB, 0xF5),
                                                Stroke::new(1.0, BLUE_FG),
                                                Stroke::new(1.0, BLUE_FG),
                                                CornerRadius::same(5),
                                                Vec2::new(56.0, 22.0),
                                            )
                                            .on_hover_text("标记为已核对，不再显示存疑")
                                            .clicked()
                                            {
                                                self.clear_doubt(&row);
                                            }
                                            ui.add_space(4.0);
                                        }
                                        if primary_button(
                                            ui,
                                            RichText::new("提交")
                                                .size(10.5)
                                                .color(Color32::WHITE),
                                            true,
                                            CornerRadius::same(5),
                                            Vec2::new(40.0, 22.0),
                                        )
                                        .on_hover_text("保存修改后的原文并转换注单")
                                        .clicked()
                                        {
                                            let text = self.edit_text.clone();
                                            if self.doubt_cleared.remove(&edit_key(&row)) {
                                                doubt::save(&self.doubt_cleared);
                                            }
                                            let conv = safe_convert(
                                                &text,
                                                &self.keywords,
                                                &self.replacements,
                                            );
                                            self.auto_converted
                                                .insert(edit_key(&row), conv.clone());
                                            self.converted = Some(conv);
                                            self.save_order_edit(&row, text);
                                        }
                                    });
                                });
                                if let Some((msg, err)) = &self.ocr_msg {
                                    let color = if *err { RED } else { MONEY_GREEN };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(msg).size(10.0).color(color),
                                        )
                                        .wrap(),
                                    );
                                }
                                ui.add_space(4.0);
                                let original_box = egui::Frame::new()
                                    .fill(STRIPE)
                                    .stroke(Stroke::new(1.0, BORDER))
                                    // 原文面板：内容左右各留 5px 内边距，文本框不做圆角。
                                    .corner_radius(CornerRadius::ZERO)
                                    .inner_margin(Margin::symmetric(5, 8))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.set_min_height((body_h - 60.0).max(80.0));
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false, false])
                                            .id_salt("detail_edit_scroll")
                                            .show(ui, |ui| {
                                                // 固定 id，右键菜单和选区恢复都依赖它。
                                                let edit_id = egui::Id::unique((
                                                    "order_source_edit",
                                                    row.sort,
                                                    row.chat.as_str(),
                                                    row.sender.as_str(),
                                                ));
                                                // egui 的 TextEdit 会把右键按下当成普通点击，
                                                // 把已有选区折叠成一个光标。这里在右键按下前
                                                // 记住选区，画完立即恢复，保证菜单里的复制 /
                                                // 剪切 / 插入拖仍然作用于原来的选中内容。
                                                let prev_selection =
                                                    egui::text_edit::TextEditState::load(
                                                        ui.ctx(),
                                                        edit_id,
                                                    )
                                                    .and_then(|s| s.cursor.char_range())
                                                    .filter(|r| !r.is_empty());
                                                // 右键按下：开始一次菜单会话，记住右键前的选区。
                                                if ui.input(|i| i.pointer.secondary_pressed()) {
                                                    self.source_sel_snapshot = prev_selection
                                                        .map(|r| (edit_id, r));
                                                }
                                                let mut edit_out = ui
                                                    .scope(|ui| {
                                                        // 输入框 hover 高亮不要圆角。
                                                        let mut style = (**ui.style()).clone();
                                                        style.visuals.widgets.inactive.corner_radius =
                                                            CornerRadius::ZERO;
                                                        style.visuals.widgets.hovered.corner_radius =
                                                            CornerRadius::ZERO;
                                                        style.visuals.widgets.active.corner_radius =
                                                            CornerRadius::ZERO;
                                                        // 原文文本框行高比字体自然行高再松 3px。
                                                        style.spacing.extra_text_line_spacing = 3.0;
                                                        ui.set_style(style);
                                                        egui::TextEdit::multiline(&mut self.edit_text)
                                                            .id(edit_id)
                                                            .frame(egui::Frame::NONE)
                                                            .margin(Margin::ZERO)
                                                            .desired_width(f32::INFINITY)
                                                            .desired_rows(20)
                                                            .font(egui::FontId::proportional(12.5))
                                                            .hint_text("在这里修改原文")
                                                            .show(ui)
                                                    })
                                                    .inner;
                                                // 文本框失去焦点（菜单打开）时 egui 会折叠选区，
                                                // 菜单存续期间每帧用快照恢复，保证所有菜单项
                                                // 都能拿到原来的选择范围。
                                                let saved_selection = self
                                                    .source_sel_snapshot
                                                    .as_ref()
                                                    .filter(|(id, _)| *id == edit_id)
                                                    .map(|(_, range)| *range);
                                                if let Some(sel) = saved_selection {
                                                    if edit_out
                                                        .state
                                                        .cursor
                                                        .char_range()
                                                        .is_none_or(|r| r.is_empty())
                                                    {
                                                        edit_out
                                                            .state
                                                            .cursor
                                                            .set_char_range(Some(sel));
                                                        // store 会消耗 state，先克隆一份写回；
                                                        // 本帧后续还要读它来生成右键菜单。
                                                        edit_out
                                                            .state
                                                            .clone()
                                                            .store(ui.ctx(), edit_id);
                                                    }
                                                }
                                                let sel_range = edit_out
                                                    .state
                                                    .cursor
                                                    .char_range()
                                                    .or_else(|| {
                                                        self.source_sel_snapshot
                                                            .as_ref()
                                                            .filter(|(id, _)| *id == edit_id)
                                                            .map(|(_, range)| *range)
                                                    });
                                                let sel_text = sel_range
                                                    .map(|r| {
                                                        r.slice_str(&self.edit_text).to_string()
                                                    })
                                                    .unwrap_or_default();
                                                let has_sel = !sel_text.trim().is_empty();
                                                // 空白选区也允许做批量回车 / 批量替换
                                                // （参考项目按 selectionStart/End 判，不 trim）。
                                                let has_any_sel =
                                                    sel_range.is_some_and(|r| !r.is_empty());
                                                let can_drag = has_sel
                                                    && convert::insert_drag_text(
                                                        &sel_text,
                                                        &self.keywords,
                                                    )
                                                    .is_some();
                                                let mul_sep = self
                                                    .keywords
                                                    .get(crate::keywords::Field::Multiplication)
                                                    .iter()
                                                    .find(|k| !k.is_empty())
                                                    .cloned()
                                                    .unwrap_or_else(|| "拖".to_string());
                                                // 菜单是否在这帧之前就已经打开（用于判断这一下
                                                // 鼠标点击是在点菜单，而不是点文本框）。
                                                let menu_was_open =
                                                    edit_out.response.context_menu_opened();
                                                edit_out.response.context_menu(|ui| {
                                                    compact_context_menu(ui);
                                                    if ui
                                                        .add_enabled(
                                                            has_sel,
                                                            egui::Button::new("复制")
                                                                .shortcut_text("Ctrl+C"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action =
                                                            Some((edit_id, SourceMenuAction::Copy, sel_range));
                                                        ui.close();
                                                    }
                                                    if ui
                                                        .add(
                                                            egui::Button::new("粘贴")
                                                                .shortcut_text("Ctrl+V"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action =
                                                            Some((edit_id, SourceMenuAction::Paste, sel_range));
                                                        ui.close();
                                                    }
                                                    if ui
                                                        .add_enabled(
                                                            has_sel,
                                                            egui::Button::new("剪切")
                                                                .shortcut_text("Ctrl+X"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action =
                                                            Some((edit_id, SourceMenuAction::Cut, sel_range));
                                                        ui.close();
                                                    }
                                                    ui.separator();
                                                    if ui
                                                        .add(
                                                            egui::Button::new("全选")
                                                                .shortcut_text("Ctrl+A"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action =
                                                            Some((edit_id, SourceMenuAction::SelectAll, sel_range));
                                                        ui.close();
                                                    }
                                                    ui.separator();
                                                    if ui
                                                        .add_enabled(
                                                            can_drag,
                                                            egui::Button::new(format!(
                                                                "插入{mul_sep}"
                                                            )),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action = Some((
                                                            edit_id,
                                                            SourceMenuAction::InsertDrag,
                                                            sel_range,
                                                        ));
                                                        ui.close();
                                                    }
                                                    if ui
                                                        .add_enabled(
                                                            has_sel
                                                                && !sel_text.contains('\n'),
                                                            egui::Button::new("批量回车"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action = Some((
                                                            edit_id,
                                                            SourceMenuAction::BatchNewline,
                                                            sel_range,
                                                        ));
                                                        ui.close();
                                                    }
                                                    if ui
                                                        .add_enabled(
                                                            has_any_sel,
                                                            egui::Button::new("批量替换"),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.pending_source_action = Some((
                                                            edit_id,
                                                            SourceMenuAction::BatchReplace,
                                                            sel_range,
                                                        ));
                                                        ui.close();
                                                    }
                                                });
                                                if let Some((id, action, range)) =
                                                    self.pending_source_action.take()
                                                {
                                                    self.source_menu_action(
                                                        ui.ctx(),
                                                        id,
                                                        action,
                                                        range,
                                                    );
                                                }
                                                // 只有在「菜单本来没开」时发生的左键点击
                                                // 才清掉快照；点菜单项那一下不能清，否则
                                                // 选区会立刻丢失，后面的复制/剪切就失效。
                                                if ui.input(|i| i.pointer.primary_pressed())
                                                    && !menu_was_open
                                                    && !ui.input(|i| i.pointer.secondary_pressed())
                                                {
                                                    self.source_sel_snapshot = None;
                                                }
                                            });
                                    });
                                // 文本编辑默认的 I 形光标细、在浅色背景上不容易找；
                                // 整个原文框改用清晰的手形光标，并加一圈描边提示可编辑区域。
                                if let Some(pos) = ui.ctx().pointer_hover_pos() {
                                    if original_box.response.rect.contains(pos) {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        ui.painter().rect_stroke(
                                            original_box.response.rect,
                                            CornerRadius::same(8),
                                            Stroke::new(1.5, GREEN),
                                            egui::StrokeKind::Inside,
                                        );
                                    }
                                }
                            },
                        );
                        ui.add_space(3.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(mid_w, body_h),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_min_size(Vec2::new(mid_w, body_h));
                                let converted = self.converted.clone();
                                let mul_sep = self
                                    .keywords
                                    .get(crate::keywords::Field::Multiplication)
                                    .iter()
                                    .find(|k| !k.is_empty())
                                    .cloned()
                                    .unwrap_or_else(|| "拖".to_string());
                                let win_total: f64 = converted
                                    .as_ref()
                                    .map(|out| {
                                        out.bets
                                            .iter()
                                            .map(|b| {
                                                settle::evaluate(
                                                    b,
                                                    self.draws.get(&b.lottery),
                                                    &mul_sep,
                                                    self.detail_odds(&b.lottery),
                                                )
                                                .win_amount
                                            })
                                            .sum()
                                    })
                                    .unwrap_or(0.0);
                                if show_reasons {
                                    // 存疑原因横幅：放在明细上方、占满明细列宽度。
                                    // 以前是明细右侧的独立列（占 30% 宽度），会把表格
                                    // 挤到只剩几列、要横向滚动才看得全。
                                    let failed = status == OrderStatus::Failed;
                                    let color = if failed { RED } else { BLUE_FG };
                                    let title = format!(
                                        "{}（{}）",
                                        if failed { "失败原因" } else { "存疑原因" },
                                        reasons.len()
                                    );
                                    egui::CollapsingHeader::new(
                                        RichText::new(title).size(11.0).color(color),
                                    )
                                    .id_salt("doubt_reasons")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt("doubt_reason_scroll")
                                            .max_height(72.0)
                                            .auto_shrink([false, true])
                                            .show(ui, |ui| {
                                                for r in &reasons {
                                                    ui.horizontal_top(|ui| {
                                                        ui.label(
                                                            RichText::new("•")
                                                                .size(11.0)
                                                                .color(color),
                                                        );
                                                        ui.add(
                                                            egui::Label::new(
                                                                RichText::new(r)
                                                                    .size(10.5)
                                                                    .color(color),
                                                            )
                                                            .wrap()
                                                            .selectable(true),
                                                        );
                                                    });
                                                }
                                            });
                                    });
                                    ui.add_space(4.0);
                                }
                                ui.horizontal(|ui| {
                                    if let Some(out) = &converted {
                                        ui.label(
                                            RichText::new(format!(
                                                "总金额 {}",
                                                convert::fmt_amount(out.total_stake)
                                            ))
                                            .size(13.5)
                                            .strong()
                                            .color(GREEN_DEEP),
                                        );
                                        if win_total > 0.0 {
                                            ui.add_space(10.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "中奖 {}",
                                                    fmt_money(win_total)
                                                ))
                                                .size(13.5)
                                                .strong()
                                                .color(BLUE_FG),
                                            );
                                        }
                                        ui.add_space(10.0);
                                    }
                                    section_label(ui, "明细");
                                });
                                ui.add_space(6.0);
                                // 列宽固定，不随内容或窗口变化；内容列固定宽度内换行。
                                let (table_w, widths) = detail_widths();
                                egui::ScrollArea::horizontal()
                                    .id_salt("detail_table_hscroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        // 边框画在表格内容内侧：横向滚动区会裁掉
                                        // 外扩的描边，导致右缘看不到线。
                                        let table = egui::Frame::new().show(ui, |ui| {
                                                ui.set_width(table_w);
                                                // 表头与第一行之间不要额外的 item_spacing；
                                                // 单元格自己的上下留白固定 3px。
                                                ui.spacing_mut().item_spacing.y = 0.0;
                                                detail_table_header(ui, &widths);
                                                egui::ScrollArea::vertical()
                                                    .auto_shrink([false, false])
                                                    .id_salt("detail_table_scroll")
                                                    .show(ui, |ui| {
                                                        match &converted {
                                                            Some(out) if !out.bets.is_empty() => {
                                                                ui.spacing_mut()
                                                                    .item_spacing
                                                                    .y = 0.0;
                                                                for item in &out.bets {
                                                                    detail_table_row(
                                                                        ui,
                                                                        item,
                                                                        &widths,
                                                                        &mul_sep,
                                                                    );
                                                                }
                                                            }
                                                            Some(_) => {
                                                                ui.add_space(12.0);
                                                                ui.label(
                                                                    RichText::new(
                                                                        "没有可转换的注单内容",
                                                                    )
                                                                    .size(12.0)
                                                                    .color(SUB),
                                                                );
                                                            }
                                                            None => {
                                                                ui.add_space(12.0);
                                                                ui.label(
                                                                    RichText::new(
                                                                        "点击原文上方的「提交」转换注单",
                                                                    )
                                                                    .size(12.0)
                                                                    .color(SUB),
                                                                );
                                                            }
                                                        }
                                                    });
                                            });
                                        let rect = table.response.rect;
                                        ui.painter().rect_stroke(
                                            rect.shrink(0.5),
                                            CornerRadius::ZERO,
                                            Stroke::new(1.0, BORDER),
                                            egui::StrokeKind::Inside,
                                        );
                                    });
                            },
                        );
                    });
                } else {
                    empty_detail(
                        ui,
                        "未选择注单",
                        "点击一条注单查看原文与明细；点左侧成员可筛选该成员的注单。",
                    );
                }
            });
    }

    fn orders_panel(&mut self, ui: &mut egui::Ui, data: &AppData) {
        // 已撤回页签只显示当前时段内的记录，计数也按这个范围算。
        let revoked_count = self.revoked_in_range().len();
        // 标题行右侧的紧凑时段：`11-26 17:00–21:30`，完整说明放悬停。
        let range_short = format!(
            "{}月{}日 {}–{}",
            self.import_date.month(),
            self.import_date.day(),
            self.import_start.format("%H:%M"),
            self.import_end.format("%H:%M"),
        );
        let range_tip = format!(
            "当前只显示 {}年{}月{}日 {}–{} 的消息（含引用 / 已撤回）",
            self.import_date.year(),
            self.import_date.month(),
            self.import_date.day(),
            self.import_start.format("%H:%M"),
            self.import_end.format("%H:%M"),
        );
        egui::Frame::new()
            .fill(CARD)
            // 注单消息面板左右留白固定 2px。
            .inner_margin(Margin::symmetric(2, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());

                let visible = self.visible_order_indices();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("注单消息").size(12.5).strong().color(TEXT));
                    ui.spacing_mut().button_padding = Vec2::new(8.0, 2.0);
                    ui.add_space(6.0);
                    let single = egui::Button::new(
                        RichText::new("单条录入").size(11.0).color(GREEN),
                    )
                    .corner_radius(CornerRadius::same(5))
                    .min_size(Vec2::new(0.0, 20.0));
                    if ui
                        .add(single)
                        .on_hover_text("手动粘贴一条注单原文并转换录入")
                        .clicked()
                    {
                        self.single_open = true;
                        self.refresh_single_members(data);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&range_short).size(9.5).color(SUB))
                            .on_hover_text(range_tip);
                    });
                });
                // 三个页签：未撤回 / 引用 / 已撤回。
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let tabs = [
                        (OrderTab::Active, format!("未撤回 {}", self.orders.len())),
                        (OrderTab::References, format!("引用 {}", self.references.len())),
                        (OrderTab::Revoked, format!("已撤回 {revoked_count}")),
                    ];
                    for (tab, label) in tabs {
                        if order_tab_button(ui, &label, self.order_tab == tab).clicked() {
                            self.order_tab = tab;
                        }
                    }
                });
                // 未撤回以外的页签：分隔线后直接是列表。
                ui.add_space(3.0);
                if self.order_tab != OrderTab::Active {
                    hairline(ui);
                    ui.add_space(3.0);
                    if self.order_tab == OrderTab::References {
                        self.reference_tab_ui(ui);
                    } else {
                        self.revoked_tab_ui(ui);
                    }
                    return;
                }
                // 未撤回：上/下一条等操作，合并成列表上方的一行。
                // （分组过滤已移到左侧成员列表标题的位置。）
                ui.add_space(2.0);

                let mut nav: Option<isize> = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding.x = 6.0;
                    // 当前分组的成员数，放在导航按钮前面。
                    ui.label(RichText::new("分组成员数量：").size(10.0).color(SUB));
                    count_pill(ui, self.rail_member_count());
                    if ui
                        .add(tiny_button("上一条", TEXT))
                        .on_hover_text("选中上一条注单")
                        .clicked()
                    {
                        nav = Some(-1);
                    }
                    if ui
                        .add(tiny_button("下一条", TEXT))
                        .on_hover_text("选中下一条注单")
                        .clicked()
                    {
                        nav = Some(1);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let count = if visible.len() == self.orders.len() {
                            visible.len().to_string()
                        } else {
                            format!("{}/{}", visible.len(), self.orders.len())
                        };
                        ui.label(RichText::new(count).size(10.0).color(SUB));
                    });
                });
                if let Some(dir) = nav {
                    let visible = self.visible_order_indices();
                    self.select_adjacent_order(&visible, dir);
                }
                ui.add_space(3.0);
                hairline(ui);
                ui.add_space(2.0);

                match &self.orders_state {
                    OrdersState::Idle => {
                        empty_state(ui, "暂无注单消息");
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("用顶部「导入注单」回填历史消息")
                                    .size(11.5)
                                    .color(SUB),
                            );
                        });
                    }
                    OrdersState::Loading(s) => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(60.0);
                            ui.add(egui::Spinner::new().size(24.0).color(GREEN));
                            ui.add_space(12.0);
                            ui.label(RichText::new(s).size(12.5).color(SUB));
                        });
                    }
                    OrdersState::Failed(e) => {
                        ui.add_space(12.0);
                        ui.label(RichText::new("读取消息失败").size(13.0).strong().color(RED));
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).size(12.0).color(TEXT));
                    }
                    OrdersState::Ready => {
                        if self.orders.is_empty() {
                            empty_state(ui, "所选时间范围内没有消息");
                        } else if visible.is_empty() {
                            if self.member_focus.is_some() {
                                empty_state(ui, "该成员暂无注单消息");
                            } else {
                                empty_state(ui, "该分组暂无注单消息");
                            }
                        } else {
                            // 行与行之间只留 1px；踩中信息码的行下面还要多一行。
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let mut delete_row: Option<MsgRow> = None;
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .id_salt("orders_scroll")
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    for &i in &visible {
                                        let row = self.orders[i].clone();
                                        let edit_key = edit_key(&row);
                                        let sel_key = (
                                            row.sort,
                                            row.chat.clone(),
                                            row.nick.clone(),
                                            row.content.clone(),
                                        );
                                        let conv = self
                                            .auto_converted
                                            .get(&edit_key)
                                            .cloned()
                                            .unwrap_or_else(|| {
                                                let mut c = safe_convert(
                                                    &row.content,
                                                    &self.keywords,
                                                    &self.replacements,
                                                );
                                                self.apply_doubt_override(
                                                    &edit_key,
                                                    &mut c,
                                                );
                                                c
                                            });
                                        let info = self.info_hits_for(&row, &conv);
                                        let status = if info.is_some() {
                                            OrderStatus::Info
                                        } else {
                                            order_status(Some(&conv))
                                        };
                                        let selected = self
                                            .selected_order_key
                                            .as_ref()
                                            == Some(&sel_key);
                                        let response = msg_row(
                                            ui,
                                            &row,
                                            i,
                                            selected,
                                            status,
                                            info.as_ref(),
                                        );
                                        if selected && std::mem::take(&mut self.scroll_to_order) {
                                            response.scroll_to_me(Some(Align::Center));
                                        }
                                        if let Some(hit) = &info {
                                            let groups = self.info_hit_groups(hit);
                                            info_hit_line(ui, &groups);
                                        }
                                        response.context_menu(|ui| {
                                            compact_context_menu(ui);
                                            let del = egui::Button::new(
                                                RichText::new("删除").size(12.0).color(RED),
                                            );
                                            if ui.add(del).clicked() {
                                                delete_row = Some(row.clone());
                                                ui.close();
                                            }
                                        });
                                        if response.clicked() {
                                            self.selected_order_key = Some(sel_key);
                                            self.selected_order_row = Some(row.clone());
                                            self.converted = Some(conv);
                                            self.edit_text = row.content.clone();
                                            self.edit_time = row.time.clone();
                                            self.ocr_msg = None;
                                        }
                                    }
                                });
                            if let Some(row) = delete_row {
                                self.delete_order(&row);
                            }
                        }
                    }
                }
            });
    }

    /// 「引用」页签：在注单面板里直接查看引用消息。
    fn reference_tab_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let reserve = if self.reference_search.is_empty() { 0.0 } else { 40.0 };
            let width = (ui.available_width() - reserve).max(80.0);
            let mut resp = ui.add(
                egui::TextEdit::singleline(&mut self.reference_search)
                    .font(FontId::proportional(11.0))
                    .margin(Margin::symmetric(4, 1))
                    .hint_text(RichText::new("搜索引用 / 被引用 / 发送人").size(9.0))
                    .desired_width(width),
            );
            text_edit_context_menu(ui.ctx(), resp.id, &mut self.reference_search, &mut resp);
            if !self.reference_search.is_empty()
                && ui
                    .add(tiny_button("×", SUB))
                    .on_hover_text("清除搜索")
                    .clicked()
            {
                self.reference_search.clear();
            }
        });
        if let Some(msg) = &self.reference_msg {
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    RichText::new(msg)
                        .size(10.0)
                        .color(if self.reference_msg_error { RED } else { MUTED }),
                )
                .wrap(),
            );
        }
        ui.add_space(4.0);

        let query = self.reference_search.trim().to_lowercase();
        let rows: Vec<MsgRow> = self
            .references
            .iter()
            .rev()
            .filter(|r| {
                query.is_empty()
                    || r.content.to_lowercase().contains(&query)
                    || r.nick.to_lowercase().contains(&query)
                    || r.quote.as_ref().is_some_and(|q| {
                        q.content.to_lowercase().contains(&query)
                            || q.from.to_lowercase().contains(&query)
                    })
            })
            .cloned()
            .collect();
        if rows.is_empty() {
            empty_state(
                ui,
                if self.references.is_empty() {
                    "暂无引用消息"
                } else {
                    "没有匹配的引用"
                },
            );
            return;
        }

        let mut locate: Option<MsgRow> = None;
        let mut delete: Option<MsgRow> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("reference_tab_scroll")
            .show(ui, |ui| {
                for row in &rows {
                    let action = reference_card(ui, row);
                    if action.locate {
                        locate = Some(row.clone());
                    }
                    if action.delete {
                        delete = Some(row.clone());
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some(row) = delete {
            if self.delete_quoted_message(&row, ui.ctx()) {
                self.remove_reference(&row);
            }
        }
        if let Some(row) = locate {
            // 定位成功时 select_order_index 会切回「未撤回」；失败时留在本页签
            // 才能看到「正在读取 / 找不到」的提示。
            self.locate_quoted_message(&row, ui.ctx());
        }
    }

    /// 「已撤回」页签：本地持久化的已撤回消息。
    fn revoked_tab_ui(&mut self, ui: &mut egui::Ui) {
        let mut rows = self.revoked_in_range();
        if rows.is_empty() {
            empty_state(ui, "当前时段暂无已撤回消息");
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("对方撤回消息后会自动记录在这里，重新导入也不会再出现。")
                        .size(10.5)
                        .color(SUB),
                );
            });
            return;
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.sort));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("revoked_tab_scroll")
            .show(ui, |ui| {
                for r in rows {
                    egui::Frame::new()
                        .fill(CARD)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&r.time).size(10.5).color(SUB));
                                if !r.nick.is_empty() {
                                    ui.label(
                                        RichText::new(&r.nick)
                                            .size(11.5)
                                            .strong()
                                            .color(TEXT),
                                    );
                                }
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new("已撤回")
                                                .size(10.0)
                                                .strong()
                                                .color(RED)
                                                .background_color(RED_BG),
                                        );
                                    },
                                );
                            });
                            ui.add_space(2.0);
                            ui.label(RichText::new(&r.content).size(12.0).color(TEXT));
                        });
                    ui.add_space(6.0);
                }
            });
    }

    /// 当前分组 / 成员过滤下要显示的注单下标（保持原列表顺序）。
    fn visible_order_indices(&self) -> Vec<usize> {
        let group = self
            .order_filter
            .as_ref()
            .and_then(|name| self.groups.iter().find(|g| &g.name == name));
        self.orders
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                self.member_focus.as_ref().is_none_or(|k| k.matches_row(r))
                    && group.is_none_or(|g| g.matches(r))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// 在过滤后的注单列表里向上（-1）或向下（1）选中一条。
    fn select_adjacent_order(&mut self, visible: &[usize], dir: isize) {
        if visible.is_empty() {
            return;
        }
        let current = self.selected_order_key.as_ref().and_then(|key| {
            visible.iter().position(|&i| {
                let r = &self.orders[i];
                r.sort == key.0 && r.chat == key.1 && r.nick == key.2 && r.content == key.3
            })
        });
        let pos = match (current, dir < 0) {
            (Some(p), true) => (p + visible.len() - 1) % visible.len(),
            (Some(p), false) => (p + 1) % visible.len(),
            (None, true) => visible.len() - 1,
            (None, false) => 0,
        };
        let row = self.orders[visible[pos]].clone();
        let key = (row.sort, row.chat.clone(), row.nick.clone(), row.content.clone());
        let conv = self
            .auto_converted
            .get(&edit_key(&row))
            .cloned()
            .unwrap_or_else(|| {
                let mut conv = safe_convert(&row.content, &self.keywords, &self.replacements);
                self.apply_doubt_override(&edit_key(&row), &mut conv);
                conv
            });
        self.selected_order_key = Some(key);
        self.edit_time = row.time.clone();
        self.selected_order_row = Some(row.clone());
        self.scroll_to_order = true;
        self.converted = Some(conv);
        self.edit_text = row.content;
        self.ocr_msg = None;
    }

    // ---------------- 选定会员页 ----------------

    fn select_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        self.select_header(ui, data);
        ui.add_space(6.0);
        self.groups_bar(ui);
        ui.add_space(8.0);
        let frame = egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(10, 8));
        frame.show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            match self.tab {
                Tab::Contacts => self.contacts_ui(ui, data),
                Tab::Groups => self.groups_ui(ui, data),
            }
        });
    }

    fn select_header(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let hint = if self.tab == Tab::Contacts {
            "搜索联系人"
        } else {
            "搜索群聊或成员"
        };
        let mut new_tab = self.tab;
        ui.horizontal(|ui| {
            if tab_button(
                ui,
                format!("联系人   {}", data.contacts.len()),
                self.tab == Tab::Contacts,
            ) {
                new_tab = Tab::Contacts;
            }
            if tab_button(
                ui,
                format!("群聊   {}", data.groups.len()),
                self.tab == Tab::Groups,
            ) {
                new_tab = Tab::Groups;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xF5, 0xF6, 0xF7))
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(16))
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        let edit = egui::TextEdit::singleline(&mut self.search)
                            .frame(egui::Frame::NONE)
                            .hint_text(RichText::new(hint).color(SUB))
                            .desired_width(240.0)
                            .align(Align2::LEFT_CENTER);
                        let mut resp = ui.add(edit);
                        text_edit_context_menu(ui.ctx(), resp.id, &mut self.search, &mut resp);
                    });
            });
        });
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "提示：点击整行即可选择；群聊点击左侧箭头展开。右键联系人 / 群成员可快速加入分组，右键群聊可全选成员。若同时勾选群聊和成员，以成员选择为准；勾选后可「＋ 新建分组」",
                )
                .size(11.0)
                .color(SUB),
            )
            .wrap(),
        );
        self.tab = new_tab;
    }

    // ---------------- 会员分组 ----------------

    /// 选定会员页顶部的分组管理条：新建、覆盖加入、仅选本组、成员管理、删除。
    fn groups_bar(&mut self, ui: &mut egui::Ui) {
        let has_sel = !self.selected_contacts.is_empty() || !self.selected_members.is_empty();
        let mut create = false;
        let mut delete: Option<usize> = None;
        let mut apply: Option<usize> = None;
        let mut overwrite: Option<usize> = None;
        let mut append: Option<usize> = None;
        let mut manage: Option<String> = None;
        let mut rename: Option<String> = None;

        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("会员分组").size(13.0).strong().color(TEXT));
                    ui.label(
                        RichText::new(format!("{} 个", self.groups.len()))
                            .size(11.5)
                            .color(SUB),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(tiny_button("＋ 新建分组", GREEN)).clicked() {
                            create = true;
                        }
                    });
                });
                if self.groups.is_empty() {
                    ui.label(
                        RichText::new(
                            "新建分组默认为空；勾选联系人 / 群成员后点「加入当前分组」覆盖进去。主页注单消息可按分组过滤。",
                        )
                        .size(11.5)
                        .color(SUB),
                    );
                    return;
                }
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .max_height(96.0)
                    .auto_shrink([false, true])
                    .id_salt("group_bar_scroll")
                    .show(ui, |ui| {
                        for (i, g) in self.groups.iter().enumerate() {
                            let (c, m) = g.counts();
                            ui.horizontal(|ui| {
                                let name = egui::Button::new(
                                    RichText::new(&g.name).size(13.0).strong().color(TEXT),
                                )
                                .frame(false);
                                if ui
                                    .add(name)
                                    .on_hover_text("选中本组：把该组成员勾选到会员树")
                                    .clicked()
                                {
                                    apply = Some(i);
                                }
                                ui.label(
                                    RichText::new(format!("联系人 {c} · 群成员 {m}"))
                                        .size(11.5)
                                        .color(SUB),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(tiny_button("删除", RED)).clicked() {
                                        delete = Some(i);
                                    }
                                    if ui.add(tiny_button("改名", TEXT)).clicked() {
                                        rename = Some(g.name.clone());
                                    }
                                    if ui.add(tiny_button("成员管理", TEXT)).clicked() {
                                        manage = Some(g.name.clone());
                                    }
                                    if ui
                                        .add_enabled(has_sel, tiny_button("追加本组", GREEN))
                                        .on_disabled_hover_text("先勾选联系人 / 群成员（整群选择不会加入分组）")
                                        .on_hover_text("把当前勾选追加进本组，原有成员保留")
                                        .clicked()
                                    {
                                        append = Some(i);
                                    }
                                    if ui
                                        .add_enabled(has_sel, tiny_button("覆盖本组", TEXT))
                                        .on_disabled_hover_text("先勾选联系人 / 群成员（整群选择不会加入分组）")
                                        .on_hover_text("用当前勾选替换本组全部成员")
                                        .clicked()
                                    {
                                        overwrite = Some(i);
                                    }
                                    if ui.add(tiny_button("仅选本组", GREEN)).clicked() {
                                        apply = Some(i);
                                    }
                                });
                            });
                        }
                    });
            });

        if create {
            self.group_dialog_open = true;
            self.group_name.clear();
            self.group_error = None;
            self.group_manage = None;
            self.group_delete = None;
        }
        if let Some(i) = overwrite {
            self.groups[i].replace_selection(&self.selected_contacts, &self.selected_members);
            groups::save(&self.groups);
            // 分组内容变化会改变导入 / 监控来源。
            self.selection_dirty = true;
        }
        if let Some(i) = append {
            let contacts = self.selected_contacts.clone();
            let members = self.selected_members.clone();
            self.groups[i].contacts.extend(contacts);
            self.groups[i].members.extend(members);
            groups::save(&self.groups);
            self.selection_dirty = true;
        }
        if let Some(i) = apply {
            let g = &self.groups[i];
            self.selected_contacts = g.contacts.clone();
            self.selected_members = g.members.clone();
            self.selected_groups.clear();
            // 展开含有该组成员的群聊，树形结构里能直接看到勾选状态。
            for (chat, _) in &g.members {
                self.expanded.insert(chat.clone());
            }
            self.selection_dirty = true;
        }
        if let Some(name) = rename {
            self.group_rename = Some(name.clone());
            self.group_rename_input = name;
            self.group_rename_error = None;
            self.group_manage = None;
            self.group_delete = None;
        }
        if let Some(name) = manage {
            self.group_manage = Some(name);
            self.group_rename = None;
            self.group_delete = None;
        }
        if let Some(i) = delete {
            self.group_delete = Some(i);
            self.group_manage = None;
            self.group_rename = None;
        }
    }

    fn group_dialog(&mut self, ctx: &egui::Context) {
        if !self.group_dialog_open {
            return;
        }
        let mut open = self.group_dialog_open;
        let mut confirm = false;
        let mut cancel = false;
        let error = self.group_error.clone();

        egui::Window::new("新建会员分组")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(RichText::new("分组名称").size(13.0).color(SUB));
                let edit = egui::TextEdit::singleline(&mut self.group_name)
                    .hint_text(RichText::new("例如：A 组、老客户").color(SUB))
                    .desired_width(f32::INFINITY);
                let mut resp = ui.add(edit);
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.group_name, &mut resp);
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "新分组默认为空。创建后勾选联系人 / 群成员，点该分组的「加入当前分组」覆盖进去。",
                    )
                    .size(12.0)
                    .color(SUB),
                );
                if let Some(e) = &error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).size(12.0).color(RED));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(
                            ui,
                            RichText::new("创建分组").size(14.0).color(Color32::WHITE),
                            true,
                            CornerRadius::same(16),
                            Vec2::new(96.0, 32.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
                if enter {
                    confirm = true;
                }
            });

        if confirm {
            self.create_group();
        } else if cancel || !open {
            self.group_dialog_open = false;
            self.group_name.clear();
            self.group_error = None;
        }
    }

    fn create_group(&mut self) {
        let name = sanitize_group_name(&self.group_name);
        if name.is_empty() {
            self.group_error = Some("请输入分组名称。".into());
            return;
        }
        if self.groups.iter().any(|g| g.name == name) {
            self.group_error = Some("已有同名分组，请换一个名称。".into());
            return;
        }
        self.groups.push(MemberGroup {
            name,
            ..Default::default()
        });
        groups::save(&self.groups);
        // 首个分组会让来源从「会员选择」切换到「全部分组」。
        self.selection_dirty = true;
        self.group_dialog_open = false;
        self.group_name.clear();
        self.group_error = None;
    }

    fn group_rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(old) = self.group_rename.clone() else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let error = self.group_rename_error.clone();

        egui::Window::new("修改分组名称")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(RichText::new("分组名称").size(13.0).color(SUB));
                let edit = egui::TextEdit::singleline(&mut self.group_rename_input)
                    .hint_text(RichText::new("例如：A 组、老客户").color(SUB))
                    .desired_width(f32::INFINITY);
                let mut resp = ui.add(edit);
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.group_rename_input, &mut resp);
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if let Some(e) = &error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).size(12.0).color(RED));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(
                            ui,
                            RichText::new("保存").size(14.0).color(Color32::WHITE),
                            true,
                            CornerRadius::same(16),
                            Vec2::new(88.0, 32.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
                if enter {
                    confirm = true;
                }
            });

        if confirm {
            self.rename_group(&old);
        } else if cancel || !open {
            self.group_rename = None;
            self.group_rename_error = None;
        }
    }

    fn rename_group(&mut self, old: &str) {
        let name = sanitize_group_name(&self.group_rename_input);
        if name.is_empty() {
            self.group_rename_error = Some("请输入分组名称。".into());
            return;
        }
        if self
            .groups
            .iter()
            .any(|g| g.name != old && g.name == name)
        {
            self.group_rename_error = Some("已有同名分组，请换一个名称。".into());
            return;
        }
        let Some(g) = self.groups.iter_mut().find(|g| g.name == old) else {
            self.group_rename = None;
            return;
        };
        g.name = name.clone();
        // 下拉过滤 / 正在管理的分组同步改名。
        if self.order_filter.as_deref() == Some(old) {
            self.order_filter = Some(name.clone());
        }
        if self.group_manage.as_deref() == Some(old) {
            self.group_manage = Some(name.clone());
        }
        groups::save(&self.groups);
        self.group_rename = None;
        self.group_rename_input.clear();
        self.group_rename_error = None;
    }

    /// 管理某个分组内的联系人 / 群成员，可逐条移除。
    fn group_manage_dialog(&mut self, ctx: &egui::Context, data: &AppData) {
        let Some(name) = self.group_manage.clone() else {
            return;
        };
        let Some(idx) = self.groups.iter().position(|g| g.name == name) else {
            self.group_manage = None;
            return;
        };

        let mut contacts: Vec<(String, String)> = self.groups[idx]
            .contacts
            .iter()
            .map(|u| (u.clone(), display_of_contact(data, u)))
            .collect();
        contacts.sort_by(|a, b| a.1.cmp(&b.1));
        let mut members: Vec<((String, String), String)> = self.groups[idx]
            .members
            .iter()
            .map(|k| (k.clone(), display_of_member(data, &k.0, &k.1)))
            .collect();
        members.sort_by(|a, b| a.1.cmp(&b.1));

        let mut open = true;
        let mut close_clicked = false;
        let mut remove_contact: Option<String> = None;
        let mut remove_member: Option<(String, String)> = None;
        let mut toggle_contact: Option<String> = None;
        let mut toggle_member: Option<(String, String)> = None;

        egui::Window::new(format!("管理分组「{name}」"))
            .collapsible(false)
            .resizable(true)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_size([440.0, 420.0])
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "移除分组内的联系人 / 群成员；点「设为上家」后，他发的注单在风险页按减额抵扣（金额可为负）。只影响分组本身，不影响会员选择和已导入的注单。",
                    )
                    .size(11.5)
                    .color(SUB),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .auto_shrink([false, false])
                    .id_salt("group_manage_scroll")
                    .show(ui, |ui| {
                        if contacts.is_empty() && members.is_empty() {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "该分组还没有成员。关闭后在「选定会员」页勾选联系人 / 群成员，再点「加入当前分组」。",
                                )
                                .size(11.5)
                                .color(SUB),
                            );
                            return;
                        }
                        ui.label(
                            RichText::new(format!("联系人 {}", contacts.len()))
                                .size(12.5)
                                .strong()
                                .color(SUB),
                        );
                        if contacts.is_empty() {
                            ui.label(RichText::new("（无）").size(12.0).color(SUB));
                        }
                        for (u, display) in &contacts {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(display).size(13.0).color(TEXT))
                                    .on_hover_text(u);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(tiny_button("移除", RED)).clicked() {
                                        remove_contact = Some(u.clone());
                                    }
                                    let is_up = self.groups[idx].upline_contacts.contains(u);
                                    let label = if is_up { "取消上家" } else { "设为上家" };
                                    let color = if is_up { GOLD_FG } else { SUB };
                                    if ui.add(tiny_button(label, color)).clicked() {
                                        toggle_contact = Some(u.clone());
                                    }
                                });
                            });
                        }
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("群成员 {}", members.len()))
                                .size(12.5)
                                .strong()
                                .color(SUB),
                        );
                        if members.is_empty() {
                            ui.label(RichText::new("（无）").size(12.0).color(SUB));
                        }
                        for (key, display) in &members {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(display).size(13.0).color(TEXT))
                                    .on_hover_text(format!("群 {} / 成员 {}", key.0, key.1));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(tiny_button("移除", RED)).clicked() {
                                        remove_member = Some(key.clone());
                                    }
                                    let is_up =
                                        self.groups[idx].upline_members.contains(key);
                                    let label = if is_up { "取消上家" } else { "设为上家" };
                                    let color = if is_up { GOLD_FG } else { SUB };
                                    if ui.add(tiny_button(label, color)).clicked() {
                                        toggle_member = Some(key.clone());
                                    }
                                });
                            });
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost_button_sized(ui, RichText::new("关闭").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            close_clicked = true;
                        }
                    });
                });
            });

        let mut changed = false;
        if let Some(u) = remove_contact {
            if self.groups[idx].contacts.remove(&u) {
                changed = true;
                // 移出分组的同时取消上家标记。
                self.groups[idx].upline_contacts.remove(&u);
                // 同步取消树形结构里的勾选。
                self.selected_contacts.remove(&u);
            }
        }
        if let Some(k) = remove_member {
            if self.groups[idx].members.remove(&k) {
                changed = true;
                self.groups[idx].upline_members.remove(&k);
                self.selected_members.remove(&k);
                self.expanded.insert(k.0.clone());
            }
        }
        if let Some(u) = toggle_contact {
            if !self.groups[idx].upline_contacts.remove(&u) {
                self.groups[idx].upline_contacts.insert(u);
            }
            changed = true;
        }
        if let Some(k) = toggle_member {
            if !self.groups[idx].upline_members.remove(&k) {
                self.groups[idx].upline_members.insert(k);
            }
            changed = true;
        }
        if changed {
            groups::save(&self.groups);
            self.selection_dirty = true;
        }
        if !open || close_clicked {
            self.group_manage = None;
        }
    }

    fn group_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.group_delete else {
            return;
        };
        let name = match self.groups.get(idx) {
            Some(g) => g.name.clone(),
            None => {
                self.group_delete = None;
                return;
            }
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new("删除分组")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(360.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(format!("确定删除分组「{name}」？"))
                        .size(13.5)
                        .color(TEXT),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("只删除分组本身，当前会员选择与注单消息不受影响。")
                        .size(11.5)
                        .color(SUB),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if danger_button(
                            ui,
                            RichText::new("删除").size(14.0).color(Color32::WHITE),
                            true,
                            CornerRadius::same(16),
                            Vec2::new(88.0, 32.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });

        if confirm {
            self.groups.remove(idx);
            if self.order_filter.as_deref() == Some(name.as_str()) {
                self.order_filter = None;
            }
            if self.group_manage.as_deref() == Some(name.as_str()) {
                self.group_manage = None;
            }
            groups::save(&self.groups);
            self.selection_dirty = true;
            self.group_delete = None;
        } else if cancel || !open {
            self.group_delete = None;
        }
    }

    /// 「关键词」弹窗：玩法关键词 / 金额识别 / 区域 & 其他。
    fn keywords_dialog(&mut self, ctx: &egui::Context) {
        if !self.keywords_open {
            return;
        }
        let mut open = true;
        let mut section = self.keyword_section;

        let metas = keywords::groups();
        let mut all_items: HashMap<Field, Vec<String>> = HashMap::new();
        for g in metas {
            all_items.insert(g.field, self.keywords.get(g.field).to_vec());
        }
        let mut buffers: HashMap<Field, String> = metas
            .iter()
            .map(|g| {
                (
                    g.field,
                    self.keyword_inputs.get(&g.field).cloned().unwrap_or_default(),
                )
            })
            .collect();
        let mut owners: HashMap<String, &'static str> = HashMap::new();
        for g in metas {
            if let Some(items) = all_items.get(&g.field) {
                for w in items {
                    owners.insert(w.clone(), g.title);
                }
            }
        }
        let mut zodiac_items: Vec<(String, String)> = self
            .keywords
            .zodiac_variants
            .iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect();
        let mut changed = false;
        let mut reset = false;

        egui::Window::new("关键词")
            .collapsible(false)
            .resizable(true)
            .default_size([700.0, 640.0])
            .min_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.horizontal(|ui| {
                    for sec in Section::ALL {
                        if tab_button(ui, sec.label().to_string(), section == *sec) {
                            section = *sec;
                        }
                        ui.add_space(6.0);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(tiny_button("恢复默认", SUB))
                            .on_hover_text("把关键词恢复为内置默认配置，并覆盖本机当前设置")
                            .clicked()
                        {
                            reset = true;
                        }
                    });
                });
                ui.add_space(6.0);
                hairline(ui);
                ui.add_space(6.0);
                ui.label(RichText::new(section.description()).size(11.5).color(SUB));
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("keywords_scroll")
                    .show(ui, |ui| {
                        for g in metas.iter().filter(|g| g.section == section) {
                            let Some(items) = all_items.get_mut(&g.field) else {
                                continue;
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(g.title).size(12.5).strong().color(TEXT),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    count_pill(ui, items.len());
                                });
                            });
                            ui.label(RichText::new(g.description).size(10.5).color(SUB));
                            ui.add_space(5.0);

                            let mut remove: Option<usize> = None;
                            ui.horizontal_wrapped(|ui| {
                                for (i, item) in items.iter().enumerate() {
                                    let chip = egui::Button::new(
                                        RichText::new(format!("{item}  ×")).size(11.5).color(TEXT),
                                    )
                                    .corner_radius(CornerRadius::same(10))
                                    .min_size(Vec2::new(0.0, 24.0));
                                    if ui
                                        .add(chip)
                                        .on_hover_text(format!("点击删除「{item}」"))
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                }
                                if items.is_empty() {
                                    ui.label(RichText::new("暂无关键词").size(11.0).color(SUB));
                                }
                            });
                            if let Some(i) = remove {
                                let word = items.remove(i);
                                owners.remove(&word);
                                changed = true;
                            }

                            ui.add_space(5.0);
                            let buf = buffers.entry(g.field).or_default();
                            let mut add = false;
                            ui.horizontal(|ui| {
                                let mut resp = ui.add(
                                    egui::TextEdit::singleline(&mut *buf)
                                        .hint_text(g.placeholder)
                                        .desired_width(220.0),
                                );
                                text_edit_context_menu(
                                    ui.ctx(),
                                    resp.id,
                                    &mut *buf,
                                    &mut resp,
                                );
                                let word = buf.trim().to_string();
                                let duplicate = !word.is_empty() && items.iter().any(|x| *x == word);
                                let owner = if !word.is_empty() && !duplicate {
                                    owners.get(&word).copied()
                                } else {
                                    None
                                };
                                let blocked = word.is_empty() || duplicate || owner.is_some();
                                let btn = egui::Button::new(
                                    RichText::new("添加").size(12.0).color(TEXT),
                                )
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(56.0, 26.0));
                                if ui.add_enabled(!blocked, btn).clicked() {
                                    add = true;
                                } else if owner.is_some() {
                                    ui.label(
                                        RichText::new(format!(
                                            "「{word}」已用于「{}」",
                                            owner.unwrap()
                                        ))
                                        .size(10.5)
                                        .color(RED),
                                    );
                                } else if duplicate {
                                    ui.label(
                                        RichText::new(format!("「{word}」已在本组"))
                                            .size(10.5)
                                            .color(RED),
                                    );
                                }
                            });
                            if add {
                                let word = buf.trim().to_string();
                                if !word.is_empty() {
                                    items.push(word.clone());
                                    owners.insert(word, g.title);
                                    buf.clear();
                                    changed = true;
                                }
                            }
                            ui.add_space(8.0);
                            hairline(ui);
                            ui.add_space(10.0);
                        }

                        if section == Section::Region {
                            ui.label(
                                RichText::new("生肖生僻字映射").size(12.5).strong().color(TEXT),
                            );
                            ui.label(
                                RichText::new(
                                    "设置易输错的生肖字与正确生肖字的对应关系（如：免→兔、候→猴）。",
                                )
                                .size(10.5)
                                .color(SUB),
                            );
                            ui.add_space(5.0);
                            let mut remove: Option<String> = None;
                            ui.horizontal_wrapped(|ui| {
                                for (variant, correct) in &zodiac_items {
                                    let chip = egui::Button::new(
                                        RichText::new(format!("{variant} → {correct}  ×"))
                                            .size(11.5)
                                            .color(TEXT),
                                    )
                                    .corner_radius(CornerRadius::same(10))
                                    .min_size(Vec2::new(0.0, 24.0));
                                    if ui
                                        .add(chip)
                                        .on_hover_text(format!("点击删除「{variant}→{correct}」"))
                                        .clicked()
                                    {
                                        remove = Some(variant.clone());
                                    }
                                }
                                if zodiac_items.is_empty() {
                                    ui.label(RichText::new("暂无映射").size(11.0).color(SUB));
                                }
                            });
                            if let Some(v) = remove {
                                zodiac_items.retain(|(a, _)| *a != v);
                                self.keywords.zodiac_variants.remove(&v);
                                changed = true;
                            }
                            ui.add_space(5.0);
                            ui.horizontal(|ui| {
                                let mut resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.keyword_variant)
                                        .hint_text("生僻字")
                                        .desired_width(80.0),
                                );
                                text_edit_context_menu(
                                    ui.ctx(),
                                    resp.id,
                                    &mut self.keyword_variant,
                                    &mut resp,
                                );
                                ui.label(RichText::new("→").size(12.0).color(SUB));
                                let mut resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.keyword_correct)
                                        .hint_text("正确生肖")
                                        .desired_width(80.0),
                                );
                                text_edit_context_menu(
                                    ui.ctx(),
                                    resp.id,
                                    &mut self.keyword_correct,
                                    &mut resp,
                                );
                                let v = self
                                    .keyword_variant
                                    .trim()
                                    .chars()
                                    .next()
                                    .map(|c| c.to_string())
                                    .unwrap_or_default();
                                let c = self
                                    .keyword_correct
                                    .trim()
                                    .chars()
                                    .next()
                                    .map(|c| c.to_string())
                                    .unwrap_or_default();
                                let btn = egui::Button::new(
                                    RichText::new("添加").size(12.0).color(TEXT),
                                )
                                .corner_radius(CornerRadius::same(6))
                                .min_size(Vec2::new(56.0, 26.0));
                                if ui.add_enabled(!v.is_empty() && !c.is_empty(), btn).clicked() {
                                    self.keywords.zodiac_variants.insert(v, c);
                                    self.keyword_variant.clear();
                                    self.keyword_correct.clear();
                                    changed = true;
                                }
                            });
                        }
                    });
            });

        if reset {
            // 恢复为编译进 exe 的默认关键词，并覆盖本机保存的配置。
            self.keywords = KeywordSettings::default();
            self.keyword_inputs.clear();
            self.keyword_variant.clear();
            self.keyword_correct.clear();
            keywords::save(&self.keywords);
            self.recompute_all_conversions();
        } else {
            for (f, s) in buffers {
                self.keyword_inputs.insert(f, s);
            }
            if changed {
                for (f, v) in all_items {
                    self.keywords.set(f, v);
                }
                keywords::save(&self.keywords);
                // 关键词变了，之前的自动转换结果（含存疑状态）全部失效：重算所有注单。
                self.recompute_all_conversions();
            }
        }
        self.keyword_section = section;
        if !open {
            self.keywords_open = false;
        }
    }

    /// 「替换词」弹窗：转换前按顺序把 A 词替换成 B 词。
    fn replacements_dialog(&mut self, ctx: &egui::Context) {
        if !self.replacements_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut reset = false;
        let mut remove: Option<usize> = None;
        let mut add = false;

        egui::Window::new("替换词")
            .collapsible(false)
            .resizable(true)
            .default_size([540.0, 480.0])
            .min_width(440.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "转换前按顺序把左边的词替换成右边的词，例如把常写错的「复试」替换为「复式」。",
                    )
                    .size(11.0)
                    .color(SUB),
                );
                ui.add_space(8.0);
                hairline(ui);
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(300.0)
                    .id_salt("replacements_scroll")
                    .show(ui, |ui| {
                        if self.replacements.is_empty() {
                            ui.label(RichText::new("暂无替换规则").size(11.5).color(SUB));
                        }
                        for (i, (from, to)) in self.replacements.iter().enumerate() {
                            let chip = egui::Button::new(
                                RichText::new(format!("{from} → {to}  ×"))
                                    .size(11.5)
                                    .color(TEXT),
                            )
                            .corner_radius(CornerRadius::same(10))
                            .min_size(Vec2::new(0.0, 24.0));
                            if ui
                                .add(chip)
                                .on_hover_text("点击删除这条规则")
                                .clicked()
                            {
                                remove = Some(i);
                            }
                            ui.add_space(4.0);
                        }
                    });

                ui.add_space(8.0);
                hairline(ui);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let mut resp = ui.add(
                        egui::TextEdit::singleline(&mut self.replacement_from)
                            .hint_text("原词，如：复试")
                            .desired_width(150.0),
                    );
                    text_edit_context_menu(
                        ui.ctx(),
                        resp.id,
                        &mut self.replacement_from,
                        &mut resp,
                    );
                    ui.label(RichText::new("→").size(12.0).color(SUB));
                    let mut resp = ui.add(
                        egui::TextEdit::singleline(&mut self.replacement_to)
                            .hint_text("替换为，如：复式")
                            .desired_width(150.0),
                    );
                    text_edit_context_menu(
                        ui.ctx(),
                        resp.id,
                        &mut self.replacement_to,
                        &mut resp,
                    );
                    let from = self.replacement_from.trim().to_string();
                    let to = self.replacement_to.trim().to_string();
                    let btn = egui::Button::new(
                        RichText::new("添加").size(12.0).color(TEXT),
                    )
                    .corner_radius(CornerRadius::same(6))
                    .min_size(Vec2::new(56.0, 26.0));
                    if ui
                        .add_enabled(!from.is_empty() && !to.is_empty(), btn)
                        .clicked()
                    {
                        add = true;
                    }
                });
                if !self.replacement_from.trim().is_empty()
                    && self.replacement_to.trim().is_empty()
                {
                    ui.label(
                        RichText::new("请填写替换后的词").size(10.5).color(RED),
                    );
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("替换只影响转换识别，不会修改原文。")
                            .size(10.5)
                            .color(SUB),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(tiny_button("恢复默认", SUB))
                            .on_hover_text("恢复为内置默认替换词，并覆盖本机当前设置")
                            .clicked()
                        {
                            reset = true;
                        }
                    });
                });
            });

        if reset {
            // 恢复为编译进 exe 的默认替换词，并覆盖本机保存的规则。
            self.replacements = replacements::default_rules();
            self.replacement_from.clear();
            self.replacement_to.clear();
            replacements::save(&self.replacements);
            self.recompute_all_conversions();
        } else {
            if let Some(i) = remove {
                if i < self.replacements.len() {
                    self.replacements.remove(i);
                    changed = true;
                }
            }
            if add {
                let from = self.replacement_from.trim().to_string();
                let to = self.replacement_to.trim().to_string();
                if !from.is_empty() && !to.is_empty() {
                    if let Some(existing) = self
                        .replacements
                        .iter_mut()
                        .find(|(a, _)| a == &from)
                    {
                        existing.1 = to;
                    } else {
                        self.replacements.push((from, to));
                    }
                    self.replacement_from.clear();
                    self.replacement_to.clear();
                    changed = true;
                }
            }
            if changed {
                replacements::save(&self.replacements);
                self.recompute_all_conversions();
            }
        }
        if !open {
            self.replacements_open = false;
        }
    }

    /// 当前编辑对象（默认 / 分组 / 会员）的三地区赔率。
    fn odds_scope_sets_mut(&mut self) -> &mut [crate::odds::OddsSet; 3] {
        match self.odds_scope.clone() {
            OddsScope::Default => &mut self.odds_draft.default,
            OddsScope::Group(name) => {
                self.odds_draft.ensure_group(&name);
                self.odds_draft.groups.get_mut(&name).expect("group ensured")
            }
            OddsScope::Member(name) => {
                self.odds_draft.ensure_member(&name, None);
                self.odds_draft
                    .members
                    .get_mut(&name)
                    .expect("member ensured")
            }
        }
    }

    /// 赔率与回水：默认一套、每个分组一套、会员可单独覆盖。
    fn odds_dialog(&mut self, ctx: &egui::Context) {
        if !self.odds_open {
            return;
        }
        let group_names: Vec<String> = self.groups.iter().map(|g| g.name.clone()).collect();
        // 会员覆盖的显示名：新键按「来源身份」显示成「昵称（群名 / 联系人）」，
        // 旧版按昵称保存的键原样显示。先算好，窗口闭包里不再借 self.state。
        let (member_names, member_labels, candidates) = {
            let data = match &self.state {
                State::Ready(d) => Some(&**d),
                _ => None,
            };
            let mut labels: HashMap<String, String> = HashMap::new();
            let mut names: Vec<String> = self.odds_draft.members.keys().cloned().collect();
            for key in &names {
                labels.insert(key.clone(), member_key_label(key, data));
            }
            names.sort_by(|a, b| {
                labels
                    .get(a)
                    .cmp(&labels.get(b))
                    .then_with(|| a.cmp(b))
            });
            // 从注单里列会员：按「会话 + 发送人」区分来源，同名但不同群 / 会话
            // 的会员会各出一条，标签里带上来源。
            let mut seen: HashSet<String> = HashSet::new();
            let mut candidates: Vec<(String, String, Option<String>)> = Vec::new();
            for r in &self.orders {
                if r.nick.is_empty() {
                    continue;
                }
                let key = member_identity(&r.chat, &r.sender);
                if !seen.insert(key.clone()) || self.odds_draft.members.contains_key(&key) {
                    continue;
                }
                let label = identity_label(&r.chat, &r.sender, &r.nick, data);
                let group = self.group_with_odds_for(&r.chat, &r.sender);
                candidates.push((key, label, group));
            }
            candidates.sort_by(|a, b| a.1.cmp(&b.1));
            (names, labels, candidates)
        };

        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut add_member: Option<(String, Option<String>)> = None;
        let mut delete_member = false;
        let mut reset_region = false;
        let mut copy_macau = false;

        egui::Window::new("赔率与回水")
            .collapsible(false)
            .resizable(true)
            .default_size([780.0, 640.0])
            .min_width(620.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new(
                        "结算优先级：会员个性化 > 所属分组 > 默认。每个对象都有澳门 / 香港 / 老澳门三份，改完点保存生效。",
                    )
                    .size(11.0)
                    .color(SUB),
                );
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("编辑对象").size(12.0).color(TEXT));
                    let scope_text = match &self.odds_scope {
                        OddsScope::Default => "默认（未匹配分组 / 会员）".to_string(),
                        OddsScope::Group(name) => format!("分组：{name}"),
                        OddsScope::Member(key) => format!(
                            "会员：{}",
                            member_labels.get(key).cloned().unwrap_or_else(|| key.clone())
                        ),
                    };
                    egui::ComboBox::from_id_salt("odds_scope")
                        .width(240.0)
                        .selected_text(RichText::new(scope_text).size(12.0))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.odds_scope,
                                OddsScope::Default,
                                "默认（未匹配分组 / 会员）",
                            );
                            for name in &group_names {
                                ui.selectable_value(
                                    &mut self.odds_scope,
                                    OddsScope::Group(name.clone()),
                                    format!("分组：{name}"),
                                );
                            }
                            for key in &member_names {
                                let label = member_labels
                                    .get(key)
                                    .cloned()
                                    .unwrap_or_else(|| key.clone());
                                ui.selectable_value(
                                    &mut self.odds_scope,
                                    OddsScope::Member(key.clone()),
                                    format!("会员：{label}"),
                                );
                            }
                        });
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    let mut resp = ui.add(
                        egui::TextEdit::singleline(&mut self.odds_new_member)
                            .hint_text("会员昵称（全局）")
                            .desired_width(150.0),
                    );
                    text_edit_context_menu(
                        ui.ctx(),
                        resp.id,
                        &mut self.odds_new_member,
                        &mut resp,
                    );
                    let name = self.odds_new_member.trim().to_string();
                    if ui
                        .add_enabled(
                            !name.is_empty(),
                            tiny_button("添加会员覆盖", TEXT),
                        )
                        .on_hover_text("单独给这个会员设置赔率 / 回水，优先于分组")
                        .clicked()
                    {
                        // 手输昵称仍按昵称存一份全局覆盖；能从注单里找到来源时，
                        // 顺带把它所属分组当作复制基准。
                        let group = self
                            .orders
                            .iter()
                            .find(|r| r.nick == name)
                            .and_then(|r| self.group_with_odds_for(&r.chat, &r.sender));
                        add_member = Some((name, group));
                    }
                    if !candidates.is_empty() {
                        egui::ComboBox::from_id_salt("odds_member_pick")
                            .selected_text(RichText::new("从注单选择会员…").size(11.5))
                            .show_ui(ui, |ui| {
                                for (key, label, group) in &candidates {
                                    if ui.selectable_label(false, label).clicked() {
                                        add_member = Some((key.clone(), group.clone()));
                                    }
                                }
                            });
                    }
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for (i, label) in crate::odds::REGION_LABELS.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.odds_region == i,
                                RichText::new(*label).size(12.0),
                            )
                            .clicked()
                        {
                            self.odds_region = i;
                        }
                    }
                    ui.separator();
                    if ui
                        .add(tiny_button("本地区恢复默认", SUB))
                        .on_hover_text("只把当前对象、当前地区的赔率 / 回水恢复为默认值")
                        .clicked()
                    {
                        reset_region = true;
                    }
                    if ui
                        .add(tiny_button("澳门复制到港 / 老", SUB))
                        .on_hover_text("把当前对象的澳门那份复制到香港和老澳门")
                        .clicked()
                    {
                        copy_macau = true;
                    }
                    if matches!(self.odds_scope, OddsScope::Member(_))
                        && ui
                            .add(tiny_button("删除该会员覆盖", RED))
                            .on_hover_text("删除后该会员重新使用分组 / 默认赔率")
                            .clicked()
                    {
                        delete_member = true;
                    }
                });
                if matches!(self.odds_scope, OddsScope::Group(_)) {
                    ui.label(
                        RichText::new(
                            "该分组下所有会员默认使用这份赔率；名单里个别会员可再加「会员覆盖」。",
                        )
                        .size(10.5)
                        .color(SUB),
                    );
                } else if matches!(self.odds_scope, OddsScope::Member(_)) {
                    ui.label(
                        RichText::new(
                            "会员覆盖优先于分组；从注单选择的会员按「群聊 / 会话 + 发送人」区分，同名但来源不同的会员可以各设一份。",
                        )
                        .size(10.5)
                        .color(SUB),
                    );
                }

                ui.add_space(4.0);
                hairline(ui);
                ui.add_space(4.0);

                let region = self.odds_region.min(2);
                let set = self.odds_scope_sets_mut();
                let set = &mut set[region];
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(400.0)
                    .id_salt("odds_fields_scroll")
                    .show(ui, |ui| {
                        for group in crate::odds::FIELD_GROUPS {
                            let fields: Vec<&crate::odds::FieldDef> = crate::odds::FIELDS
                                .iter()
                                .filter(|f| f.group == *group)
                                .collect();
                            if fields.is_empty() {
                                continue;
                            }
                            egui::Frame::new()
                                .fill(CARD)
                                .stroke(Stroke::new(1.0, BORDER))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(*group)
                                            .font(bold_font(12.5))
                                            .color(TEXT),
                                    );
                                    ui.add_space(2.0);
                                    egui::Grid::new(format!("odds_grid_{group}"))
                                        .num_columns(4)
                                        .spacing([10.0, 4.0])
                                        .show(ui, |ui| {
                                            for (i, field) in fields.iter().enumerate() {
                                                ui.label(
                                                    RichText::new(field.label)
                                                        .size(11.5)
                                                        .color(SUB),
                                                );
                                                if let Some(v) =
                                                    set.value_mut(field.key)
                                                {
                                                    ui.add(
                                                        egui::DragValue::new(v)
                                                            .speed(0.1)
                                                            .max_decimals(2)
                                                            .range(0.0..=10_000_000.0)
                                                            .suffix(
                                                                if field.kind
                                                                    == crate::odds::FieldKind::Rebate
                                                                {
                                                                    "%"
                                                                } else {
                                                                    ""
                                                                },
                                                            ),
                                                    );
                                                }
                                                if i % 2 == 1 {
                                                    ui.end_row();
                                                }
                                            }
                                            if fields.len() % 2 == 1 {
                                                ui.end_row();
                                            }
                                        });
                                });
                            ui.add_space(5.0);
                        }
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if primary_button(
                        ui,
                        RichText::new("保存").size(12.5).color(Color32::WHITE),
                        true,
                        CornerRadius::same(6),
                        Vec2::new(72.0, 28.0),
                    )
                    .clicked()
                    {
                        save = true;
                    }
                    if ui.add(tiny_button("取消", SUB)).clicked() {
                        cancel = true;
                    }
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("保存后总账与明细会按新赔率重新计算。")
                            .size(10.5)
                            .color(SUB),
                    );
                });
            });

        if let Some((key, group)) = add_member {
            self.odds_draft.ensure_member(&key, group.as_deref());
            self.odds_scope = OddsScope::Member(key);
            self.odds_new_member.clear();
        }
        if reset_region {
            let region = self.odds_region.min(2);
            self.odds_scope_sets_mut()[region].reset_to_default();
        }
        if copy_macau {
            let sets = self.odds_scope_sets_mut();
            let macau = sets[0].clone();
            sets[1] = macau.clone();
            sets[2] = macau;
        }
        if delete_member {
            if let OddsScope::Member(name) = self.odds_scope.clone() {
                self.odds_draft.members.remove(&name);
            }
            self.odds_scope = OddsScope::Default;
        }
        if save {
            self.odds_draft
                .groups
                .retain(|name, _| group_names.iter().any(|g| g == name));
            self.odds = self.odds_draft.clone();
            crate::odds::save(&self.odds);
            self.odds_open = false;
            // 赔率变了，总账和明细都需要按新值重新计算。
            self.ledger_msg = None;
        } else if cancel || !open {
            self.odds_open = false;
        }
    }

    /// 刷新单条录入的可选会员：与左侧成员条一致（当前分组过滤下的联系人和群成员）。
    fn refresh_single_members(&mut self, data: &AppData) {
        let filter = self.order_filter.as_deref();
        let mut contacts: BTreeSet<String> = BTreeSet::new();
        let mut members: BTreeSet<(String, String)> = BTreeSet::new();
        for g in &self.groups {
            if filter.is_none_or(|name| g.name == name) {
                contacts.extend(g.contacts.iter().cloned());
                members.extend(g.members.iter().cloned());
            }
        }
        let mut options: Vec<SingleMemberChoice> = Vec::new();
        for c in contacts {
            let name = display_of_contact(data, &c);
            options.push(SingleMemberChoice {
                key: MemberKey::Contact(c),
                label: name.clone(),
                nick: name,
            });
        }
        for (chat, m) in members {
            let label = display_of_member(data, &chat, &m);
            let nick = data
                .groups
                .iter()
                .find(|g| g.username == chat)
                .and_then(|g| g.members.iter().find(|p| p.username == m))
                .map(|p| p.display_name.clone())
                .unwrap_or_else(|| m.clone());
            options.push(SingleMemberChoice {
                key: MemberKey::Member(chat, m),
                label,
                nick,
            });
        }
        options.sort_by(|a, b| a.label.cmp(&b.label));
        self.single_members = options;
        if let Some(selected) = &self.single_member {
            if !self.single_members.iter().any(|c| &c.key == selected) {
                self.single_member = None;
            }
        }
    }

    /// 「单条录入」弹窗：手动粘贴原文 → 转换 → 录入到注单消息列表。
    fn single_entry_dialog(&mut self, ctx: &egui::Context) {
        if !self.single_open {
            return;
        }
        let mut open = true;
        let mut convert = false;
        let mut save = false;
        let mut close = false;

        egui::Window::new("单条录入")
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 600.0])
            .min_width(440.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("投注类型").size(12.0).color(SUB));
                    egui::ComboBox::from_id_salt("single_play_type")
                        .width(118.0)
                        .selected_text(RichText::new(&self.single_play).size(12.0))
                        .show_ui(ui, |ui| {
                            for p in SINGLE_PLAY_OPTIONS {
                                ui.selectable_value(
                                    &mut self.single_play,
                                    (*p).to_string(),
                                    *p,
                                );
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(RichText::new("地区").size(12.0).color(SUB));
                    egui::ComboBox::from_id_salt("single_region_type")
                        .width(88.0)
                        .selected_text(RichText::new(&self.single_region).size(12.0))
                        .show_ui(ui, |ui| {
                            for r in SINGLE_REGION_OPTIONS {
                                ui.selectable_value(
                                    &mut self.single_region,
                                    (*r).to_string(),
                                    *r,
                                );
                            }
                        });
                    ui.add_space(10.0);
                    ui.label(RichText::new("会员").size(12.0).color(SUB));
                    let selected_label = self
                        .single_member
                        .as_ref()
                        .and_then(|k| self.single_members.iter().find(|c| &c.key == k))
                        .map(|c| c.label.clone())
                        .unwrap_or_else(|| "不指定".to_string());
                    let member_options = self.single_members.clone();
                    egui::ComboBox::from_id_salt("single_member")
                        .width(170.0)
                        .selected_text(
                            RichText::new(truncate_display(&selected_label, 14))
                                .size(12.0),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.single_member, None, "不指定");
                            for c in &member_options {
                                ui.selectable_value(
                                    &mut self.single_member,
                                    Some(c.key.clone()),
                                    &c.label,
                                );
                            }
                        });
                });
                ui.add_space(8.0);
                ui.label(RichText::new("原始文本").size(12.0).color(SUB));
                let raw_id = ui.make_persistent_id("single_raw_edit");
                let mut edit = ui.add(
                    egui::TextEdit::multiline(&mut self.single_raw)
                        .id(raw_id)
                        .desired_width(f32::INFINITY)
                        .desired_rows(7)
                        .hint_text("粘贴或输入一条注单原文，例如：三中三06.26.44五十"),
                );
                text_edit_context_menu(ctx, raw_id, &mut self.single_raw, &mut edit);
                if edit.changed() {
                    // 原文变了，旧结果不再对应当前输入。
                    self.single_preview = None;
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if primary_button(
                        ui,
                        RichText::new("转换格式").size(12.5).color(Color32::WHITE),
                        true,
                        CornerRadius::same(6),
                        Vec2::new(84.0, 26.0),
                    )
                    .clicked()
                    {
                        convert = true;
                    }
                    let can_save = self
                        .single_preview
                        .as_ref()
                        .is_some_and(|o| !o.bets.is_empty() && !o.invalid);
                    let fill = if can_save { GREEN } else { HOVER };
                    if styled_button_enabled(
                        ui,
                        RichText::new("录入")
                            .size(12.5)
                            .color(if can_save { Color32::WHITE } else { SUB }),
                        can_save,
                        fill,
                        if can_save { GREEN_HOVER } else { fill },
                        if can_save { GREEN_ACTIVE } else { fill },
                        Stroke::NONE,
                        Stroke::NONE,
                        CornerRadius::same(6),
                        Vec2::new(64.0, 26.0),
                    )
                    .on_disabled_hover_text("请先转换，且转换结果不能为空或校验失败")
                    .clicked()
                    {
                        save = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(tiny_button("关闭", SUB)).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                hairline(ui);
                ui.add_space(6.0);

                match &self.single_preview {
                    None => {
                        ui.label(
                            RichText::new("输入原文后点「转换格式」查看结果")
                                .size(11.5)
                                .color(SUB),
                        );
                    }
                    Some(out) => {
                        let (label, color) = if out.invalid {
                            ("失败", RED)
                        } else if out.doubtful {
                            ("存疑", BLUE_FG)
                        } else {
                            ("正常", GREEN_DEEP)
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("转换结果").size(12.0).strong().color(TEXT));
                            ui.label(RichText::new(label).size(11.0).strong().color(color));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} 注 · 总金额 {}",
                                        out.bets.len(),
                                        convert::fmt_amount(out.total_stake)
                                    ))
                                    .size(11.5)
                                    .strong()
                                    .color(GREEN_DEEP),
                                );
                            });
                        });
                        if !out.reasons.is_empty() {
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .id_salt("single_reasons")
                                .max_height(64.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for r in &out.reasons {
                                        ui.label(
                                            RichText::new(strip_reason_line(r))
                                                .size(11.0)
                                                .color(color),
                                        );
                                    }
                                });
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .id_salt("single_preview")
                            .max_height(200.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if out.text.is_empty() {
                                    ui.label(
                                        RichText::new("没有可转换的注单内容")
                                            .size(11.5)
                                            .color(SUB),
                                    );
                                } else {
                                    for line in out.text.lines() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(line)
                                                    .size(12.0)
                                                    .color(TEXT)
                                                    .monospace(),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    }
                                }
                            });
                    }
                }
            });

        if convert {
            self.single_convert();
        }
        if save {
            self.save_single_entry();
        }
        if close || !open {
            self.single_open = false;
        }
    }

    /// 按当前「投注类型 / 地区」补上默认值后转换单条原文。
    fn single_convert(&mut self) {
        let raw = self.single_raw.trim();
        if raw.is_empty() {
            self.single_preview = None;
            return;
        }
        let mut text = String::new();
        text.push_str(&self.single_region);
        text.push('\n');
        if self.single_play != "自动识别" {
            text.push_str(&self.single_play);
            text.push('\n');
        }
        text.push_str(raw);
        self.single_preview = Some(safe_convert(&text, &self.keywords, &self.replacements));
    }

    /// 把当前转换结果作为一条「手动录入」注单加入列表并选中。
    fn save_single_entry(&mut self) {
        self.single_convert();
        let Some(out) = self.single_preview.clone() else {
            return;
        };
        if out.bets.is_empty() || out.invalid {
            return;
        }
        let raw = self.single_raw.trim().to_string();
        if raw.is_empty() {
            return;
        }
        let member = self
            .single_member
            .clone()
            .and_then(|k| self.single_members.iter().find(|c| c.key == k).cloned());
        // 选了会员就把这条记到该会员名下，成员/分组过滤都能命中。
        let (chat, sender, nick) = match member.as_ref() {
            Some(choice) => match &choice.key {
                MemberKey::Contact(c) => (c.clone(), c.clone(), choice.nick.clone()),
                MemberKey::Member(chat, m) => (chat.clone(), m.clone(), choice.nick.clone()),
            },
            None => (
                "manual".to_string(),
                "manual".to_string(),
                "手动录入".to_string(),
            ),
        };
        let now = Local::now();
        let max_sort = self.orders.iter().map(|r| r.sort).max().unwrap_or(0);
        let sort = now.timestamp_millis().max(max_sort.saturating_add(1));
        let row = MsgRow {
            kind: "手动".to_string(),
            nick,
            content: raw.clone(),
            time: now.format("%H:%M").to_string(),
            sort,
            chat,
            sender,
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let key = (
            row.sort,
            row.chat.clone(),
            row.nick.clone(),
            row.content.clone(),
        );
        self.order_keys.insert(key.clone());
        self.auto_converted.insert(edit_key(&row), out.clone());
        self.orders.push(row.clone());
        self.orders.sort_by_key(|r| r.sort);
        // 还没导入/监控过消息时，列表停在 Idle；有手动注单后要能显示出来。
        if matches!(self.orders_state, OrdersState::Idle) {
            self.orders_state = OrdersState::Ready;
        }
        if let Some(choice) = member {
            // 选中会员：直接切到该会员的注单消息，刚录入的这条就在列表里。
            self.member_focus = Some(choice.key);
        } else {
            // 未指定会员：清掉过滤，保证手动条目在列表里能看到。
            self.member_focus = None;
            self.order_filter = None;
        }
        self.selected_order_key = Some(key);
        self.edit_time = row.time.clone();
        self.selected_order_row = Some(row);
        self.converted = Some(out);
        self.edit_text = raw;
        self.scroll_to_order = true;
        self.single_open = false;
        self.single_raw.clear();
        self.single_preview = None;
    }

    fn contacts_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let q = self.search.trim().to_lowercase();
        let list: Vec<&Person> = data
            .contacts
            .iter()
            .filter(|p| matches_person(p, &q))
            .collect();

        if list.is_empty() {
            empty_state(
                ui,
                if q.is_empty() {
                    "暂无联系人"
                } else {
                    "没有匹配的联系人"
                },
            );
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("contact_scroll")
            .show(ui, |ui| {
                for p in list {
                    self.contact_row(ui, p);
                }
                ui.add_space(8.0);
            });
    }

    fn contact_row(&mut self, ui: &mut egui::Ui, p: &Person) {
        let selected = self.selected_contacts.contains(&p.username);
        let (_, resp) = row_ui(ui, 56.0, Sense::click(), selected, |ui| {
            check_indicator(ui, selected);
            ui.add_space(4.0);
            self.avatar(ui, &p.username, &p.display_name, p.avatar.as_ref(), 40.0);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&p.display_name).size(15.0).color(TEXT));
                    if p.is_self {
                        badge(ui, "我", GREEN, Color32::from_rgb(0xE2, 0xF8, 0xEE));
                    }
                });
                let sub = if !p.remark.is_empty()
                    && !p.nick_name.is_empty()
                    && p.remark != p.nick_name
                {
                    p.nick_name.clone()
                } else if !p.alias.is_empty() {
                    format!("微信号: {}", p.alias)
                } else {
                    String::new()
                };
                if !sub.is_empty() {
                    ui.label(RichText::new(sub).size(12.5).color(SUB));
                }
            });
        });
        if resp.clicked() {
            if selected {
                self.selected_contacts.remove(&p.username);
            } else {
                self.selected_contacts.insert(p.username.clone());
            }
            self.selection_dirty = true;
        }
        // 右键：直接把这个联系人加入 / 移出任意会员分组，不用再回到顶部勾选。
        let mut toggle_group: Option<(usize, bool)> = None;
        resp.context_menu(|ui| {
            compact_context_menu(ui);
            ui.label(RichText::new("加入 / 移出会员分组").size(10.5).color(SUB));
            if self.groups.is_empty() {
                ui.label(RichText::new("还没有分组").size(11.0).color(MUTED));
            }
            for (i, g) in self.groups.iter().enumerate() {
                let in_group = g.contacts.contains(&p.username);
                if ui.selectable_label(in_group, g.name.as_str()).clicked() {
                    toggle_group = Some((i, !in_group));
                    ui.close();
                }
            }
        });
        if let Some((i, add)) = toggle_group {
            if add {
                self.groups[i].contacts.insert(p.username.clone());
            } else {
                self.groups[i].contacts.remove(&p.username);
                self.groups[i].upline_contacts.remove(&p.username);
                self.selected_contacts.remove(&p.username);
            }
            groups::save(&self.groups);
            self.selection_dirty = true;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    fn groups_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let q = self.search.trim().to_lowercase();
        let any = data.groups.iter().any(|g| {
            let name_match =
                q.is_empty() || contains_lo(&g.name, &q) || contains_lo(&g.username, &q);
            name_match || g.members.iter().any(|m| matches_person(m, &q))
        });
        if !any {
            empty_state(
                ui,
                if q.is_empty() {
                    "暂无群聊"
                } else {
                    "没有匹配的群聊或成员"
                },
            );
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("group_scroll")
            .show(ui, |ui| {
                for g in &data.groups {
                    let name_match =
                        q.is_empty() || contains_lo(&g.name, &q) || contains_lo(&g.username, &q);
                    let member_hits: Vec<&Person> = if q.is_empty() {
                        g.members.iter().collect()
                    } else {
                        g.members.iter().filter(|m| matches_person(m, &q)).collect()
                    };
                    if !name_match && member_hits.is_empty() {
                        continue;
                    }
                    let open = if q.is_empty() {
                        self.expanded.contains(&g.username)
                    } else {
                        true
                    };
                    self.group_row(ui, g, open);
                    if open {
                        for m in member_hits {
                            self.member_row(ui, g, m);
                        }
                    }
                    ui.add_space(2.0);
                }
                ui.add_space(8.0);
            });
    }

    fn group_row(&mut self, ui: &mut egui::Ui, g: &Group, open: bool) {
        let selected = self.selected_groups.contains(&g.username);
        let (rect, resp) = row_ui(ui, 56.0, Sense::click(), selected, |ui| {
            check_indicator(ui, selected);
            ui.add_space(26.0);
            self.avatar(ui, &g.username, &g.name, g.avatar.as_ref(), 40.0);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(&g.name).size(15.0).strong().color(TEXT));
                let sub = if g.has_member_list {
                    format!("{} 人", g.members.len())
                } else {
                    "成员列表未同步".to_string()
                };
                ui.label(RichText::new(sub).size(12.0).color(SUB));
            });
        });
        // 用图形画展开箭头，避免字体缺字形时出现空白/错位。
        if g.has_member_list {
            let c = egui::pos2(rect.left() + 42.0, rect.center().y);
            let pts = if open {
                vec![
                    c + Vec2::new(-4.5, -2.0),
                    c + Vec2::new(4.5, -2.0),
                    c + Vec2::new(0.0, 3.2),
                ]
            } else {
                vec![
                    c + Vec2::new(-2.0, -4.5),
                    c + Vec2::new(3.2, 0.0),
                    c + Vec2::new(-2.0, 4.5),
                ]
            };
            ui.painter()
                .add(egui::Shape::convex_polygon(pts, SUB, Stroke::NONE));
        }
        if resp.clicked() {
            let pos = resp.interact_pointer_pos().unwrap_or(rect.center());
            let in_arrow = g.has_member_list
                && self.search.trim().is_empty()
                && pos.x >= rect.left() + 34.0
                && pos.x <= rect.left() + 56.0;
            if in_arrow {
                if !self.expanded.remove(&g.username) {
                    self.expanded.insert(g.username.clone());
                }
            } else if selected {
                self.selected_groups.remove(&g.username);
            } else {
                self.selected_groups.insert(g.username.clone());
            }
            self.selection_dirty = true;
        }
        // 右键：整群成员一键全选 / 取消全选。
        let mut select_all_members: Option<bool> = None;
        resp.context_menu(|ui| {
            compact_context_menu(ui);
            if !g.has_member_list || g.members.is_empty() {
                ui.label(RichText::new("成员列表未同步").size(11.0).color(MUTED));
                return;
            }
            if ui
                .button(format!("全选 {} 个成员", g.members.len()))
                .clicked()
            {
                select_all_members = Some(true);
                ui.close();
            }
            if ui.button("取消全选本群成员").clicked() {
                select_all_members = Some(false);
                ui.close();
            }
        });
        if let Some(select) = select_all_members {
            self.expanded.insert(g.username.clone());
            for m in &g.members {
                let key = (g.username.clone(), m.username.clone());
                if select {
                    self.selected_members.insert(key);
                } else {
                    self.selected_members.remove(&key);
                }
            }
            self.selection_dirty = true;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    fn member_row(&mut self, ui: &mut egui::Ui, g: &Group, m: &Person) {
        let key = (g.username.clone(), m.username.clone());
        let selected = self.selected_members.contains(&key);
        let (_, resp) = row_ui(ui, 44.0, Sense::click(), selected, |ui| {
            ui.add_space(34.0);
            check_indicator(ui, selected);
            ui.add_space(8.0);
            self.avatar(ui, &m.username, &m.display_name, m.avatar.as_ref(), 28.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let color = if m.is_self { GREEN } else { TEXT };
                    ui.label(RichText::new(&m.display_name).size(14.0).color(color));
                    if m.is_owner {
                        badge(ui, "群主", GOLD_FG, GOLD_BG);
                    }
                    if m.is_self {
                        badge(ui, "我", GREEN, Color32::from_rgb(0xE2, 0xF8, 0xEE));
                    }
                });
                if !m.remark.is_empty() && !m.nick_name.is_empty() && m.remark != m.nick_name {
                    ui.label(RichText::new(&m.nick_name).size(11.5).color(SUB));
                }
            });
        });
        if resp.clicked() {
            if selected {
                self.selected_members.remove(&key);
            } else {
                self.selected_members.insert(key.clone());
            }
            self.selection_dirty = true;
        }
        // 右键：直接把这个群成员加入 / 移出任意会员分组。
        let mut toggle_group: Option<(usize, bool)> = None;
        resp.context_menu(|ui| {
            compact_context_menu(ui);
            ui.label(RichText::new("加入 / 移出会员分组").size(10.5).color(SUB));
            if self.groups.is_empty() {
                ui.label(RichText::new("还没有分组").size(11.0).color(MUTED));
            }
            for (i, mg) in self.groups.iter().enumerate() {
                let in_group = mg.members.contains(&key);
                if ui.selectable_label(in_group, mg.name.as_str()).clicked() {
                    toggle_group = Some((i, !in_group));
                    ui.close();
                }
            }
        });
        if let Some((i, add)) = toggle_group {
            if add {
                self.groups[i].members.insert(key.clone());
            } else {
                self.groups[i].members.remove(&key);
                self.groups[i].upline_members.remove(&key);
                self.selected_members.remove(&key);
            }
            groups::save(&self.groups);
            self.selection_dirty = true;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    // ---------------- 头像 ----------------

    fn avatar(&mut self, ui: &mut egui::Ui, name: &str, label: &str, avatar: Option<&Avatar>, size: f32) {
        let tex = self.texture_for(ui.ctx(), name, avatar);
        match tex {
            Some(t) => {
                let sized = egui::load::SizedTexture::new(t.id(), Vec2::splat(size));
                let radius = CornerRadius::same((size * 0.22) as u8);
                ui.add(egui::Image::new(sized).corner_radius(radius));
            }
            None => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
                let color = color_for(name);
                ui.painter().circle_filled(rect.center(), size * 0.5, color);
                let source = if label.is_empty() { name } else { label };
                let ch = source
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_string());
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    ch,
                    FontId::proportional(size * 0.4),
                    Color32::WHITE,
                );
            }
        }
    }

    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        avatar: Option<&Avatar>,
    ) -> Option<egui::TextureHandle> {
        if let Some(t) = self.textures.get(name) {
            return t.clone();
        }
        let tex = avatar.map(|a| {
            let img = egui::ColorImage::from_rgba_unmultiplied([a.width, a.height], &a.rgba);
            ctx.load_texture(format!("avatar:{name}"), img, egui::TextureOptions::LINEAR)
        });
        self.textures.insert(name.to_string(), tex.clone());
        tex
    }

    // ---------------- 导入弹窗 ----------------

    fn import_dialog(&mut self, ctx: &egui::Context, data: &AppData) {
        if !self.dialog_open {
            return;
        }
        let mut open = self.dialog_open;
        let mut confirm = false;
        let mut cancel = false;
        let source = self.import_source();
        let n_c = source.contacts.len();
        let n_chats = source.groups.len();
        let n_m: usize = source.groups.values().map(Vec::len).sum();
        let error = self.dialog_error.clone();

        egui::Window::new("导入注单")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(500.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new("选择要导入的日期与时间段")
                        .size(14.0)
                        .strong()
                        .color(TEXT),
                );
                ui.add_space(10.0);
                egui::Grid::new("import_form")
                    .num_columns(2)
                    .spacing([14.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("日期").size(13.0).color(SUB));
                        ui.horizontal(|ui| {
                            // 中文年 / 月 / 日下拉，不用英文日历。
                            date_picker_zh(ui, &mut self.import_date);
                            let date_btn = |ui: &mut egui::Ui, text: &str| {
                                ui.add(
                                    egui::Button::new(RichText::new(text).size(13.5).color(TEXT))
                                        .corner_radius(CornerRadius::same(8))
                                        .min_size(Vec2::new(58.0, 30.0)),
                                )
                            };
                            if date_btn(ui, "今天").clicked() {
                                self.import_date = jiff::Zoned::now().date();
                            }
                            if date_btn(ui, "昨天").clicked() {
                                self.import_date = jiff::Zoned::now()
                                    .date()
                                    .yesterday()
                                    .unwrap_or(self.import_date);
                            }
                        });
                        ui.end_row();
                        ui.label(RichText::new("开始").size(13.0).color(SUB));
                        ui.horizontal(|ui| time_picker(ui, "start", &mut self.import_start));
                        ui.end_row();
                        ui.label(RichText::new("结束").size(13.0).color(SUB));
                        ui.horizontal(|ui| time_picker(ui, "end", &mut self.import_end));
                        ui.end_row();
                    });
                ui.add_space(8.0);
                let source_text = if source.from_groups {
                    format!(
                        "来源：全部分组的所有成员 · 联系人 {n_c} · 群成员 {n_m}（{n_chats} 个群）"
                    )
                } else {
                    format!("来源：当前选择 · 联系人 {n_c} · 群聊 {n_chats} · 群成员 {n_m}")
                };
                ui.label(RichText::new(source_text).size(12.0).color(SUB));
                if let Some(e) = &error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).size(12.0).color(RED));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(
                            ui,
                            RichText::new("确定导入").size(14.0).color(Color32::WHITE),
                            true,
                            CornerRadius::same(16),
                            Vec2::new(96.0, 32.0),
                        )
                        .clicked()
                        {
                            confirm = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });

        if confirm {
            self.confirm_import(ctx, data);
        } else if cancel || !open {
            self.dialog_open = false;
        }
    }

    fn confirm_import(&mut self, ctx: &egui::Context, data: &AppData) {
        let date = match chrono::NaiveDate::from_ymd_opt(
            self.import_date.year() as i32,
            self.import_date.month() as u32,
            self.import_date.day() as u32,
        ) {
            Some(d) => d,
            None => {
                self.dialog_error = Some("日期无效。".to_string());
                return;
            }
        };
        let (start_ts, end_ts) =
            match messages::range_to_ts(date, self.import_start, self.import_end) {
                Ok(v) => v,
                Err(e) => {
                    self.dialog_error = Some(e);
                    return;
                }
            };

        // 有分组时导入全部分组下的全部成员；没有分组时沿用会员选择（含整群）。
        // 每次导入都会用当前分组重新组装来源，新增分组 / 会员立即生效。
        let source = self.import_source();
        if source.contacts.is_empty() && source.groups.is_empty() {
            self.dialog_error = Some(if source.from_groups {
                "所有分组都还没有成员，请先在「选定会员」中把联系人 / 群成员加入分组。".into()
            } else {
                "请先在「选定会员」中勾选联系人、群聊或群成员。".into()
            });
            return;
        }

        let name_of = build_name_map(data);

        let req = ImportRequest {
            account_dir: data.account_path.clone(),
            workdir: data.workdir.clone(),
            keys: data.keys.clone(),
            contacts: source.contacts,
            groups: source.groups.into_iter().collect(),
            name_of,
            start_ts,
            end_ts,
        };

        self.dialog_open = false;
        self.dialog_error = None;
        self.orders.clear();
        self.order_keys.clear();
        self.auto_converted.clear();
        self.info_hit_cache.clear();
        self.last_update.clear();
        self.orders_state = OrdersState::Loading("正在读取消息…".to_string());

        let (tx, rx) = channel();
        self.import_rx = Some(rx);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let tx_status = tx.clone();
            let ctx3 = ctx2.clone();
            let status = move |s: String| {
                let _ = tx_status.send(ImportMsg::Status(s));
                ctx3.request_repaint();
            };
            match messages::run(&req, &status) {
                Ok(res) => {
                    let _ = tx.send(ImportMsg::Done(Box::new(res)));
                }
                Err(e) => {
                    let _ = tx.send(ImportMsg::Failed(e));
                }
            }
            ctx2.request_repaint();
        });
    }

    // ---------------- 清空注单 ----------------

    fn clear_dialog(&mut self, ctx: &egui::Context, data: &AppData) {
        if !self.clear_open {
            return;
        }

        let mut contacts: HashMap<String, String> = HashMap::new();
        let mut groups: HashMap<String, (String, HashMap<String, String>)> = HashMap::new();
        for r in &self.orders {
            if r.chat.ends_with("@chatroom") {
                let name = data
                    .groups
                    .iter()
                    .find(|g| g.username == r.chat)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| r.chat.clone());
                let e = groups
                    .entry(r.chat.clone())
                    .or_insert_with(|| (name, HashMap::new()));
                e.1.entry(r.sender.clone()).or_insert_with(|| r.nick.clone());
            } else {
                let name = data
                    .contacts
                    .iter()
                    .find(|p| p.username == r.chat)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_else(|| r.chat.clone());
                contacts.insert(r.chat.clone(), name);
            }
        }
        let mut contact_list: Vec<(String, String)> = contacts.into_iter().collect();
        contact_list.sort_by(|a, b| a.1.cmp(&b.1));
        let mut group_list: Vec<ClearGroupNode> = groups
            .into_iter()
            .map(|(chat, (name, members))| {
                let mut ms: Vec<(String, String)> = members.into_iter().collect();
                ms.sort_by(|a, b| a.1.cmp(&b.1));
                ClearGroupNode {
                    chat,
                    name,
                    members: ms,
                }
            })
            .collect();
        group_list.sort_by(|a, b| a.name.cmp(&b.name));

        let mut open = self.clear_open;
        let mut do_clear = false;
        let mut cancel = false;

        egui::Window::new("清空注单")
            .collapsible(false)
            .resizable(true)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_size([520.0, 560.0])
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.label(
                    RichText::new("勾选要清空的会员；不勾选任何会员则清空全部注单。")
                        .size(12.0)
                        .color(SUB),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .id_salt("clear_scroll")
                    .show(ui, |ui| {
                        if contact_list.is_empty() && group_list.is_empty() {
                            ui.label(RichText::new("暂无注单消息").size(13.0).color(SUB));
                        }
                        if !contact_list.is_empty() {
                            ui.label(RichText::new("联系人").size(12.5).strong().color(SUB));
                            for (chat, name) in &contact_list {
                                let mut checked = self.clear_groups.contains(chat);
                                if ui
                                    .add(egui::Checkbox::new(&mut checked, name))
                                    .changed()
                                {
                                    if checked {
                                        self.clear_groups.insert(chat.clone());
                                    } else {
                                        self.clear_groups.remove(chat);
                                    }
                                }
                            }
                            ui.add_space(8.0);
                        }
                        if !group_list.is_empty() {
                            ui.label(RichText::new("群聊").size(12.5).strong().color(SUB));
                            for node in &group_list {
                                let id =
                                    ui.make_persistent_id(("clear_group", &node.chat));
                                let state =
                                    egui::collapsing_header::CollapsingState::load_with_default_open(
                                        ui.ctx(),
                                        id,
                                        false,
                                    );
                                state
                                    .show_header(ui, |ui| {
                                        let mut checked =
                                            self.clear_groups.contains(&node.chat);
                                        if ui
                                            .add(egui::Checkbox::new(&mut checked, &node.name))
                                            .on_hover_text("勾选 = 清空该群全部注单")
                                            .changed()
                                        {
                                            if checked {
                                                self.clear_groups.insert(node.chat.clone());
                                            } else {
                                                self.clear_groups.remove(&node.chat);
                                            }
                                        }
                                        ui.label(
                                            RichText::new(format!("{} 人", node.members.len()))
                                                .size(11.0)
                                                .color(SUB),
                                        );
                                    })
                                    .body(|ui| {
                                        for (sender, nick) in &node.members {
                                            let key = (node.chat.clone(), sender.clone());
                                            let mut checked =
                                                self.clear_members.contains(&key);
                                            let label = if sender == &data.self_wxid {
                                                format!("{nick}（我）")
                                            } else {
                                                nick.clone()
                                            };
                                            if ui
                                                .add(egui::Checkbox::new(&mut checked, label))
                                                .changed()
                                            {
                                                if checked {
                                                    self.clear_members.insert(key);
                                                } else {
                                                    self.clear_members.remove(&key);
                                                }
                                            }
                                        }
                                    });
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if danger_button(
                            ui,
                            RichText::new("清空").size(14.0).color(Color32::WHITE),
                            true,
                            CornerRadius::same(16),
                            Vec2::new(88.0, 32.0),
                        )
                        .clicked()
                        {
                            do_clear = true;
                        }
                        if ghost_button_sized(ui, RichText::new("取消").size(13.0).color(SUB), CornerRadius::same(14), Vec2::new(64.0, 30.0))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                });
            });

        if cancel || !open {
            self.clear_open = false;
            self.clear_groups.clear();
            self.clear_members.clear();
        }
        if do_clear {
            self.perform_clear();
            self.clear_open = false;
            self.clear_groups.clear();
            self.clear_members.clear();
        }
    }

    /// 当前时段内的已撤回记录（按原消息的 sort 毫秒判断）。
    fn revoked_in_range(&self) -> Vec<&crate::revoked::RevokedRow> {
        let Some((start_ts, end_ts)) = self.import_range_ts() else {
            return self.revoked.values().collect();
        };
        let start = start_ts.saturating_mul(1000);
        let end = end_ts.saturating_mul(1000).saturating_add(999);
        self.revoked
            .values()
            .filter(|r| r.sort >= start && r.sort <= end)
            .collect()
    }

    fn perform_clear(&mut self) {
        let clear_all = self.clear_groups.is_empty() && self.clear_members.is_empty();
        let cleared_groups = self.clear_groups.clone();
        let cleared_members = self.clear_members.clone();

        self.orders = clear_orders(
            std::mem::take(&mut self.orders),
            &cleared_groups,
            &cleared_members,
        );
        self.order_keys = self
            .orders
            .iter()
            .map(|r| (r.sort, r.chat.clone(), r.nick.clone(), r.content.clone()))
            .collect();
        let live: HashSet<EditKey> = self.orders.iter().map(edit_key).collect();
        self.auto_converted.retain(|k, _| live.contains(k));

        // 已撤回名单跟着一起清：不勾选 = 全部；勾了会员则清掉对应会话 / 发送人的记录。
        let before = self.revoked.len();
        if clear_all {
            self.revoked.clear();
        } else {
            self.revoked.retain(|_, r| {
                !(cleared_groups.contains(&r.chat)
                    || cleared_members.contains(&(r.chat.clone(), r.sender.clone())))
            });
        }
        if self.revoked.len() != before {
            crate::revoked::save(&self.revoked);
        }
        self.last_update = now_hms();
    }

    // ---------------- 加载 / 错误 ----------------

    fn loading_ui(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.26).max(40.0));
            ui.add(egui::Spinner::new().size(34.0).color(GREEN));
            ui.add_space(16.0);
            ui.label(RichText::new(&self.status).size(15.0).color(TEXT));
            ui.add_space(6.0);
        });
    }

    fn error_ui(&mut self, ui: &mut egui::Ui, msg: &str) {
        let mut retry = false;
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let card = egui::Frame::new()
                .fill(CARD)
                .stroke(Stroke::new(1.0, Color32::from_rgb(0xF0, 0xD5, 0xD5)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(22, 18));
            card.show(ui, |ui| {
                ui.set_max_width(780.0);
                ui.label(RichText::new("读取失败").size(17.0).strong().color(RED));
                ui.add_space(8.0);
                ui.label(RichText::new(msg).size(13.5).color(TEXT));
                ui.add_space(14.0);
                if primary_button(
                    ui,
                    RichText::new("重新读取").size(14.0).color(Color32::WHITE),
                    true,
                    CornerRadius::same(16),
                    Vec2::new(96.0, 32.0),
                )
                .clicked()
                {
                    retry = true;
                }
            });
        });
        if retry {
            let ctx = ui.ctx().clone();
            self.refresh(&ctx);
        }
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(
                    "本软件系记账软件，只能用于娱乐或技术探讨，不得用于赌博或其它非法用途，否则后果自负！",
                )
                .size(10.0)
                .color(SUB),
            );
        });
    }
}

impl Drop for ViewerApp {
    fn drop(&mut self) {
        self.stop_monitor();
        loader::cleanup_temp();
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 右键会清掉文本框选区，绘制前先留住上一帧的选区供右键菜单使用。
        snapshot_focused_text_selection(&ctx);
        // 加载结果先处理：登录页上的「微信是否已登录」提示就靠它。
        self.poll();
        self.poll_auth();
        self.sync_window_title(&ctx);
        // 没登录（或心跳发现令牌失效）时只画登录页。
        if !self.auth.authenticated() {
            self.login_ui(&ctx);
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.poll_import();
        self.poll_monitor();
        self.poll_ocr(&ctx);
        self.poll_quote_fetch(&ctx);

        egui::Panel::top("wv_header")
            .frame(
                egui::Frame::new()
                    .fill(TOOLBAR_BG)
                    .inner_margin(Margin::symmetric(14, 3))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x46, 0x4D, 0x54))),
            )
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::bottom("wv_status")
            .frame(
                egui::Frame::new()
                    .fill(CARD)
                    .inner_margin(Margin::symmetric(18, 2))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.footer(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::ZERO),
            )
            .show(ui, |ui| {
                let state = std::mem::replace(&mut self.state, State::Loading);
                match &state {
                    State::Loading => self.loading_ui(ui),
                    State::Failed(msg) => self.error_ui(ui, msg),
                    State::Ready(data) => {
                        match self.page {
                            Page::Home => self.home_ui(ui, data),
                            Page::Select => self.select_ui(ui, data),
                            Page::Ledger => self.ledger_ui(ui),
                            Page::Risk => self.risk_ui(ui),
                        }
                        self.sync_monitor(&ctx, data);
                        self.import_dialog(&ctx, data);
                        self.clear_dialog(&ctx, data);
                        self.group_dialog(&ctx);
                        self.group_rename_dialog(&ctx);
                        self.group_manage_dialog(&ctx, data);
                        self.group_delete_dialog(&ctx);
                    }
                }
                self.state = state;
                // 关键词 / 替换词设置不依赖微信数据，加载中也可以打开。
                self.keywords_dialog(&ctx);
                self.replacements_dialog(&ctx);
                self.odds_dialog(&ctx);
                self.single_entry_dialog(&ctx);
                self.batch_replace_dialog(&ctx);
                self.draw_dialog(&ctx);
                self.reconcile_dialog(&ctx);
                self.bill_export_dialog(&ctx);
                self.info_code_dialog(&ctx);
                self.auth_dialog(&ctx);
            });

        // 独立窗口：引用类型消息列表。
        self.reference_window(&ctx);
        // 补读到的、不在注单列表里的被引用原文。
        self.quote_original_dialog(&ctx);

        if matches!(self.state, State::Loading)
            || matches!(self.orders_state, OrdersState::Loading(_))
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.monitor_rx.is_some() {
            // 监控期间定期唤醒：窗口最小化 / 空闲时也要能轮询到新引用并弹窗。
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        // 心跳（60 秒）把会话判无效后，界面要能自己切回登录页。
        ctx.request_repaint_after(Duration::from_secs(2));
    }
}

impl ViewerApp {
    /// 轮询登录 / 改密 / 刷新这些后台线程的结果。
    fn poll_auth(&mut self) {
        if let Some(rx) = &self.login_rx {
            match rx.try_recv() {
                Ok(out) => {
                    self.login_rx = None;
                    if out.success {
                        self.login_msg = None;
                        self.login_pass.clear();
                        // 用服务器下发的属性词映射整表替换内置表，并重算已缓存的转换结果。
                        if let Some(words) = self.auth.attr_words() {
                            crate::settle::set_attr_map(words);
                            self.recompute_all_conversions();
                        }
                    } else {
                        // 语气由错误码决定：in_use / 尝试过多 / 令牌失效是「等一下」，
                        // 用警告色；其余才是红色的错误。
                        let warning = crate::auth::is_warning(&out.code);
                        let mut text = out.message.clone();
                        if out.retry_after_ms > 0 {
                            text.push_str(&format!(
                                "（{} 秒后可重试）",
                                (out.retry_after_ms + 999) / 1000
                            ));
                        }
                        self.login_msg = Some((!warning, text));
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.login_rx = None;
                    self.login_msg = Some((true, "登录请求失败，请重试".to_string()));
                }
            }
        }
        if let Some(rx) = &self.pwd_rx {
            match rx.try_recv() {
                Ok(out) => {
                    self.pwd_rx = None;
                    if out.success {
                        self.pwd_msg = Some((false, out.message.clone()));
                        self.pwd_old.clear();
                        self.pwd_new.clear();
                        self.pwd_confirm.clear();
                    } else {
                        self.pwd_msg = Some((true, out.message.clone()));
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.pwd_rx = None;
                    self.pwd_msg = Some((true, "请求失败，请重试".to_string()));
                }
            }
        }
        if let Some(rx) = &self.auth_refresh_rx {
            match rx.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => self.auth_refresh_rx = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        // 会话失效（心跳）后授权弹窗也要跟着关掉，并告诉用户为什么又回到登录页。
        let now_authenticated = self.auth.authenticated();
        if !now_authenticated {
            if self.was_authenticated {
                self.login_msg = Some((
                    false,
                    crate::auth::default_message("token_invalid").to_string(),
                ));
                // 会话没了就退回内置属性词表，别让上一段会话的映射继续算钱。
                crate::settle::clear_attr_map();
                self.recompute_all_conversions();
            }
            self.auth_open = false;
        }
        self.was_authenticated = now_authenticated;
    }

    /// 窗口标题显示「软件版本号 · 账号 · 剩余时间」。
    /// 账号 / 剩余时间不再占用顶部工具栏，放进系统标题栏，省出按钮空间。
    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let acct = self.auth.state();
        let title = if self.auth.authenticated() && !acct.username.is_empty() {
            format!(
                "小掌柜 v{} · 账号 {} · 剩余 {}",
                env!("CARGO_PKG_VERSION"),
                acct.username,
                short_remaining(&acct.remaining_text)
            )
        } else {
            format!("小掌柜 v{}", env!("CARGO_PKG_VERSION"))
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// 登录页：没登录时铺满整个窗口。
    fn login_ui(&mut self, ctx: &egui::Context) {
        // 微信刚被启动时等几秒再自动检测一次：它多半会自动登录，
        // 登录完成后内存里才会出现数据库密钥。
        if self.wx_auto_check_at.is_some_and(|at| Instant::now() >= at) {
            self.wx_auto_check_at = None;
            if !matches!(self.state, State::Loading) {
                self.refresh(ctx);
            }
        }
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("login_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter().rect_filled(
                    screen,
                    CornerRadius::ZERO,
                    Color32::from_rgb(0x10, 0x18, 0x2B),
                );
                ui.allocate_rect(screen, Sense::hover());
            });

        let (wx_color, wx_text, wx_tip) = self.wechat_login_status();
        egui::Window::new("login_panel")
            .title_bar(false)
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .fixed_size([400.0, 322.0])
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("小掌柜 · 登录").size(20.0).strong().color(TEXT));
                });
                ui.add_space(8.0);
                // 微信登录状态：绿色 = 确认已登录；金色 = 未登录 / 无法确认。
                let (wx_fill, wx_border) = if wx_color == GREEN {
                    (GREEN_TINT, Color32::from_rgb(0xBF, 0xE3, 0xCF))
                } else {
                    (GOLD_BG, Color32::from_rgb(0xEF, 0xD8, 0xA6))
                };
                egui::Frame::new()
                    .fill(wx_fill)
                    .stroke(Stroke::new(1.0, wx_border))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(RichText::new("●").size(9.0).color(wx_color));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&wx_text).size(11.0).color(wx_color),
                                )
                                .truncate(),
                            );
                        })
                        .response
                        .on_hover_text(&wx_tip);
                    });
                ui.add_space(8.0);
                ui.label(RichText::new("账号").size(12.0).color(SUB));
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.login_user)
                        .desired_width(f32::INFINITY)
                        .hint_text("请输入管理员提供的账号"),
                );
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.login_user, &mut resp);
                ui.add_space(6.0);
                ui.label(RichText::new("密码").size(12.0).color(SUB));
                let mut pass_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.login_pass)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("请输入密码"),
                );
                text_edit_context_menu(ui.ctx(), pass_resp.id, &mut self.login_pass, &mut pass_resp);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.login_remember, RichText::new("记住密码").size(11.5));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let loading = matches!(self.state, State::Loading);
                        let color = if loading { MUTED } else { GREEN_DEEP };
                        if ui
                            .add_enabled(!loading, tiny_button("重新检测", color))
                            .on_hover_text("重新读取微信数据，确认当前微信登录状态")
                            .clicked()
                        {
                            self.refresh(ctx);
                        }
                    });
                });
                ui.add_space(6.0);
                if let Some((is_error, msg)) = &self.login_msg {
                    // 错误用红色；「被顶下线 / 已退出 / 尝试过多」用警告色。
                    let color = if *is_error { RED } else { GOLD_FG };
                    ui.label(RichText::new(msg).size(11.0).color(color));
                }
                let inflight = self.login_rx.is_some();
                // 还在检测微信登录状态时先不让登录，避免状态没确认就往下走。
                let checking = matches!(self.state, State::Loading);
                let can_login = !inflight && !checking;
                let enter = pass_resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(4.0);
                ui.vertical_centered(|ui| {
                    if primary_button(
                        ui,
                        RichText::new(if inflight {
                            "登录中…"
                        } else if checking {
                            "检测中…"
                        } else {
                            "登录"
                        })
                        .size(13.0)
                        // 禁用时文字用灰色，避免白字落在浅色禁用底上看不见。
                        .color(if can_login { Color32::WHITE } else { SUB }),
                        can_login,
                        CornerRadius::same(6),
                        Vec2::new(140.0, 30.0),
                    )
                    .clicked()
                        || (enter && can_login)
                    {
                        self.submit_login();
                    }
                });
            });
    }

    /// 登录页顶部「微信登录状态」：颜色、文案、悬浮说明。
    ///
    /// 判据来自后台加载器：密钥必须是从**正在运行的微信进程内存**里拿到的，
    /// 才算当前确实已登录；用本地缓存解开的数据库只能说明「以前登录过」。
    fn wechat_login_status(&mut self) -> (Color32, String, String) {
        // 登录页每 100ms 重绘一次，进程快照每秒最多建一个。
        let now = Instant::now();
        let was_running = self.wx_proc_running;
        if self
            .wx_proc_checked
            .map_or(true, |t| now.duration_since(t) >= Duration::from_secs(1))
        {
            self.wx_proc_running = !crate::keyextract::weixin_pids().is_empty();
            self.wx_proc_checked = Some(now);
        }
        if self.wx_proc_running && !was_running {
            // 刚启动：4 秒后自动重检一次，给微信自动登录留时间。
            self.wx_auto_check_at = Some(now + Duration::from_secs(4));
        } else if !self.wx_proc_running {
            self.wx_auto_check_at = None;
        }
        match &self.state {
            State::Loading => {
                let tip = if self.status.is_empty() {
                    "正在读取微信数据，并确认数据库密钥是否来自正在运行的微信进程。".to_string()
                } else {
                    format!(
                        "{}\n正在确认数据库密钥是否来自正在运行的微信进程。",
                        self.status
                    )
                };
                (SUB, "正在检测微信登录状态…".to_string(), tip)
            }
            State::Ready(d) if d.key_live && self.wx_proc_running => (
                GREEN,
                format!("当前微信已登录：{}", d.self_name),
                format!(
                    "微信账号：{}（{}）\n数据目录：{}",
                    d.self_name, d.self_wxid, d.data_dir
                ),
            ),
            State::Ready(d) if d.key_live => (
                GOLD_FG,
                "微信进程已退出（上次检测时已登录）".to_string(),
                format!(
                    "本次检测时微信处于登录状态，但 Weixin.exe 现在已不在运行。\n微信账号：{}（{}）",
                    d.self_name, d.self_wxid
                ),
            ),
            State::Ready(d) => {
                let tip = format!(
                    "本次数据来自「{}」，只能说明以前登录过微信。\n微信账号：{}（{}）",
                    d.key_source, d.self_name, d.self_wxid
                );
                if self.wx_proc_running {
                    (
                        GOLD_FG,
                        "无法确认微信登录状态，可点「重新检测」".to_string(),
                        format!(
                            "{tip}\n可能原因：微信还停在登录界面，或本程序与微信的权限不一致（可尝试以管理员身份运行本程序）。"
                        ),
                    )
                } else {
                    (
                        GOLD_FG,
                        "微信未运行，当前使用本地缓存数据".to_string(),
                        tip,
                    )
                }
            }
            State::Failed(err) => {
                let tip = format!("{err}\n\n若微信已经登录，请点「重新检测」。");
                if self.wx_proc_running {
                    (
                        GOLD_FG,
                        "请在微信中完成登录后点「重新检测」".to_string(),
                        format!("{tip}\n若微信已登录仍提示失败，请试试以管理员身份运行本程序。"),
                    )
                } else {
                    (
                        GOLD_FG,
                        "未检测到微信登录，请先启动并登录微信".to_string(),
                        tip,
                    )
                }
            }
        }
    }

    fn submit_login(&mut self) {
        if self.login_rx.is_some() || matches!(self.state, State::Loading) {
            return;
        }
        let user = self.login_user.trim().to_string();
        if user.is_empty() || self.login_pass.is_empty() {
            self.login_msg = Some((true, "请输入账号和密码。".to_string()));
            return;
        }
        let auth = self.auth.clone();
        let pass = self.login_pass.clone();
        let remember = self.login_remember;
        let (tx, rx) = channel();
        self.login_rx = Some(rx);
        self.login_msg = None;
        std::thread::spawn(move || {
            let _ = tx.send(auth.login(&user, &pass, remember));
        });
    }

    /// 打开 / 关闭「授权」弹窗；打开时后台刷新一次账号信息，免得显示旧的到期日。
    fn toggle_auth_dialog(&mut self) {
        self.auth_open = !self.auth_open;
        if !self.auth_open {
            return;
        }
        self.pwd_msg = None;
        self.pwd_old.clear();
        self.pwd_new.clear();
        self.pwd_confirm.clear();
        if self.auth_refresh_rx.is_none() {
            let auth = self.auth.clone();
            let (tx, rx) = channel();
            self.auth_refresh_rx = Some(rx);
            std::thread::spawn(move || {
                auth.refresh();
                let _ = tx.send(());
            });
        }
    }

    /// 「授权」弹窗：当前账号、到期时间、备注，以及改密 / 退出登录。
    fn auth_dialog(&mut self, ctx: &egui::Context) {
        if !self.auth_open {
            return;
        }
        let state = self.auth.state();
        let mut open = self.auth_open;
        egui::Window::new("权限管理")
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .open(&mut open)
            .show(ctx, |ui| {
                window_header_cursor(ui);
                ui.horizontal(|ui| {
                    auth_stat_card(ui, "账号", &state.username, TEXT, 180.0);
                    auth_stat_card(ui, "账号到期", &state.remaining_text, TEXT, 180.0);
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("备注：{}", if state.note.is_empty() { "—" } else { &state.note }))
                        .size(11.5)
                        .color(SUB),
                );
                ui.label(
                    RichText::new("到期时间、账号问题都请联系管理员。")
                        .size(10.5)
                        .color(MUTED),
                );
                ui.add_space(8.0);
                hairline(ui);
                ui.add_space(8.0);
                ui.label(RichText::new("修改密码").size(13.0).strong().color(TEXT));
                ui.label(RichText::new("原密码").size(11.5).color(SUB));
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.pwd_old)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.pwd_old, &mut resp);
                ui.label(RichText::new("新密码").size(11.5).color(SUB));
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.pwd_new)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("至少 6 位"),
                );
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.pwd_new, &mut resp);
                ui.label(RichText::new("确认新密码").size(11.5).color(SUB));
                let mut resp = ui.add(
                    egui::TextEdit::singleline(&mut self.pwd_confirm)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
                text_edit_context_menu(ui.ctx(), resp.id, &mut self.pwd_confirm, &mut resp);
                if let Some((err, msg)) = &self.pwd_msg {
                    ui.label(
                        RichText::new(msg)
                            .size(11.0)
                            .color(if *err { RED } else { GREEN_DEEP }),
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let busy = self.pwd_rx.is_some();
                    if primary_button(
                        ui,
                        RichText::new(if busy { "提交中…" } else { "修改密码" })
                            .size(12.5)
                            .color(Color32::WHITE),
                        !busy,
                        CornerRadius::same(6),
                        Vec2::new(96.0, 28.0),
                    )
                    .clicked()
                    {
                        self.submit_password_change();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(tiny_button("退出登录", RED)).clicked() {
                            self.auth.logout();
                            self.auth_open = false;
                            self.login_msg = Some((false, "已退出登录".to_string()));
                        }
                    });
                });
            });
        self.auth_open = open;
    }

    fn submit_password_change(&mut self) {
        if self.pwd_rx.is_some() {
            return;
        }
        if self.pwd_old.is_empty() || self.pwd_new.is_empty() {
            self.pwd_msg = Some((true, "请填写原密码和新密码。".to_string()));
            return;
        }
        if self.pwd_new.chars().count() < 6 {
            self.pwd_msg = Some((true, "新密码至少 6 位。".to_string()));
            return;
        }
        if self.pwd_new != self.pwd_confirm {
            self.pwd_msg = Some((true, "两次输入的新密码不一致。".to_string()));
            return;
        }
        let auth = self.auth.clone();
        let old = self.pwd_old.clone();
        let new = self.pwd_new.clone();
        let (tx, rx) = channel();
        self.pwd_rx = Some(rx);
        self.pwd_msg = None;
        std::thread::spawn(move || {
            let _ = tx.send(auth.change_password(&old, &new));
        });
    }
}

// ---------------- 小组件 ----------------

/// 清空规则：
/// - 没有勾选任何会员 → 清空全部；
/// - 勾选了群/会话 → 清空该会话的全部行；
/// - 勾选了群成员 → 只清空该成员在该群的行。
/// 中文日期选择：年 / 月 / 日三个下拉，避免 egui_extras 自带的英文日历。
fn date_picker_zh(ui: &mut egui::Ui, date: &mut JDate) {
    let mut year = date.year();
    let mut month = date.month();
    let mut day = date.day();
    let now_year = jiff::Zoned::now().year();

    egui::ComboBox::from_id_salt("import_date_year")
        .width(82.0)
        .selected_text(RichText::new(format!("{year}年")).size(13.0))
        .show_ui(ui, |ui| {
            let start = (now_year - 10).min(year);
            let end = (now_year + 1).max(year);
            for y in start..=end {
                if ui.selectable_label(y == year, format!("{y}年")).clicked() {
                    year = y;
                }
            }
        });
    egui::ComboBox::from_id_salt("import_date_month")
        .width(62.0)
        .selected_text(RichText::new(format!("{month}月")).size(13.0))
        .show_ui(ui, |ui| {
            for m in 1..=12i8 {
                if ui.selectable_label(m == month, format!("{m}月")).clicked() {
                    month = m;
                }
            }
        });
    let max_day = days_in_month(year, month);
    if day > max_day {
        day = max_day;
    }
    egui::ComboBox::from_id_salt("import_date_day")
        .width(62.0)
        .selected_text(RichText::new(format!("{day}日")).size(13.0))
        .show_ui(ui, |ui| {
            for d in 1..=max_day {
                if ui.selectable_label(d == day, format!("{d}日")).clicked() {
                    day = d;
                }
            }
        });
    if let Ok(next) = JDate::new(year, month, day) {
        *date = next;
    }
}

fn days_in_month(year: i16, month: i8) -> i8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

fn clear_orders(
    orders: Vec<MsgRow>,
    groups: &HashSet<String>,
    members: &HashSet<(String, String)>,
) -> Vec<MsgRow> {
    if groups.is_empty() && members.is_empty() {
        return Vec::new();
    }
    orders
        .into_iter()
        .filter(|r| {
            if groups.contains(&r.chat) {
                return false;
            }
            !members.contains(&(r.chat.clone(), r.sender.clone()))
        })
        .collect()
}

/// 把会员选择转换成导入过滤条件：
/// - 群内有勾选成员 → 只取这些成员；
/// - 只勾了群、没有勾成员 → 空列表（表示整群）。
/// 汇总传入分组的联系人 / 群成员（调用方可额外补充当前勾选，导入流程只用分组）。
/// 群成员始终按 `(群, 成员)` 保存：同一 wxid 在不同群聊里各自成为一条过滤条件。
fn source_from_groups(
    groups: &[MemberGroup],
    selected_contacts: &HashSet<String>,
    selected_members: &HashSet<(String, String)>,
) -> ImportSource {
    let mut contacts: BTreeSet<String> = selected_contacts.iter().cloned().collect();
    let mut chats: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (chat, member) in selected_members {
        chats
            .entry(chat.clone())
            .or_default()
            .insert(member.clone());
    }
    for g in groups {
        contacts.extend(g.contacts.iter().cloned());
        for (chat, member) in &g.members {
            chats
                .entry(chat.clone())
                .or_default()
                .insert(member.clone());
        }
    }
    ImportSource {
        from_groups: true,
        contacts: contacts.into_iter().collect(),
        groups: chats
            .into_iter()
            .map(|(chat, members)| (chat, members.into_iter().collect()))
            .collect(),
    }
}

fn edit_key(r: &MsgRow) -> EditKey {
    (r.sort, r.chat.clone(), r.sender.clone())
}

/// 单条消息转换失败不能把整个程序带崩：捕获 panic，记录原文到临时目录，
/// 该条按「没有可转换内容」处理，其余注单继续导入。
fn safe_convert(
    text: &str,
    settings: &KeywordSettings,
    replacements: &[Rule],
) -> ConvertOutput {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        convert::convert_with_replacements(text, settings, replacements)
    })) {
        Ok(out) => out,
        Err(_) => {
            let dir = std::env::temp_dir().join("WechatViewer");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join("convert_error.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                use std::io::Write;
                let _ = writeln!(f, "----- {} -----\n{}", now_hms(), text);
            }
            ConvertOutput::default()
        }
    }
}

fn display_of_contact(data: &AppData, username: &str) -> String {
    data.contacts
        .iter()
        .find(|p| p.username == username)
        .map(|p| p.display_name.clone())
        .unwrap_or_else(|| username.to_string())
}

fn display_of_member(data: &AppData, chat: &str, member: &str) -> String {
    let group = data.groups.iter().find(|g| g.username == chat);
    let group_name = group
        .map(|g| g.name.clone())
        .unwrap_or_else(|| chat.to_string());
    let member_name = group
        .and_then(|g| g.members.iter().find(|p| p.username == member))
        .map(|p| p.display_name.clone())
        .unwrap_or_else(|| member.to_string());
    format!("{group_name} / {member_name}")
}

/// 会员赔率覆盖的「来源身份」：同一会话 + 发送人。
/// 同名但来自不同群聊 / 会话的会员因此能各存一份覆盖。
fn member_identity(chat: &str, sender: &str) -> String {
    format!("m:{chat}:{sender}")
}

fn identity_parts(key: &str) -> Option<(&str, &str)> {
    key.strip_prefix("m:")?.split_once(':')
}

/// 赔率弹窗里的会员显示名：`昵称（群名 / 联系人）`。
fn identity_label(chat: &str, sender: &str, nick: &str, data: Option<&AppData>) -> String {
    if chat.ends_with("@chatroom") {
        let group = data.and_then(|d| d.groups.iter().find(|g| g.username == chat));
        let group_name = group
            .map(|g| g.name.clone())
            .unwrap_or_else(|| chat.to_string());
        let person = group
            .and_then(|g| g.members.iter().find(|p| p.username == sender))
            .map(|p| p.display_name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if nick.is_empty() {
                    sender.to_string()
                } else {
                    nick.to_string()
                }
            });
        format!("{person}（{group_name}）")
    } else {
        let person = data
            .and_then(|d| d.contacts.iter().find(|p| p.username == sender))
            .map(|p| p.display_name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if nick.is_empty() {
                    sender.to_string()
                } else {
                    nick.to_string()
                }
            });
        format!("{person}（联系人）")
    }
}

/// 已保存的会员覆盖键 → 显示名。旧版按昵称保存的键原样显示。
fn member_key_label(key: &str, data: Option<&AppData>) -> String {
    match identity_parts(key) {
        Some((chat, sender)) => identity_label(chat, sender, "", data),
        None => key.to_string(),
    }
}

fn build_name_map(data: &AppData) -> HashMap<String, String> {
    let mut name_of: HashMap<String, String> = HashMap::new();
    for p in &data.contacts {
        name_of.insert(p.username.clone(), p.display_name.clone());
    }
    for g in &data.groups {
        name_of
            .entry(g.username.clone())
            .or_insert_with(|| g.name.clone());
        for m in &g.members {
            name_of.insert(m.username.clone(), m.display_name.clone());
        }
    }
    name_of.insert(data.self_wxid.clone(), data.self_name.clone());
    name_of
}

fn group_filters(
    selected_groups: &HashSet<String>,
    selected_members: &HashSet<(String, String)>,
) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for (g, m) in selected_members {
        groups.entry(g.clone()).or_default().push(m.clone());
    }
    for g in selected_groups {
        groups.entry(g.clone()).or_default();
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::{
        check_draw_row, clear_orders, detail_content_lines, find_quote_in_rows,
        find_quoted_image_index, find_revoke_target, group_filters, parse_draw_paste,
        quote_match_score, revoke_actor, revoke_candidates, source_from_groups,
    };
    use crate::model::{MemberGroup, MsgRow, QuoteRef};
    use eframe::egui;
    use std::collections::{HashMap, HashSet};

    /// 固定宽度的内容列：长内容要按行数换行，短内容保持一行。
    #[test]
    fn detail_content_wraps_within_fixed_width() {
        let ctx = egui::Context::default();
        let mut lines = 0;
        let output = ctx.run_ui(Default::default(), |ui| {
            lines = detail_content_lines(
                ui,
                "01,02,03,04,05,06,07,08,09,10,11,12,13,14,15,16",
                "拖",
                156.0,
            );
        });
        output.drop_without_applying_deltas();
        assert!(lines > 1, "long content should wrap, lines={lines}");

        let output = ctx.run_ui(Default::default(), |ui| {
            lines = detail_content_lines(ui, "01,02", "拖", 156.0);
        });
        output.drop_without_applying_deltas();
        assert_eq!(lines, 1);
    }

    /// 行高必须盖住实际换行后的内容，否则最后一行会被行底边裁掉。
    #[test]
    fn detail_row_height_fits_wrapped_content() {
        let ctx = egui::Context::default();
        super::install_fonts(&ctx);
        super::install_style(&ctx);
        let mut bad: Vec<(usize, f32, f32)> = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            for content in [
                "08,18,28,38,48,04,14,12,49",
                "(4尾)拖(8尾)拖(马)",
                "01,02,03,04,05,06,07,08,09,10,11,12,13,14,15,16",
                "01,02,03,04,05,06,07,08,09,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24",
            ] {
                let width = 156.0;
                let lines = super::detail_content_lines(ui, content, "拖", width);
                let row_h = super::detail_content_row_height(lines);
                let content_h = ui
                    .scope(|ui| {
                        ui.set_width(width - 8.0);
                        ui.spacing_mut().item_spacing = egui::Vec2::new(3.0, 2.0);
                        ui.horizontal_wrapped(|ui| {
                            for part in super::detail_content_parts(content, "拖") {
                                match part {
                                    super::DetailContentPart::Token(t) => {
                                        super::detail_token_box(ui, &t)
                                    }
                                    super::DetailContentPart::Sep(s) => {
                                        let (label, color) =
                                            super::detail_sep_display(&s);
                                        ui.label(
                                            egui::RichText::new(label)
                                                .size(10.0)
                                                .color(color),
                                        );
                                    }
                                }
                            }
                        });
                    })
                    .response
                    .rect
                    .height();
                // Frame 上下各 1px 描边。
                if content_h + 2.0 > row_h {
                    bad.push((lines, row_h, content_h + 2.0));
                }
            }
        });
        output.drop_without_applying_deltas();
        assert!(bad.is_empty(), "行高不足: {bad:?}");
    }

    /// 顶部开奖号码下面要显示每个号码对应的生肖（平码 + 特码）。
    #[test]
    fn draw_summary_shows_zodiac_under_numbers() {
        use egui_kittest::kittest::Queryable;

        let mut draws = crate::draw::Draws::default();
        draws.macau = crate::draw::RegionDraw {
            draw: vec![1, 2, 3, 4, 5, 6],
            special: Some(7),
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_accessibility_check(false)
            .build_ui(move |ui| {
                super::install_fonts(ui.ctx());
                super::draw_summary_ui(ui, &draws, "开奖：");
            });
        harness.run();
        // 1..=6 依次是 马 蛇 龙 兔 虎 牛，特码 7 是鼠。
        for zodiac in ["马", "蛇", "龙", "兔", "虎", "牛", "鼠"] {
            assert!(
                harness.get_all_by_label(zodiac).next().is_some(),
                "号码下方的生肖「{zodiac}」没有显示"
            );
        }
    }

    /// 开奖摘要放在横向布局里（风险页）时，生肖行仍要排在号码球下方，
    /// 不能被 Frame 继承横向布局排到右边、把彩种框撑宽。
    #[test]
    fn draw_summary_stacks_zodiacs_below_numbers_in_horizontal_parent() {
        use egui_kittest::kittest::Queryable;

        let mut draws = crate::draw::Draws::default();
        draws.macau = crate::draw::RegionDraw {
            draw: vec![1, 2, 3, 4, 5, 6],
            special: Some(7),
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_accessibility_check(false)
            .with_size(egui::vec2(600.0, 200.0))
            .build_ui(move |ui| {
                super::install_fonts(ui.ctx());
                ui.horizontal(|ui| {
                    ui.label("页签");
                    super::draw_summary_ui(ui, &draws, "");
                });
            });
        harness.run();
        let name = harness
            .get_all_by_label("澳")
            .next()
            .expect("地区名")
            .rect();
        for zodiac in ["马", "蛇", "龙", "兔", "虎", "牛", "鼠"] {
            let node = harness
                .get_all_by_label(zodiac)
                .next()
                .unwrap_or_else(|| panic!("生肖「{zodiac}」没画"));
            assert!(
                node.rect().top() >= name.bottom(),
                "生肖「{zodiac}」应该排在号码球下方：zodiac={:?} name={:?}",
                node.rect(),
                name
            );
        }
    }

    /// 窗口标题里的剩余时间只显示最大的时间单位。
    #[test]
    fn remaining_title_text_keeps_largest_unit() {
        assert_eq!(super::short_remaining("365天 12小时 35分钟"), "365天");
        assert_eq!(super::short_remaining("12小时 35分钟"), "12小时");
        assert_eq!(super::short_remaining("永不过期"), "永不过期");
    }

    /// 右键点文本框会先清掉选区；右键菜单仍应能对上右键前的选区，
    /// 因而「复制」保持可用。
    #[test]
    fn text_edit_right_click_menu_keeps_previous_selection() {
        use egui_kittest::kittest::{NodeT, Queryable};

        let id = egui::Id::unique("ctx_menu_test_edit");
        let text = std::rc::Rc::new(std::cell::RefCell::new("hello world".to_string()));
        let edit_text = text.clone();
        let changed = std::rc::Rc::new(std::cell::Cell::new(false));
        let changed_ui = changed.clone();
        let mut harness = egui_kittest::Harness::builder()
            .with_accessibility_check(false)
            .with_size(egui::vec2(420.0, 200.0))
            .build_ui(move |ui| {
                super::install_fonts(ui.ctx());
                super::snapshot_focused_text_selection(ui.ctx());
                let mut text = edit_text.borrow_mut();
                let mut resp = ui.add(egui::TextEdit::singleline(&mut *text).id(id));
                super::text_edit_context_menu(ui.ctx(), id, &mut *text, &mut resp);
                if resp.changed() {
                    changed_ui.set(true);
                }
            });
        harness.run();

        // 聚焦文本框，再选中前 5 个字符。
        harness.get_by_role(egui::Role::TextInput).focus();
        harness.run();
        let mut st = egui::TextEdit::load_state(&harness.ctx, id).unwrap_or_default();
        st.cursor
            .set_char_range(Some(egui::text_selection::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(5),
            )));
        st.store(&harness.ctx, id);
        harness.run();

        harness.get_by_role(egui::Role::TextInput).click_secondary();
        harness.run();

        let copy = harness.get_by_label_contains("复制");
        assert!(
            !copy.accesskit_node().is_disabled(),
            "右键菜单里的「复制」应使用右键前的选区"
        );

        // 剪切也应该作用在右键前的选区上，并真的改到字符串。
        harness.get_by_label_contains("剪切").click();
        harness.run();
        assert_eq!(&*text.borrow(), " world");
        assert!(changed.get(), "菜单剪切后应补上 changed 标记");
    }

    #[test]
    fn window_header_shows_move_cursor() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(800.0, 600.0),
        );
        let run = |pointer: egui::Pos2| {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.events.push(egui::Event::PointerMoved(pointer));
            let mut output = ctx.run_ui(input, |root| {
                egui::Window::new("测试窗口")
                    .fixed_pos(egui::pos2(100.0, 100.0))
                    .default_width(400.0)
                    .show(root.ctx(), |ui| {
                        super::window_header_cursor(ui);
                        ui.allocate_space(egui::vec2(300.0, 300.0));
                    });
            });
            output.textures_delta.clear();
            output.platform_output.cursor_icon
        };
        // 第一帧让窗口布局落定，第二帧再检查标题栏与内容区的光标。
        let _ = run(egui::pos2(10.0, 10.0));
        assert_eq!(run(egui::pos2(200.0, 115.0)), egui::CursorIcon::Move);
        assert_ne!(run(egui::pos2(200.0, 300.0)), egui::CursorIcon::Move);
    }

    #[test]
    fn quote_match_prefers_same_chat_and_sender() {
        let quote = QuoteRef {
            content: "三中三 01,02,03 各10".to_string(),
            from: "张三".to_string(),
            from_usr: "u1".to_string(),
            create_time: 0,
            msg_type: 1,
        };
        let row = |chat: &str, sender: &str, nick: &str, time: &str, content: &str| MsgRow {
            kind: "文字".to_string(),
            nick: nick.to_string(),
            content: content.to_string(),
            time: time.to_string(),
            sort: 0,
            chat: chat.to_string(),
            sender: sender.to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let other = row("g1", "u2", "李四", "21:01", "三中三 01,02,03 各10");
        let same = row("g1", "u1", "张三", "21:01", "三中三 01,02,03 各10");
        let other_chat = row("g2", "u1", "张三", "21:01", "三中三 01,02,03 各10");
        let wrong = row("g1", "u1", "张三", "21:01", "三中三 04,05,06 各10");
        let s_other = quote_match_score(&other, "g1", &quote, Some("21:01")).unwrap();
        let s_same = quote_match_score(&same, "g1", &quote, Some("21:01")).unwrap();
        let s_chat = quote_match_score(&other_chat, "g1", &quote, Some("21:01")).unwrap();
        assert!(s_same > s_other);
        assert!(s_same > s_chat);
        assert!(quote_match_score(&wrong, "g1", &quote, Some("21:01")).is_none());
    }

    #[test]
    fn quote_match_ignores_whitespace_differences() {
        let quote = QuoteRef {
            content: "三中三 01,02,03 各10".to_string(),
            ..Default::default()
        };
        let order = MsgRow {
            kind: "文字".to_string(),
            nick: "张三".to_string(),
            content: "三中三 01,02,03\n各10".to_string(),
            time: "21:01".to_string(),
            sort: 0,
            chat: "g1".to_string(),
            sender: "u1".to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        assert!(quote_match_score(&order, "g1", &quote, None).is_some());
    }

    #[test]
    fn image_quote_locates_image_by_sender_and_time() {
        let image = |chat: &str, sender: &str, time: &str, sort: i64| MsgRow {
            kind: "图片".to_string(),
            nick: "锦鲤".to_string(),
            content: "[图片]".to_string(),
            time: time.to_string(),
            sort,
            chat: chat.to_string(),
            sender: sender.to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let orders = vec![
            image("g1", "u1", "19:23", 1_700_000_000_000 - 60_000),
            image("g1", "u1", "19:24", 1_700_000_000_000),
            image("g1", "u1", "19:30", 1_700_000_000_000 + 360_000),
            image("g2", "u1", "19:24", 1_700_000_000_000),
        ];
        let quote = QuoteRef {
            content: "[图片]".to_string(),
            from: "锦鲤".to_string(),
            from_usr: "u1".to_string(),
            create_time: 1_700_000_000,
            msg_type: 3,
        };
        let reference = MsgRow {
            kind: "引用".to_string(),
            nick: "一羊迁徙".to_string(),
            content: "退".to_string(),
            time: "19:25".to_string(),
            sort: 1_700_000_030_000,
            chat: "g1".to_string(),
            sender: "operator".to_string(),
            link_url: None,
            quote: Some(quote.clone()),
            revoke_target: None,
        };
        assert_eq!(find_quoted_image_index(&orders, &reference, &quote), Some(1));

        // 发送人不同则不匹配。
        let wrong = MsgRow {
            quote: Some(QuoteRef {
                from_usr: "u2".to_string(),
                ..quote
            }),
            ..reference
        };
        assert_eq!(find_quoted_image_index(&orders, &wrong, wrong.quote.as_ref().unwrap()), None);
    }

    #[test]
    fn find_quote_in_rows_matches_text_and_empty_content() {
        let msg = |kind: &str, sender: &str, nick: &str, time: &str, content: &str, sort: i64| MsgRow {
            kind: kind.to_string(),
            nick: nick.to_string(),
            content: content.to_string(),
            time: time.to_string(),
            sort,
            chat: "g1".to_string(),
            sender: sender.to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };

        // 文字引用：按正文匹配。
        let rows = vec![
            msg("文字", "u2", "李四", "21:01", "别的消息", 1),
            msg("文字", "u1", "张三", "21:02", "三中三 01,02,03 各10", 2),
        ];
        let quote = QuoteRef {
            content: "三中三 01,02,03 各10".to_string(),
            from: "张三".to_string(),
            from_usr: "u1".to_string(),
            create_time: 0,
            msg_type: 1,
        };
        let host = MsgRow {
            quote: Some(quote.clone()),
            ..msg("引用", "u3", "王五", "21:05", "", 3)
        };
        let found = find_quote_in_rows(&rows, &host, &quote).expect("应该找到文字原文");
        assert_eq!(found.content, "三中三 01,02,03 各10");

        // 语音 / 视频等没有可读正文：同会话 + 发送人 + 时间最近的兜底。
        let rows = vec![
            msg("语音", "u1", "张三", "21:01", "[语音]", 1_700_000_000_000),
            msg("语音", "u2", "李四", "21:01", "[语音]", 1_700_000_001_000),
        ];
        let quote = QuoteRef {
            content: String::new(),
            from: "张三".to_string(),
            from_usr: "u1".to_string(),
            create_time: 1_700_000_000,
            msg_type: 34,
        };
        let host = MsgRow {
            quote: Some(quote.clone()),
            ..msg("引用", "u3", "王五", "21:05", "", 3)
        };
        let found = find_quote_in_rows(&rows, &host, &quote).expect("应该按发送人找到语音");
        assert_eq!(found.sender, "u1");
    }

    #[test]
    fn revoke_notice_extracts_actor_and_target() {
        assert_eq!(
            revoke_actor("\"一羊迁徙\" 撤回了一条消息").as_deref(),
            Some("一羊迁徙")
        );
        assert_eq!(revoke_actor("你撤回了一条消息").as_deref(), Some("你"));
        assert_eq!(revoke_actor("普通消息"), None);

        let msg = |chat: &str, sender: &str, nick: &str, content: &str, sort: i64| MsgRow {
            kind: "文字".to_string(),
            nick: nick.to_string(),
            content: content.to_string(),
            time: "21:00".to_string(),
            sort,
            chat: chat.to_string(),
            sender: sender.to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let revoke = MsgRow {
            kind: "撤回".to_string(),
            nick: String::new(),
            content: "\"张三\" 撤回了一条消息".to_string(),
            time: "21:04".to_string(),
            sort: 3000,
            chat: "g1@chatroom".to_string(),
            sender: String::new(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };

        // 群聊：按撤回提示里的昵称匹配，取该发送人最近一条。
        let orders = vec![
            msg("g1@chatroom", "u2", "李四", "别人的消息", 1000),
            msg("g1@chatroom", "u1", "张三", "更早的张三消息", 1500),
            msg("g1@chatroom", "u1", "张三", "被撤回的消息", 2000),
        ];
        let found = find_revoke_target(&revoke, &orders, &[], "self", &HashMap::new())
            .expect("应该找到张三最近一条");
        assert_eq!(found.content, "被撤回的消息");

        // 群昵称 / 备注名对不上时，用「名字 → username」再匹配。
        let orders = vec![msg("g1@chatroom", "u1", "备注张三", "被撤回的消息", 2000)];
        let mut names = HashMap::new();
        names.insert("张三".to_string(), "u1".to_string());
        let found = find_revoke_target(&revoke, &orders, &[], "self", &names)
            .expect("名字映射应该能兜住");
        assert_eq!(found.content, "被撤回的消息");

        // 「你撤回了一条消息」：匹配自己发的最近一条。
        let revoke_self = MsgRow {
            content: "你撤回了一条消息".to_string(),
            sort: 5000,
            chat: "wxid_b".to_string(),
            ..revoke
        };
        let orders = vec![
            msg("wxid_b", "self", "我", "自己发的", 4000),
            msg("wxid_b", "wxid_b", "对方", "对方发的", 4500),
        ];
        let found = find_revoke_target(&revoke_self, &orders, &[], "self", &HashMap::new())
            .expect("应该找到自己发的");
        assert_eq!(found.content, "自己发的");
    }

    /// 旧消息里如果已经有被撤回的原消息，名称匹配要取它而不是更早的那条
    /// （重新导入时它会被过滤掉不再显示，但匹配时仍要在旧消息池里）。
    #[test]
    fn revoke_candidates_keep_already_revoked_original() {
        let msg = |content: &str, kind: &str, sort: i64| MsgRow {
            kind: kind.to_string(),
            nick: "张三".to_string(),
            content: content.to_string(),
            time: "21:00".to_string(),
            sort,
            chat: "g1@chatroom".to_string(),
            sender: "u1".to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let earlier = msg("更早的正常消息", "文字", 1500);
        let revoked_original = msg("被撤回的注单", "文字", 2000);
        let notice = msg("\"张三\" 撤回了一条消息", "撤回", 3000);

        let candidates = revoke_candidates(&[earlier, revoked_original, notice.clone()]);
        assert_eq!(candidates.len(), 2, "撤回提示本身不应进入候选池");

        let found = find_revoke_target(&notice, &candidates, &[], "self", &HashMap::new())
            .expect("应该找到被撤回的原消息");
        assert_eq!(found.content, "被撤回的注单");
        assert_eq!(found.sort, 2000);
    }

    /// 撤回提示带 newmsgid 时按原消息 id 精确匹配；原消息不在本批时不能
    /// 拿同发送人的其他消息顶替，否则会把没撤回的消息误记成已撤回。
    #[test]
    fn revoke_target_id_prevents_matching_unrevoked_message() {
        let msg = |content: &str, sort: i64| MsgRow {
            kind: "文字".to_string(),
            nick: "张三".to_string(),
            content: content.to_string(),
            time: "21:00".to_string(),
            sort,
            chat: "g1@chatroom".to_string(),
            sender: "u1".to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let notice = MsgRow {
            kind: "撤回".to_string(),
            nick: String::new(),
            content: "\"张三\" 撤回了一条消息".to_string(),
            time: "21:05".to_string(),
            sort: 5000,
            chat: "g1@chatroom".to_string(),
            sender: String::new(),
            link_url: None,
            quote: None,
            revoke_target: Some(2000),
        };

        // 精确命中 2000，即使 3000 是同发送人更晚的一条。
        let orders = vec![
            msg("真正被撤回的", 2000),
            msg("没被撤回的", 3000),
        ];
        let found = find_revoke_target(&notice, &orders, &[], "self", &HashMap::new())
            .expect("应该精确找到 2000");
        assert_eq!(found.content, "真正被撤回的");

        // 原消息 2000 不在本批（例如在导入时段之外）→ 不匹配任何消息。
        let orders = vec![msg("没被撤回的", 3000)];
        assert!(
            find_revoke_target(&notice, &orders, &[], "self", &HashMap::new()).is_none(),
            "不能退而把 3000 当成撤回目标"
        );

        // `Some(0)`：有 newmsgid 但库里查不到原消息，同样不许猜。
        let unresolved = MsgRow {
            revoke_target: Some(0),
            ..notice.clone()
        };
        assert!(
            find_revoke_target(&unresolved, &orders, &[], "self", &HashMap::new()).is_none()
        );

        // 微信把撤回提示放在原消息的位置时，两条 sort 相同，也要精确命中。
        let in_place = MsgRow {
            sort: 2000,
            ..notice.clone()
        };
        let orders = vec![msg("更早的", 1000), msg("被撤回的", 2000)];
        let found = find_revoke_target(&in_place, &orders, &[], "self", &HashMap::new())
            .expect("sort 相同也应该命中");
        assert_eq!(found.content, "被撤回的");

        // 没有 newmsgid 的旧式提示走名称匹配时，同样允许 sort 相同并优先取它。
        let plain = MsgRow {
            revoke_target: None,
            ..in_place
        };
        let found = find_revoke_target(&plain, &orders, &[], "self", &HashMap::new())
            .expect("旧式提示也应取 sort 相同的那条");
        assert_eq!(found.content, "被撤回的");
    }

    /// 没有 newmsgid 的撤回提示不能拿本批新读到的上一条消息顶替：
    /// 原消息撤回后已被微信替换掉，库里剩下的上一条其实是没撤回的。
    #[test]
    fn revoke_without_id_does_not_guess_from_current_batch() {
        let order = MsgRow {
            kind: "文字".to_string(),
            nick: "顺顺顺顺顺顺".to_string(),
            content: "15.20.25三中三40".to_string(),
            time: "19:35".to_string(),
            sort: 1795692907000,
            chat: "57217918789@chatroom".to_string(),
            sender: "wxid_btnfemgm2k9k12".to_string(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        let notice = MsgRow {
            kind: "撤回".to_string(),
            nick: "系统".to_string(),
            content: "\"顺顺顺顺顺顺\" 撤回了一条消息".to_string(),
            time: "19:35".to_string(),
            sort: 1795692933000,
            chat: "57217918789@chatroom".to_string(),
            sender: String::new(),
            link_url: None,
            quote: None,
            revoke_target: None,
        };
        // 本批里只有这条没撤回的消息 → 不记录。
        assert!(
            find_revoke_target(&notice, &[], &[order.clone()], "self", &HashMap::new())
                .is_none(),
            "不能把本批里的上一条消息当成撤回目标"
        );
        // 监控时原消息是之前已经读到的旧消息 → 可以命中。
        let found = find_revoke_target(&notice, &[order], &[], "self", &HashMap::new())
            .expect("旧消息里的原消息应该命中");
        assert_eq!(found.content, "15.20.25三中三40");
    }

    #[test]
    fn code_report_text_merges_complete_zodiacs_by_amount() {
        // 马 = 01,13,25,37,49；这一组五个号都吃了 91 → 合并成「马」。
        let mut reduce: Vec<(u32, f64)> = vec![
            (1, 91.0),
            (13, 91.0),
            (25, 91.0),
            (37, 91.0),
            (49, 91.0),
            (8, 91.0),
            (2, 50.0),
        ];
        reduce.sort_by_key(|(n, _)| *n);
        let text = super::code_report_text(0, &reduce);
        assert_eq!(text, "澳 特码\n马.8各91\n2各50");
    }

    #[test]
    fn draw_paste_accepts_common_formats() {
        for text in [
            "01 02 03 04 05 06 07",
            "01020304050607",
            "1,2,3,4,5,6,7",
            "01、02、03、04、05、06、特07",
        ] {
            let row = parse_draw_paste(text).unwrap();
            assert_eq!(row[0], "1");
            assert_eq!(row[6], "7");
        }
        assert!(parse_draw_paste("01 02 03").is_err());
        assert!(parse_draw_paste("01 01 03 04 05 06 07").is_err());
        assert!(parse_draw_paste("01 02 03 04 05 06 50").is_err());
    }

    #[test]
    fn draw_row_check_reports_missing_and_duplicates() {
        let empty = [String::new(), String::new(), String::new(), String::new(), String::new(), String::new(), String::new()];
        assert!(check_draw_row(&empty).error.is_none());

        let mut row = parse_draw_paste("01 02 03 04 05 06 07").unwrap();
        row[2] = "02".to_string();
        assert!(check_draw_row(&row).error.unwrap().contains("重复"));

        let mut row = parse_draw_paste("01 02 03 04 05 06 07").unwrap();
        row[0].clear();
        assert!(check_draw_row(&row).error.unwrap().contains("还差 1"));
    }

    fn set<T: std::cmp::Eq + std::hash::Hash>(items: &[T]) -> HashSet<T>
    where
        T: Clone,
    {
        items.iter().cloned().collect()
    }

    #[test]
    fn member_selection_overrides_group() {
        let g = set(&["g1".to_string()]);
        let m = set(&[("g1".to_string(), "u1".to_string())]);
        let f = group_filters(&g, &m);
        assert_eq!(f.get("g1"), Some(&vec!["u1".to_string()]));
    }

    #[test]
    fn group_only_means_whole_group() {
        let g = set(&["g1".to_string()]);
        let m = HashSet::new();
        let f = group_filters(&g, &m);
        assert_eq!(f.get("g1"), Some(&vec![]));
    }

    #[test]
    fn member_only_means_member_filter() {
        let g = HashSet::new();
        let m = set(&[("g1".to_string(), "u1".to_string())]);
        let f = group_filters(&g, &m);
        assert_eq!(f.get("g1"), Some(&vec!["u1".to_string()]));
    }

    #[test]
    fn group_source_keeps_same_member_per_parent_chat() {
        let mut a = MemberGroup::default();
        a.contacts.insert("wxid_c".into());
        a.members.insert(("g1@chatroom".into(), "wxid_u".into()));
        let mut b = MemberGroup::default();
        b.members.insert(("g2@chatroom".into(), "wxid_u".into()));

        let src = source_from_groups(&[a, b], &HashSet::new(), &HashSet::new());
        assert!(src.from_groups);
        assert!(src.contacts.contains(&"wxid_c".to_string()));
        // 同一成员在两个群聊里各自保留，不合并。
        assert_eq!(
            src.groups.get("g1@chatroom"),
            Some(&vec!["wxid_u".to_string()])
        );
        assert_eq!(
            src.groups.get("g2@chatroom"),
            Some(&vec!["wxid_u".to_string()])
        );
    }

    #[test]
    fn group_source_includes_current_selection() {
        let mut a = MemberGroup::default();
        a.members.insert(("g1@chatroom".into(), "wxid_u".into()));
        let sel_c = set(&["wxid_x".to_string()]);
        let sel_m = set(&[("g2@chatroom".to_string(), "wxid_y".to_string())]);

        let src = source_from_groups(&[a], &sel_c, &sel_m);
        assert!(src.contacts.contains(&"wxid_x".to_string()));
        assert_eq!(
            src.groups.get("g2@chatroom"),
            Some(&vec!["wxid_y".to_string()])
        );
        assert_eq!(
            src.groups.get("g1@chatroom"),
            Some(&vec!["wxid_u".to_string()])
        );
    }

    fn row(chat: &str, sender: &str, content: &str, sort: i64) -> MsgRow {
        MsgRow {
            kind: "文字".into(),
            nick: sender.into(),
            content: content.into(),
            time: "20:00".into(),
            sort,
            chat: chat.into(),
            sender: sender.into(),
            link_url: None,
            quote: None,
            revoke_target: None,
        }
    }

    #[test]
    fn clear_all_when_nothing_checked() {
        let orders = vec![row("g1", "u1", "a", 1), row("g2", "u2", "b", 2)];
        let out = clear_orders(orders, &HashSet::new(), &HashSet::new());
        assert!(out.is_empty());
    }

    #[test]
    fn clear_group_removes_whole_chat() {
        let orders = vec![row("g1", "u1", "a", 1), row("g1", "u2", "b", 2), row("g2", "u3", "c", 3)];
        let groups = set(&["g1".to_string()]);
        let out = clear_orders(orders, &groups, &HashSet::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chat, "g2");
    }

    #[test]
    fn clear_member_removes_only_that_sender() {
        let orders = vec![
            row("g1", "u1", "a", 1),
            row("g1", "u2", "b", 2),
            row("g2", "u1", "c", 3),
        ];
        let members = set(&[("g1".to_string(), "u1".to_string())]);
        let out = clear_orders(orders, &HashSet::new(), &members);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].content, "b");
        assert_eq!(out[1].content, "c");
    }
}

fn now_hms() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn sanitize_group_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 统一的按钮悬浮样式：不在 Button 上写死 fill / stroke，而是把
/// inactive / hovered / active 三档交给 egui 的状态样式，于是所有按钮
/// 都有背景提亮、描边高亮和手型光标。
fn styled_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    fill: Color32,
    hover_fill: Color32,
    active_fill: Color32,
    stroke: Stroke,
    hover_stroke: Stroke,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    styled_button_enabled(
        ui,
        text,
        true,
        fill,
        hover_fill,
        active_fill,
        stroke,
        hover_stroke,
        corner,
        min_size,
    )
}

fn styled_button_enabled(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    enabled: bool,
    fill: Color32,
    hover_fill: Color32,
    active_fill: Color32,
    stroke: Stroke,
    hover_stroke: Stroke,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    let mut style = (**ui.style()).clone();
    style.visuals.widgets.inactive.weak_bg_fill = fill;
    style.visuals.widgets.inactive.bg_stroke = stroke;
    style.visuals.widgets.inactive.corner_radius = corner;
    style.visuals.widgets.hovered.weak_bg_fill = hover_fill;
    style.visuals.widgets.hovered.bg_stroke = hover_stroke;
    style.visuals.widgets.hovered.corner_radius = corner;
    style.visuals.widgets.hovered.expansion = 1.0;
    style.visuals.widgets.active.weak_bg_fill = active_fill;
    style.visuals.widgets.active.bg_stroke = hover_stroke;
    style.visuals.widgets.active.corner_radius = corner;
    style.visuals.widgets.active.expansion = 0.5;
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add_enabled(
            enabled,
            egui::Button::new(text)
                .corner_radius(corner)
                .min_size(min_size),
        )
    })
    .inner
}

fn ghost_button_sized(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    styled_button(
        ui,
        text,
        Color32::TRANSPARENT,
        BTN_HOVER_BG,
        BTN_ACTIVE_BG,
        Stroke::NONE,
        Stroke::new(1.0, BTN_HOVER_BORDER),
        corner,
        min_size,
    )
}

/// 顶部工具栏按钮的一套配色：填充、悬停、按下、边框、悬停边框。
#[derive(Clone, Copy)]
struct ToolbarTone {
    fill: Color32,
    hover: Color32,
    active: Color32,
    edge: Color32,
    hover_edge: Color32,
}

const TOOLBAR_TONE: ToolbarTone = ToolbarTone {
    fill: TOOLBAR_BTN,
    hover: TOOLBAR_BTN_HOVER,
    active: TOOLBAR_BTN_ACTIVE,
    edge: TOOLBAR_BTN_EDGE,
    hover_edge: TOOLBAR_BTN_EDGE_HOVER,
};

/// 主操作（导入注单）：绿色实心。
const TOOLBAR_TONE_GREEN: ToolbarTone = ToolbarTone {
    fill: GREEN,
    hover: GREEN_HOVER,
    active: GREEN_ACTIVE,
    edge: GREEN,
    hover_edge: GREEN_HOVER,
};

/// 删除类操作（清空注单）：红色实心。
const TOOLBAR_TONE_RED: ToolbarTone = ToolbarTone {
    fill: RED_SOLID,
    hover: RED_SOLID_HOVER,
    active: RED_SOLID_ACTIVE,
    edge: RED_SOLID,
    hover_edge: RED_SOLID_HOVER,
};

/// 顶部工具栏的功能分组标题：小号弱化文字，悬浮时说明本组包含什么。
fn top_group_label(ui: &mut egui::Ui, text: &str, tip: &str) {
    ui.label(RichText::new(text).size(9.5).color(TOOLBAR_SUB))
        .on_hover_text(tip);
    ui.add_space(5.0);
}

/// 顶部工具栏的分组间隔：留白 + 浅色竖线 + 留白。
fn top_group_gap(ui: &mut egui::Ui) {
    ui.add_space(9.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 16.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, TOOLBAR_BTN_EDGE));
    ui.add_space(9.0);
}

/// 顶部深色工具栏的按钮：深底上的浅色按钮，白字；禁用时无填充、灰字、灰边。
fn top_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    top_button_tone(ui, text, width, true, TOOLBAR_TONE)
}

fn top_button_enabled(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    enabled: bool,
) -> egui::Response {
    top_button_tone(ui, text, width, enabled, TOOLBAR_TONE)
}

fn top_button_tone(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    enabled: bool,
    tone: ToolbarTone,
) -> egui::Response {
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = Vec2::new(9.0, 2.0);
    style.visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TOOLBAR_OFF_EDGE);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TOOLBAR_OFF_TEXT);
    ui.scope(|ui| {
        ui.set_style(style);
        styled_button_enabled(
            ui,
            RichText::new(text)
                .size(11.5)
                .color(if enabled {
                    Color32::WHITE
                } else {
                    TOOLBAR_OFF_TEXT
                }),
            enabled,
            tone.fill,
            tone.hover,
            tone.active,
            Stroke::new(1.0, tone.edge),
            Stroke::new(1.0, tone.hover_edge),
            CornerRadius::same(6),
            Vec2::new(width, 22.0),
        )
    })
    .inner
}

/// 窗口标题里只显示最大的一个时间单位（如「365天」），完整时间在「权限」弹窗里。
fn short_remaining(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or(text)
}

/// 帧首记录当前聚焦文本框的选区。
///
/// 右键点进 TextEdit 会先按点击位置重设光标、清掉原选区；右键菜单里的
/// 「复制 / 剪切」就无从下手。所以在任何 UI 绘制之前把上一帧的选区存下来，
/// 菜单打开期间用它兜底。
fn snapshot_focused_text_selection(ctx: &egui::Context) {
    let Some(focused) = ctx.memory(|m| m.focused()) else {
        return;
    };
    let Some(range) = egui::TextEdit::load_state(ctx, focused)
        .and_then(|s| s.cursor.char_range())
        .filter(|r| !r.is_empty())
    else {
        return;
    };
    ctx.data_mut(|d| d.insert_temp(text_sel_snapshot_id(), Some((focused, range))));
}

fn text_sel_snapshot_id() -> egui::Id {
    egui::Id::unique("wv_text_selection_snapshot")
}

/// 文本框的右键菜单：剪切 / 复制 / 粘贴 / 全选。
/// `id` 是 TextEdit 的控件 id（默认取 `response.id` 即可），用来读写光标与选区。
fn text_edit_context_menu(
    ctx: &egui::Context,
    id: egui::Id,
    text: &mut String,
    response: &mut egui::Response,
) {
    let state = egui::TextEdit::load_state(ctx, id);
    let current = state
        .as_ref()
        .and_then(|s| s.cursor.char_range())
        .filter(|r| !r.is_empty());
    let current_empty = current.is_none();
    let menu_open = response.context_menu_opened();
    // 右键按下 / 按住 / 松开：TextEdit 已经把选区清掉，允许用帧首快照兜底。
    let right_click = response.secondary_clicked()
        || ctx.input(|i| i.pointer.secondary_down());
    let range = current.or_else(|| {
        if menu_open || right_click {
            ctx.data(|d| {
                d.get_temp::<Option<(egui::Id, egui::text_selection::CCursorRange)>>(
                    text_sel_snapshot_id(),
                )
            })
            .flatten()
            .filter(|(sid, _)| *sid == id)
            .map(|(_, r)| r)
        } else {
            None
        }
    });

    let mut edited = false;
    response.context_menu(|ui| {
        compact_context_menu(ui);
        let range = range.map(|r| {
            let a = r.primary.index.0.min(r.secondary.index.0);
            let b = r.primary.index.0.max(r.secondary.index.0);
            (a, b)
        });
        let has_sel = range.is_some_and(|(a, b)| a < b);
        let selected = match range {
            Some((a, b)) if a < b => {
                let start = text.char_indices().nth(a).map(|(i, _)| i).unwrap_or(text.len());
                let end = text.char_indices().nth(b).map(|(i, _)| i).unwrap_or(text.len());
                text[start..end].to_string()
            }
            _ => String::new(),
        };

        let mut set_caret: Option<usize> = None;
        if ui
            .add_enabled(has_sel, egui::Button::new("剪切").shortcut_text("Ctrl+X"))
            .clicked()
        {
            ui.ctx().copy_text(selected.clone());
            if let Some((a, b)) = range {
                set_caret = Some(replace_chars(text, a, b, ""));
                edited = true;
            }
            ui.close();
        }
        if ui
            .add_enabled(has_sel, egui::Button::new("复制").shortcut_text("Ctrl+C"))
            .clicked()
        {
            ui.ctx().copy_text(selected.clone());
            ui.close();
        }
        if ui.button("粘贴").on_hover_text("Ctrl+V").clicked() {
            if let Some(pasted) = crate::clipboard::read_text() {
                let (a, b) = range.unwrap_or_else(|| {
                    let n = text.chars().count();
                    (n, n)
                });
                set_caret = Some(replace_chars(text, a, b, &pasted));
                edited = true;
            }
            ui.close();
        }
        if ui.button("全选").on_hover_text("Ctrl+A").clicked() {
            let n = text.chars().count();
            let mut st = state.clone().unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(n),
            )));
            st.store(ctx, id);
            ui.close();
        }

        if let Some(caret) = set_caret {
            let mut st = state.unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::one(
                egui::text::CCursor::new(caret),
            )));
            st.store(ctx, id);
        }
    });
    if edited {
        // 菜单里的剪切 / 粘贴直接改了字符串，不会触发 TextEdit 自身的
        // changed 标记；补上，依赖 changed() 的保存 / 过滤逻辑才会运行。
        response.mark_changed();
    }

    // 菜单已关闭、也没有正在右键时，帧首快照就没用了；清掉，避免下次
    // 在没有选区时误用上一轮的选区。
    if current_empty && !menu_open && !right_click {
        ctx.data_mut(|d| {
            if d.get_temp::<Option<(egui::Id, egui::text_selection::CCursorRange)>>(
                text_sel_snapshot_id(),
            )
            .flatten()
            .is_some_and(|(sid, _)| sid == id)
            {
                d.remove_temp::<Option<(egui::Id, egui::text_selection::CCursorRange)>>(
                    text_sel_snapshot_id(),
                );
            }
        });
    }
}

/// 把字符区间 `[a, b)` 换成 `with`，返回换完后光标应在的字符位置。
fn replace_chars(text: &mut String, a: usize, b: usize, with: &str) -> usize {
    let start = text.char_indices().nth(a).map(|(i, _)| i).unwrap_or(text.len());
    let end = text.char_indices().nth(b).map(|(i, _)| i).unwrap_or(text.len());
    text.replace_range(start..end, with);
    a + with.chars().count()
}

/// 右键菜单统一缩小一档。
fn compact_context_menu(ui: &mut egui::Ui) {
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = Vec2::new(7.0, 2.0);
    style.spacing.item_spacing = Vec2::new(4.0, 2.0);
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(10.5));
    ui.set_style(style);
}

fn primary_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    enabled: bool,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    styled_button_enabled(
        ui,
        text,
        enabled,
        GREEN,
        GREEN_HOVER,
        GREEN_ACTIVE,
        Stroke::NONE,
        Stroke::new(1.0, Color32::from_rgb(0x86, 0xEF, 0xAC)),
        corner,
        min_size,
    )
}

fn blue_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    enabled: bool,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    styled_button_enabled(
        ui,
        text,
        enabled,
        MONEY_BLUE,
        BLUE_SOLID_HOVER,
        BLUE_SOLID_ACTIVE,
        Stroke::NONE,
        Stroke::new(1.0, Color32::from_rgb(0x93, 0xC5, 0xFD)),
        corner,
        min_size,
    )
}

fn danger_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    enabled: bool,
    corner: CornerRadius,
    min_size: Vec2,
) -> egui::Response {
    styled_button_enabled(
        ui,
        text,
        enabled,
        RED_SOLID,
        RED_SOLID_HOVER,
        RED_SOLID_ACTIVE,
        Stroke::NONE,
        Stroke::new(1.0, Color32::from_rgb(0xFC, 0xA5, 0xA5)),
        corner,
        min_size,
    )
}

fn tiny_button(text: &str, color: Color32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).size(11.5).color(color))
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 24.0))
}

/// 「注单消息」面板顶部的小页签（未撤回 / 引用 / 已撤回）。
fn order_tab_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    // 页签做紧凑一点，把高度留给下面的消息列表。
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = Vec2::new(8.0, 1.0);
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add(
            egui::Button::new(
                RichText::new(text)
                    .size(10.0)
                    .strong()
                    .color(if selected { GREEN_DEEP } else { SUB }),
            )
            .fill(if selected { GREEN_TINT } else { CARD })
            .stroke(Stroke::new(
                1.0,
                if selected { GREEN } else { BORDER },
            ))
            .corner_radius(CornerRadius::same(5))
            .min_size(Vec2::new(0.0, 20.0)),
        )
    })
    .inner
}

fn tab_button(ui: &mut egui::Ui, label: String, selected: bool) -> bool {
    let text = RichText::new(label)
        .size(13.5)
        .color(if selected { Color32::WHITE } else { TEXT });
    let (fill, hover, stroke) = if selected {
        (GREEN, GREEN_HOVER, Stroke::NONE)
    } else {
        (
            CARD,
            BTN_HOVER_BG,
            Stroke::new(1.0, BORDER),
        )
    };
    styled_button(
        ui,
        text,
        fill,
        hover,
        if selected { GREEN_ACTIVE } else { BTN_ACTIVE_BG },
        stroke,
        if selected {
            Stroke::NONE
        } else {
            Stroke::new(1.0, BTN_HOVER_BORDER)
        },
        CornerRadius::same(8),
        Vec2::new(118.0, 32.0),
    )
    .clicked()
}

fn time_picker(ui: &mut egui::Ui, id: &str, time: &mut NaiveTime) {
    let mut h = time.hour();
    let mut m = time.minute();
    egui::ComboBox::from_id_salt(format!("{id}_hour"))
        .width(58.0)
        .selected_text(RichText::new(format!("{h:02}")).size(13.0))
        .show_ui(ui, |ui| {
            for hh in 0..24u32 {
                ui.selectable_value(&mut h, hh, format!("{hh:02}"));
            }
        });
    ui.label(RichText::new(":").size(14.0).color(SUB));
    egui::ComboBox::from_id_salt(format!("{id}_minute"))
        .width(58.0)
        .height(230.0)
        .selected_text(RichText::new(format!("{m:02}")).size(13.0))
        .show_ui(ui, |ui| {
            for mm in 0..60u32 {
                ui.selectable_value(&mut m, mm, format!("{mm:02}"));
            }
        });
    if let Some(t) = NaiveTime::from_hms_opt(h, m, 0) {
        *time = t;
    }
}

fn check_indicator(ui: &mut egui::Ui, checked: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(16.0));
    if checked {
        ui.painter()
            .rect_filled(box_rect, CornerRadius::same(4), GREEN);
        let c = box_rect.center();
        let stroke = Stroke::new(2.0, Color32::WHITE);
        ui.painter().line_segment(
            [c + Vec2::new(-4.0, 0.5), c + Vec2::new(-1.0, 3.5)],
            stroke,
        );
        ui.painter().line_segment(
            [c + Vec2::new(-1.0, 3.5), c + Vec2::new(4.0, -3.0)],
            stroke,
        );
    } else {
        ui.painter()
            .rect_filled(box_rect, CornerRadius::same(4), CARD);
        ui.painter().rect_stroke(
            box_rect,
            CornerRadius::same(4),
            Stroke::new(1.0, Color32::from_rgb(0xC6, 0xCB, 0xD2)),
            egui::StrokeKind::Inside,
        );
    }
}

fn row_ui(
    ui: &mut egui::Ui,
    height: f32,
    sense: Sense,
    selected: bool,
    add: impl FnOnce(&mut egui::Ui),
) -> (egui::Rect, egui::Response) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    let bg_rect = rect.shrink2(Vec2::new(2.0, 1.0));
    if selected {
        ui.painter().rect_filled(
            bg_rect,
            CornerRadius::same(8),
            Color32::from_rgb(0xEC, 0xFA, 0xF2),
        );
    }
    if resp.hovered() {
        let color = if selected {
            Color32::from_rgb(0xDD, 0xF6, 0xE8)
        } else {
            HOVER
        };
        ui.painter()
            .rect_filled(bg_rect, CornerRadius::same(8), color);
    }
    let inner = rect.shrink2(Vec2::new(14.0, 6.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    add(&mut child);
    (rect, resp)
}

fn badge(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(10.5), fg);
    let pad = Vec2::new(5.0, 1.5);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    ui.painter().galley(rect.min + pad, galley, fg);
}

fn empty_state(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(64.0);
        empty_mark(ui);
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(12.5).color(SUB));
    });
}

fn empty_detail(ui: &mut egui::Ui, title: &str, hint: &str) {
    let h = ui.available_height();
    ui.vertical_centered(|ui| {
        ui.add_space((h * 0.28).max(60.0));
        empty_mark(ui);
        ui.add_space(14.0);
        ui.label(RichText::new(title).size(13.0).strong().color(TEXT));
        ui.add_space(5.0);
        ui.add(
            egui::Label::new(RichText::new(hint).size(11.5).color(SUB))
                .wrap(),
        );
    });
}

/// 空态占位：一个安静的页面轮廓，避免大段文字孤零零地悬在中间。
fn empty_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        CornerRadius::same(9),
        Stroke::new(1.0, BORDER),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        egui::Rect::from_center_size(rect.center(), Vec2::new(14.0, 9.0)),
        CornerRadius::same(3),
        Color32::from_rgb(0xED, 0xF0, 0xF3),
    );
}

fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, BORDER));
}

fn count_pill(ui: &mut egui::Ui, n: usize) {
    let text = n.to_string();
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(10.5), SUB);
    let pad = Vec2::new(6.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(0xEC, 0xEF, 0xF2));
    ui.painter().galley(rect.min + pad, galley, SUB);
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(SUB));
}

/// 成员索引条目：色点 + 名称；选中时铺绿底并在左侧压一条书签线。
fn rail_item(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
    let fill = if selected {
        GREEN_TINT
    } else if resp.hovered() {
        HOVER
    } else {
        CARD
    };
    let stroke = if selected {
        Stroke::new(1.0, Color32::from_rgb(0xB7, 0xD8, 0xC7))
    } else {
        Stroke::new(1.0, Color32::from_rgb(0xE4, 0xE8, 0xEE))
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(3),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                rect.min + Vec2::new(1.0, 6.0),
                Vec2::new(3.0, (rect.height() - 12.0).max(2.0)),
            ),
            CornerRadius::same(2),
            GREEN,
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(5.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.add(
        egui::Label::new(
            RichText::new(label)
                .size(11.0)
                .color(if selected { GREEN_DEEP } else { TEXT }),
        )
        .truncate()
        // 整行才是点击目标；文字本身不参与选择/拖动，否则悬停在文字上会变成
        // I 形光标并吞掉点击。
        .selectable(false),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn contains_lo(hay: &str, q: &str) -> bool {
    hay.to_lowercase().contains(q)
}

fn matches_person(p: &Person, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    [&p.display_name, &p.nick_name, &p.remark, &p.alias, &p.username]
        .iter()
        .any(|s| !s.is_empty() && contains_lo(s, q))
}

fn color_for(name: &str) -> Color32 {
    const PALETTE: [Color32; 10] = [
        Color32::from_rgb(0x5B, 0x8D, 0xEF),
        Color32::from_rgb(0xEF, 0x8B, 0x5B),
        Color32::from_rgb(0x35, 0xB5, 0x8A),
        Color32::from_rgb(0x9B, 0x7B, 0xD8),
        Color32::from_rgb(0xE0, 0x7B, 0x9A),
        Color32::from_rgb(0x4F, 0xA8, 0xD8),
        Color32::from_rgb(0xC9, 0x9B, 0x4A),
        Color32::from_rgb(0x6E, 0xB8, 0x5F),
        Color32::from_rgb(0xB8, 0x6F, 0xC9),
        Color32::from_rgb(0x7A, 0x8A, 0x99),
    ];
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

/// 注单消息一行：`状态 | 昵称 | 内容 | 时间`，不再用表头和分列。
fn msg_row(
    ui: &mut egui::Ui,
    r: &MsgRow,
    index: usize,
    selected: bool,
    status: OrderStatus,
    _info: Option<&crate::info_code::OrderInfoHits>,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    let is_image = r.kind == "图片";
    let is_info = !is_image && matches!(status, OrderStatus::Info);
    let success = !is_image && matches!(status, OrderStatus::Success);
    let tint = if is_image {
        None
    } else {
        match status {
            OrderStatus::Doubtful => Some((BLUE_BG, BLUE_HOVER)),
            OrderStatus::Failed => Some((RED_BG, RED_HOVER)),
            // 成功：整行浅绿底 + 绿字。
            OrderStatus::Success => Some((
                GREEN_TINT,
                Color32::from_rgb(0xDD, 0xF3, 0xE7),
            )),
            // 信息：整行黄色。
            OrderStatus::Info => Some((
                GOLD_BG,
                Color32::from_rgb(0xFF, 0xF1, 0xC8),
            )),
        }
    };
    // 选中行的底色比该状态的普通底色再深一档，和「成功绿底」区分开，
    // 否则成功行被选中时看不出高亮。
    let selected_bg = if is_image {
        BLUE_HOVER
    } else {
        match status {
            OrderStatus::Doubtful => Color32::from_rgb(0xCF, 0xE2, 0xF8),
            OrderStatus::Failed => Color32::from_rgb(0xFF, 0xD8, 0xD3),
            OrderStatus::Success => Color32::from_rgb(0xC9, 0xEB, 0xD8),
            OrderStatus::Info => Color32::from_rgb(0xFF, 0xE4, 0xA6),
        }
    };
    let bg = if selected {
        selected_bg
    } else if resp.hovered() {
        tint.map_or(HOVER, |(_, hover)| hover)
    } else if let Some((normal, _)) = tint {
        normal
    } else if index % 2 == 1 {
        STRIPE
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    }
    let (status_label, status_color, fg) = if is_image {
        ("图片", BLUE_FG, TEXT)
    } else {
        match status {
            OrderStatus::Doubtful => ("存疑", BLUE_FG, BLUE_FG),
            OrderStatus::Failed => ("失败", RED, RED),
            OrderStatus::Success => ("成功", GREEN_DEEP, GREEN_DEEP),
            OrderStatus::Info => ("信息", GOLD_FG, GOLD_FG),
        }
    };
    let sep_color = if is_info {
        GOLD_FG
    } else if success {
        GREEN_DEEP
    } else {
        MUTED
    };
    let time_color = if is_info {
        GOLD_FG
    } else if success {
        GREEN_DEEP
    } else {
        SUB
    };
    if selected {
        let bar = if is_image {
            BLUE_FG
        } else {
            match status {
                OrderStatus::Doubtful => BLUE_FG,
                OrderStatus::Failed => RED,
                OrderStatus::Success => GREEN,
                OrderStatus::Info => GOLD_FG,
            }
        };
        // 选中标识竖线加粗到 3px，配合更深的选中底色，一眼能看出当前选中的是哪一条。
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())),
            CornerRadius::ZERO,
            bar,
        );
    }

    // 右侧「| 时间」固定宽、贴右对齐；左侧内容占剩余宽度，超出省略。
    let time_text = format!("| {}", r.time);
    let time_font = FontId::proportional(12.0);
    let time_w = ui
        .painter()
        .layout_no_wrap(time_text.clone(), time_font.clone(), time_color)
        .size()
        .x;
    let pad = 5.0;
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.add_space(pad);
    right.add(
        egui::Label::new(
            RichText::new(time_text).size(12.0).color(time_color),
        )
        .truncate()
        .selectable(false),
    );

    let left_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + pad, rect.min.y),
        egui::pos2(
            (rect.max.x - time_w - pad * 2.0).max(rect.min.x + pad + 20.0),
            rect.max.y,
        ),
    );
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = left_rect.width().max(20.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let fmt = |color: Color32| egui::TextFormat {
        font_id: FontId::proportional(12.0),
        color,
        ..Default::default()
    };
    job.append(status_label, 0.0, fmt(status_color));
    job.append(" | ", 0.0, fmt(sep_color));
    job.append(&truncate_display(&r.nick, 4), 0.0, fmt(fg));
    job.append(" | ", 0.0, fmt(sep_color));
    job.append(&truncate_display(&r.content, 12), 0.0, fmt(fg));
    left.add(egui::Label::new(job).selectable(false));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 踩中信息码的行下面那一条黄色明细：具体组合、每组金额、合计。
fn info_small_button(ui: &mut egui::Ui, label: &str, color: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(10.5).color(color))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(0.0, 20.0)),
    )
}

fn info_hit_line(ui: &mut egui::Ui, groups: &[(String, f64)]) {
    if groups.is_empty() {
        return;
    }
    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // 一条信息码一行，胶囊不并排。
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (text, amount) in groups {
                    egui::Frame::new()
                        .fill(INFO_CHIP_BG)
                        .stroke(Stroke::new(1.0, INFO_CHIP_BORDER))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(5, 0))
                        .show(ui, |ui| {
                            // 文本和金额必须在同一行：给文本一个固定上限，超出省略，
                            // 金额跟在后面，不能再被顶到下一行。
                            let text_w = ui
                                .painter()
                                .layout_no_wrap(
                                    text.clone(),
                                    FontId::proportional(11.0),
                                    INFO_CHIP_TEXT,
                                )
                                .size()
                                .x
                                .min(160.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::ZERO;
                                ui.allocate_ui_with_layout(
                                    Vec2::new(text_w, 16.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(text)
                                                    .size(11.0)
                                                    .color(INFO_CHIP_TEXT),
                                            )
                                            .truncate()
                                            .selectable(false),
                                        )
                                        .on_hover_text(text);
                                    },
                                );
                                ui.label(
                                    RichText::new("│")
                                        .size(10.0)
                                        .color(INFO_CHIP_BORDER),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(fmt_money(*amount))
                                            .size(11.0)
                                            .strong()
                                            .color(INFO_CHIP_AMOUNT),
                                    )
                                    .truncate()
                                    .selectable(false),
                                );
                            });
                        });
                }
            });
        });
}

/// 鼠标停在 egui 窗口 / 弹窗的标题栏（可拖动区域）时，把光标换成十字移动箭头。
///
/// 标题栏由 egui 在内容闭包之前绘制，这里用内容矩形反推它的位置；
/// 右上角关闭按钮区域不处理，保留手型光标。
fn window_header_cursor(ui: &egui::Ui) {
    let Some(pos) = ui.ctx().pointer_hover_pos() else {
        return;
    };
    // 只处理最上层窗口，避免后台窗口的标题栏抢走光标。
    // 测试环境下 layer_id_at 可能只返回 background，这时按几何位置继续判断。
    if let Some(top) = ui.ctx().layer_id_at(pos) {
        if top != ui.layer_id() && top != egui::LayerId::background() {
            return;
        }
    }
    let style = ui.style();
    let margin = style.spacing.window_margin;
    let (mt, mb) = (margin.top as f32, margin.bottom as f32);
    let (ml, mr) = (margin.left as f32, margin.right as f32);
    let heading_h = ui
        .ctx()
        .fonts_mut(|f| f.row_height(&egui::TextStyle::Heading.resolve(style)));
    let content = ui.max_rect();
    // 内容矩形已经让出了窗口内边距，往上推回标题栏的范围。
    let body_top = content.top() - mt;
    let header_top = body_top - (heading_h + mt + mb) - 2.0;
    let left = content.left() - ml - 4.0;
    let right = content.right() + mr + 4.0;
    let header = egui::Rect::from_min_max(
        egui::pos2(left, header_top),
        egui::pos2(right, body_top),
    );
    if !header.contains(pos) {
        return;
    }
    // 关闭按钮在标题栏右上角，应该保持手型。
    let close = egui::Rect::from_min_max(
        egui::pos2(right - 44.0, header_top),
        egui::pos2(right, body_top),
    );
    if close.contains(pos) {
        return;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
}

fn separator_line(ui: &mut egui::Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, height), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, BORDER));
}

/// 明细表固定列宽：彩种 / 玩法 / 内容 / 注数 / 单注 / 小计。
/// 改这里即可调整各列宽度；内容列超出后在列内换行，表格总宽 = 六列之和。
const DETAIL_COL_WIDTHS: [f32; 6] = [45.0, 58.0, 156.0, 35.0, 35.0, 35.0];

fn detail_widths() -> (f32, [f32; 6]) {
    (DETAIL_COL_WIDTHS.iter().sum(), DETAIL_COL_WIDTHS)
}

/// 明细表单行行高；内容换行时按行数翻倍，内容在单元格里垂直居中。
const DETAIL_ROW_H: f32 = 26.0;

fn detail_content_row_height(lines: usize) -> f32 {
    DETAIL_ROW_H * lines.max(1) as f32
}

fn detail_part_width(ui: &egui::Ui, part: &DetailContentPart) -> f32 {
    match part {
        DetailContentPart::Token(t) => {
            let color = detail_token_color(t);
            ui.painter()
                .layout_no_wrap(t.clone(), FontId::proportional(11.5), color)
                .size()
                .x
                + 8.0
        }
        DetailContentPart::Sep(s) => {
            let (label, _) = detail_sep_display(s);
            ui.painter()
                .layout_no_wrap(
                    label.to_string(),
                    FontId::proportional(10.0),
                    Color32::PLACEHOLDER,
                )
                .size()
                .x
        }
    }
}

/// 内容列在给定列宽下会折成几行（与 `detail_content_cell` 的换行规则一致）。
fn detail_content_lines(
    ui: &egui::Ui,
    content: &str,
    mul_sep: &str,
    width: f32,
) -> usize {
    // Frame 左右各 3px 内边距 + 各 1px 描边，内容可用宽度是 width - 8。
    let max_w = (width - 8.0).max(10.0);
    let mut lines = 1usize;
    let mut x = 0.0f32;
    for (i, part) in detail_content_parts(content, mul_sep).iter().enumerate() {
        let piece = detail_part_width(ui, part);
        if i > 0 {
            if x + 3.0 + piece > max_w && x > 0.0 {
                lines += 1;
                x = 0.0;
            } else {
                x += 3.0;
            }
        }
        x += piece;
    }
    lines
}

fn detail_table_header(ui: &mut egui::Ui, widths: &[f32; 6]) {
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 26.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, HEAD_BG);
    ui.painter().hline(
        rect.x_range(),
        rect.max.y - 0.5,
        Stroke::new(1.0, BORDER),
    );
    let mut x = rect.min.x;
    for (i, (w, t)) in widths
        .iter()
        .zip(["彩种", "玩法", "内容", "注数", "单注", "小计"])
        .enumerate()
    {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            egui::vec2(*w, rect.height()),
        );
        if i + 1 < widths.len() {
            ui.painter().vline(
                cell.max.x - 0.5,
                rect.y_range(),
                Stroke::new(1.0, BORDER),
            );
        }
        // 「内容」表头与左对齐的内容块保持一致；其余列仍居中。
        let content_col = i == 2;
        ui.painter().text(
            if content_col {
                egui::pos2(cell.min.x + 3.0, cell.center().y)
            } else {
                cell.center()
            },
            if content_col {
                Align2::LEFT_CENTER
            } else {
                Align2::CENTER_CENTER
            },
            t,
            FontId::proportional(11.5),
            SUB,
        );
        x += *w;
    }
}

fn detail_table_row(
    ui: &mut egui::Ui,
    item: &BetItem,
    widths: &[f32; 6],
    mul_sep: &str,
) {
    let total: f32 = widths.iter().sum();
    // 内容列可能换行：按实际行数计算行高，其它列在变高的行里垂直居中。
    let content_lines = detail_content_lines(ui, &item.content, mul_sep, widths[2]);
    let row_h = detail_content_row_height(content_lines);
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(total, row_h), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = 0.0;
    detail_text_cell(&mut child, widths[0], &item.lottery, TEXT, 12.5, true, row_h);
    detail_text_cell(
        &mut child,
        widths[1],
        &item.play,
        detail_play_color(&item.play),
        12.5,
        true,
        row_h,
    );
    detail_content_cell(
        &mut child,
        widths[2],
        &item.content,
        mul_sep,
        row_h,
        content_lines,
    );
    detail_text_cell(
        &mut child,
        widths[3],
        &convert::fmt_amount(item.bets),
        TEXT,
        12.5,
        true,
        row_h,
    );
    detail_text_cell(
        &mut child,
        widths[4],
        &stake_amount_text(&item.stake),
        MONEY_BLUE,
        12.5,
        true,
        row_h,
    );
    detail_text_cell(
        &mut child,
        widths[5],
        &fmt_money(item.amount),
        MONEY_RED,
        12.5,
        true,
        row_h,
    );
    // 行底边 + 列分隔线；外框由外层 Frame 画。
    ui.painter().hline(
        rect.x_range(),
        rect.max.y - 0.5,
        Stroke::new(1.0, BORDER),
    );
    let mut x = rect.min.x;
    for w in widths.iter().take(widths.len() - 1) {
        x += *w;
        ui.painter()
            .vline(x - 0.5, rect.y_range(), Stroke::new(1.0, BORDER));
    }
}

fn detail_text_cell(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    color: Color32,
    size: f32,
    center: bool,
    row_h: f32,
) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (width - 6.0).max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color,
            ..Default::default()
        },
    );
    if center {
        // 固定高度单元格 + centered_and_justified：水平、垂直都落在单元格正中心。
        ui.allocate_ui_with_layout(
            Vec2::new(width, row_h),
            Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.add(egui::Label::new(job).selectable(false));
            },
        );
    } else {
        ui.allocate_ui_with_layout(
            Vec2::new(width, row_h),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_min_width(width);
                ui.set_max_width(width);
                ui.add_space(3.0);
                ui.add(egui::Label::new(job).selectable(false));
            },
        );
    }
}

fn detail_content_cell(
    ui: &mut egui::Ui,
    width: f32,
    content: &str,
    mul_sep: &str,
    row_h: f32,
    lines: usize,
) {
    let inner_w = (width - 8.0).max(10.0);
    // Frame 占位不会走父布局的垂直居中（只按第一行的高度定位）。
    // 先按行数算出整块内容的高度，再把整块内容放在上下剩余空间的正中间。
    let line_h = detail_line_height(ui);
    let content_h = line_h * lines as f32 + 2.0 * lines.saturating_sub(1) as f32;
    let top = ((row_h - content_h) * 0.5).max(0.0);
    ui.allocate_ui_with_layout(
        Vec2::new(width, row_h),
        Layout::top_down(Align::Min),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            ui.add_space(top);
            egui::Frame::new()
                .inner_margin(Margin::symmetric(3, 0))
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    // 内容超过列宽时自动换行，不会再把表格往右撑开。
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 2.0);
                    ui.horizontal_wrapped(|ui| {
                        for part in detail_content_parts(content, mul_sep) {
                            match part {
                                DetailContentPart::Token(t) => detail_token_box(ui, &t),
                                DetailContentPart::Sep(s) => {
                                    let (label, color) = detail_sep_display(&s);
                                    ui.label(RichText::new(label).size(10.0).color(color));
                                }
                            }
                        }
                    });
                });
        },
    );
}

/// 换行布局里每一行的实际高度（至少是 `interact_size.y`，不是文字高度）。
fn detail_line_height(ui: &egui::Ui) -> f32 {
    let token_box_h = ui
        .painter()
        .layout_no_wrap(
            "0".to_string(),
            FontId::proportional(11.5),
            Color32::PLACEHOLDER,
        )
        .size()
        .y
        + 2.0; // 方块上下各 1px 描边
    ui.spacing().interact_size.y.max(token_box_h)
}

enum DetailContentPart {
    Token(String),
    Sep(String),
}

/// 内容列：数字 / 生肖 / 尾数等切成方块；逗号、括号全部去掉，拖只作为组间分隔符。
fn detail_content_parts(content: &str, mul_sep: &str) -> Vec<DetailContentPart> {
    let mut parts = Vec::new();
    if !mul_sep.is_empty() && content.contains(mul_sep) {
        for (i, group) in content.split(mul_sep).enumerate() {
            if i > 0 {
                parts.push(DetailContentPart::Sep(mul_sep.to_string()));
            }
            push_detail_group(&mut parts, group);
        }
    } else {
        push_detail_group(&mut parts, content);
    }
    parts
}

/// 把一段内容切成方块。内容里残留的「拖」（分隔词配置成碰 / t 等时可能出现）
/// 也按组间分隔符处理，保证内容列的「拖」统一显示成红色「X」。
fn push_detail_group(parts: &mut Vec<DetailContentPart>, group: &str) {
    for tok in detail_split_tokens(group) {
        if tok.contains('拖') {
            for (i, seg) in tok.split('拖').enumerate() {
                if i > 0 {
                    parts.push(DetailContentPart::Sep("拖".to_string()));
                }
                if !seg.is_empty() {
                    parts.push(DetailContentPart::Token(seg.to_string()));
                }
            }
        } else {
            parts.push(DetailContentPart::Token(tok));
        }
    }
}

/// 内容列的分隔符显示：「拖」（配置成 t / T 时同样）统一显示成红色「X」。
fn detail_sep_display(sep: &str) -> (&str, Color32) {
    let t = sep.trim();
    if t == "拖" || t.eq_ignore_ascii_case("t") {
        ("X", RED)
    } else {
        (sep, MUTED)
    }
}

fn detail_split_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                ',' | '，'
                    | '、'
                    | '；'
                    | ';'
                    | '|'
                    | '｜'
                    | '/'
                    | '／'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
                    | '「'
                    | '」'
                    | '-'
                    | '－'
            )
    })
    .map(str::trim)
    .filter(|t| !t.is_empty())
    .map(str::to_string)
    .collect()
}

fn detail_token_box(ui: &mut egui::Ui, token: &str) {
    let color = detail_token_color(token);
    // 手动画方块而不用 Frame：Frame 通过 allocate_rect 占位，会绕过
    // horizontal_wrapped 的换行判断，导致内容一多就冲出列宽。
    let galley = ui.painter().layout_no_wrap(
        token.to_string(),
        FontId::proportional(11.5),
        color,
    );
    // 与 detail_part_width 保持一致：文字 + 左右内边距 6 + 描边 2。
    let size = galley.size() + Vec2::new(8.0, 2.0);
    let (rect, _resp) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(3),
        Color32::from_rgb(0xF2, 0xF6, 0xFC),
        Stroke::new(1.0, Color32::from_rgb(0xC9, 0xD6, 0xE6)),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + Vec2::new(4.0, 1.0), galley, color);
}

/// 明细「玩法」列的文字色：与中奖明细的玩法徽章用同一套颜色，未知玩法用默认文字色。
fn detail_play_color(play: &str) -> Color32 {
    let (display, fg, _) = crate::win_style::badge(play);
    if display.is_empty() {
        return TEXT;
    }
    Color32::from_rgb(fg[0], fg[1], fg[2])
}

fn detail_token_color(token: &str) -> Color32 {
    if let Ok(n) = token.parse::<u32>() {
        if (1..=49).contains(&n) {
            return settle::wave_of(n).map(wave_color).unwrap_or(TEXT);
        }
    }
    zodiac_color(token).unwrap_or(TEXT)
}

fn draw_inputs_from(draws: &draw::Draws) -> [[String; 7]; 3] {
    let mut out: [[String; 7]; 3] = Default::default();
    for (i, r) in [&draws.macau, &draws.hongkong, &draws.old_macau]
        .iter()
        .enumerate()
    {
        for (j, s) in out[i].iter_mut().enumerate().take(6) {
            if let Some(n) = r.draw.get(j) {
                *s = n.to_string();
            }
        }
        if let Some(sp) = r.special {
            out[i][6] = sp.to_string();
        }
    }
    out
}

/// 一个地区输入行的实时状态：已经填了几个、第一个错误是什么。
struct DrawRowCheck {
    filled: usize,
    error: Option<String>,
}

fn unique_in_draw_row(row: &[String; 7], index: usize, value: u32) -> bool {
    row.iter().enumerate().all(|(k, s)| {
        k == index || s.trim().parse::<u32>().ok() != Some(value)
    })
}

fn check_draw_row(row: &[String; 7]) -> DrawRowCheck {
    let filled = row.iter().filter(|s| !s.trim().is_empty()).count();
    if filled == 0 {
        return DrawRowCheck {
            filled,
            error: None,
        };
    }
    let mut nums: Vec<u32> = Vec::new();
    for (j, s) in row.iter().enumerate() {
        let s = s.trim();
        if s.is_empty() {
            continue;
        }
        let pos = if j == 6 {
            "特码".to_string()
        } else {
            format!("第 {} 个", j + 1)
        };
        let Ok(n) = s.parse::<u32>() else {
            return DrawRowCheck {
                filled,
                error: Some(format!("{pos}「{s}」不是号码")),
            };
        };
        if !(1..=49).contains(&n) {
            return DrawRowCheck {
                filled,
                error: Some(format!("{pos}「{n}」超出 1-49")),
            };
        }
        if nums.contains(&n) {
            return DrawRowCheck {
                filled,
                error: Some(format!("号码 {n:02} 重复了")),
            };
        }
        nums.push(n);
    }
    if filled < 7 {
        return DrawRowCheck {
            filled,
            error: Some(format!("还差 {} 个号码", 7 - filled)),
        };
    }
    DrawRowCheck {
        filled,
        error: None,
    }
}

/// 整期粘贴：支持「01 02 03 04 05 06 07」「1,2,3,4,5,6,7」
/// 「01020304050607」或「1234567」。
fn parse_draw_paste(text: &str) -> Result<[String; 7], String> {
    let tokens: Vec<&str> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    let mut nums: Vec<u32> = Vec::new();
    if tokens.len() == 1 {
        let digits = tokens[0];
        if digits.len() == 7 {
            for c in digits.chars() {
                nums.push(c.to_digit(10).unwrap_or(0));
            }
        } else if digits.len() == 14 {
            for k in (0..14).step_by(2) {
                nums.push(digits[k..k + 2].parse().unwrap_or(0));
            }
        } else {
            return Err("请粘贴 7 个号码，如：01 02 03 04 05 06 07".to_string());
        }
    } else {
        for token in &tokens {
            let n = token
                .parse::<u32>()
                .map_err(|_| format!("「{token}」不是号码"))?;
            nums.push(n);
        }
    }
    if nums.len() != 7 {
        return Err(format!("需要 7 个号码，当前识别到 {} 个", nums.len()));
    }
    for (j, n) in nums.iter().enumerate() {
        if !(1..=49).contains(n) {
            return Err(format!("第 {} 个号码 {n} 超出 1-49", j + 1));
        }
        if nums[..j].contains(n) {
            return Err(format!("号码 {n:02} 重复了"));
        }
    }
    Ok(std::array::from_fn(|i| nums[i].to_string()))
}

fn parse_draw_inputs(inputs: &[[String; 7]; 3]) -> Result<draw::Draws, String> {
    let names = ["澳门", "香港", "老澳门"];
    let mut draws = draw::Draws::default();
    for (i, row) in inputs.iter().enumerate() {
        if row.iter().all(|s| s.trim().is_empty()) {
            continue;
        }
        let mut nums: Vec<u32> = Vec::new();
        for (j, s) in row.iter().enumerate() {
            let s = s.trim();
            let pos = if j == 6 {
                "特码".to_string()
            } else {
                format!("第 {} 个", j + 1)
            };
            if s.is_empty() {
                return Err(format!("{}：{}没有填", names[i], pos));
            }
            let n: u32 = s
                .parse()
                .map_err(|_| format!("{}：{}「{}」不是号码", names[i], pos, s))?;
            if !(1..=49).contains(&n) {
                return Err(format!("{}：{}「{}」超出 1-49", names[i], pos, n));
            }
            if nums.contains(&n) {
                return Err(format!("{}：号码 {} 重复", names[i], n));
            }
            nums.push(n);
        }
        let region = draw::RegionDraw {
            draw: nums[..6].to_vec(),
            special: Some(nums[6]),
        };
        match i {
            0 => draws.macau = region,
            1 => draws.hongkong = region,
            _ => draws.old_macau = region,
        }
    }
    Ok(draws)
}

/// 号码球的波色底：红波 / 蓝波 / 绿波，查不到用灰色。
fn wave_ball_color(n: u32) -> Color32 {
    crate::settle::wave_of(n).map(wave_color).unwrap_or(MUTED)
}

/// 一个波色号码球（开奖展示与号码盘共用）。`special` 时加一圈白描边。
/// 返回球占用的矩形，调用方可以据此在正下方画生肖。
fn draw_number_ball(ui: &mut egui::Ui, n: u32, d: f32, special: bool) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(d), Sense::hover());
    let p = ui.painter();
    p.circle_filled(rect.center(), d / 2.0, wave_ball_color(n));
    if special {
        p.circle_stroke(
            rect.center(),
            d / 2.0 - 1.0,
            Stroke::new(1.5, Color32::WHITE),
        );
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{n:02}"),
        bold_font(d * 0.52),
        Color32::WHITE,
    );
    rect
}

/// 开奖号码的富样式展示：地区 + 每个号码一个波色圆圈，特码加白圈。
/// `prefix` 为空时不显示前缀；没有录入时显示「尚未录入开奖」。
fn draw_summary_ui(ui: &mut egui::Ui, draws: &draw::Draws, prefix: &str) {
    // 一个地区都没录入时不显示前缀，只留「尚未录入开奖」，避免出现
    // 「开奖：尚未录入开奖」。
    let any_ready = draws.macau.ready() || draws.hongkong.ready() || draws.old_macau.ready();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        if any_ready && !prefix.is_empty() {
            ui.label(RichText::new(prefix).size(10.5).color(SUB));
        }
        let mut any = false;
        for (i, (name, r)) in [
            ("澳", &draws.macau),
            ("港", &draws.hongkong),
            ("老", &draws.old_macau),
        ]
        .iter()
        .enumerate()
        {
            if !r.ready() {
                continue;
            }
            if any {
                ui.add_space(6.0);
            }
            any = true;
            // 每个彩种一个带边框的圆角框，一眼就能分开澳 / 港 / 老。
            egui::Frame::new()
                .stroke(Stroke::new(1.0, region_color(i)))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(6, 1))
                .show(ui, |ui| {
                    // 外层可能是横向布局（风险页是「页签 + 开奖」一行）。
                    // Frame 的内容 UI 会继承外层的横向布局，不强制竖排的话，
                    // 下面的生肖行会被排到号码球右边，把彩种框撑宽、三区被裁掉。
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // 第一行画号码球，同时记下每个球的圆心；第二行直接用这些
                        // 圆心画生肖。两行不再各自排版，间距 / 四舍五入都不会错位。
                        let mut balls: Vec<(f32, u32)> = Vec::new();
                        let row = ui
                            .horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                ui.label(
                                    RichText::new(*name)
                                        .size(9.5)
                                        .strong()
                                        .color(region_color(i)),
                                );
                                for &n in &r.draw {
                                    let rect = draw_number_ball(ui, n, 17.0, false);
                                    balls.push((rect.center().x, n));
                                }
                                ui.label(
                                    RichText::new("特").size(9.0).strong().color(SPECIAL_RED),
                                );
                                if let Some(s) = r.special {
                                    let rect = draw_number_ball(ui, s, 17.0, true);
                                    balls.push((rect.center().x, s));
                                }
                            })
                            .response
                            .rect;
                        let (strip, _) = ui
                            .allocate_exact_size(Vec2::new(row.width(), 12.0), Sense::hover());
                        for &(cx, n) in &balls {
                            let zodiac = crate::settle::zodiac_of(n).unwrap_or("");
                            if zodiac.is_empty() {
                                continue;
                            }
                            // 每个生肖标签直接摆在对应球心的正下方。
                            let cell = egui::Rect::from_center_size(
                                egui::pos2(cx, strip.center().y),
                                Vec2::new(17.0, 12.0),
                            );
                            ui.place(
                                cell,
                                egui::Label::new(
                                    RichText::new(zodiac).size(9.0).color(MUTED),
                                )
                                .selectable(false),
                            );
                        }
                    });
                });
        }
        if !any {
            ui.label(RichText::new("尚未录入开奖").size(10.5).color(MUTED));
        }
    });
}

/// 开奖富样式在一行里的宽度（用于右侧对齐：放进 right_to_left 布局会被整体
/// 反序，所以改为先留白再正常从左往右画）。
fn draw_summary_width(p: &egui::Painter, draws: &draw::Draws, d: f32) -> f32 {
    let mut w = 0.0f32;
    let mut regions = 0;
    for (name, r) in [
        ("澳", &draws.macau),
        ("港", &draws.hongkong),
        ("老", &draws.old_macau),
    ] {
        if !r.ready() {
            continue;
        }
        if regions > 0 {
            // 彩种组之间的间距。
            w += 6.0 + 3.0;
        }
        regions += 1;
        let mut items = 1; // 地区字
        w += text_width(p, name, &bold_font(9.5));
        w += r.draw.len() as f32 * d;
        items += r.draw.len();
        w += text_width(p, "特", &bold_font(9.0));
        items += 1;
        if r.special.is_some() {
            w += d;
            items += 1;
        }
        w += (items - 1) as f32 * 3.0;
        // 彩种框的左右内边距与边框（各 6px 内边距 + 1px 描边）。
        w += 12.0 + 2.0;
    }
    if regions == 0 {
        return text_width(p, "尚未录入开奖", &FontId::proportional(10.5));
    }
    w
}

/// 号码盘上的一个球：点一下返回 true（由调用方填进输入框）。
/// 已经录进本区的号码变淡、点不动；悬停显示生肖。
fn pick_ball(ui: &mut egui::Ui, n: u32, used: bool, d: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let p = ui.painter();
    let base = wave_ball_color(n);
    let fill = if used {
        Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 70)
    } else {
        base
    };
    p.circle_filled(rect.center(), d / 2.0, fill);
    if !used && resp.hovered() {
        p.circle_stroke(
            rect.center(),
            d / 2.0 - 0.5,
            Stroke::new(1.5, Color32::WHITE),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        format!("{n:02}"),
        bold_font(d * 0.52),
        if used {
            Color32::from_white_alpha(150)
        } else {
            Color32::WHITE
        },
    );
    if let Some(z) = crate::settle::zodiac_of(n) {
        resp.clone().on_hover_text(format!("{n:02} {z}"));
    }
    resp.clicked() && !used
}

fn fmt_money(v: f64) -> String {
    convert::fmt_amount(v)
}

struct BillTableRow {
    no: usize,
    /// 来源注单在 `self.orders` 里的下标（序号格点击跳转用）。
    idx: usize,
    amount: f64,
    hk: f64,
    old: f64,
    win: f64,
    breakdown: Vec<(String, f64, f64)>,
}

fn bill_category(play: &str) -> &'static str {
    match play {
        "三中三" | "二中二" | "三中二" | "特碰" | "二全中" => "小码",
        "特码" => "特码",
        "六肖中一" => "六肖中一",
        "平特" => "平特",
        "平码" => "平码",
        "三中一" => "三中一",
        "四中一" => "四中一",
        "五中一" => "五中一",
        _ if play.ends_with("连肖") => "连肖",
        _ if play.ends_with("连尾") => "连尾",
        _ if play.ends_with("不中") => "不中",
        _ => "其他",
    }
}

fn category_order(cat: &str) -> usize {
    [
        "小码",
        "特码",
        "连肖",
        "六肖中一",
        "连尾",
        "平特",
        "平码",
        "不中",
        "三中一",
        "四中一",
        "五中一",
        "其他",
    ]
    .iter()
    .position(|c| *c == cat)
    .unwrap_or(99)
}

fn play_order(label: &str) -> usize {
    [
        "三中三",
        "二中二",
        "三中二",
        "特碰",
        "特码",
        "二连肖",
        "三连肖",
        "四连肖",
        "五连肖",
        "平特",
        "平码",
        "五不中",
        "六不中",
        "七不中",
        "八不中",
        "九不中",
        "十不中",
        "三中一",
        "四中一",
        "五中一",
        "六肖中一",
        "二连尾",
        "三连尾",
        "四连尾",
        "二全中",
    ]
    .iter()
    .position(|c| *c == label)
    .unwrap_or(999)
}

/// 中奖明细按固定玩法顺序排列（与参考页一致）。
fn sorted_breakdown(items: &[(String, f64, f64)]) -> Vec<(String, f64, f64)> {
    let mut sorted = items.to_vec();
    sorted.sort_by_key(|(label, _, _)| play_order(label));
    sorted
}

fn export_dir() -> std::path::PathBuf {
    let base = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let pictures = base.join("Pictures");
    if pictures.is_dir() {
        pictures
    } else {
        std::env::temp_dir()
    }
}

fn reconcile_export_path() -> std::path::PathBuf {
    export_dir().join(format!(
        "对账-{}.txt",
        Local::now().format("%Y%m%d-%H%M%S")
    ))
}

fn reconcile_bettor_card(ui: &mut egui::Ui, b: &reconcile::BettorDiff) {
    use reconcile::Status;
    let (label, color, bg) = match b.status {
        Status::Same => ("一致", MONEY_GREEN, BAND_AMOUNT_BG),
        Status::Diff => ("不一致", MONEY_RED, RED_BG),
        Status::LocalOnly => ("仅本机", ORANGE, ORANGE_BG),
        Status::RemoteOnly => ("仅对方", ORANGE, ORANGE_BG),
    };
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&b.name)
                        .size(13.5)
                        .strong()
                        .color(TEXT),
                );
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(label)
                                .size(10.5)
                                .strong()
                                .color(color),
                        );
                    });
            });
            if b.status == Status::Same {
                ui.label(
                    RichText::new("五个总额、码与逐批明细全部一致。")
                        .size(11.0)
                        .color(MUTED),
                );
                return;
            }
            ui.add_space(4.0);
            egui::Grid::new(format!("rc_grid_{}", b.name))
                .num_columns(3)
                .spacing([18.0, 3.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("").size(11.0));
                    ui.label(RichText::new("本机").size(11.0).strong().color(BILL_TEXT));
                    ui.label(RichText::new("对方").size(11.0).strong().color(BILL_TEXT));
                    ui.end_row();
                    for row in &b.summary {
                        let c = if row.equal { MUTED } else { MONEY_RED };
                        ui.label(
                            RichText::new(row.label)
                                .size(11.0)
                                .strong()
                                .color(BILL_TEXT),
                        );
                        ui.label(RichText::new(&row.local).size(11.0).color(c));
                        ui.label(RichText::new(&row.remote).size(11.0).color(c));
                        ui.end_row();
                    }
                });
            if !b.batches.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("逐批差异")
                        .size(11.0)
                        .strong()
                        .color(BILL_TEXT),
                );
                reconcile_batch_table(ui, &format!("rc_batch_{}", b.name), &b.batches);
            }
            match b.status {
                Status::LocalOnly => {
                    ui.label(
                        RichText::new("本机有、对方没有这位投注人。")
                            .size(10.5)
                            .color(ORANGE),
                    );
                }
                Status::RemoteOnly => {
                    ui.label(
                        RichText::new("对方有、本机没有这位投注人。")
                            .size(10.5)
                            .color(ORANGE),
                    );
                }
                _ => {}
            }
        });
}

/// 「逐批差异」带线表格：外框 + 列线 + 行线，比裸 Grid 更容易对齐阅读。
fn reconcile_batch_table(ui: &mut egui::Ui, id: &str, rows: &[reconcile::BatchRow]) {
    let line = Stroke::new(1.0, BORDER);
    // 单元格内边距；每行高度由文字 + 上下内边距决定。
    let pad = Margin::symmetric(6, 3);
    let mut rects: Vec<egui::Rect> = Vec::new();
    let grid = egui::Grid::new(id)
        .num_columns(3)
        .spacing([0.0, 0.0])
        .show(ui, |ui| {
            let mut cell = |ui: &mut egui::Ui, text: &str, color: Color32| {
                let resp = egui::Frame::new()
                    .inner_margin(pad)
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).size(10.5).color(color));
                    })
                    .response;
                rects.push(resp.rect);
            };
            cell(ui, "#", MUTED);
            cell(ui, "本机", MUTED);
            cell(ui, "对方", MUTED);
            ui.end_row();
            for row in rows {
                cell(ui, &row.no.to_string(), CELL_NO);
                cell(ui, &row.local, BILL_TEXT);
                cell(ui, &row.remote, BILL_TEXT);
                ui.end_row();
            }
        });
    let rect = grid.response.rect;
    let painter = ui.painter();
    painter.rect_stroke(rect, CornerRadius::ZERO, line, egui::StrokeKind::Inside);
    if rects.len() >= 3 {
        // 单元格不填满列宽，列线取下一列的起始 x（Grid 间距为 0）。
        for col in 1..3 {
            painter.vline(rects[col].min.x, rect.y_range(), line);
        }
        // 行线取每一行第一个单元格的顶边。
        for row_start in (3..rects.len()).step_by(3) {
            painter.hline(rect.x_range(), rects[row_start].min.y, line);
        }
    }
}

#[derive(Default, Clone, Copy)]
struct ReferenceCardAction {
    locate: bool,
    delete: bool,
}

/// 画一条引用卡片，返回用户点了「定位」还是「删除被引用消息」。
fn reference_card(ui: &mut egui::Ui, row: &MsgRow) -> ReferenceCardAction {
    let mut locate = false;
    let mut delete = false;
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&row.time).size(10.5).color(SUB));
                if !row.nick.is_empty() {
                    ui.label(
                        RichText::new(&row.nick)
                            .size(11.5)
                            .strong()
                            .color(TEXT),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let recall = row.kind == "撤回";
                    ui.label(
                        RichText::new(if recall { "撤回" } else { "引用" })
                            .size(10.0)
                            .strong()
                            .color(if recall {
                                ORANGE
                            } else {
                                Color32::from_rgb(0x6A, 0x1B, 0x9A)
                            })
                            .background_color(if recall {
                                ORANGE_BG
                            } else {
                                Color32::from_rgb(0xF3, 0xE8, 0xFF)
                            }),
                    );
                });
            });
            ui.add_space(2.0);
            if !row.content.is_empty() {
                // 引用消息本身的原文用红色，便于和元信息 / 被引用块区分。
                ui.label(RichText::new(&row.content).size(12.5).color(RED));
            }

            if let Some(q) = &row.quote {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(SUBTOTAL_BG)
                    .stroke(Stroke::new(1.0, BILL_LINE))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("被引用")
                                    .size(9.5)
                                    .strong()
                                    .color(Color32::from_rgb(0x6A, 0x1B, 0x9A)),
                            );
                            if !q.from.is_empty() {
                                ui.label(
                                    RichText::new(&q.from).size(10.5).color(SUB),
                                );
                            }
                            if q.create_time > 0 {
                                if let Some(t) = chrono::Local
                                    .timestamp_opt(q.create_time, 0)
                                    .single()
                                {
                                    ui.label(
                                        RichText::new(
                                            t.format("%Y-%m-%d %H:%M").to_string(),
                                        )
                                        .size(10.0)
                                        .color(MUTED),
                                    );
                                }
                            }
                        });
                        ui.label(
                            RichText::new(if q.content.is_empty() {
                                "（无文字内容）"
                            } else {
                                q.content.as_str()
                            })
                            .size(11.5)
                            .color(TEXT),
                        );
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if styled_button(
                        ui,
                        RichText::new("定位被引用消息")
                            .size(11.5)
                            .strong()
                            .color(GREEN_DEEP),
                        GREEN_TINT,
                        Color32::from_rgb(0xD8, 0xEF, 0xE2),
                        Color32::from_rgb(0xCB, 0xEA, 0xD9),
                        Stroke::new(1.0, Color32::from_rgb(0xB7, 0xE4, 0xCB)),
                        Stroke::new(1.0, GREEN),
                        CornerRadius::same(6),
                        Vec2::new(0.0, 24.0),
                    )
                    .clicked()
                    {
                        locate = true;
                    }
                    if styled_button(
                        ui,
                        RichText::new("删除被引用消息")
                            .size(11.5)
                            .strong()
                            .color(RED),
                        RED_BG,
                        RED_HOVER,
                        Color32::from_rgb(0xFF, 0xD5, 0xCF),
                        Stroke::new(1.0, RED),
                        Stroke::new(1.0, RED),
                        CornerRadius::same(6),
                        Vec2::new(0.0, 24.0),
                    )
                    .on_hover_text("从注单消息列表删除被引用的那条消息（重新导入会恢复）")
                    .clicked()
                    {
                        delete = true;
                    }
                });
            } else if let Some(url) = &row.link_url {
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(truncate_display(url, 64))
                            .font(FontId::monospace(10.5))
                            .color(MUTED),
                    );
                    if ui.link("打开").on_hover_text(url.as_str()).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                    if ui.add(tiny_button("复制链接", SUB)).clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                });
            }

        });
    ReferenceCardAction { locate, delete }
}

fn count_text_pill(ui: &mut egui::Ui, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(11.0), MUTED);
    let pad = Vec2::new(7.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), BILL_HEAD);
    ui.painter().galley(rect.min + pad, galley, MUTED);
}

fn view_tab(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let text = RichText::new(label)
        .font(if active {
            bold_font(12.5)
        } else {
            FontId::proportional(12.5)
        })
        .color(if active { Color32::WHITE } else { MUTED });
    styled_button(
        ui,
        text,
        if active { MONEY_BLUE } else { CARD },
        if active { BLUE_SOLID_HOVER } else { BTN_HOVER_BG },
        if active { BLUE_SOLID_ACTIVE } else { BTN_ACTIVE_BG },
        Stroke::NONE,
        Stroke::NONE,
        CornerRadius::ZERO,
        Vec2::new(52.0, 26.0),
    )
    .clicked()
}

// ---------------- 风险面板小组件 ----------------

fn risk_tab_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    styled_button(
        ui,
        RichText::new(label).size(11.5).color(if active { Color32::WHITE } else { MUTED }),
        if active { MONEY_BLUE } else { Color32::TRANSPARENT },
        if active { BLUE_SOLID_HOVER } else { BTN_HOVER_BG },
        if active { BLUE_SOLID_ACTIVE } else { BTN_ACTIVE_BG },
        Stroke::NONE,
        Stroke::new(1.0, BTN_HOVER_BORDER),
        CornerRadius::same(5),
        Vec2::new(50.0, 22.0),
    )
    .clicked()
}

fn risk_region_tab_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    styled_button(
        ui,
        RichText::new(label)
            .size(12.0)
            .color(if active { Color32::WHITE } else { MUTED }),
        if active { REF_NAVY } else { REF_HEAD_TINT },
        if active {
            Color32::from_rgb(0x1E, 0x2B, 0x40)
        } else {
            HOVER
        },
        if active { REF_NAVY } else { HOVER },
        Stroke::new(1.0, if active { REF_NAVY } else { REF_LINE }),
        Stroke::new(1.0, BTN_HOVER_BORDER),
        CornerRadius::same(6),
        Vec2::new(0.0, 24.0),
    )
    .clicked()
}

/// 小码减额弹窗的地区名（与 `risk::REGION_CHARS` 下标一致）
const XM_REGION_NAMES: [&str; 3] = ["澳门", "香港", "老澳门"];

/// 整数不带小数，否则保留两位。
fn xm_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{:.0}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// 小码减额弹窗的外壳：遮罩 + 居中窗口 + 标题行（右上角关闭）。
/// 点遮罩或「关闭」时返回 true。与「概率」弹窗同一套样式。
fn xm_modal(
    ctx: &egui::Context,
    id: &'static str,
    title: &str,
    subtitle: &str,
    width: f32,
    // 固定整个弹窗的高度（像素）；None = 按内容自动撑开。
    fixed_height: Option<f32>,
    // 未固定高度时的正文限高比例。
    body_ratio: f32,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut close = false;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique(id))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let resp = ui.allocate_rect(
                egui::Rect::from_min_size(screen.min, screen.size()),
                Sense::click(),
            );
            ui.painter()
                .rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(70));
            if resp.clicked() {
                close = true;
            }
        });
    let mut window = egui::Window::new(id)
        .title_bar(false)
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .default_width(width);
    if let Some(h) = fixed_height {
        window = window.fixed_size([width, h]);
    }
    window
        .show(ctx, |ui| {
            egui::Frame::new()
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(width - 28.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(title).size(16.0).strong().color(TEXT));
                            ui.label(RichText::new(subtitle).size(11.5).color(MUTED));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if secondary_small_button(ui, "关闭").clicked() {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(8.0);
                    hairline(ui);
                    ui.add_space(8.0);
                    // 内容可能很长（组合一多就有几百行）：正文限高、超出就在窗口里滚动，
                    // 不让窗口把整个界面撑出屏幕。
                    // 固定高度的弹窗里，正文用剩下的全部高度；否则按屏幕比例限高。
                    let max_h = if fixed_height.is_some() {
                        ui.available_height()
                    } else {
                        ui.ctx().content_rect().height() * body_ratio
                    };
                    egui::ScrollArea::vertical()
                        .max_height(max_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| body(ui));
                });
        });
    close
}

fn secondary_small_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    styled_button(
        ui,
        RichText::new(label).size(11.5).color(SUB),
        CARD,
        BTN_HOVER_BG,
        BTN_ACTIVE_BG,
        Stroke::new(1.0, BORDER),
        Stroke::new(1.0, BTN_HOVER_BORDER),
        CornerRadius::same(8),
        Vec2::new(56.0, 24.0),
    )
}

fn lian_size_label(n: usize) -> &'static str {
    match n {
        2 => "二连肖",
        3 => "三连肖",
        4 => "四连肖",
        _ => "五连肖",
    }
}

fn wave_color(name: &str) -> Color32 {
    match name {
        "红波" => Color32::from_rgb(0xDC, 0x26, 0x26),
        "蓝波" => Color32::from_rgb(0x25, 0x63, 0xEB),
        _ => Color32::from_rgb(0x16, 0xA3, 0x4A),
    }
}

/// 地区色与参考项目同一套：澳蓝 / 港玫红 / 老橙。
fn region_color(region: usize) -> Color32 {
    match region {
        0 => Color32::from_rgb(0x25, 0x63, 0xEB),
        1 => Color32::from_rgb(0xE1, 0x1D, 0x48),
        _ => Color32::from_rgb(0xD9, 0x77, 0x06),
    }
}

/// 小码「类型」色块的（底色, 文字色），四档与参考项目一致。
fn type_tag_colors(label: &str) -> (Color32, Color32) {
    match label {
        "三中三" => (
            Color32::from_rgb(0xDB, 0xEA, 0xFE),
            Color32::from_rgb(0x1D, 0x4E, 0xD8),
        ),
        "特碰" => (
            Color32::from_rgb(0xFF, 0xE4, 0xE6),
            Color32::from_rgb(0xB9, 0x1C, 0x1C),
        ),
        "三中二" => (
            Color32::from_rgb(0xED, 0xE9, 0xFE),
            Color32::from_rgb(0x7C, 0x3A, 0xED),
        ),
        _ => (
            Color32::from_rgb(0xD1, 0xFA, 0xE5),
            Color32::from_rgb(0x04, 0x78, 0x57),
        ),
    }
}

/// 平特表里生肖行按参考项目的生肖配色上色；尾数 / 号码组不上色。
fn zodiac_color(name: &str) -> Option<Color32> {
    let c = match name {
        "鼠" => Color32::from_rgb(0x4F, 0x46, 0xE5),
        "牛" => Color32::from_rgb(0xB4, 0x53, 0x09),
        "虎" => Color32::from_rgb(0xEA, 0x58, 0x0C),
        "兔" => Color32::from_rgb(0xDB, 0x27, 0x77),
        "龙" => Color32::from_rgb(0x05, 0x96, 0x69),
        "蛇" => Color32::from_rgb(0x15, 0x80, 0x3D),
        "马" => Color32::from_rgb(0xE1, 0x1D, 0x48),
        "羊" => Color32::from_rgb(0x4D, 0x7C, 0x0F),
        "猴" => Color32::from_rgb(0x7C, 0x3A, 0xED),
        "鸡" => Color32::from_rgb(0xDC, 0x26, 0x26),
        "狗" => Color32::from_rgb(0x02, 0x84, 0xC7),
        "猪" => Color32::from_rgb(0xC0, 0x26, 0xD3),
        _ => return None,
    };
    Some(c)
}

fn profit_color(v: f64) -> Color32 {
    if v < -0.005 {
        MONEY_RED
    } else if v > 0.005 {
        MONEY_GREEN
    } else {
        MUTED
    }
}

/// 特码风险列的正负色：负（赔出去）红且加粗，正（收进来）绿。
fn risk_value_color(v: f64) -> (Color32, bool) {
    if v < -0.005 {
        (REF_RISK_RED, true)
    } else if v > 0.005 {
        (REF_RISK_GREEN, false)
    } else {
        (REF_NAVY, false)
    }
}

fn format_probability(p: f64) -> String {
    if p <= 0.0 {
        return "0%".to_string();
    }
    let pct = p * 100.0;
    if pct < 0.005 {
        "<0.01%".to_string()
    } else {
        format!("{pct:.2}%")
    }
}

fn risk_num_label(ui: &mut egui::Ui, text: &str, color: Color32, strong: bool) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let mut text = RichText::new(text).size(12.0).color(color);
        if strong {
            text = text.strong();
        }
        ui.label(text);
    });
}

/// 投注表一栏的行体。空栏（只有连肖会遇到）画一行居中的空态。
fn risk_column_body(
    ui: &mut egui::Ui,
    widths: &[f32],
    list: crate::risk::PanelList,
    rows: &[crate::risk::PanelRow],
    row_h: f32,
    with_risk: bool,
) {
    let total: f32 = widths.iter().sum();
    ui.set_min_width(total);
    // 每一行是一个 allocate_exact_size，行高必须就是 row_h：默认的纵向 item_spacing
    // 会把 14px 的行撑成 21px，49 行当场溢出。
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    if rows.is_empty() {
        let empty = match list {
            crate::risk::PanelList::LianXiao => "该区没有连肖记录",
            crate::risk::PanelList::Special => "该区没有特码记录",
            crate::risk::PanelList::FlatSpecial => "该区没有平特记录",
            crate::risk::PanelList::FlatNumber => "该区没有平码记录",
        };
        let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 64.0), Sense::hover());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            empty,
            FontId::proportional(12.0),
            MUTED,
        );
        return;
    }
    for (i, row) in rows.iter().enumerate() {
        let risk_text = with_risk.then(|| fmt_money(row.risk));
        risk_table_row(
            ui,
            widths,
            list,
            row,
            i + 1,
            risk_text.as_deref(),
            risk_first_color(row),
            row_h,
        );
    }
}

fn risk_first_color(row: &crate::risk::PanelRow) -> Color32 {
    if let Some(n) = row.num {
        return settle::wave_of(n).map(wave_color).unwrap_or(TEXT);
    }
    zodiac_color(&row.key).unwrap_or(TEXT)
}

/// 吃码输入：非数 / 负数当 0，超过该号当前金额夹到金额（与参考项目一致）。
fn code_report_reduce_of(map: &HashMap<u32, String>, num: u32, stake: f64) -> f64 {
    let raw = map
        .get(&num)
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    if raw <= 0.0 {
        0.0
    } else {
        raw.min(stake.max(0.0))
    }
}

/// 上报正文（不含首行「地区 特码」）：按金额降序分组，组内号码升序；
/// 完整生肖合并成生肖名（如 `马蛇兔.8.1各91`）。
fn code_report_detail(reduce: &[(u32, f64)]) -> String {
    let mut groups: BTreeMap<i64, Vec<u32>> = BTreeMap::new();
    for &(num, amount) in reduce {
        let cents = (amount * 100.0).round() as i64;
        groups.entry(cents).or_default().push(num);
    }
    groups
        .iter()
        .rev()
        .map(|(cents, nums)| {
            format_code_report_group(nums, *cents as f64 / 100.0)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 完整上报文本：首行 `澳 特码`，下面是正文。
fn code_report_text(region: usize, reduce: &[(u32, f64)]) -> String {
    let detail = code_report_detail(reduce);
    if detail.is_empty() {
        String::new()
    } else {
        format!("{} 特码\n{}", crate::risk::REGION_CHARS[region], detail)
    }
}

fn format_code_report_group(nums: &[u32], amount: f64) -> String {
    let mut nums: Vec<u32> = nums.to_vec();
    nums.sort_unstable();
    let set: BTreeSet<u32> = nums.iter().copied().collect();
    let mut consumed: BTreeSet<u32> = BTreeSet::new();
    let mut zodiacs = String::new();
    for name in crate::settle::zodiac_names() {
        let Some(z_nums) = crate::settle::zodiac_numbers(name) else {
            continue;
        };
        if !z_nums.is_empty() && z_nums.iter().all(|n| set.contains(n)) {
            zodiacs.push_str(name);
            consumed.extend(z_nums);
        }
    }
    let remaining: Vec<String> = nums
        .iter()
        .filter(|n| !consumed.contains(n))
        .map(|n| n.to_string())
        .collect();
    let mut out = zodiacs;
    if !remaining.is_empty() {
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&remaining.join("."));
    }
    format!("{}各{}", out, crate::convert::fmt_amount(amount))
}

fn risk_col_widths(total: f32, list: crate::risk::PanelList) -> Vec<f32> {
    let fractions: &[f32] = match list {
        crate::risk::PanelList::Special => &[0.19, 0.33, 0.34, 0.14],
        crate::risk::PanelList::LianXiao => &[0.45, 0.35, 0.20],
        crate::risk::PanelList::FlatSpecial => &[0.40, 0.40, 0.20],
        crate::risk::PanelList::FlatNumber => &[0.25, 0.55, 0.20],
    };
    fractions.iter().map(|f| total * f).collect()
}

/// 投注表表头：第 3 列（风险）可点。返回是否点了风险列。
fn risk_header_row(
    ui: &mut egui::Ui,
    widths: &[f32],
    headings: &[&str],
    with_risk: bool,
    region: usize,
    desc: bool,
) -> bool {
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 26.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, REF_HEAD_BG);
    ui.painter()
        .hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, REF_LINE));
    let mut x = rect.min.x;
    let mut clicked = false;
    for (i, heading) in headings.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            Vec2::new(widths[i], rect.height()),
        );
        let right = i > 0;
        let mut text = (*heading).to_string();
        if with_risk && i == 2 {
            text.push_str(if desc { " ▼" } else { " ▲" });
        }
        let pos = if right {
            egui::pos2(cell.max.x - 8.0, cell.center().y)
        } else {
            egui::pos2(cell.min.x + 8.0, cell.center().y)
        };
        ui.painter().text(
            pos,
            if right {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            text,
            FontId::proportional(12.0),
            REF_NAVY,
        );
        if with_risk && i == 2 {
            let resp = ui.interact(cell, ui.scope_id().with(("risk_hdr", region)), Sense::click());
            if resp.clicked() {
                clicked = true;
            }
            let _ = resp.on_hover_text("点击切换风险排序方向");
        }
        x += widths[i];
    }
    clicked
}

#[allow(clippy::too_many_arguments)]
fn risk_table_row(
    ui: &mut egui::Ui,
    widths: &[f32],
    list: crate::risk::PanelList,
    row: &crate::risk::PanelRow,
    id: usize,
    risk_text: Option<&str>,
    first_color: Color32,
    row_h: f32,
) {
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, row_h), Sense::hover());
    if row.hit {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, REF_HIT_BG);
    }
    ui.painter()
        .hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, REF_LINE));

    // (文案, 颜色, 加粗, 右对齐)
    let amount_color = if row.stake.abs() < 0.005 {
        REF_ZERO
    } else {
        REF_NAVY
    };
    let mut cells: Vec<(String, Color32, bool, bool)> = vec![
        (row.label.clone(), first_color, false, false),
        (fmt_money(row.stake), amount_color, false, true),
    ];
    if list == crate::risk::PanelList::Special {
        let (color, bold) = risk_value_color(row.risk);
        cells.push((
            risk_text.unwrap_or_default().to_string(),
            color,
            bold,
            true,
        ));
    }
    cells.push((id.to_string(), REF_ID, false, true));

    let mut x = rect.min.x;
    for (i, (text, color, bold, right)) in cells.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            Vec2::new(widths[i], rect.height()),
        );
        let pos = if *right {
            egui::pos2(cell.max.x - 8.0, cell.center().y)
        } else {
            egui::pos2(cell.min.x + 8.0, cell.center().y)
        };
        let font = if *bold {
            bold_font(12.0)
        } else {
            FontId::proportional(12.0)
        };
        ui.painter().text(
            pos,
            if *right {
                Align2::RIGHT_CENTER
            } else {
                Align2::LEFT_CENTER
            },
            text,
            font,
            *color,
        );
        x += widths[i];
    }
}

fn risk_stat_card(ui: &mut egui::Ui, label: &str, value: &str, color: Color32, width: f32) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(20.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.0).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(value).size(16.0).strong().color(color));
                });
            });
        });
}

/// 授权弹窗的信息卡：标签在上、值在下。到期时间可能很长（「365天 12小时 35分钟」），
/// 单行右对齐会顶到标签上并溢出卡片，所以这里给值单独一行，超出宽度就省略。
fn auth_stat_card(ui: &mut egui::Ui, label: &str, value: &str, color: Color32, width: f32) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(20.0));
            ui.label(RichText::new(label).size(11.0).color(MUTED));
            ui.add(
                egui::Label::new(RichText::new(value).size(15.0).strong().color(color))
                    .truncate()
                    .selectable(false),
            );
        });
}

fn risk_prob_card(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    width: f32,
) -> egui::Response {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(20.0));
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(11.0).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(value).size(16.0).strong().color(TEXT));
                });
            });
            ui.interact(ui.min_rect(), ui.scope_id().with(("risk_prob", label)), Sense::hover())
        })
        .inner
}

fn risk_xiaoma_widths(total: f32) -> Vec<f32> {
    [0.09, 0.09, 0.12, 0.16, 0.14, 0.14, 0.14, 0.12]
        .iter()
        .map(|f| total * f)
        .collect()
}

/// 小码表头：投注额 / 风险 / 盈亏三列可点。返回点中的排序键。
fn risk_xiaoma_header_row(
    ui: &mut egui::Ui,
    widths: &[f32],
    sort: RiskSort,
    desc: bool,
) -> Option<RiskSort> {
    let headings = ["#", "地点", "类型", "组合", "投注额", "风险", "盈亏", "时间"];
    let sortable = [
        (4, RiskSort::Stake),
        (5, RiskSort::Risk),
        (6, RiskSort::Profit),
    ];
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 25.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, REF_HEAD_BG);
    ui.painter()
        .hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, REF_LINE));
    let mut x = rect.min.x;
    let mut clicked = None;
    for (i, heading) in headings.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            Vec2::new(widths[i], rect.height()),
        );
        // 时间列居中对齐；金额 / 风险 / 盈亏列右对齐。
        let center = i == 7;
        let right = (4..=6).contains(&i);
        let mut text = heading.to_string();
        if let Some((_, key)) = sortable.iter().find(|(idx, _)| *idx == i) {
            if *key == sort {
                text.push_str(if desc { " ▼" } else { " ▲" });
            }
        }
        let (pos, align) = if center {
            (cell.center(), Align2::CENTER_CENTER)
        } else if right {
            (
                egui::pos2(cell.max.x - 8.0, cell.center().y),
                Align2::RIGHT_CENTER,
            )
        } else {
            (
                egui::pos2(cell.min.x + 8.0, cell.center().y),
                Align2::LEFT_CENTER,
            )
        };
        ui.painter().text(
            pos,
            align,
            text,
            FontId::proportional(12.0),
            REF_NAVY,
        );
        if let Some((_, key)) = sortable.iter().find(|(idx, _)| *idx == i) {
            let resp = ui.interact(cell, ui.scope_id().with(("risk_xm_hdr", i)), Sense::click());
            if resp.clicked() {
                clicked = Some(*key);
            }
        }
        x += widths[i];
    }
    clicked
}

fn risk_xiaoma_row(
    ui: &mut egui::Ui,
    widths: &[f32],
    row: &crate::risk::XiaoMaRow,
    id: usize,
) {
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 24.0), Sense::hover());
    if row.hit {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, REF_HIT_BG);
    }
    ui.painter()
        .hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, REF_LINE));

    // (文案, 颜色, 右对齐, 加粗, 字号)
    let id_text = id.to_string();
    let stake_text = fmt_money(row.stake);
    let risk_text = fmt_money(row.risk);
    let profit_text = fmt_money(row.profit);
    let cells: [( &str, Color32, bool, bool, f32); 8] = [
        (id_text.as_str(), REF_NAVY, false, false, 12.0),
        (
            crate::risk::REGION_CHARS[row.region],
            region_color(row.region),
            false,
            true,
            12.0,
        ),
        (
            row.label.as_str(),
            type_tag_colors(&row.label).1,
            false,
            true,
            12.0,
        ),
        (row.key.as_str(), REF_NAVY, false, false, 12.0),
        (stake_text.as_str(), MONEY_BLUE, true, true, 12.0),
        (risk_text.as_str(), REF_NAVY, true, false, 12.0),
        (profit_text.as_str(), profit_color(row.profit), true, true, 12.0),
        (row.time.as_str(), MUTED, false, false, 13.0),
    ];
    let mut x = rect.min.x;
    for (i, (text, color, right, bold, size)) in cells.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            Vec2::new(widths[i], rect.height()),
        );
        if i == 2 {
            // 类型列：参考项目是带底色的圆角色块，不是纯文字。
            let (bg, fg) = type_tag_colors(&row.label);
            let galley =
                ui.painter()
                    .layout_no_wrap(row.label.clone(), bold_font(12.0), fg);
            let pill_w = galley.size().x + 16.0;
            let pill = egui::Rect::from_min_size(
                egui::pos2(cell.min.x + 8.0, cell.center().y - 9.0),
                Vec2::new(pill_w, 18.0),
            );
            ui.painter()
                .rect_filled(pill, CornerRadius::same(3), bg);
            ui.painter().galley(
                egui::pos2(pill.min.x + 8.0, pill.center().y - galley.size().y / 2.0),
                galley,
                fg,
            );
            x += widths[i];
            continue;
        }
        let font = if *bold {
            bold_font(*size)
        } else if i == 3 {
            FontId::monospace(*size)
        } else {
            FontId::proportional(*size)
        };
        // 时间列与表头一致，居中显示。
        let (pos, align) = if i == 7 {
            (cell.center(), Align2::CENTER_CENTER)
        } else if *right {
            (
                egui::pos2(cell.max.x - 8.0, cell.center().y),
                Align2::RIGHT_CENTER,
            )
        } else {
            (
                egui::pos2(cell.min.x + 8.0, cell.center().y),
                Align2::LEFT_CENTER,
            )
        };
        ui.painter().text(pos, align, text, font, *color);
        x += widths[i];
    }
}

fn bill_block_ui(
    ui: &mut egui::Ui,
    block: &LedgerBillBlock,
    category_view: bool,
    index: usize,
) -> (egui::Rect, Option<usize>) {
    let mut clicked: Option<usize> = None;
    let resp = egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, Color32::from_rgb(0xE2, 0xE8, 0xF0)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&block.name)
                        .font(bold_font(16.5))
                        .color(TEXT),
                );
                count_text_pill(ui, &format!("{} 批", block.rows.len()));
            });
            ui.add_space(2.0);
            hairline(ui);
            ui.add_space(8.0);
            egui::ScrollArea::horizontal()
                .id_salt(format!("bill_block_{index}_{}", block.name))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let mut table_id = 0usize;
                        if category_view {
                            let mut cats: BTreeMap<
                                &'static str,
                                (Vec<BillTableRow>, f64, f64, f64),
                            > = BTreeMap::new();
                            for row in &block.rows {
                                for (cat, slice) in &row.cats {
                                    if slice.amount <= 0.0 && slice.win <= 0.0 {
                                        continue;
                                    }
                                    let e = cats.entry(*cat).or_insert((
                                        Vec::new(),
                                        0.0,
                                        0.0,
                                        0.0,
                                    ));
                                    e.0.push(BillTableRow {
                                        no: row.no,
                                        idx: row.idx,
                                        amount: slice.amount,
                                        hk: slice.hk,
                                        old: slice.old,
                                        win: slice.win,
                                        breakdown: sorted_breakdown(&slice.breakdown),
                                    });
                                    e.1 += slice.amount;
                                    e.2 += slice.win;
                                    e.3 += slice.rebate;
                                }
                            }
                            let mut list: Vec<_> = cats.into_iter().collect();
                            list.sort_by_key(|(cat, _)| category_order(cat));
                            for (cat, (rows, amount, win, rebate)) in list {
                                // 一个类别超过 30 行时往右续列，续列重复类别表头；
                                // 类别合计只在最后一列下面出一次（与参考页一致）。
                                let chunks: Vec<&[BillTableRow]> = if rows.is_empty() {
                                    vec![&rows[..]]
                                } else {
                                    rows.chunks(30).collect()
                                };
                                let total_chunks = chunks.len();
                                for (ci, chunk) in chunks.into_iter().enumerate() {
                                    ui.vertical(|ui| {
                                        let (w, hit) = draw_bill_table(
                                            ui,
                                            chunk,
                                            None,
                                            Some(cat),
                                            0,
                                            table_id,
                                        );
                                        table_id += 1;
                                        if hit.is_some() {
                                            clicked = hit;
                                        }
                                        if ci + 1 == total_chunks {
                                            ui.add_space(4.0);
                                            totals_box(ui, amount, win, rebate, w);
                                        }
                                    });
                                    ui.add_space(12.0);
                                }
                            }
                        } else {
                            let multi_col = block.rows.len() > 30;
                            for chunk in block.rows.chunks(30) {
                                let rows: Vec<BillTableRow> = chunk
                                    .iter()
                                    .map(|r| BillTableRow {
                                        no: r.no,
                                        idx: r.idx,
                                        amount: r.amount,
                                        hk: r.hk,
                                        old: r.old,
                                        win: r.win,
                                        breakdown: sorted_breakdown(&r.breakdown),
                                    })
                                    .collect();
                                let amount: f64 = chunk.iter().map(|r| r.amount).sum();
                                let special: f64 =
                                    chunk.iter().map(|r| r.special_win_stake).sum();
                                let (_, hit) = draw_bill_table(
                                    ui,
                                    &rows,
                                    Some((amount, special)),
                                    None,
                                    if multi_col { 30 - chunk.len() } else { 0 },
                                    table_id,
                                );
                                table_id += 1;
                                if hit.is_some() {
                                    clicked = hit;
                                }
                                ui.add_space(12.0);
                            }
                        }
                    });
                });
            if !block.win_stakes.is_empty() {
                win_band(
                    ui,
                    "中奖注额",
                    &block.win_stakes,
                    BAND_STAKE_BG,
                    BAND_STAKE_BORDER,
                );
            }
            if !block.win_amounts.is_empty() {
                win_band(
                    ui,
                    "中奖金额",
                    &block.win_amounts,
                    BAND_AMOUNT_BG,
                    BAND_AMOUNT_BORDER,
                );
            }
            totals_bar(ui, block.amount, block.win, block.rebate);
        });
    (resp.response.rect, clicked)
}

fn text_width(p: &egui::Painter, text: &str, font: &FontId) -> f32 {
    p.layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER)
        .size()
        .x
}

fn draw_bill_table(
    ui: &mut egui::Ui,
    rows: &[BillTableRow],
    subtotal: Option<(f64, f64)>,
    cat_title: Option<&str>,
    pad_rows: usize,
    // 同一帧里多张表共用序号格的交互 id，用这个序号区分。
    table_id: usize,
) -> (f32, Option<usize>) {
    let p = ui.painter();
    let hdr_font = bold_font(12.5);
    let no_font = FontId::proportional(13.0);
    let amount_font = bold_font(14.0);
    let small_font = FontId::proportional(11.5);

    let mut no_w = text_width(p, "#", &hdr_font);
    let mut amt_w = text_width(p, "金额", &hdr_font);
    let mut win_w = text_width(p, "中奖", &hdr_font);
    for row in rows {
        no_w = no_w.max(text_width(p, &row.no.to_string(), &no_font));
        amt_w = amt_w.max(stake_cell_width(p, row.amount, row.hk, row.old, &amount_font));
        win_w = win_w.max(win_content_width(p, row.win, &row.breakdown));
    }
    if let Some((amount, special)) = subtotal {
        amt_w = amt_w.max(text_width(p, &fmt_money(amount), &bold_font(14.0)));
        let sub = text_width(p, "特码 ", &small_font)
            + text_width(p, &fmt_money(special), &bold_font(13.5));
        win_w = win_w.max(sub);
    }
    // 列宽 = 最宽内容 + 约 1 个中文字，再留一点内边距。
    let no_w = (no_w + 22.0).max(44.0);
    let amt_w = (amt_w + 28.0).max(78.0);
    let mut win_w = (win_w + 28.0).max(98.0);
    if let Some(title) = cat_title {
        let need = text_width(p, title, &bold_font(13.5)) + 24.0;
        if need > no_w + amt_w + win_w {
            win_w = need - no_w - amt_w;
        }
    }
    let w = no_w + amt_w + win_w;
    let cat_h = if cat_title.is_some() { 22.0 } else { 0.0 };
    let head_h = 26.0;
    let row_h = 26.0;
    let sub_h = if subtotal.is_some() { 26.0 } else { 0.0 };
    let h = cat_h
        + head_h
        + (rows.len() + pad_rows) as f32 * row_h
        + sub_h;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, CARD);
    let x0 = rect.left();
    let x1 = x0 + no_w;
    let x2 = x1 + amt_w;
    let x3 = rect.right();
    let mut y = rect.top();
    let line = Stroke::new(1.0, BILL_LINE);
    let mut ys: Vec<f32> = vec![y];

    if let Some(title) = cat_title {
        let r = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x3, y + cat_h));
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_rgb(0xE2, 0xE8, 0xF0));
        p.text(
            r.center(),
            Align2::CENTER_CENTER,
            title,
            bold_font(13.5),
            Color32::from_rgb(0x17, 0x32, 0x4D),
        );
        y += cat_h;
        ys.push(y);
    }

    let r = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x3, y + head_h));
    p.rect_filled(r, CornerRadius::ZERO, BILL_HEAD);
    for (x, t) in [(x0, "#"), (x1, "金额"), (x2, "中奖")] {
        p.text(
            egui::pos2(x + 8.0, y + head_h / 2.0),
            Align2::LEFT_CENTER,
            t,
            hdr_font.clone(),
            BILL_TEXT,
        );
    }
    y += head_h;
    ys.push(y);

    // 序号格：数字用一个小方块包起来，点一下跳到主页对应的注单消息。
    let mut clicked: Option<usize> = None;
    for row in rows {
        let cell = egui::Rect::from_min_max(
            egui::pos2(x0, y),
            egui::pos2(x1, y + row_h),
        );
        let resp = ui.interact(
            cell,
            ui.make_persistent_id(("bill_no", table_id, row.idx)),
            Sense::click(),
        );
        let hovered = resp.hovered();
        if resp.clicked() {
            clicked = Some(row.idx);
        }
        let box_rect =
            egui::Rect::from_center_size(cell.center(), Vec2::new(24.0, 18.0));
        p.rect_filled(box_rect, CornerRadius::same(4), Color32::WHITE);
        p.rect_stroke(
            box_rect,
            CornerRadius::same(4),
            Stroke::new(1.0, if hovered { MONEY_BLUE } else { BILL_LINE }),
            egui::StrokeKind::Inside,
        );
        p.text(
            cell.center(),
            Align2::CENTER_CENTER,
            row.no.to_string(),
            no_font.clone(),
            if hovered { MONEY_BLUE } else { CELL_NO },
        );
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            resp.on_hover_text("点击跳到主页这条注单消息");
        }
        draw_stake_cell_at(p, x1, x2, y, row_h, row.amount, row.hk, row.old, MONEY_BLUE);
        draw_win_cell_at(p, x2, x3, y, row_h, row.win, &row.breakdown);
        y += row_h;
        ys.push(y);
    }
    for _ in 0..pad_rows {
        y += row_h;
        ys.push(y);
    }

    if let Some((amount, special)) = subtotal {
        let r = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x3, y + sub_h));
        p.rect_filled(r, CornerRadius::ZERO, SUBTOTAL_BG);
        p.text(
            egui::pos2((x0 + x1) / 2.0, y + sub_h / 2.0),
            Align2::CENTER_CENTER,
            "小计",
            bold_font(13.5),
            BILL_TEXT,
        );
        draw_number_cell_at(p, x1, x2, y, sub_h, amount, MONEY_BLUE);
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "特码 ",
            0.0,
            egui::TextFormat {
                font_id: small_font.clone(),
                color: MUTED,
                ..Default::default()
            },
        );
        job.append(
            &fmt_money(special),
            0.0,
            egui::TextFormat {
                font_id: bold_font(13.5),
                color: SPECIAL_RED,
                ..Default::default()
            },
        );
        let g = p.layout_job(job);
        let clip = p.with_clip_rect(egui::Rect::from_min_max(
            egui::pos2(x2, y),
            egui::pos2(x3, y + sub_h),
        ));
        clip.galley(
            egui::pos2(x2 + 8.0, y + sub_h / 2.0 - g.size().y / 2.0),
            g,
            SPECIAL_RED,
        );
        y += sub_h;
        ys.push(y);
    }

    for yy in &ys[1..] {
        p.hline(rect.x_range(), *yy, line);
    }
    // 类别表头是一整格，内部不画竖分隔线；左右外框仍覆盖整个表格。
    let body_top = rect.top() + cat_h;
    p.vline(x1, egui::Rangef::new(body_top, rect.bottom()), line);
    p.vline(x2, egui::Rangef::new(body_top, rect.bottom()), line);
    p.rect_stroke(rect, CornerRadius::ZERO, line, egui::StrokeKind::Inside);
    (w, clicked)
}

/// 金额单元格的总宽度：主金额 + 「(港N)」「(老N)」两段小字。
fn stake_cell_width(
    p: &egui::Painter,
    amount: f64,
    hk: f64,
    old: f64,
    main_font: &FontId,
) -> f32 {
    let small = FontId::proportional(11.5);
    let tag = bold_font(11.5);
    let mut w = text_width(p, &fmt_money(amount), main_font);
    for (ch, v) in [("港", hk), ("老", old)] {
        if v <= 0.0 {
            continue;
        }
        // 2px 段间距 +「()」+ 徽章（字宽 + 左右各 2px 内边距/边框）+ 金额
        w += 2.0
            + text_width(p, "(", &small)
            + text_width(p, ch, &tag)
            + 4.0
            + text_width(p, &fmt_money(v), &small)
            + text_width(p, ")", &small);
    }
    w
}

/// 金额单元格：`100(港50)(老20)`。
/// 括号与括号里的金额是灰色小字，「港」「老」各按参考项目的小徽章样式上色
/// （白底 + 浅色边框 + 红 / 琥珀文字）。
#[allow(clippy::too_many_arguments)]
fn draw_stake_cell_at(
    p: &egui::Painter,
    x0: f32,
    x1: f32,
    y: f32,
    h: f32,
    amount: f64,
    hk: f64,
    old: f64,
    color: Color32,
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, y),
        egui::pos2(x1, y + h),
    ));
    let cy = y + h / 2.0;
    let main_font = bold_font(14.0);
    let main = fmt_money(amount);
    let mut pen = x0 + 8.0;
    clip.text(
        egui::pos2(pen, cy),
        Align2::LEFT_CENTER,
        &main,
        main_font.clone(),
        color,
    );
    pen += text_width(p, &main, &main_font);

    let small = FontId::proportional(11.5);
    let tag_font = bold_font(11.5);
    for (ch, v, fg, border) in [
        ("港", hk, TAG_HK, TAG_HK_BORDER),
        ("老", old, TAG_OLD, TAG_OLD_BORDER),
    ] {
        if v <= 0.0 {
            continue;
        }
        pen += 2.0;
        clip.text(egui::pos2(pen, cy), Align2::LEFT_CENTER, "(", small.clone(), MUTED);
        pen += text_width(p, "(", &small);

        let tag_w = text_width(p, ch, &tag_font);
        let r = egui::Rect::from_center_size(
            egui::pos2(pen + tag_w / 2.0, cy),
            egui::vec2(tag_w + 4.0, (h - 10.0).min(15.0)),
        );
        clip.rect_filled(r, CornerRadius::same(2), Color32::WHITE);
        clip.rect_stroke(
            r,
            CornerRadius::same(2),
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        clip.text(r.center(), Align2::CENTER_CENTER, ch, tag_font.clone(), fg);
        pen += tag_w + 4.0;

        let num = fmt_money(v);
        clip.text(egui::pos2(pen, cy), Align2::LEFT_CENTER, &num, small.clone(), MUTED);
        pen += text_width(p, &num, &small);
        clip.text(egui::pos2(pen, cy), Align2::LEFT_CENTER, ")", small.clone(), MUTED);
        pen += text_width(p, ")", &small);
    }
}

fn draw_number_cell_at(
    p: &egui::Painter,
    x0: f32,
    x1: f32,
    y: f32,
    h: f32,
    v: f64,
    color: Color32,
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, y),
        egui::pos2(x1, y + h),
    ));
    clip.text(
        egui::pos2(x0 + 8.0, y + h / 2.0),
        Align2::LEFT_CENTER,
        fmt_money(v),
        bold_font(14.0),
        color,
    );
}

/// 中奖列的玩法徽章：显示名、文字色、边框色。
fn badge_style(label: &str) -> (String, Color32, Color32) {
    let (display, fg, border) = crate::win_style::badge(label);
    let display = if display.is_empty() {
        label.to_string()
    } else {
        display.to_string()
    };
    (
        display,
        Color32::from_rgb(fg[0], fg[1], fg[2]),
        Color32::from_rgb(border[0], border[1], border[2]),
    )
}

const BADGE_FONT: f32 = 10.5;
const BADGE_PAD_X: f32 = 3.0;
const BADGE_HEIGHT: f32 = 15.0;
const BADGE_GAP: f32 = 3.0;

fn badge_box_width(p: &egui::Painter, display: &str) -> f32 {
    text_width(p, display, &bold_font(BADGE_FONT)) + BADGE_PAD_X * 2.0 + 2.0
}

fn breakdown_text_width(
    p: &egui::Painter,
    entries: &[(String, f64, f64)],
    small: &FontId,
) -> f32 {
    let sorted = sorted_breakdown(entries);
    let mut w = text_width(p, " (", small);
    for (i, (label, stake, odds)) in sorted.iter().enumerate() {
        if i > 0 {
            w += text_width(p, "、", small);
        }
        let (display, _, _) = badge_style(label);
        w += badge_box_width(p, &display) + BADGE_GAP;
        w += text_width(
            p,
            &format!("{}×{}", fmt_money(*stake), fmt_money(*odds)),
            small,
        );
    }
    w + text_width(p, ")", small)
}

fn win_content_width(
    p: &egui::Painter,
    win: f64,
    entries: &[(String, f64, f64)],
) -> f32 {
    let main = text_width(p, &fmt_money(win), &bold_font(14.0));
    if win <= 0.0 || entries.is_empty() {
        main
    } else {
        main + breakdown_text_width(p, entries, &FontId::proportional(11.5))
    }
}

fn draw_win_cell_at(
    p: &egui::Painter,
    x0: f32,
    x1: f32,
    y: f32,
    h: f32,
    win: f64,
    entries: &[(String, f64, f64)],
) {
    let clip = p.with_clip_rect(egui::Rect::from_min_max(
        egui::pos2(x0, y),
        egui::pos2(x1, y + h),
    ));
    let cy = y + h / 2.0;
    let main_color = if win > 0.0 { MONEY_GREEN } else { MUTED };
    let main = fmt_money(win);
    let mut pen = x0 + 8.0;
    clip.text(
        egui::pos2(pen, cy),
        Align2::LEFT_CENTER,
        &main,
        bold_font(14.0),
        main_color,
    );
    pen += text_width(p, &main, &bold_font(14.0));
    if win <= 0.0 || entries.is_empty() {
        return;
    }

    let small = FontId::proportional(11.5);
    let badge_font = bold_font(BADGE_FONT);
    clip.text(
        egui::pos2(pen, cy),
        Align2::LEFT_CENTER,
        " (",
        small.clone(),
        MUTED,
    );
    pen += text_width(p, " (", &small);
    for (i, (label, stake, odds)) in sorted_breakdown(entries).iter().enumerate() {
        if i > 0 {
            clip.text(
                egui::pos2(pen, cy),
                Align2::LEFT_CENTER,
                "、",
                small.clone(),
                MUTED,
            );
            pen += text_width(p, "、", &small);
        }
        let (display, fg, border) = badge_style(label);
        let bw = badge_box_width(p, &display);
        let box_rect = egui::Rect::from_min_max(
            egui::pos2(pen, cy - BADGE_HEIGHT / 2.0),
            egui::pos2(pen + bw, cy + BADGE_HEIGHT / 2.0),
        );
        clip.rect_filled(box_rect, CornerRadius::same(3), Color32::WHITE);
        clip.rect_stroke(
            box_rect,
            CornerRadius::same(3),
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        clip.text(
            box_rect.center(),
            Align2::CENTER_CENTER,
            &display,
            badge_font.clone(),
            fg,
        );
        pen += bw + BADGE_GAP;
        let detail = format!("{}×{}", fmt_money(*stake), fmt_money(*odds));
        clip.text(
            egui::pos2(pen, cy),
            Align2::LEFT_CENTER,
            &detail,
            small.clone(),
            MUTED,
        );
        pen += text_width(p, &detail, &small);
    }
    clip.text(
        egui::pos2(pen, cy),
        Align2::LEFT_CENTER,
        ")",
        small.clone(),
        MUTED,
    );
}

fn win_band(
    ui: &mut egui::Ui,
    title: &str,
    items: &BTreeMap<String, f64>,
    bg: Color32,
    border: Color32,
) {
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(title)
                        .font(bold_font(13.5))
                        .color(Color32::from_rgb(0x47, 0x55, 0x69)),
                );
                let mut values: Vec<_> =
                    items.iter().filter(|(_, v)| **v > 0.0).collect();
                values.sort_by_key(|(k, _)| play_order(k));
                for (label, value) in values {
                    ui.label(RichText::new(label).size(13.0).color(BILL_TEXT));
                    ui.label(
                        RichText::new(fmt_money(*value))
                            .font(bold_font(14.0))
                            .color(MONEY_GREEN),
                    );
                }
            });
        });
}

fn totals_bar(ui: &mut egui::Ui, amount: f64, win: f64, rebate: f64) {
    let net = amount - win - rebate;
    egui::Frame::new()
        .fill(TOTALS_BG)
        .stroke(Stroke::new(1.0, BILL_LINE))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for (label, value, color) in [
                    ("总金额", amount, BILL_TEXT),
                    ("总中奖", win, MONEY_GREEN),
                    ("总回水", rebate, BILL_TEXT),
                    (
                        "总输赢",
                        net,
                        if net >= 0.0 { MONEY_GREEN } else { MONEY_RED },
                    ),
                ] {
                    ui.label(
                        RichText::new(label)
                            .font(bold_font(13.5))
                            .color(Color32::from_rgb(0x1E, 0x29, 0x3B)),
                    );
                    ui.label(
                        RichText::new(fmt_money(value))
                            .font(bold_font(14.0))
                            .color(color),
                    );
                    ui.add_space(18.0);
                }
            });
        });
}

fn totals_box(ui: &mut egui::Ui, amount: f64, win: f64, rebate: f64, width: f32) {
    let net = amount - win - rebate;
    egui::Frame::new()
        .fill(SUBTOTAL_BG)
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x94, 0xA3, 0xB8)))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            // Frame 的内边距在内容宽度之外，减掉左右各 10px，让整个框
            // 和最后一列表格严格同宽。
            ui.set_width((width - 20.0).max(120.0));
            for (label, value, color) in [
                ("金额", amount, BILL_TEXT),
                ("中奖", win, MONEY_GREEN),
                ("回水", rebate, BILL_TEXT),
                (
                    "输赢",
                    net,
                    if net >= 0.0 { MONEY_GREEN } else { MONEY_RED },
                ),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).size(13.0).color(BILL_TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(fmt_money(value))
                                .font(bold_font(13.5))
                                .color(color),
                        );
                    });
                });
            }
        });
}

/// 「一注」列只显示金额，不显示「各组 / 各数」标记。
fn stake_amount_text(stake: &str) -> &str {
    stake
        .strip_prefix("各组 ")
        .or_else(|| stake.strip_prefix("各数 "))
        .unwrap_or(stake)
}

/// 单条录入的预览里去掉「第 N 行：」前缀（补了默认地区/玩法行后行号会偏移）。
fn strip_reason_line(reason: &str) -> &str {
    if let Some(rest) = reason.strip_prefix("第 ") {
        if let Some(pos) = rest.find('：') {
            return &rest[pos + '：'.len_utf8()..];
        }
    }
    reason
}

/// 去掉所有空白后比较，兼容微信引用里多出来的换行 / 空格。
fn compact_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 给一条注单打「像不像被引用消息」的分：正文匹配是前提，同会话 / 同发送人 / 同时间加分。
/// 图片引用没有正文可比，按「同一会话 + 发送人 + 时间最接近」定位。
fn find_quoted_image_index(
    orders: &[MsgRow],
    row: &MsgRow,
    quote: &QuoteRef,
) -> Option<usize> {
    let quote_ms = quote.create_time.saturating_mul(1000);
    let mut best: Option<(i64, usize)> = None;
    for (i, order) in orders.iter().enumerate() {
        if order.chat != row.chat || order.kind != "图片" {
            continue;
        }
        // from_usr 可能是旧数据里的群 username（xxx@chatroom），那就退回按昵称。
        let sender_known =
            !quote.from_usr.is_empty() && !quote.from_usr.ends_with("@chatroom");
        if sender_known {
            if order.sender != quote.from_usr {
                continue;
            }
        } else if !quote.from.is_empty() && order.nick != quote.from {
            continue;
        }
        let diff = if quote_ms > 0 && order.sort > 0 {
            (order.sort - quote_ms).abs()
        } else {
            i64::MAX
        };
        // 只接受 10 分钟内的图片，避免引用消息不在本次导入范围时误定位。
        if quote_ms > 0 && diff > 600_000 {
            continue;
        }
        if best.is_none_or(|(best_diff, _)| diff < best_diff) {
            best = Some((diff, i));
        }
    }
    best.map(|(_, i)| i)
}

/// 一条消息在「已撤回」名单里的身份：(sort, 会话, 发送人)。
fn revoke_key_of(row: &MsgRow) -> crate::revoked::RevokeKey {
    (row.sort, row.chat.clone(), row.sender.clone())
}

/// 一批消息里可以作为撤回目标的候选：撤回提示自身除外；**已撤回的原消息
/// 仍然保留**。重新导入时原消息会被过滤掉不再显示，但撤回提示仍要能在
/// 候选池里找到它，否则会退而匹配更早的同发送人消息，误记成新的「已撤回」。
fn revoke_candidates(rows: &[MsgRow]) -> Vec<MsgRow> {
    rows.iter()
        .filter(|r| r.kind != "撤回")
        .cloned()
        .collect()
}

/// 从撤回提示里取撤回人昵称：`"张三" 撤回了一条消息` → `张三`。
fn revoke_actor(content: &str) -> Option<String> {
    let text = content.trim();
    let idx = text
        .find("撤回了一条消息")
        .or_else(|| text.find("撤回了"))?;
    let name = text[..idx]
        .trim()
        .trim_matches(|c: char| c.is_whitespace() || "\"'“”「」『』：:".contains(c))
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 找被撤回的那一条。
///
/// - 撤回提示带原消息 id（`revoke_target`）时：在旧消息 + 本批里精确找，找不到就
///   放弃，绝不猜。
/// - 没有 id 时（不少微信版本就是这样）：**只在旧消息（之前导入 / 监控到的）里**
///   按「同会话、撤回提示之前、发送人匹配」取最近一条。本批新读到的消息不能当
///   候选——原消息被撤回后会被微信替换掉，本批里还留着的上一条并不是它。
///
/// 群聊按撤回提示里的昵称匹配；单聊按「不是自己」的那一方匹配；
/// `你撤回了一条消息` 匹配自己。
fn find_revoke_target(
    revoke: &MsgRow,
    orders: &[MsgRow],
    batch: &[MsgRow],
    self_wxid: &str,
    name_to_usr: &HashMap<String, String>,
) -> Option<MsgRow> {
    // 撤回提示解析出了原消息 id：只认这一条。**找不到就返回 None**，
    // 不能退而匹配「同发送人最近一条」——原消息可能在导入时段之外，
    // 那样会把一条没撤回的消息误记成已撤回。
    if let Some(target_sort) = revoke.revoke_target {
        if target_sort <= 0 {
            return None;
        }
        // 不再要求 `m.sort < revoke.sort`：微信常把撤回提示放在原消息的位置，
        // 两条的 sort 可能相同（甚至提示略靠前）；id 匹配是权威的。
        return orders
            .iter()
            .chain(batch.iter())
            .find(|m| {
                m.chat == revoke.chat && m.sort == target_sort && m.kind != "撤回"
            })
            .cloned();
    }
    let actor = revoke_actor(&revoke.content);
    let is_group = revoke.chat.ends_with("@chatroom");
    let mut best: Option<&MsgRow> = None;
    // 只查旧消息：本批新读到的上一条不是被撤回的那条（原消息已被替换掉）。
    for m in orders {
        // 允许 sort 相同：撤回提示可能就放在原消息的位置上。
        if m.chat != revoke.chat || m.sort > revoke.sort {
            continue;
        }
        if matches!(m.kind.as_str(), "撤回" | "系统") {
            continue;
        }
        let actor_is_self = |name: &str| {
            name == "你"
                || (!self_wxid.is_empty()
                    && name_to_usr.get(name).is_some_and(|usr| *usr == self_wxid))
        };
        let matches = match actor.as_deref() {
            Some(name) => {
                if actor_is_self(name) {
                    !self_wxid.is_empty() && m.sender == self_wxid
                } else if is_group {
                    // 备注名 / 群昵称可能对不上：再用「显示名 → username」兜一次。
                    m.nick == name
                        || name_to_usr
                            .get(name)
                            .is_some_and(|usr| *usr == m.sender)
                } else {
                    !self_wxid.is_empty() && m.sender != self_wxid
                }
            }
            None => !is_group && !self_wxid.is_empty() && m.sender != self_wxid,
        };
        if !matches {
            continue;
        }
        if best.is_none_or(|b| m.sort > b.sort) {
            best = Some(m);
        }
    }
    best.cloned()
}

/// 在一组消息里找「被引用消息」。
///
/// 文字 / 链接按正文匹配；图片按发送人 + 时间；没有可读正文的类型（语音 /
/// 视频 / 表情等）按「同会话 + 发送人 + 时间最近」兜底。
fn find_quote_in_rows(rows: &[MsgRow], row: &MsgRow, quote: &QuoteRef) -> Option<MsgRow> {
    if quote.msg_type == 3 {
        if let Some(i) = find_quoted_image_index(rows, row, quote) {
            return Some(rows[i].clone());
        }
    }
    let quote_ms = quote.create_time.saturating_mul(1000);
    let sender_matches = |o: &MsgRow| -> bool {
        if !quote.from_usr.is_empty() && !quote.from_usr.ends_with("@chatroom") {
            o.sender == quote.from_usr
        } else if !quote.from.is_empty() {
            o.nick == quote.from
        } else {
            true
        }
    };
    let target = compact_text(quote.content.trim());
    if target.is_empty() {
        return rows
            .iter()
            .filter(|o| o.chat == row.chat && sender_matches(o))
            .map(|o| ((o.sort - quote_ms).abs(), o))
            .min_by_key(|(diff, _)| *diff)
            .map(|(_, o)| o.clone());
    }
    let quote_time = if quote.create_time > 0 {
        Local
            .timestamp_opt(quote.create_time, 0)
            .single()
            .map(|t| format!("{}:{:02}", t.hour(), t.minute()))
    } else {
        None
    };
    let mut best: Option<(i32, usize)> = None;
    for (i, order) in rows.iter().enumerate() {
        let Some(score) = quote_match_score(order, &row.chat, quote, quote_time.as_deref()) else {
            continue;
        };
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| rows[i].clone())
}

fn quote_match_score(
    order: &MsgRow,
    chat: &str,
    quote: &QuoteRef,
    quote_time: Option<&str>,
) -> Option<i32> {
    let target = quote.content.trim();
    let target_compact = compact_text(target);
    if target_compact.is_empty() {
        return None;
    }
    let content = order.content.trim();
    let compact = compact_text(content);
    // 长消息在引用里可能被截断成「...」，去掉省略号后按前缀再试一次。
    let target_core = target_compact.trim_end_matches(['.', '…']);
    let base = if content == target {
        100
    } else if compact == target_compact {
        92
    } else if target_compact.chars().count() >= 6 && compact.contains(&target_compact) {
        55
    } else if target_core.chars().count() >= 6 && compact.contains(target_core) {
        50
    } else {
        return None;
    };
    let mut score = base;
    if order.chat == chat {
        score += 25;
    }
    if !quote.from_usr.is_empty() && order.sender == quote.from_usr {
        score += 40;
    } else if !quote.from.is_empty() && order.nick == quote.from {
        score += 18;
    }
    if let Some(t) = quote_time {
        if order.time == t {
            score += 15;
        }
    }
    Some(score)
}

fn truncate_display(s: &str, max: usize) -> String {
    let mut it = s.chars();
    let mut out: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        out.push('…');
    }
    out

}

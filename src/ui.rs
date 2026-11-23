//! 账无忧界面：
//! - 主页：顶部按钮（选定会员 / 导入注单），左侧成员竖排 Tab，中间常驻注单消息列表，右侧注单详情面板；
//! - 选定会员页：联系人 / 群聊两个 Tab，群聊树形展开成员，全部可勾选；
//! - 导入注单：弹窗设置日期与时段，导入后在所选时段内实时追加新消息。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

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
// 顶部工具栏：深灰底 + 比底色略浅的按钮，文字统一白色。
const TOOLBAR_BG: Color32 = Color32::from_rgb(0x54, 0x5C, 0x64);
const TOOLBAR_BTN: Color32 = Color32::from_rgb(0x6B, 0x73, 0x7B);
const TOOLBAR_BTN_HOVER: Color32 = Color32::from_rgb(0x7C, 0x84, 0x8C);
const TOOLBAR_BTN_ACTIVE: Color32 = Color32::from_rgb(0x60, 0x68, 0x70);
const TOOLBAR_BTN_OFF: Color32 = Color32::from_rgb(0x5E, 0x65, 0x6C);
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

/// 「风险」页面的五个页签。
#[derive(Clone, Copy, PartialEq, Eq)]
enum RiskTab {
    XiaoMa,
    Special,
    LianXiao,
    FlatSpecial,
    FlatNumber,
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
    last_update: String,
    import_rx: Option<Receiver<ImportMsg>>,

    // 实时监控（选定会员后常驻）
    selection_dirty: bool,
    monitor_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    monitor_rx: Option<Receiver<MonitorMsg>>,

    // 清空注单弹窗
    clear_open: bool,
    clear_groups: HashSet<String>,
    clear_members: HashSet<(String, String)>,

    // 主页成员条与详情面板
    /// 左侧竖排选中的成员（详情面板显示该成员的消息）
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
    /// 连肖子页签规格（2/3/4/5），默认二连肖
    risk_lian_size: usize,
    /// 连肖每一栏各自的页码（1 起）
    risk_lian_pages: [usize; 3],
    /// 特码每一栏各自的风险排序方向：false = 升序（默认，赔得最狠的在前）
    risk_panel_desc: [bool; 3],
    /// 三栏折叠状态：老澳门默认收起
    risk_collapsed: [bool; 3],

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
    win: f64,
    win_stake: f64,
    rebate: f64,
    breakdown: Vec<(String, f64, f64)>,
}

/// 出账单的一行（一批注单）。
struct LedgerBillRow {
    no: usize,
    amount: f64,
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
            last_update: String::new(),
            import_rx: None,
            selection_dirty: true,
            monitor_stop: None,
            monitor_rx: None,
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
            reconcile_only_diff: false,
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
            edit_text: String::new(),
            edit_time: String::new(),
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
            risk_lian_size: 2,
            risk_lian_pages: [1, 1, 1],
            risk_panel_desc: [false, false, false],
            risk_collapsed: [false, false, true],
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
        self.reconcile_only_diff = false;
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
        for mut r in rows {
            // 这条消息保存过编辑后的原文：用保存的文本覆盖微信原文。
            if let Some(edited) = self.order_edits.get(&edit_key(&r)) {
                r.content = edited.clone();
            }
            if let Some(t) = self.order_times.get(&edit_key(&r)) {
                r.time = t.clone();
            }
            // 引用 / 撤回都不参与注单转换，统一进「引用列表」独立窗口。
            // 它优先于下面的占位过滤：正文就算是「1」也仍然保留。
            if r.kind == "引用" || r.kind == "撤回" {
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

    /// 选定会员 / 分组后常驻监控：有可读来源就运行；无来源则停止。
    /// 监控只追加「从现在开始」的新消息，历史消息由「导入注单」负责回填。
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

        if self.monitor_rx.is_none() || self.selection_dirty {
            let req = ImportRequest {
                account_dir: data.account_path.clone(),
                workdir: data.workdir.clone(),
                keys: data.keys.clone(),
                contacts: source.contacts,
                groups: source.groups.into_iter().collect(),
                name_of: build_name_map(data),
                start_ts: 0,
                end_ts: i64::MAX,
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
                ui.label(
                    RichText::new(format!("当前已保存：{}", draw_summary(&self.draws)))
                        .size(10.5)
                        .color(MUTED),
                );
                ui.add_space(8.0);

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
                                        let resp = ui.add(
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
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.draw_paste[i])
                                        .hint_text("01 02 03 04 05 06 07")
                                        .desired_width(220.0)
                                        .font(FontId::proportional(11.5)),
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
                        });
                    ui.add_space(6.0);
                }

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
                            // 面板拉高后让录入框跟着长，而不是下面留一大片空白。
                            let rows = ((ui.available_height() - 200.0) / 14.5)
                                .clamp(14.0, 30.0) as usize;
                            let draft_id = egui::Id::unique("info_code_draft");
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
            .with_inner_size([620.0, 680.0])
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
        ui.horizontal(|ui| {
            ui.label(RichText::new("引用列表").font(bold_font(18.0)).color(TEXT));
            count_text_pill(ui, &format!("{} 条", self.references.len()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(tiny_button("关闭", SUB)).clicked() {
                    self.reference_open = false;
                    self.reference_popup = 0;
                }
            });
        });
        ui.label(
            RichText::new(
                "引用 / 撤回类型的消息不会进入注单列表；这里保留引用内容与被引用原文，可一键定位回注单列表。",
            )
            .size(11.0)
            .color(SUB),
        );
        ui.add_space(6.0);

        let mut clear = false;
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.reference_search)
                    .hint_text("搜索引用 / 被引用原文 / 发送人")
                    .desired_width(260.0),
            );
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

        ui.add_space(4.0);
        egui::CollapsingHeader::new("过滤设置")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "原文（去掉首尾空白后）完全等于下面任意一行时，不导入注单列表；引用类型消息不受影响。默认忽略「1」「2」。",
                    )
                    .size(10.5)
                    .color(SUB),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.reference_ignore_edit)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("每行一个，例如：1\n2"),
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
                        "过滤规则保存后对下一次导入生效；引用 / 撤回类型消息固定只进入本列表。",
                    )
                    .size(10.0)
                    .color(SUB),
                );
            });

        ui.add_space(4.0);
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
                        "暂无引用 / 撤回消息"
                    } else {
                        "没有匹配的引用"
                    })
                    .size(12.0)
                    .color(SUB),
                );
                if self.references.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("导入注单后，引用与撤回消息会自动出现在这里。")
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
            self.delete_quoted_message(&row);
            // 按需求：删除被引用消息时，连同引用列表里的这条引用一起移除。
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
    fn delete_quoted_message(&mut self, row: &MsgRow) {
        let Some(index) = self.find_quoted_order(row) else {
            let target = row
                .quote
                .as_ref()
                .map(|q| q.content.trim())
                .unwrap_or("");
            self.reference_msg = Some(format!(
                "未在注单列表中找到被引用消息：{}",
                truncate_display(target, 24)
            ));
            self.reference_msg_error = true;
            return;
        };
        let target = self.orders[index].clone();
        self.delete_order(&target);
        self.reference_msg = Some(format!(
            "已删除被引用消息（{} {}）",
            target.time, target.nick
        ));
        self.reference_msg_error = false;
    }

    /// 在注单消息列表里找到并选中被引用的那条消息。
    ///
    /// 匹配顺序：正文完全一致 → 去掉空白后一致 → 正文包含；
    /// 再按同一会话、被引用发送人、时间做加权，避免群里同一句话多个人说过时选错。
    fn locate_quoted_message(&mut self, row: &MsgRow, ctx: &egui::Context) {
        let Some(quote) = &row.quote else {
            self.reference_msg = Some("这条引用没有可定位的原文。".to_string());
            self.reference_msg_error = true;
            return;
        };
        let target = quote.content.trim();
        let Some(index) = self.find_quoted_order(row) else {
            self.reference_msg = Some(format!(
                "未在注单列表中找到被引用消息：{}",
                truncate_display(target, 24)
            ));
            self.reference_msg_error = true;
            return;
        };
        // 清掉成员 / 分组过滤，保证目标行一定在可见列表里。
        self.member_focus = None;
        self.order_filter = None;
        self.page = Page::Home;
        let target_row = self.orders[index].clone();
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
        self.scroll_to_order = true;
        self.reference_msg = Some(format!(
            "已定位到被引用消息（{} {}）",
            target_row.time, target_row.nick
        ));
        self.reference_msg_error = false;
        // 把主窗口拉到最前，用户能立刻看到选中行。
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Focus,
        );
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.batch_replace_to)
                        .id(input_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("留空表示删除"),
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

    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let n_c = self.selected_contacts.len();
        let n_g = self.selected_groups.len();
        let n_m = self.selected_members.len();

        ui.horizontal(|ui| {
            match self.page {
                Page::Home => {
                    if top_button(ui, "选定会员", 76.0).clicked() {
                        self.page = Page::Select;
                    }
                    ui.add_space(4.0);
                    let ready = matches!(self.state, State::Ready(_));
                    if top_button_enabled(ui, "导入注单", 76.0, ready)
                        .on_hover_text("选择日期与时间段后导入消息")
                        .clicked()
                    {
                        self.dialog_error = None;
                        self.dialog_open = true;
                        self.clear_open = false;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                    ui.add_space(4.0);
                    if top_button_enabled(ui, "清空注单", 76.0, ready)
                        .on_hover_text("按会员清空注单；不勾选会员则清空全部")
                        .clicked()
                    {
                        self.dialog_open = false;
                        self.clear_open = true;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                    ui.add_space(4.0);
                    if top_button(ui, "关键词", 60.0)
                        .on_hover_text("玩法关键词 / 金额识别 / 区域 & 其他")
                        .clicked()
                    {
                        self.keywords_open = true;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(4.0);
                    if top_button(ui, "替换词", 60.0)
                        .on_hover_text("转换前把 A 词替换为 B 词")
                        .clicked()
                    {
                        self.replacements_open = true;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                    }
                    ui.add_space(4.0);
                    if top_button(ui, "赔率", 52.0)
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
                    ui.add_space(4.0);
                    if top_button(ui, "开奖", 52.0)
                        .on_hover_text("录入开奖号码，用于总账对奖")
                        .clicked()
                    {
                        self.draw_inputs = draw_inputs_from(&self.draws);
                        self.draw_paste = Default::default();
                        self.draw_paste_error = Default::default();
                        self.draw_error = None;
                        self.draw_open = true;
                    }
                    ui.add_space(4.0);
                    if top_button(ui, "总账", 52.0)
                        .on_hover_text("出账单：按会员汇总投注金额、中奖、回水和输赢")
                        .clicked()
                    {
                        self.page = Page::Ledger;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(4.0);
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
                    ui.add_space(4.0);
                    if top_button(ui, "风险", 52.0)
                        .on_hover_text("按号码 / 组合查看整区风险敞口")
                        .clicked()
                    {
                        self.page = Page::Risk;
                        self.dialog_open = false;
                        self.clear_open = false;
                        self.keywords_open = false;
                        self.replacements_open = false;
                    }
                    ui.add_space(4.0);
                    if top_button(ui, "信息码", 60.0)
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
                if top_button(ui, "⟳", 22.0)
                    .on_hover_text("重新读取微信数据（会清空当前注单）")
                    .clicked()
                {
                    self.refresh(ctx);
                }
                match &self.state {
                    // 主页顶栏最右侧不再显示昵称 / 联系人数 / 群聊数，只保留刷新按钮。
                    State::Ready(_) => {}
                    State::Loading => {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(&self.status).size(11.0).color(TOOLBAR_SUB),
                        );
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

    /// 某条注单在指定地区的赔率：会员覆盖 → 分组 → 默认。
    fn odds_for_row(&self, row: &MsgRow, lottery: &str) -> &crate::odds::OddsSet {
        let group = self.group_with_odds_for(&row.chat, &row.sender);
        self.odds
            .resolve_lottery(&row.nick, group.as_deref(), lottery)
    }

    /// 详情面板正在展示的会员（注单详情优先，其次单条录入选中的会员）。
    fn detail_odds(&self, lottery: &str) -> &crate::odds::OddsSet {
        let (member, group) = if let Some(row) = &self.selected_order_row {
            (
                row.nick.clone(),
                self.group_with_odds_for(&row.chat, &row.sender),
            )
        } else if let Some(key) = &self.single_member {
            let member = self
                .single_members
                .iter()
                .find(|c| &c.key == key)
                .map(|c| c.nick.clone())
                .unwrap_or_default();
            (member, self.group_with_odds_for_member(key))
        } else {
            (String::new(), None)
        };
        self.odds
            .resolve_lottery(&member, group.as_deref(), lottery)
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
                amount: conv.total_stake,
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
                    ui.add(
                        egui::TextEdit::singleline(&mut self.bill_export_dir)
                            .desired_width(320.0)
                            .hint_text("文件夹路径"),
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
                        hk: 0.0,
                        old: 0.0,
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
                                    "{} · {} 位投注人 · {} 批",
                                    local.date,
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
                                RichText::new(format!("日期 {}", local.date))
                                    .size(11.0)
                                    .color(MUTED),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(format!(
                                    "投注人 {} 位",
                                    local.bettors.len()
                                ))
                                .size(11.0)
                                .color(MUTED),
                            );
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(format!("总码 {}", local.total_code))
                                    .size(11.0)
                                    .strong()
                                    .color(BILL_TEXT),
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
                        ui.add(
                            egui::TextEdit::multiline(&mut self.reconcile_pasted)
                                .desired_width(f32::INFINITY)
                                .desired_rows(8)
                                .hint_text(
                                    "把对方发来的对账文本整段粘到这里（不用改一个字）",
                                ),
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
                                    "粘上对方的文本后这里会立刻出结果：先看总码，不一致再往下看是哪个投注人、第几批。",
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
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new("总码").size(11.5).color(BILL_TEXT),
                                    );
                                    ui.label(
                                        RichText::new(&d.total_local)
                                            .size(11.5)
                                            .monospace()
                                            .color(BILL_TEXT),
                                    );
                                    ui.label(
                                        RichText::new(if d.total_equal {
                                            "="
                                        } else {
                                            "≠"
                                        })
                                        .size(13.0)
                                        .strong()
                                        .color(if d.total_equal {
                                            MONEY_GREEN
                                        } else {
                                            MONEY_RED
                                        }),
                                    );
                                    ui.label(
                                        RichText::new(&d.total_remote)
                                            .size(11.5)
                                            .monospace()
                                            .color(BILL_TEXT),
                                    );
                                    ui.label(
                                        RichText::new(if d.total_equal {
                                            "一致"
                                        } else {
                                            "不一致"
                                        })
                                        .size(11.0)
                                        .color(if d.total_equal {
                                            MONEY_GREEN
                                        } else {
                                            MONEY_RED
                                        }),
                                    );
                                });
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

        let draw_text = draw_summary(&self.draws);
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut category_view = self.ledger_category;

        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.set_min_height(ui.available_height());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("bill_scroll")
                    .show(ui, |ui| {
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
                        ui.add_space(6.0);
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
                        ui.add(
                            egui::Label::new(
                                RichText::new(draw_text.clone())
                                    .size(11.0)
                                    .color(if self.draws.any_ready() {
                                        MONEY_BLUE
                                    } else {
                                        MUTED
                                    }),
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
                            bill_block_ui(ui, block, category_view, bi);
                            ui.add_space(18.0);
                        }
                    });
            });
    }

    // ---------------- 风险页面 ----------------

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
            for bet in conv.bets {
                out.push(crate::risk::BetRow::new(bet, &row.time));
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
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let color = if self.draws.any_ready() {
                            MONEY_BLUE
                        } else {
                            MUTED
                        };
                        ui.label(RichText::new(draw_summary(&self.draws)).size(11.5).color(color));
                    });
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
    fn risk_bet_ui(
        &mut self,
        ui: &mut egui::Ui,
        list: crate::risk::PanelList,
        bets: &[crate::risk::BetRow],
        odds_group: Option<&str>,
    ) {
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

        let columns = crate::risk::panel(
            list,
            bets,
            &self.draws,
            &self.odds,
            odds_group,
            self.risk_lian_size,
        );
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
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.risk_xm_search)
                        .desired_width(130.0)
                        .hint_text("搜索组合")
                        .font(egui::TextStyle::Small),
                );
                if resp.changed() {
                    self.risk_xm_page = 1;
                }
                if secondary_small_button(ui, "概率").clicked() {
                    self.risk_prob_open = true;
                }
            });
        });
        ui.add_space(8.0);

        let all_rows =
            crate::risk::xiaoma(bets, &self.draws, &self.odds, odds_group, mul_sep);
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
            ui.add(
                egui::TextEdit::singleline(&mut self.risk_xm_jump)
                    .desired_width(48.0)
                    .font(font.clone())
                    .hint_text(RichText::new(page.to_string()).size(11.5)),
            );
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

    /// 左侧竖排成员 Tab：全部分组 = 所有联系人 / 群成员；选定某分组 = 该分组成员。
    fn member_rail(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let filter = self.order_filter.clone();
        let mut contacts: BTreeSet<String> = BTreeSet::new();
        let mut members: BTreeSet<(String, String)> = BTreeSet::new();
        match &filter {
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

        let title = match &filter {
            Some(name) => format!("「{name}」"),
            None => "全部成员".to_string(),
        };
        egui::Frame::new()
            .fill(RAIL_BG)
            // 全部成员面板左右留白固定 2px。
            .inner_margin(Margin::symmetric(2, 6))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(10.5).strong().color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        count_pill(ui, keys.len());
                    });
                });
                ui.add_space(4.0);
                hairline(ui);
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .id_salt("member_rail_scroll")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for key in &keys {
                            let label = truncate_display(&name_of(key), 4);
                            let selected = self.member_focus.as_ref() == Some(key);
                            if rail_item(ui, &label, selected) {
                                if selected && self.selected_order_row.is_some() {
                                    // 已选中成员但正在看注单详情：回到该成员的注单消息。
                                    self.selected_order_row = None;
                                    self.selected_order_key = None;
                                    self.converted = None;
                                    self.edit_text.clear();
                                } else if selected {
                                    // 再点一次：取消成员筛选，注单列表恢复全部。
                                    self.member_focus = None;
                                    self.selected_order_row = None;
                                    self.selected_order_key = None;
                                    self.converted = None;
                                    self.edit_text.clear();
                                } else {
                                    self.member_focus = Some(key.clone());
                                    self.selected_order_row = None;
                                    self.selected_order_key = None;
                                    self.converted = None;
                                    self.edit_text.clear();
                                }
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
                    let (left_w, mid_w, reason_w) = if show_reasons {
                        let left_w = 220.0;
                        let reason_w = (total_w * 0.30).max(110.0);
                        // 原文与明细间隔 3px；明细与原因栏之间仍是分隔线 + 10px。
                        let mid_w = (total_w - left_w - reason_w - 14.0).max(180.0);
                        (left_w, mid_w, reason_w)
                    } else {
                        let left_w = 220.0;
                        let mid_w = (total_w - left_w - 3.0).max(160.0);
                        (left_w, mid_w, 0.0)
                    };
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
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(3.0);
                                        let resp = ui
                                            .add(
                                                egui::TextEdit::singleline(&mut self.edit_time)
                                                    .desired_width(68.0)
                                                    .font(egui::TextStyle::Small)
                                                    .hint_text("时间"),
                                            )
                                            .on_hover_text("可修改这条注单的时间");
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
                                    if let Some(out) = &self.converted {
                                        ui.label(
                                            RichText::new(format!("{} 注", out.bets.len()))
                                                .size(10.5)
                                                .color(SUB),
                                        );
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(3.0);
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
                                ui.add_space(4.0);
                                let original_box = egui::Frame::new()
                                    .fill(STRIPE)
                                    .stroke(Stroke::new(1.0, BORDER))
                                    // 原文面板：左右各 2px，文本框不做圆角。
                                    .corner_radius(CornerRadius::ZERO)
                                    .inner_margin(Margin::symmetric(2, 8))
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
                                                            .font(egui::FontId::proportional(13.5))
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
                                // 列宽按内容自适应；窗口再窄时在底部出现水平滚动条。
                                let items: &[BetItem] = converted
                                    .as_ref()
                                    .map(|c| c.bets.as_slice())
                                    .unwrap_or(&[]);
                                let (table_w, widths) =
                                    detail_widths(ui, ui.available_width(), items, &mul_sep);
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
                        if show_reasons {
                            separator_line(ui, body_h);
                            ui.add_space(10.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(reason_w, body_h),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_min_size(Vec2::new(reason_w, body_h));
                                    let failed = status == OrderStatus::Failed;
                                    let color = if failed { RED } else { BLUE_FG };
                                    section_label(
                                        ui,
                                        if failed { "失败原因" } else { "存疑原因" },
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .id_salt("doubt_reason_scroll")
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
                                                ui.add_space(6.0);
                                            }
                                        });
                                },
                            );
                        }
                    });
                } else if let Some(key) = &self.member_focus {
                    let mut rows: Vec<&MsgRow> = self
                        .orders
                        .iter()
                        .filter(|r| key.matches_row(r))
                        .collect();
                    if let Some(name) = &self.order_filter {
                        if let Some(g) = self.groups.iter().find(|g| &g.name == name) {
                            rows.retain(|r| g.matches(r));
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("成员消息").size(12.5).strong().color(TEXT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            count_pill(ui, rows.len());
                        });
                    });
                    ui.add_space(4.0);
                    hairline(ui);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .id_salt("member_msg_scroll")
                        .show(ui, |ui| {
                            if rows.is_empty() {
                                ui.add_space(24.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("该成员当前没有注单消息")
                                            .size(12.0)
                                            .color(SUB),
                                    );
                                });
                            } else {
                                for (i, r) in rows.iter().enumerate() {
                                    if i > 0 {
                                        hairline(ui);
                                    }
                                    ui.add_space(9.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&r.time)
                                                .size(10.5)
                                                .monospace()
                                                .color(SUB),
                                        );
                                        ui.add_space(10.0);
                                        ui.label(RichText::new(&r.kind).size(10.5).color(SUB));
                                    });
                                    ui.add_space(3.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&r.content).size(13.0).color(TEXT),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                    ui.add_space(9.0);
                                }
                            }
                        });
                } else {
                    empty_detail(
                        ui,
                        "未选择注单",
                        "点击左侧成员查看其全部消息，或点击一条注单查看原文与明细。",
                    );
                }
            });
    }

    fn orders_panel(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let group_names: Vec<String> = self.groups.iter().map(|g| g.name.clone()).collect();
        egui::Frame::new()
            .fill(CARD)
            // 注单消息面板左右留白固定 2px。
            .inner_margin(Margin::symmetric(2, 6))
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
                        let count = if visible.len() == self.orders.len() {
                            format!("{} 条", visible.len())
                        } else {
                            format!("{} / {} 条", visible.len(), self.orders.len())
                        };
                        ui.label(RichText::new(count).size(11.0).color(SUB));
                    });
                });
                ui.add_space(4.0);

                let mut nav: Option<isize> = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.spacing_mut().button_padding.x = 6.0;
                    if !group_names.is_empty() {
                        let selected = self
                            .order_filter
                            .clone()
                            .unwrap_or_else(|| "全部分组".to_string());
                        egui::ComboBox::from_id_salt("order_group_filter")
                            .width(96.0)
                            .selected_text(RichText::new(selected).size(11.5))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.order_filter, None, "全部分组");
                                for name in &group_names {
                                    ui.selectable_value(
                                        &mut self.order_filter,
                                        Some(name.clone()),
                                        name,
                                    );
                                }
                            });
                    }
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
                });
                if let Some(dir) = nav {
                    let visible = self.visible_order_indices();
                    self.select_adjacent_order(&visible, dir);
                }
                ui.add_space(4.0);
                hairline(ui);

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
                        ui.add(edit);
                    });
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("提示：点击整行即可选择；群聊点击左侧箭头展开。若同时勾选群聊和成员，以成员选择为准；勾选后可「＋ 新建分组」")
                .size(11.0)
                .color(SUB),
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
                                        .add_enabled(has_sel, tiny_button("加入当前分组", GREEN))
                                        .on_disabled_hover_text("先勾选联系人 / 群成员（整群选择不会加入分组）")
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
                let resp = ui.add(edit);
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
                let resp = ui.add(edit);
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
                        "移除分组内的联系人 / 群成员；只影响分组本身，不影响会员选择和已导入的注单。",
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
                // 同步取消树形结构里的勾选。
                self.selected_contacts.remove(&u);
            }
        }
        if let Some(k) = remove_member {
            if self.groups[idx].members.remove(&k) {
                changed = true;
                self.selected_members.remove(&k);
                self.expanded.insert(k.0.clone());
            }
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
                                ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .hint_text(g.placeholder)
                                        .desired_width(220.0),
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
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.keyword_variant)
                                        .hint_text("生僻字")
                                        .desired_width(80.0),
                                );
                                ui.label(RichText::new("→").size(12.0).color(SUB));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.keyword_correct)
                                        .hint_text("正确生肖")
                                        .desired_width(80.0),
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
                    ui.add(
                        egui::TextEdit::singleline(&mut self.replacement_from)
                            .hint_text("原词，如：复试")
                            .desired_width(150.0),
                    );
                    ui.label(RichText::new("→").size(12.0).color(SUB));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.replacement_to)
                            .hint_text("替换为，如：复式")
                            .desired_width(150.0),
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
                ui.label(
                    RichText::new("替换只影响转换识别，不会修改原文。")
                        .size(10.5)
                        .color(SUB),
                );
            });

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
        let member_names: Vec<String> = self.odds_draft.members.keys().cloned().collect();
        let mut candidates: Vec<String> = self
            .orders
            .iter()
            .map(|r| r.nick.clone())
            .filter(|n| !n.is_empty())
            .collect();
        candidates.sort();
        candidates.dedup();
        candidates.retain(|n| !self.odds_draft.members.contains_key(n));

        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let mut add_member: Option<String> = None;
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
                        OddsScope::Member(name) => format!("会员：{name}"),
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
                            for name in &member_names {
                                ui.selectable_value(
                                    &mut self.odds_scope,
                                    OddsScope::Member(name.clone()),
                                    format!("会员：{name}"),
                                );
                            }
                        });
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.odds_new_member)
                            .hint_text("会员昵称")
                            .desired_width(150.0),
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
                        add_member = Some(name);
                    }
                    if !candidates.is_empty() {
                        egui::ComboBox::from_id_salt("odds_member_pick")
                            .selected_text(RichText::new("从注单选择会员…").size(11.5))
                            .show_ui(ui, |ui| {
                                for name in &candidates {
                                    if ui.selectable_label(false, name).clicked() {
                                        add_member = Some(name.clone());
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
                        RichText::new("会员覆盖优先于分组，只影响这个昵称的注单。")
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

        if let Some(name) = add_member {
            let group = self
                .orders
                .iter()
                .find(|r| r.nick == name)
                .and_then(|r| self.group_with_odds_for(&r.chat, &r.sender));
            self.odds_draft.ensure_member(&name, group.as_deref());
            self.odds_scope = OddsScope::Member(name);
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
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.single_raw)
                        .desired_width(f32::INFINITY)
                        .desired_rows(7)
                        .hint_text("粘贴或输入一条注单原文，例如：三中三06.26.44五十"),
                );
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
                self.selected_members.insert(key);
            }
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
                            ui.add(
                                egui_extras::DatePickerButton::new(&mut self.import_date)
                                    .id_salt("import_date")
                                    .format("%Y-%m-%d"),
                            );
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

    fn perform_clear(&mut self) {
        self.orders = clear_orders(
            std::mem::take(&mut self.orders),
            &self.clear_groups,
            &self.clear_members,
        );
        self.order_keys = self
            .orders
            .iter()
            .map(|r| (r.sort, r.chat.clone(), r.nick.clone(), r.content.clone()))
            .collect();
        let live: HashSet<EditKey> = self.orders.iter().map(edit_key).collect();
        self.auto_converted.retain(|k, _| live.contains(k));
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
                .size(11.5)
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
        self.poll();
        let ctx = ui.ctx().clone();
        self.poll_import();
        self.poll_monitor();

        egui::Panel::top("wv_header")
            .frame(
                egui::Frame::new()
                    .fill(TOOLBAR_BG)
                    .inner_margin(Margin::symmetric(14, 3))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x46, 0x4D, 0x54))),
            )
            .show(ui, |ui| self.top_bar(ui, &ctx));

        egui::Panel::bottom("wv_status")
            .frame(
                egui::Frame::new()
                    .fill(CARD)
                    .inner_margin(Margin::symmetric(18, 6))
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
            });

        // 独立窗口：引用类型消息列表。
        self.reference_window(&ctx);

        if matches!(self.state, State::Loading)
            || matches!(self.orders_state, OrdersState::Loading(_))
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.monitor_rx.is_some() {
            // 监控期间定期唤醒：窗口最小化 / 空闲时也要能轮询到新引用并弹窗。
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

// ---------------- 小组件 ----------------

/// 清空规则：
/// - 没有勾选任何会员 → 清空全部；
/// - 勾选了群/会话 → 清空该会话的全部行；
/// - 勾选了群成员 → 只清空该成员在该群的行。
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
        check_draw_row, clear_orders, find_quoted_image_index, group_filters,
        parse_draw_paste, quote_match_score, source_from_groups,
    };
    use crate::model::{MemberGroup, MsgRow, QuoteRef};
    use eframe::egui;
    use std::collections::HashSet;

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

/// 顶部深色工具栏的按钮：比底色浅一档，白字。
fn top_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    top_button_enabled(ui, text, width, true)
}

fn top_button_enabled(
    ui: &mut egui::Ui,
    text: &str,
    width: f32,
    enabled: bool,
) -> egui::Response {
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = Vec2::new(9.0, 2.0);
    style.visuals.widgets.noninteractive.weak_bg_fill = TOOLBAR_BTN_OFF;
    style.visuals.widgets.noninteractive.bg_fill = TOOLBAR_BTN_OFF;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.noninteractive.fg_stroke =
        Stroke::new(1.0, Color32::from_rgb(0xC3, 0xC8, 0xCD));
    ui.scope(|ui| {
        ui.set_style(style);
        styled_button_enabled(
            ui,
            RichText::new(text)
                .size(11.5)
                .color(if enabled {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(0xC3, 0xC8, 0xCD)
                }),
            enabled,
            TOOLBAR_BTN,
            TOOLBAR_BTN_HOVER,
            TOOLBAR_BTN_ACTIVE,
            Stroke::NONE,
            Stroke::new(1.0, TOOLBAR_BTN_HOVER),
            CornerRadius::same(6),
            Vec2::new(width, 22.0),
        )
    })
    .inner
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
    let bg = if is_info && selected {
        GOLD_BG
    } else if selected {
        GREEN_TINT
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
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
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

/// 明细表六列宽度：彩种 / 玩法 / 内容 / 注数 / 单注 / 小计。
/// 明细表列宽：每一列按「表头 + 该列所有内容」的最大宽度自适应，
/// 单元格左右各 3px 内边距；剩余宽度全部给内容列。
fn detail_widths(
    ui: &egui::Ui,
    avail: f32,
    items: &[BetItem],
    mul_sep: &str,
) -> (f32, [f32; 6]) {
    let header_font = FontId::proportional(12.5);
    let body_font = FontId::proportional(13.5);
    let measure = |text: &str, font: &FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    };
    let headers = ["彩种", "玩法", "内容", "注数", "单注", "小计"];
    let mut inner = [0.0f32; 6];
    for (i, h) in headers.iter().enumerate() {
        inner[i] = measure(h, &header_font);
    }
    for item in items {
        inner[0] = inner[0].max(measure(&item.lottery, &body_font));
        inner[1] = inner[1].max(measure(&item.play, &body_font));
        inner[2] = inner[2].max(detail_content_width(ui, &item.content, mul_sep));
        inner[3] = inner[3].max(measure(&convert::fmt_amount(item.bets), &body_font));
        inner[4] = inner[4].max(measure(&stake_amount_text(&item.stake), &body_font));
        inner[5] = inner[5].max(measure(&fmt_money(item.amount), &body_font));
    }
    // 左右各 3px；+1 兜住字体度量的亚像素误差。
    let pad = 6.0 + 1.0;
    let mut widths = [0.0f32; 6];
    for (i, w) in widths.iter_mut().enumerate() {
        *w = (inner[i] + pad).max(24.0);
    }
    let total: f32 = widths.iter().sum();
    let table_w = avail.max(total).max(380.0);
    // 窗口比内容宽时，多出来的宽度给内容列，表格仍然铺满。
    widths[2] += (table_w - total).max(0.0);
    (table_w, widths)
}

/// 内容列按方块实际宽度测量：方块左右各 3px 内边距 + 左右各 1px 描边，
/// 方块间距 3px。这里必须和 `detail_token_box` 的实际占宽一致，
/// 否则内容列会比内容本身窄，尾部被裁掉。
fn detail_content_width(ui: &egui::Ui, content: &str, mul_sep: &str) -> f32 {
    let mut w = 0.0;
    let mut n = 0usize;
    for part in detail_content_parts(content, mul_sep) {
        let piece = match part {
            DetailContentPart::Token(t) => {
                let color = detail_token_color(&t);
                ui.painter()
                    .layout_no_wrap(t, FontId::proportional(12.5), color)
                    .size()
                    .x
                    + 8.0
            }
            DetailContentPart::Sep(s) => {
                let label = if s.eq_ignore_ascii_case("t") {
                    "拖".to_string()
                } else {
                    s
                };
                ui.painter()
                    .layout_no_wrap(
                        label,
                        FontId::proportional(11.0),
                        Color32::PLACEHOLDER,
                    )
                    .size()
                    .x
            }
        };
        w += piece;
        n += 1;
    }
    if n > 1 {
        w += 3.0 * (n - 1) as f32;
    }
    w
}

fn detail_table_header(ui: &mut egui::Ui, widths: &[f32; 6]) {
    let total: f32 = widths.iter().sum();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 30.0), Sense::hover());
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
            FontId::proportional(12.5),
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
    // 行高固定 34（比原来的 ~43 低约 10px；再矮会切到加粗字的上下沿）。
    let row_h = 34.0;
    let (rect, _resp) =
        ui.allocate_exact_size(Vec2::new(total, row_h), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = 0.0;
    detail_text_cell(&mut child, widths[0], &item.lottery, TEXT, 13.5, true, row_h);
    detail_text_cell(&mut child, widths[1], &item.play, TEXT, 13.5, true, row_h);
    detail_content_cell(&mut child, widths[2], &item.content, mul_sep, row_h);
    detail_text_cell(
        &mut child,
        widths[3],
        &convert::fmt_amount(item.bets),
        TEXT,
        13.5,
        true,
        row_h,
    );
    detail_text_cell(
        &mut child,
        widths[4],
        &stake_amount_text(&item.stake),
        MONEY_BLUE,
        13.5,
        true,
        row_h,
    );
    detail_text_cell(
        &mut child,
        widths[5],
        &fmt_money(item.amount),
        MONEY_RED,
        13.5,
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
) {
    let inner_w = (width - 6.0).max(10.0);
    ui.allocate_ui_with_layout(
        Vec2::new(width, row_h),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            egui::Frame::new()
                .inner_margin(Margin::symmetric(3, 0))
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        for (i, part) in
                            detail_content_parts(content, mul_sep).into_iter().enumerate()
                        {
                            if i > 0 {
                                ui.add_space(3.0);
                            }
                            match part {
                                DetailContentPart::Token(t) => detail_token_box(ui, &t),
                                DetailContentPart::Sep(s) => {
                                    let label = if s.eq_ignore_ascii_case("t") {
                                        "拖".to_string()
                                    } else {
                                        s
                                    };
                                    ui.label(
                                        RichText::new(label).size(11.0).color(MUTED),
                                    );
                                }
                            }
                        }
                    });
                });
        },
    );
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
            for tok in detail_split_tokens(group) {
                parts.push(DetailContentPart::Token(tok));
            }
        }
    } else {
        for tok in detail_split_tokens(content) {
            parts.push(DetailContentPart::Token(tok));
        }
    }
    parts
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
    egui::Frame::new()
        .fill(Color32::from_rgb(0xF2, 0xF6, 0xFC))
        .stroke(Stroke::new(1.0, Color32::from_rgb(0xC9, 0xD6, 0xE6)))
        .corner_radius(CornerRadius::same(3))
        // 内容方块上下不留额外空隙，行高才能压低。
        .inner_margin(Margin::symmetric(3, 0))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(token).size(12.5).color(color))
                    .extend()
                    .selectable(false),
            );
        });
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

fn draw_summary(draws: &draw::Draws) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (name, r) in [
        ("澳", &draws.macau),
        ("港", &draws.hongkong),
        ("老", &draws.old_macau),
    ] {
        if r.ready() {
            let nums: Vec<String> = r.draw.iter().map(|n| format!("{n:02}")).collect();
            parts.push(format!(
                "{name} {} 特 {:02}",
                nums.join(" "),
                r.special.unwrap_or(0)
            ));
        }
    }
    if parts.is_empty() {
        "尚未录入开奖".to_string()
    } else {
        format!("开奖：{}", parts.join(" · "))
    }
}

fn fmt_money(v: f64) -> String {
    convert::fmt_amount(v)
}

struct BillTableRow {
    no: usize,
    amount: f64,
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
                egui::Grid::new(format!("rc_batch_{}", b.name))
                    .num_columns(3)
                    .spacing([18.0, 3.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("#").size(10.5).color(MUTED));
                        ui.label(RichText::new("本机").size(10.5).color(MUTED));
                        ui.label(RichText::new("对方").size(10.5).color(MUTED));
                        ui.end_row();
                        for row in &b.batches {
                            ui.label(
                                RichText::new(row.no.to_string())
                                    .size(10.5)
                                    .color(CELL_NO),
                            );
                            ui.label(RichText::new(&row.local).size(10.5).color(BILL_TEXT));
                            ui.label(
                                RichText::new(&row.remote).size(10.5).color(BILL_TEXT),
                            );
                            ui.end_row();
                        }
                    });
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
                ui.label(RichText::new(&row.content).size(12.5).color(TEXT));
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
                    if ui.add(tiny_button("复制引用", SUB)).clicked() {
                        ui.ctx().copy_text(q.content.clone());
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

            ui.horizontal(|ui| {
                if ui.add(tiny_button("复制本条", SUB)).clicked() {
                    ui.ctx().copy_text(row.content.clone());
                }
            });
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
) -> egui::Rect {
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
                                        amount: slice.amount,
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
                                        let w = draw_bill_table(
                                            ui,
                                            chunk,
                                            None,
                                            Some(cat),
                                            0,
                                        );
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
                                        amount: r.amount,
                                        win: r.win,
                                        breakdown: sorted_breakdown(&r.breakdown),
                                    })
                                    .collect();
                                let amount: f64 = chunk.iter().map(|r| r.amount).sum();
                                let special: f64 =
                                    chunk.iter().map(|r| r.special_win_stake).sum();
                                draw_bill_table(
                                    ui,
                                    &rows,
                                    Some((amount, special)),
                                    None,
                                    if multi_col { 30 - chunk.len() } else { 0 },
                                );
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
    resp.response.rect
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
) -> f32 {
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
        amt_w = amt_w.max(text_width(p, &fmt_money(row.amount), &amount_font));
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

    for row in rows {
        p.text(
            egui::pos2((x0 + x1) / 2.0, y + row_h / 2.0),
            Align2::CENTER_CENTER,
            row.no.to_string(),
            no_font.clone(),
            CELL_NO,
        );
        draw_number_cell_at(p, x1, x2, y, row_h, row.amount, MONEY_BLUE);
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
    w
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

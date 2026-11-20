//! 账无忧界面：
//! - 主页：顶部按钮（选定会员 / 导入注单），左侧常驻注单消息列表，右侧会员摘要；
//! - 选定会员页：联系人 / 群聊两个 Tab，群聊树形展开成员，全部可勾选；
//! - 导入注单：弹窗设置日期与时段，导入后在所选时段内实时追加新消息。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, NaiveTime, Timelike};
use jiff::civil::Date as JDate;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Sense, Stroke,
    Vec2,
};

use crate::loader::{self, Msg};
use crate::messages::{self, ImportMsg, ImportRequest, MonitorMsg};
use crate::model::{AppData, Avatar, Group, ImportResult, MsgRow, Person};

const GREEN: Color32 = Color32::from_rgb(0x07, 0xC1, 0x60);
const BG: Color32 = Color32::from_rgb(0xF5, 0xF6, 0xF7);
const CARD: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const TEXT: Color32 = Color32::from_rgb(0x1F, 0x23, 0x29);
const SUB: Color32 = Color32::from_rgb(0x8A, 0x90, 0x99);
const HOVER: Color32 = Color32::from_rgb(0xEE, 0xF0, 0xF2);
const BORDER: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEB);
const GOLD_FG: Color32 = Color32::from_rgb(0xA8, 0x6A, 0x00);
const GOLD_BG: Color32 = Color32::from_rgb(0xFF, 0xF1, 0xD0);
const RED: Color32 = Color32::from_rgb(0xC0, 0x39, 0x2B);
const MONITOR_INTERVAL_MS: u64 = 3000;

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk".into(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
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
            break;
        }
    }
    ctx.set_fonts(fonts);
}

pub fn install_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = CARD;
        style.visuals.extreme_bg_color = CARD;
        style.visuals.faint_bg_color = Color32::from_rgb(0xFA, 0xFB, 0xFC);
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = Color32::from_rgb(0xD9, 0xF6, 0xE7);
        style.visuals.selection.stroke = Stroke::new(1.0, GREEN);
        style.visuals.hyperlink_color = GREEN;
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        style.visuals.widgets.inactive.bg_fill = CARD;
        style.visuals.widgets.inactive.weak_bg_fill = CARD;
        style.visuals.widgets.inactive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(0xC6, 0xCB, 0xD2));
        style.visuals.widgets.inactive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(0x6B, 0x72, 0x7A));
        style.visuals.widgets.hovered.bg_fill = HOVER;
        style.visuals.widgets.hovered.weak_bg_fill = HOVER;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, GREEN);
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, GREEN);
        style.visuals.widgets.active.bg_fill = HOVER;
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.text_styles = [
            (egui::TextStyle::Heading, FontId::proportional(20.0)),
            (egui::TextStyle::Body, FontId::proportional(14.5)),
            (egui::TextStyle::Button, FontId::proportional(14.0)),
            (egui::TextStyle::Small, FontId::proportional(12.0)),
            (egui::TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
    });
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Contacts,
    Groups,
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

    // 导入弹窗
    import_date: JDate,
    import_start: NaiveTime,
    import_end: NaiveTime,
    dialog_open: bool,
    dialog_error: Option<String>,

    // 注单列表与实时监控
    orders: Vec<MsgRow>,
    order_keys: HashSet<(i64, String, String)>,
    orders_state: OrdersState,
    last_update: String,
    import_rx: Option<Receiver<ImportMsg>>,

    // 实时监控（选定会员后常驻）
    selection_dirty: bool,
    monitor_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    monitor_rx: Option<Receiver<MonitorMsg>>,
    monitor_status: String,

    // 清空注单弹窗
    clear_open: bool,
    clear_groups: HashSet<String>,
    clear_members: HashSet<(String, String)>,
}

struct ClearGroupNode {
    chat: String,
    name: String,
    members: Vec<(String, String)>,
}

impl ViewerApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        loader::start(ctx.clone(), tx);
        Self {
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
            import_date: jiff::Zoned::now().date(),
            import_start: NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            import_end: NaiveTime::from_hms_opt(21, 30, 0).unwrap(),
            dialog_open: false,
            dialog_error: None,
            orders: Vec::new(),
            order_keys: HashSet::new(),
            orders_state: OrdersState::Idle,
            last_update: String::new(),
            import_rx: None,
            selection_dirty: true,
            monitor_stop: None,
            monitor_rx: None,
            monitor_status: "未启动（未选择会员）".to_string(),
            clear_open: false,
            clear_groups: HashSet::new(),
            clear_members: HashSet::new(),
        }
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
        self.dialog_open = false;
        self.dialog_error = None;
        self.orders.clear();
        self.order_keys.clear();
        self.orders_state = OrdersState::Idle;
        self.last_update.clear();
        self.import_rx = None;
        self.selection_dirty = true;
        self.monitor_status = "未启动（未选择会员）".to_string();
        self.clear_open = false;
        self.clear_groups.clear();
        self.clear_members.clear();
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
                    self.fill_orders(rows);
                    self.orders_state = OrdersState::Ready;
                    self.last_update = now_hms();
                    self.import_rx = None;
                }
                ImportMsg::Failed(e) => {
                    self.orders_state = OrdersState::Failed(e);
                    self.import_rx = None;
                    self.monitor_status = "未启动".to_string();
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
                MonitorMsg::Status(s) => self.monitor_status = s,
                MonitorMsg::Stopped(s) => {
                    self.monitor_status = s;
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
        for r in rows {
            let key = (r.sort, r.nick.clone(), r.content.clone());
            if self.order_keys.insert(key) {
                self.orders.push(r);
            }
        }
        self.orders.sort_by_key(|r| r.sort);
    }

    /// 选定会员后常驻监控：只要还有会员被选中就运行；无选中成员则停止。
    /// 监控只追加「从现在开始」的新消息，历史消息由「导入注单」负责回填。
    fn sync_monitor(&mut self, ctx: &egui::Context, data: &AppData) {
        let has_selection = !self.selected_contacts.is_empty()
            || !self.selected_groups.is_empty()
            || !self.selected_members.is_empty();
        if !has_selection {
            if self.monitor_rx.is_some() || self.monitor_stop.is_some() {
                self.stop_monitor();
            }
            if self.monitor_status != "未启动（未选择会员）" {
                self.monitor_status = "未启动（未选择会员）".to_string();
            }
            self.selection_dirty = false;
            return;
        }

        if self.monitor_rx.is_none() || self.selection_dirty {
            let req = ImportRequest {
                account_dir: data.account_path.clone(),
                workdir: data.workdir.clone(),
                keys: data.keys.clone(),
                contacts: self.selected_contacts.iter().cloned().collect(),
                groups: group_filters(&self.selected_groups, &self.selected_members)
                    .into_iter()
                    .collect(),
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
            self.monitor_status = "实时监控中（每 3 秒）".to_string();
        }
        self.selection_dirty = false;
    }

    fn stop_monitor(&mut self) {
        if let Some(flag) = self.monitor_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.monitor_rx = None;
    }

    // ---------------- 顶部栏 ----------------

    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let account = match &self.state {
            State::Ready(d) => format!(
                "{} · {} 位联系人 · {} 个群聊",
                d.self_name,
                d.contacts.len(),
                d.groups.len()
            ),
            State::Loading => self.status.clone(),
            State::Failed(_) => "数据读取失败".to_string(),
        };
        let n_c = self.selected_contacts.len();
        let n_g = self.selected_groups.len();
        let n_m = self.selected_members.len();

        ui.horizontal(|ui| {
            ui.label(RichText::new("账无忧").size(22.0).strong().color(TEXT));
            ui.label(RichText::new("订单消息助手").size(11.0).color(SUB));
            ui.add_space(16.0);

            match self.page {
                Page::Home => {
                    if outline_button(ui, "选定会员", 100.0).clicked() {
                        self.page = Page::Select;
                    }
                    ui.add_space(8.0);
                    let ready = matches!(self.state, State::Ready(_));
                    let import = egui::Button::new(
                        RichText::new("导入注单").size(14.0).color(Color32::WHITE),
                    )
                    .fill(GREEN)
                    .corner_radius(CornerRadius::same(16))
                    .min_size(Vec2::new(100.0, 34.0));
                    if ui
                        .add_enabled(ready, import)
                        .on_hover_text("选择日期与时间段后导入消息")
                        .clicked()
                    {
                        self.dialog_error = None;
                        self.dialog_open = true;
                        self.clear_open = false;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                    ui.add_space(8.0);
                    let clear = egui::Button::new(
                        RichText::new("清空注单").size(14.0).color(TEXT),
                    )
                    .fill(CARD)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(16))
                    .min_size(Vec2::new(100.0, 34.0));
                    if ui
                        .add_enabled(ready, clear)
                        .on_hover_text("按会员清空注单；不勾选会员则清空全部")
                        .clicked()
                    {
                        self.dialog_open = false;
                        self.clear_open = true;
                        self.clear_groups.clear();
                        self.clear_members.clear();
                    }
                }
                Page::Select => {
                    if outline_button(ui, "←  返回主页", 108.0).clicked() {
                        self.page = Page::Home;
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!(
                            "已选  联系人 {n_c} · 群聊 {n_g} · 成员 {n_m}"
                        ))
                        .size(12.5)
                        .color(SUB),
                    );
                    let clear = egui::Button::new(RichText::new("清空选择").size(12.5).color(SUB))
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(CornerRadius::same(14))
                        .min_size(Vec2::new(72.0, 28.0));
                    if ui.add_enabled(n_c + n_g + n_m > 0, clear).clicked() {
                        self.selected_contacts.clear();
                        self.selected_groups.clear();
                        self.selected_members.clear();
                        self.selection_dirty = true;
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let refresh = egui::Button::new(RichText::new("⟳").size(16.0).color(SUB))
                    .fill(CARD)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius::same(16))
                    .min_size(Vec2::new(34.0, 34.0));
                if ui
                    .add(refresh)
                    .on_hover_text("重新读取微信数据（会清空当前注单）")
                    .clicked()
                {
                    self.refresh(ctx);
                }
                ui.add_space(10.0);
                ui.label(RichText::new(account).size(11.5).color(SUB));
            });
        });
    }

    // ---------------- 主页 ----------------

    fn home_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        let h = ui.available_height();
        let total = ui.available_width();
        let left_w = (total * 0.66).clamp(420.0, (total - 240.0).max(420.0));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(left_w, h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(left_w, h));
                    self.orders_panel(ui);
                },
            );
            ui.add_space(10.0);
            let right_w = (total - left_w - 10.0).max(200.0);
            ui.allocate_ui_with_layout(
                Vec2::new(right_w, h),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(Vec2::new(right_w, h));
                    self.summary_panel(ui, data);
                },
            );
        });
    }

    fn orders_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("注单消息").size(16.0).strong().color(TEXT));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("共 {} 条", self.orders.len()))
                            .size(12.5)
                            .color(SUB),
                    );
                    ui.add_space(6.0);
                    let monitoring = self.monitor_rx.is_some();
                    let color = if monitoring { GREEN } else { SUB };
                    ui.label(RichText::new(&self.monitor_status).size(11.5).color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.orders.is_empty()
                            && ui
                                .button(RichText::new("复制全部").size(12.0))
                                .on_hover_text("复制为制表符分隔，可直接粘贴到表格")
                                .clicked()
                        {
                            let mut text = String::from("类型\t昵称\t内容\t时间\n");
                            for r in &self.orders {
                                text.push_str(&format!(
                                    "{}\t{}\t{}\t{}\n",
                                    r.kind, r.nick, r.content, r.time
                                ));
                            }
                            ui.ctx().copy_text(text);
                        }
                    });
                });
                ui.add_space(2.0);
                ui.separator();

                match &self.orders_state {
                    OrdersState::Idle => {
                        empty_state(ui, "暂无注单消息");
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("点击顶部「导入注单」回填历史；选定会员后新消息会自动追加")
                                    .size(12.0)
                                    .color(SUB),
                            );
                        });
                    }
                    OrdersState::Loading(s) => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(50.0);
                            ui.add(egui::Spinner::new().size(28.0).color(GREEN));
                            ui.add_space(10.0);
                            ui.label(RichText::new(s).size(13.0).color(TEXT));
                        });
                    }
                    OrdersState::Failed(e) => {
                        ui.add_space(10.0);
                        ui.label(RichText::new("读取消息失败").size(14.0).strong().color(RED));
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).size(12.5).color(TEXT));
                    }
                    OrdersState::Ready => {
                        if self.orders.is_empty() {
                            empty_state(ui, "所选时间范围内没有消息");
                        } else {
                            let avail = ui.available_width();
                            let widths = [
                                54.0,
                                130.0,
                                (avail - 54.0 - 130.0 - 54.0 - 30.0).max(120.0),
                                54.0,
                            ];
                            msg_header(ui, &widths);
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .id_salt("orders_scroll")
                                .show_rows(ui, 26.0, self.orders.len(), |ui, range| {
                                    for i in range {
                                        msg_row(ui, &self.orders[i], &widths, i);
                                    }
                                });
                        }
                    }
                }
            });
    }

    fn summary_panel(&mut self, ui: &mut egui::Ui, data: &AppData) {
        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.label(RichText::new("已选定会员").size(15.0).strong().color(TEXT));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "联系人 {} · 群聊 {} · 成员 {}",
                        self.selected_contacts.len(),
                        self.selected_groups.len(),
                        self.selected_members.len()
                    ))
                    .size(12.0)
                    .color(SUB),
                );
                ui.add_space(8.0);

                let mut contact_names: Vec<String> = self
                    .selected_contacts
                    .iter()
                    .filter_map(|u| {
                        data.contacts
                            .iter()
                            .find(|p| &p.username == u)
                            .map(|p| p.display_name.clone())
                    })
                    .collect();
                contact_names.sort();
                let mut group_names: Vec<String> = self
                    .selected_groups
                    .iter()
                    .filter_map(|u| {
                        data.groups
                            .iter()
                            .find(|g| &g.username == u)
                            .map(|g| g.name.clone())
                    })
                    .collect();
                group_names.sort();
                let mut member_names: Vec<String> = self
                    .selected_members
                    .iter()
                    .map(|(g, m)| {
                        let gn = data
                            .groups
                            .iter()
                            .find(|x| &x.username == g)
                            .map(|x| x.name.clone())
                            .unwrap_or_else(|| g.clone());
                        let mn = data
                            .groups
                            .iter()
                            .find(|x| &x.username == g)
                            .and_then(|x| x.members.iter().find(|p| &p.username == m))
                            .map(|p| p.display_name.clone())
                            .unwrap_or_else(|| m.clone());
                        format!("{gn} / {mn}")
                    })
                    .collect();
                member_names.sort();

                if contact_names.is_empty() && group_names.is_empty() && member_names.is_empty() {
                    ui.label(
                        RichText::new("尚未选择。点击顶部「选定会员」勾选联系人、群聊或群成员。")
                            .size(12.0)
                            .color(SUB),
                    );
                } else {
                    name_list(ui, "联系人", &contact_names);
                    name_list(ui, "群聊", &group_names);
                    name_list(ui, "群成员", &member_names);
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(RichText::new("实时监控").size(12.5).strong().color(SUB));
                ui.label(
                    RichText::new(&self.monitor_status)
                        .size(12.0)
                        .color(if self.monitor_rx.is_some() { GREEN } else { SUB }),
                );
                if !self.last_update.is_empty() {
                    ui.label(
                        RichText::new(format!("最后更新 {}", self.last_update))
                            .size(11.5)
                            .color(SUB),
                    );
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new(
                        "选定会员后实时监控常驻；导入用于回填历史，新消息会自动追加。",
                    )
                    .size(11.0)
                    .color(SUB),
                );
            });
    }

    // ---------------- 选定会员页 ----------------

    fn select_ui(&mut self, ui: &mut egui::Ui, data: &AppData) {
        self.select_header(ui, data);
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
            RichText::new("提示：点击整行即可选择；群聊点击左侧箭头展开。若同时勾选群聊和成员，以成员选择为准")
                .size(11.0)
                .color(SUB),
        );
        self.tab = new_tab;
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
        let n_c = self.selected_contacts.len();
        let n_g = self.selected_groups.len();
        let n_m = self.selected_members.len();
        let error = self.dialog_error.clone();

        egui::Window::new("导入注单")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(500.0)
            .open(&mut open)
            .show(ctx, |ui| {
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
                            if ui.small_button("今天").clicked() {
                                self.import_date = jiff::Zoned::now().date();
                            }
                            if ui.small_button("昨天").clicked() {
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
                ui.label(
                    RichText::new(format!(
                        "来源：联系人 {} · 群聊 {} · 成员 {}",
                        n_c, n_g, n_m
                    ))
                    .size(12.0)
                    .color(SUB),
                );
                if let Some(e) = &error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).size(12.0).color(RED));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ok = egui::Button::new(
                            RichText::new("确定导入").size(14.0).color(Color32::WHITE),
                        )
                        .fill(GREEN)
                        .corner_radius(CornerRadius::same(16))
                        .min_size(Vec2::new(96.0, 32.0));
                        if ui.add(ok).clicked() {
                            confirm = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("取消").size(13.0).color(SUB))
                                    .fill(Color32::TRANSPARENT)
                                    .corner_radius(CornerRadius::same(14))
                                    .min_size(Vec2::new(64.0, 30.0)),
                            )
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

        // 成员选择优先：只要在该群勾了具体成员，就只取这些成员；
        // 只有「勾选群聊但没有勾选任何成员」时才取整群。
        let groups = group_filters(&self.selected_groups, &self.selected_members);
        let contacts: Vec<String> = self.selected_contacts.iter().cloned().collect();
        if contacts.is_empty() && groups.is_empty() {
            self.dialog_error = Some("请先在「选定会员」中勾选联系人、群聊或群成员。".into());
            return;
        }

        let name_of = build_name_map(data);

        let req = ImportRequest {
            account_dir: data.account_path.clone(),
            workdir: data.workdir.clone(),
            keys: data.keys.clone(),
            contacts,
            groups: groups.into_iter().collect(),
            name_of,
            start_ts,
            end_ts,
        };

        self.dialog_open = false;
        self.dialog_error = None;
        self.orders.clear();
        self.order_keys.clear();
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
                        let clear = egui::Button::new(
                            RichText::new("清空").size(14.0).color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(0xD9, 0x3A, 0x3A))
                        .corner_radius(CornerRadius::same(16))
                        .min_size(Vec2::new(88.0, 32.0));
                        if ui.add(clear).clicked() {
                            do_clear = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("取消").size(13.0).color(SUB))
                                    .fill(Color32::TRANSPARENT)
                                    .corner_radius(CornerRadius::same(14))
                                    .min_size(Vec2::new(64.0, 30.0)),
                            )
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
            .map(|r| (r.sort, r.nick.clone(), r.content.clone()))
            .collect();
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
            ui.label(
                RichText::new("首次读取会从已登录的微信进程内存中提取数据库密钥")
                    .size(12.0)
                    .color(SUB),
            );
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
                let btn = egui::Button::new(
                    RichText::new("重新读取").size(14.0).color(Color32::WHITE),
                )
                .fill(GREEN)
                .corner_radius(CornerRadius::same(16))
                .min_size(Vec2::new(96.0, 32.0));
                if ui.add(btn).clicked() {
                    retry = true;
                }
            });
        });
        if retry {
            let ctx = ui.ctx().clone();
            self.refresh(&ctx);
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match &self.state {
            State::Ready(d) => {
                let s = |t: String| RichText::new(t).size(11.5).color(SUB);
                ui.label(s(format!("已登录  {}  {}", d.self_name, d.self_wxid)));
                ui.label(s("·".into()));
                ui.label(s(format!(
                    "联系人 {} · 群聊 {} · 成员 {}",
                    d.contacts.len(),
                    d.groups.len(),
                    d.groups.iter().map(|g| g.members.len()).sum::<usize>()
                )));
                ui.label(s("·".into()));
                ui.label(s(format!(
                    "头像 本地 {} / 补拉 {} / 缺失 {}",
                    d.cached_avatars, d.downloaded_avatars, d.missing_avatars
                )));
                ui.label(s("·".into()));
                ui.label(s(format!("密钥 {}", d.key_source)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(s(d.data_dir.clone()))
                        .on_hover_text(format!("账号目录: {}", d.account_dir));
                });
            }
            State::Loading => {
                ui.label(RichText::new(&self.status).size(11.5).color(SUB));
            }
            State::Failed(_) => {
                ui.label(RichText::new("读取失败").size(11.5).color(SUB));
            }
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
                    .fill(CARD)
                    .inner_margin(Margin::symmetric(18, 12))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.top_bar(ui, &ctx));

        egui::Panel::bottom("wv_status")
            .frame(
                egui::Frame::new()
                    .fill(CARD)
                    .inner_margin(Margin::symmetric(18, 6))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(14, 10)),
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
                        }
                        self.sync_monitor(&ctx, data);
                        self.import_dialog(&ctx, data);
                        self.clear_dialog(&ctx, data);
                    }
                }
                self.state = state;
            });

        if matches!(self.state, State::Loading)
            || matches!(self.orders_state, OrdersState::Loading(_))
        {
            ctx.request_repaint_after(Duration::from_millis(100));
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
    use super::{clear_orders, group_filters};
    use crate::model::MsgRow;
    use std::collections::HashSet;

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

    fn row(chat: &str, sender: &str, content: &str, sort: i64) -> MsgRow {
        MsgRow {
            kind: "文字".into(),
            nick: sender.into(),
            content: content.into(),
            time: "20:00".into(),
            sort,
            chat: chat.into(),
            sender: sender.into(),
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

fn outline_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(14.0).color(TEXT))
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(16))
            .min_size(Vec2::new(width, 34.0)),
    )
}

fn name_list(ui: &mut egui::Ui, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(12.0).strong().color(SUB));
    let mut shown = 0;
    for name in items.iter().take(6) {
        ui.label(RichText::new(name).size(12.5).color(TEXT));
        shown += 1;
    }
    if items.len() > shown {
        ui.label(
            RichText::new(format!("等 {} 个", items.len()))
                .size(11.5)
                .color(SUB),
        );
    }
}

fn tab_button(ui: &mut egui::Ui, label: String, selected: bool) -> bool {
    let text = RichText::new(label)
        .size(14.0)
        .color(if selected { Color32::WHITE } else { TEXT });
    let button = egui::Button::new(text)
        .fill(if selected { GREEN } else { CARD })
        .stroke(if selected {
            Stroke::NONE
        } else {
            Stroke::new(1.0, BORDER)
        })
        .corner_radius(CornerRadius::same(17))
        .min_size(Vec2::new(118.0, 34.0));
    ui.add(button).clicked()
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
        ui.add_space(70.0);
        ui.label(RichText::new(text).size(14.0).color(SUB));
    });
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

fn msg_header(ui: &mut egui::Ui, widths: &[f32; 4]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    for (w, t) in widths.iter().zip(["类型", "昵称", "内容", "时间"]) {
        cell(&mut child, *w, t, SUB, true);
    }
}

fn msg_row(ui: &mut egui::Ui, r: &MsgRow, widths: &[f32; 4], index: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    if index % 2 == 1 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::ZERO,
            Color32::from_rgb(0xF8, 0xF9, 0xFA),
        );
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let kind_color = match r.kind.as_str() {
        "图片" => Color32::from_rgb(0x2C, 0x7C, 0xD1),
        "文字" => TEXT,
        _ => SUB,
    };
    cell(&mut child, widths[0], &r.kind, kind_color, false);
    cell(
        &mut child,
        widths[1],
        &truncate_display(&r.nick, 16),
        TEXT,
        false,
    );
    cell(&mut child, widths[2], &r.content, TEXT, false);
    cell(&mut child, widths[3], &r.time, SUB, false);
}

fn cell(ui: &mut egui::Ui, width: f32, text: &str, color: Color32, strong: bool) {
    let mut rich = RichText::new(text).size(13.0).color(color);
    if strong {
        rich = rich.strong();
    }
    ui.allocate_ui_with_layout(
        Vec2::new(width, 22.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.add(egui::Label::new(rich).truncate());
        },
    );
}

fn truncate_display(s: &str, max: usize) -> String {
    let mut it = s.chars();
    let mut out: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        out.push('…');
    }
    out
}

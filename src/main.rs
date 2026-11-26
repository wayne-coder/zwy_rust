#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod avatars;
mod bill_image;
mod cache;
mod clipboard;
mod convert;
mod decrypt;
mod doubt;
mod draw;
mod edits;
mod groups;
mod info_code;
mod keyextract;
mod keywords;
mod loader;
mod locate;
mod messages;
mod model;
mod ocr;
mod odds;
mod reconcile;
mod reference;
mod replacements;
mod revoked;
mod risk;
mod settle;
mod sqlite;
mod times;
mod ui;
mod win_style;
mod xiaoma_dup;
mod xiaoma_merge;
mod xiaoma_offsets;

use eframe::egui;

// 由 build.rs 生成：有 assets/app.ico 或 assets/app.png 时返回窗口图标。
include!(concat!(env!("OUT_DIR"), "/app_icon.rs"));

/// GUI 子系统下 panic 信息看不到：同时写一份到临时目录，便于定位闪退。
fn install_panic_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = std::env::temp_dir().join("WechatViewer");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("panic.log"))
        {
            use std::io::Write;
            let when = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "===== {when} =====");
            let _ = writeln!(f, "{info}");
            if let Some(loc) = info.location() {
                let _ = writeln!(f, "at {}:{}:{}", loc.file(), loc.line(), loc.column());
            }
            let _ = writeln!(f);
        }
        default(info);
    }));
}

fn main() -> eframe::Result {
    install_panic_log();
    let argv: Vec<String> = std::env::args().collect();

    if let Some(i) = argv.iter().position(|a| a == "--import-test") {
        let user = argv.get(i + 1).cloned().unwrap_or_default();
        let result = loader::import_selftest(
            &user,
            argv.get(i + 2).map(String::as_str),
            argv.get(i + 3).map(String::as_str),
            argv.get(i + 4).map(String::as_str),
            argv.get(i + 5).and_then(|s| s.parse::<i64>().ok()),
        );
        loader::cleanup_temp();
        let out = std::env::temp_dir()
            .join("WechatViewer")
            .join("import_test.txt");
        if let Some(dir) = out.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        return match result {
            Ok(s) => {
                let _ = std::fs::write(&out, &s);
                println!("{s}");
                Ok(())
            }
            Err(e) => {
                eprintln!("消息自检失败:\n{e}");
                std::process::exit(1);
            }
        };
    }

    if argv.iter().any(|a| a == "--selftest") {
        let result = loader::selftest();
        loader::cleanup_temp();
        match result {
            Ok(s) => {
                // GUI 子系统下 stdout 不可用，写一份到临时目录，便于命令行/脚本查看。
                let out = std::env::temp_dir().join("WechatViewer").join("selftest.txt");
                if let Some(dir) = out.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&out, &s);
                println!("{s}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("自检失败:\n{e}");
                std::process::exit(1);
            }
        }
    }

    // 导出 OCR 识别字典：把当前关键词设置里的字符 + 数字符号写成一字一行的
    // PaddleOCR 字典文件，供训练专属模型和推理共用。
    if let Some(i) = argv.iter().position(|a| a == "--export-ocr-dict") {
        let path = argv
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| "ppocr_keys.txt".to_string());
        let text = ocr::dict_text(&keywords::load());
        match std::fs::write(&path, &text) {
            Ok(()) => {
                println!("已导出 {} 个字符到 {path}", text.lines().count());
                return Ok(());
            }
            Err(e) => {
                eprintln!("导出字典失败：{e}");
                std::process::exit(1);
            }
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1090.0, 780.0])
        // 最小宽度要能同时放下：成员条 72 + 注单消息 268 + 原文列 260 +
        // 明细表格最小 380（含分隔线 / 边距），同时顶部工具栏的分组按钮
        // 也不能互相遮挡——两者叠加后与默认宽度 1090 基本一致，再窄就会裁掉内容。
        .with_min_inner_size([1090.0, 520.0])
        // 标题栏图标右侧显示软件名与版本号。
        .with_title(concat!("账无忧 v", env!("CARGO_PKG_VERSION")));
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "账无忧",
        options,
        Box::new(|cc| {
            ui::install_fonts(&cc.egui_ctx);
            ui::install_style(&cc.egui_ctx);
            Ok(Box::new(ui::ViewerApp::new(&cc.egui_ctx)) as Box<dyn eframe::App>)
        }),
    )
}

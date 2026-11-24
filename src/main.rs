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
mod odds;
mod reconcile;
mod reference;
mod replacements;
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

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 780.0])
            .with_min_inner_size([760.0, 520.0])
            .with_title("账无忧"),
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

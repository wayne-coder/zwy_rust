#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod avatars;
mod cache;
mod decrypt;
mod keyextract;
mod loader;
mod locate;
mod messages;
mod model;
mod sqlite;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
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

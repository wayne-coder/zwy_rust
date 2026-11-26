//! 构建期资源：把 `assets/app.ico` 嵌进 exe 的图标资源，并生成窗口图标加载代码。
//!
//! 图标是可选的：没有放 `assets/app.ico` 时照常构建，使用程序默认图标。
//!
//! - `assets/app.ico`：资源管理器 / 任务栏看到的 exe 文件图标（建议含
//!   16/32/48/256 多种尺寸）；
//! - `assets/app.png`（或 `assets/logo.png`）：可选，窗口左上角 / 任务栏运行时
//!   图标。没有 PNG 时回退使用 ICO 里的图像。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.png");
    println!("cargo:rerun-if-changed=assets/logo.png");

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let assets = manifest.join("assets");
    let ico = assets.join("app.ico");
    let png = assets.join("app.png");
    let logo = assets.join("logo.png");

    if ico.is_file() {
        embed_exe_icon(&manifest, &ico, &out_dir);
    }

    write_window_icon_loader(&out_dir, vec![png, logo, ico]);
}

/// 用 windres 把 .ico 编译成 COFF 资源对象，再让链接器带进 exe。
fn embed_exe_icon(manifest: &Path, ico: &Path, out_dir: &Path) {
    let rc = out_dir.join("app_icon.rc");
    // RC 文件里用正斜杠，避免反斜杠被当成转义。
    let ico_path = ico.to_string_lossy().replace('\\', "/");
    if fs::write(&rc, format!("1 ICON \"{ico_path}\"\n")).is_err() {
        return;
    }
    let obj = out_dir.join("app_icon.o");
    let mut cmd = Command::new(find_windres(manifest));
    cmd.arg("-O").arg("coff");
    // 自带的 MinGW 只有 windres 没有 gcc，而 windres 默认要调 gcc 预处理；
    // 给一个只把 RC 原样输出的极简预处理器，避免构建时依赖 gcc。
    if let Some(preprocessor) = pass_through_preprocessor(manifest, out_dir) {
        cmd.arg(format!("--preprocessor={preprocessor}"));
    }
    let success = cmd
        .arg("-i")
        .arg(&rc)
        .arg("-o")
        .arg(&obj)
        .status()
        .is_ok_and(|s| s.success());
    if !success {
        println!("cargo:warning=windres 不可用，exe 图标未嵌入（其余功能不受影响）");
        return;
    }
    let obj = obj.to_string_lossy().replace('\\', "/");
    // 只把图标资源链接进主程序，避免测试二进制也带上。
    println!("cargo:rustc-link-arg-bin=wechat_viewer={obj}");
}

/// 生成 windres 的 `--preprocessor`：优先用 rustc 编译一个小程序原样输出 RC；
/// 编译失败时退回自带的 MSYS `cat --`（windres 只读 stdout，不检查退出码）。
fn pass_through_preprocessor(manifest: &Path, out_dir: &Path) -> Option<String> {
    let src = out_dir.join("rc_preprocessor.rs");
    let exe = out_dir.join("rc_preprocessor.exe");
    let source = r#"fn main() {
    let Some(path) = std::env::args_os().skip(1).find(|a| !a.to_string_lossy().starts_with('-')) else {
        return;
    };
    if let Ok(bytes) = std::fs::read(&path) {
        use std::io::Write as _;
        let _ = std::io::stdout().write_all(&bytes);
    }
}
"#;
    if fs::write(&src, source).is_ok() {
        let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let ok = Command::new(rustc)
            .arg("--edition=2021")
            .arg("-O")
            .arg("-o")
            .arg(&exe)
            .arg(&src)
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Some(exe.to_string_lossy().replace('\\', "/"));
        }
    }
    let cat = manifest.join("tools").join("msys64").join("usr").join("bin").join("cat.exe");
    if cat.is_file() {
        return Some(format!("{} --", cat.to_string_lossy().replace('\\', "/")));
    }
    None
}

/// 优先找工程自带的 MinGW（tools/msys64），否则用 PATH 里的 windres。
fn find_windres(manifest: &Path) -> PathBuf {
    let bundled = manifest
        .join("tools")
        .join("msys64")
        .join("mingw64")
        .join("bin")
        .join("windres.exe");
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("windres")
    }
}

/// 生成 `app_icon()`：运行时把 PNG / ICO 解码成 egui 的窗口图标。
fn write_window_icon_loader(out_dir: &Path, candidates: Vec<PathBuf>) {
    let source = match candidates.into_iter().find(|p| p.is_file()) {
        Some(path) => {
            let path = path.to_string_lossy().replace('\\', "/");
            format!(
                r#"pub fn app_icon() -> Option<::eframe::egui::IconData> {{
    let bytes: &[u8] = include_bytes!("{path}");
    let image = ::image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(::eframe::egui::IconData {{
        rgba: image.into_raw(),
        width,
        height,
    }})
}}
"#
            )
        }
        None => "pub fn app_icon() -> Option<::eframe::egui::IconData> { None }\n".to_string(),
    };
    let _ = fs::write(out_dir.join("app_icon.rs"), source);
}

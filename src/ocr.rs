//! 剪贴板截图 → 文字：用 Windows 自带的 OCR 引擎（Windows.Media.Ocr）。
//!
//! 不联网、不引入第三方识别服务：读完剪贴板里的图片（PNG 或 DIB），
//! 转成 BGRA8 的 SoftwareBitmap 交给系统 OCR。识别在后台线程里跑，
//! 结果由界面线程轮询后填进「原文」编辑框。

use std::collections::HashSet;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use windows::core::Interface;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine;
use windows::Win32::System::WinRT::{IMemoryBufferByteAccess, RoInitialize, RO_INIT_MULTITHREADED};

use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

/// CF_DIB：剪贴板里的设备无关位图（PrintScreen / 截图工具常见）。
const CF_DIB: u32 = 8;

struct Bitmap {
    width: u32,
    height: u32,
    /// RGBA8，逐像素 4 字节，alpha 已合成为不透明。
    rgba: Vec<u8>,
}

/// 读取剪贴板图片并识别成文字。必须在后台线程调用（会阻塞）。
pub fn recognize_clipboard() -> Result<String, String> {
    let bitmap = read_clipboard_bitmap()?;
    recognize_bitmap(&bitmap)
}

fn recognize_bitmap(bitmap: &Bitmap) -> Result<String, String> {
    // WinRT 的 OCR 需要先初始化 COM。已经初始化过（例如换了套间模式）时
    // RoInitialize 会返回错误，这里忽略——引擎照样能创建。
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let engine = create_engine()?;
    let software = to_software_bitmap(bitmap)?;
    let op = engine
        .RecognizeAsync(&software)
        .map_err(|e| format!("调用 OCR 引擎失败：{e}"))?;
    let result = block_on(op.into_future()).map_err(|e| format!("识别失败：{e}"))?;
    let text = result
        .Text()
        .map_err(|e| format!("读取识别结果失败：{e}"))?
        .to_string();
    let text = compact_cjk_spaces(&text);
    if text.is_empty() {
        return Err("图片里没有识别到文字".to_string());
    }
    Ok(text)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// 清理 OCR 文本：Windows OCR 会在汉字之间插空格（「三 中 三」），
/// 这里把两侧都是汉字的空格删掉；数字、英文字母之间的空格保留。
fn compact_cjk_spaces(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            while matches!(out.last(), Some(' ') | Some('\t')) {
                out.pop();
            }
            out.push('\n');
            continue;
        }
        if c == ' ' || c == '\t' {
            let prev = out.iter().rev().copied().find(|c| *c != ' ' && *c != '\t');
            let next = chars[i + 1..]
                .iter()
                .copied()
                .find(|c| *c != ' ' && *c != '\t');
            if let (Some(p), Some(n)) = (prev, next) {
                if is_cjk(p) && is_cjk(n) {
                    continue;
                }
            }
            // 行首 / 连续空白统一成一个空格。
            if matches!(out.last(), None | Some(' ') | Some('\n')) {
                continue;
            }
        }
        out.push(c);
    }
    let text: String = out.into_iter().collect();
    // 去掉尾随空白与多余的空行。
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

// ── 词库 ─────────────────────────────────────────────────────────────────

/// 业务词库：关键词设置里的全部词 + 生肖 + 地区 / 金额 / 组合标记。
/// 识别纠错和训练字典都以它为准，用户改关键词后自动跟着变。
pub fn vocabulary(settings: &crate::keywords::KeywordSettings) -> HashSet<String> {
    let mut vocab: HashSet<String> = HashSet::new();
    for &field in crate::keywords::ALL_FIELDS {
        for word in settings.get(field) {
            let word = word.trim();
            if !word.is_empty() {
                vocab.insert(word.to_string());
            }
        }
    }
    for zodiac in crate::settle::zodiac_names() {
        vocab.insert(zodiac.to_string());
    }
    for word in [
        "澳门", "香港", "老澳门", "特码", "平码", "平特", "生肖", "尾数", "头数", "波色",
        "红波", "蓝波", "绿波", "合数", "各", "包", "元", "注", "倍", "中", "特", "平",
        "拖", "尾", "头", "肖", "连", "碰", "组", "数", "红", "蓝", "绿", "大", "小",
        "单", "双",
    ] {
        vocab.insert(word.to_string());
    }
    vocab
}

/// 生成 PaddleOCR 识别字典（每行一个字符）。字符集 = 当前词库 + 数字 + 常用符号。
/// 训练和推理必须使用同一份字典。
pub fn dict_text(settings: &crate::keywords::KeywordSettings) -> String {
    let mut chars: std::collections::BTreeSet<char> = std::collections::BTreeSet::new();
    for word in vocabulary(settings) {
        for c in word.chars() {
            if !c.is_whitespace() {
                chars.insert(c);
            }
        }
    }
    for c in "0123456789.-/:%¥=×".chars() {
        chars.insert(c);
    }
    let mut out = String::new();
    for c in chars {
        out.push(c);
        out.push('\n');
    }
    out
}

// ── 图像预处理 ───────────────────────────────────────────────────────────

/// 送给 Windows OCR 之前先把图片整理成「浅底深字、够大、够清楚」：
/// 灰度 → 深底反色 → 2%/98% 对比度拉伸 → 小图放大 → 轻度锐化。
fn preprocess(img: Bitmap) -> Bitmap {
    let (w, h) = (img.width, img.height);
    let n = w as usize * h as usize;
    if n == 0 {
        return img;
    }
    let mut gray = Vec::with_capacity(n);
    for p in img.rgba.chunks_exact(4) {
        let v = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        gray.push(v.round().clamp(0.0, 255.0) as u8);
    }
    // 深色背景（夜间模式截图）先反色，OCR 对白底黑字更稳。
    let mean = gray.iter().map(|&v| v as u64).sum::<u64>() / n as u64;
    if mean < 110 {
        for v in &mut gray {
            *v = 255 - *v;
        }
    }
    // 对比度拉伸：忽略两端各 2% 的极端像素，避免单个高光影响整体。
    let mut hist = [0u64; 256];
    for &v in &gray {
        hist[v as usize] += 1;
    }
    let lo = percentile(&hist, n, 0.02);
    let hi = percentile(&hist, n, 0.98);
    if hi as u16 > lo as u16 + 8 {
        let span = (hi - lo) as f32;
        for v in &mut gray {
            *v = (((*v as f32 - lo as f32) * 255.0 / span) as i32).clamp(0, 255) as u8;
        }
    }
    let gray_img = match image::GrayImage::from_raw(w, h, gray) {
        Some(g) => g,
        None => return img,
    };
    // 长边不足 1600 的小图放大到约 1600（最多 3 倍）；大图保持原样。
    let max_dim = w.max(h) as f32;
    let scale = if max_dim < 1600.0 {
        (1600.0 / max_dim).clamp(1.0, 3.0)
    } else {
        1.0
    };
    let scaled = if scale > 1.0 {
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        image::imageops::resize(&gray_img, nw, nh, image::imageops::FilterType::Lanczos3)
    } else {
        gray_img
    };
    // 轻度反锐化：让笔画边缘更清楚，OCR 对小字更敏感。
    let blurred = image::imageops::blur(&scaled, 1.0);
    let (sw, sh) = scaled.dimensions();
    let mut rgba = Vec::with_capacity(sw as usize * sh as usize * 4);
    for (g, b) in scaled.pixels().zip(blurred.pixels()) {
        let v = (g[0] as i32 + (g[0] as i32 - b[0] as i32)).clamp(0, 255) as u8;
        rgba.extend_from_slice(&[v, v, v, 255]);
    }
    Bitmap {
        width: sw,
        height: sh,
        rgba,
    }
}

/// 直方图百分位（0..1）。
fn percentile(hist: &[u64; 256], n: usize, p: f32) -> u8 {
    let target = (n as f32 * p) as u64;
    let mut acc = 0u64;
    for (v, &count) in hist.iter().enumerate() {
        acc += count;
        if acc > target {
            return v as u8;
        }
    }
    255
}

// ── 词库校正 ─────────────────────────────────────────────────────────────

/// 用业务词库修正 OCR 文本，返回（修正后文本, 修正处数）。
///
/// 只做「不会猜错」的三类修正：
/// 1. 全角字母 / 数字 → 半角；
/// 2. 数字串里被认成字母的字符（`1OO` → `100`、`S00` → `500`）；
/// 3. 汉字片段与词库里的玩法 / 生肖 / 标记词只差一个字，且那个字是常见的
///    字形易混字（如 `待码`→`特码`、`三申三`→`三中三`）。
///
/// 有歧义（同时能匹配到两个词）就不改，留给转换器标存疑。
pub fn correct_text(text: &str, vocab: &HashSet<String>) -> (String, usize) {
    let mut changed = 0usize;
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| correct_line(line, vocab, &mut changed))
        .collect();
    (lines.join("\n"), changed)
}

fn correct_line(line: &str, vocab: &HashSet<String>, changed: &mut usize) -> String {
    // 1. 全角 → 半角（只处理 ASCII 区段，中文标点保持不变）。
    let full: Vec<char> = line.chars().map(to_halfwidth).collect();
    // 2. 数字串里的易混字母。
    let mut digit_fixed = String::with_capacity(line.len());
    let mut i = 0;
    while i < full.len() {
        if full[i].is_ascii_alphanumeric() {
            let start = i;
            while i < full.len() && full[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let run = &full[start..i];
            let has_digit = run.iter().any(|c| c.is_ascii_digit());
            let all_fixable = run.iter().all(|c| ascii_digit_fix(*c) != *c);
            let fix = has_digit || (run.len() >= 2 && all_fixable);
            for &orig in run {
                let c = if fix { ascii_digit_fix(orig) } else { orig };
                if c != orig {
                    *changed += 1;
                }
                digit_fixed.push(c);
            }
        } else {
            digit_fixed.push(full[i]);
            i += 1;
        }
    }
    // 3. 汉字片段按词库纠正。
    let chars: Vec<char> = digit_fixed.chars().collect();
    let mut out = String::with_capacity(digit_fixed.len());
    let mut i = 0;
    while i < chars.len() {
        if is_cjk(chars[i]) {
            let start = i;
            while i < chars.len() && is_cjk(chars[i]) {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            out.push_str(&correct_cjk_run(&run, vocab, changed));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 在一个连续汉字片段里按词库从左往右匹配：
/// 先认词库里原样的词（长词优先，避免拆坏「六肖中一」这类词），
/// 再尝试「只差一个易混字」的唯一候选；都对不上就原样保留一个字。
fn correct_cjk_run(run: &str, vocab: &HashSet<String>, changed: &mut usize) -> String {
    let chars: Vec<char> = run.chars().collect();
    let mut out = String::with_capacity(run.len());
    let mut i = 0;
    while i < chars.len() {
        let remaining = chars.len() - i;
        let max_len = remaining.min(6);
        let mut matched = false;
        // 先找词库里完全一致的词（长词优先）。
        for len in (2..=max_len).rev() {
            let window: String = chars[i..i + len].iter().collect();
            if vocab.contains(&window) {
                out.push_str(&window);
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        // 再找唯一「差一个易混字」的词。
        for len in (2..=max_len).rev() {
            let window: String = chars[i..i + len].iter().collect();
            if let Some(fixed) = snap_token(&window, vocab) {
                out.push_str(&fixed);
                *changed += 1;
                i += len;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// 全角 ASCII（！..～）转半角。
fn to_halfwidth(c: char) -> char {
    match c as u32 {
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// 数字上下文里容易认错的字母 → 数字；不认识的字符原样返回。
fn ascii_digit_fix(c: char) -> char {
    match c {
        'O' | 'o' | 'Q' => '0',
        'I' | 'l' => '1',
        'S' | 's' => '5',
        'B' => '8',
        'Z' | 'z' => '2',
        _ => c,
    }
}

/// 字形易混字：OCR 认错的字 → 正确的候选字。
fn confusion_targets(c: char) -> &'static [char] {
    match c {
        '待' | '持' | '侍' => &['特'],
        '申' | '忠' | '钟' | '仲' => &['中'],
        '名' | '冬' => &['各'],
        '兰' | '拦' => &['三'],
        '亓' | '沅' => &['元'],
        '乎' | '苹' => &['平'],
        '玛' => &['码'],
        '往' | '汪' => &['注'],
        '陪' | '培' => &['倍'],
        '屋' => &['尾'],
        '削' | '莦' | '硝' => &['肖'],
        '免' => &['兔'],
        '候' => &['猴'],
        _ => &[],
    }
}

fn is_confusable(from: char, to: char) -> bool {
    confusion_targets(from).contains(&to)
}

/// 把一段汉字与词库做单字差匹配：只有唯一候选、且差的那一个是易混字才替换。
fn snap_token(run: &str, vocab: &HashSet<String>) -> Option<String> {
    let run_chars: Vec<char> = run.chars().collect();
    let n = run_chars.len();
    if !(2..=6).contains(&n) || vocab.contains(run) {
        return None;
    }
    let mut best: Option<&String> = None;
    let mut hits = 0;
    for word in vocab {
        let word_chars: Vec<char> = word.chars().collect();
        if word_chars.len() != n {
            continue;
        }
        let mut diff = None;
        let mut ok = true;
        for k in 0..n {
            if word_chars[k] != run_chars[k] {
                if diff.is_some() {
                    ok = false;
                    break;
                }
                diff = Some(k);
            }
        }
        if !ok {
            continue;
        }
        let Some(k) = diff else { continue };
        if !is_confusable(run_chars[k], word_chars[k]) {
            continue;
        }
        hits += 1;
        if hits > 1 {
            return None; // 有歧义
        }
        best = Some(word);
    }
    best.cloned()
}

// ── 剪贴板图片 ────────────────────────────────────────────────────────────

fn read_clipboard_bitmap() -> Result<Bitmap, String> {
    // 优先 PNG：浏览器、截图工具复制出来的图片一般都有这个格式。
    if let Some(bytes) = clipboard_bytes(register_format("PNG")) {
        if let Ok(img) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png) {
            let img = img.into_rgba8();
            let (w, h) = img.dimensions();
            return Ok(preprocess(Bitmap {
                width: w,
                height: h,
                rgba: normalize_alpha(img.into_raw()),
            }));
        }
    }
    if let Some(bytes) = clipboard_bytes(CF_DIB) {
        if let Some(bmp) = decode_dib(&bytes) {
            return Ok(preprocess(bmp));
        }
    }
    Err("剪贴板里没有图片，请先复制或截图".to_string())
}

fn register_format(name: &str) -> u32 {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe { RegisterClipboardFormatW(wide.as_ptr()) }
}

fn clipboard_bytes(format: u32) -> Option<Vec<u8>> {
    if format == 0 {
        return None;
    }
    unsafe {
        if IsClipboardFormatAvailable(format) == 0 {
            return None;
        }
        // 剪贴板偶尔被别的程序占用，稍等重试几次。
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let handle = GetClipboardData(format);
        let mut out = None;
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u8;
            if !ptr.is_null() {
                let size = GlobalSize(handle);
                // 正常截图也就几 MB；超过 128MB 直接放弃，避免异常数据。
                if size > 0 && size <= 128 * 1024 * 1024 {
                    out = Some(std::slice::from_raw_parts(ptr, size).to_vec());
                }
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        out
    }
}

/// 解析 CF_DIB（BITMAPINFOHEADER + 像素），支持 24 / 32 位。
fn decode_dib(b: &[u8]) -> Option<Bitmap> {
    if b.len() < 40 {
        return None;
    }
    let header = u32::from_le_bytes(b[0..4].try_into().ok()?) as usize;
    if header < 40 || b.len() < header {
        return None;
    }
    let raw_w = i32::from_le_bytes(b[4..8].try_into().ok()?);
    let raw_h = i32::from_le_bytes(b[8..12].try_into().ok()?);
    let planes = u16::from_le_bytes(b[12..14].try_into().ok()?);
    let bpp = u16::from_le_bytes(b[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(b[16..20].try_into().ok()?);
    if raw_w <= 0 || raw_h == 0 || planes != 1 {
        return None;
    }
    if bpp != 24 && bpp != 32 {
        return None;
    }
    // BI_RGB / BI_BITFIELDS；其他压缩方式（JPEG / PNG 内嵌）不支持。
    if compression != 0 && compression != 3 {
        return None;
    }
    let width = raw_w as u32;
    let height = raw_h.unsigned_abs();
    if width > 20_000 || height > 20_000 {
        return None;
    }

    let mut off = header;
    if bpp <= 8 {
        let clr_used = u32::from_le_bytes(b[32..36].try_into().ok()?) as usize;
        let colors = if clr_used != 0 { clr_used } else { 1 << bpp };
        off += colors * 4;
    } else if compression == 3 && header == 40 {
        // BI_BITFIELDS 的 3 个掩码紧跟在 40 字节头部后面。
        off += 12;
    }

    let stride = ((width as usize * bpp as usize + 31) / 32) * 4;
    let need = stride.checked_mul(height as usize)?;
    if b.len() < off.checked_add(need)? {
        return None;
    }
    let top_down = raw_h < 0;
    let bytes_pp = bpp as usize / 8;
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let src_y = if top_down { y } else { height as usize - 1 - y };
        let row = &b[off + src_y * stride..];
        let dst = &mut rgba[y * width as usize * 4..];
        for x in 0..width as usize {
            let px = &row[x * bytes_pp..];
            let (r, g, bl, a) = if bpp == 32 {
                (px[2], px[1], px[0], px[3])
            } else {
                (px[2], px[1], px[0], 255)
            };
            dst[x * 4] = r;
            dst[x * 4 + 1] = g;
            dst[x * 4 + 2] = bl;
            dst[x * 4 + 3] = a;
        }
    }
    Some(Bitmap {
        width,
        height,
        rgba: normalize_alpha(rgba),
    })
}

/// 把透明度合成到白底上并统一成不透明。
///
/// GDI 截图的 32 位 DIB 经常把 alpha 全写成 0，那其实是「不透明」而不是
/// 「全透明」；所以整图都没有有效 alpha 时按不透明处理。
fn normalize_alpha(mut rgba: Vec<u8>) -> Vec<u8> {
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 0);
    if has_alpha {
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * a + 255 * (255 - a)) / 255) as u8;
                }
            }
            p[3] = 255;
        }
    } else {
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    rgba
}

// ── Windows OCR ───────────────────────────────────────────────────────────

fn create_engine() -> Result<OcrEngine, String> {
    let mut last = String::new();
    if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(engine);
    }
    for tag in ["zh-Hans-CN", "zh-Hant-TW", "en-US"] {
        let lang = match windows::Globalization::Language::CreateLanguage(&tag.into()) {
            Ok(l) => l,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        match OcrEngine::TryCreateFromLanguage(&lang) {
            Ok(engine) => return Ok(engine),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "系统没有可用的 OCR 语言包（请在 Windows 设置里安装中文识别语言）{last}"
    ))
}

fn to_software_bitmap(img: &Bitmap) -> Result<SoftwareBitmap, String> {
    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8,
        img.width as i32,
        img.height as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(|e| format!("创建位图失败：{e}"))?;
    let buffer = bitmap
        .LockBuffer(BitmapBufferAccessMode::Write)
        .map_err(|e| format!("锁定位图失败：{e}"))?;
    let plane = buffer
        .GetPlaneDescription(0)
        .map_err(|e| format!("读取位图布局失败：{e}"))?;
    // 字节访问接口挂在 IMemoryBufferReference 上（BitmapBuffer 本身 QI 不到）。
    let reference = buffer
        .CreateReference()
        .map_err(|e| format!("访问位图内存失败：{e}"))?;
    let access: IMemoryBufferByteAccess = reference
        .cast()
        .map_err(|e| format!("访问位图内存失败：{e}"))?;
    let mut data: *mut u8 = std::ptr::null_mut();
    let mut capacity = 0u32;
    unsafe {
        access
            .GetBuffer(&mut data, &mut capacity)
            .map_err(|e| format!("读取位图内存失败：{e}"))?;
    }

    let w = img.width as usize;
    let h = img.height as usize;
    let stride = plane.Stride as usize;
    let start = plane.StartIndex as usize;
    if stride < w * 4 || (capacity as usize) < start + stride * h {
        return Err("位图内存大小异常".to_string());
    }
    unsafe {
        let base = data.add(start);
        for y in 0..h {
            let dst = std::slice::from_raw_parts_mut(base.add(y * stride), w * 4);
            let src = &img.rgba[y * w * 4..(y + 1) * w * 4];
            for x in 0..w {
                let p = &src[x * 4..];
                // RGBA → BGRA（alpha 已是 255，预乘不改变颜色）。
                dst[x * 4] = p[2];
                dst[x * 4 + 1] = p[1];
                dst[x * 4 + 2] = p[0];
                dst[x * 4 + 3] = p[3];
            }
        }
    }
    drop(buffer);
    Ok(bitmap)
}

/// 阻塞等待 WinRT 的异步结果：poll 到 Pending 时挂起线程，完成回调会唤醒。
fn block_on<F: Future>(fut: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut: Pin<Box<F>> = Box::pin(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::{point, Font, FontArc, ScaleFont};

    /// 手动画一张「168 三中三」的位图，不依赖剪贴板。
    fn sample_bitmap() -> Bitmap {
        let (w, h) = (620u32, 180u32);
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        let bytes = [r"C:\Windows\Fonts\Dengb.ttf", r"C:\Windows\Fonts\simhei.ttf"]
            .iter()
            .find_map(|p| std::fs::read(p).ok())
            .expect("需要系统里的中文字体");
        let font = FontArc::try_from_vec(bytes).unwrap();
        let size = 96.0;
        let scaled = font.as_scaled(size);
        let mut pen = 24.0;
        let baseline = 120.0;
        for ch in "168 三中三".chars() {
            let gid = scaled.glyph_id(ch);
            let glyph = gid.with_scale_and_position(size, point(pen, baseline));
            if let Some(outlined) = font.outline_glyph(glyph) {
                let b = outlined.px_bounds();
                outlined.draw(|gx, gy, cov| {
                    let x = b.min.x as i32 + gx as i32;
                    let y = b.min.y as i32 + gy as i32;
                    if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                        let i = ((y as u32 * w + x as u32) * 4) as usize;
                        let v = (255.0 * (1.0 - cov)) as u8;
                        rgba[i] = v;
                        rgba[i + 1] = v;
                        rgba[i + 2] = v;
                    }
                });
            }
            pen += scaled.h_advance(gid);
        }
        Bitmap {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn vocabulary_covers_defaults() {
        let settings = crate::keywords::KeywordSettings::default();
        let vocab = vocabulary(&settings);
        for word in ["三中三", "特码", "平特", "六肖中一", "老澳门"] {
            assert!(vocab.contains(word), "词库缺少 {word}");
        }
        for zodiac in crate::settle::zodiac_names() {
            assert!(vocab.contains(zodiac), "词库缺少生肖 {zodiac}");
        }
    }

    #[test]
    fn dict_text_is_one_char_per_line() {
        let settings = crate::keywords::KeywordSettings::default();
        let dict = dict_text(&settings);
        let lines: Vec<&str> = dict.lines().collect();
        assert!(lines.iter().all(|line| line.chars().count() == 1));
        for c in ['0', '9', '三', '特', '马', '元'] {
            let s = c.to_string();
            assert!(lines.iter().any(|line| *line == s), "字典缺少 {c}");
        }
    }

    #[test]
    fn fixes_digit_and_fullwidth_confusions() {
        let vocab = HashSet::new();
        let (text, changed) = correct_text("三中三各１OO元", &vocab);
        assert_eq!(text, "三中三各100元");
        assert_eq!(changed, 2);
    }

    #[test]
    fn snaps_confusable_char_to_vocab_word() {
        let vocab: HashSet<String> = ["三中三", "三中二", "特码", "平码", "二中二"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(correct_text("待码各100", &vocab).0, "特码各100");
        assert_eq!(correct_text("三申三各100", &vocab).0, "三中三各100");
        // 已经是词库里的词就不动。
        assert_eq!(correct_text("三中二各100", &vocab).0, "三中二各100");
        // 没有易混字时不动。
        let (text, changed) = correct_text("阿强各100", &vocab);
        assert_eq!(text, "阿强各100");
        assert_eq!(changed, 0);
    }

    #[test]
    fn preprocess_outputs_opaque_gray() {
        let img = Bitmap {
            width: 4,
            height: 4,
            rgba: vec![0; 4 * 4 * 4],
        };
        let out = preprocess(img);
        assert!(out.width >= 4 && out.height >= 4);
        assert!(
            out.rgba
                .chunks_exact(4)
                .all(|p| p[3] == 255 && p[0] == p[1] && p[1] == p[2])
        );
    }

    #[test]
    fn removes_spaces_between_cjk() {
        assert_eq!(compact_cjk_spaces("168 三 中 三
10"), "168 三中三
10");
        assert_eq!(compact_cjk_spaces("特 码 100"), "特码 100");
    }

    #[test]
    fn keeps_spaces_between_non_cjk() {
        assert_eq!(compact_cjk_spaces("abc def 三中三"), "abc def 三中三");
        assert_eq!(compact_cjk_spaces("01 02 03"), "01 02 03");
    }

    #[test]
    #[ignore = "需要系统安装 OCR 语言包；手动运行 cargo test ocr -- --ignored"]
    fn recognizes_rendered_text() {
        let text = recognize_bitmap(&preprocess(sample_bitmap())).expect("OCR 应该成功");
        assert!(text.contains("168"), "识别结果：{text:?}");
        assert!(text.contains("三中三"), "识别结果：{text:?}");
    }
}

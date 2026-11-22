//! 读取 Windows 剪贴板文本（详情面板右键菜单的「粘贴」用）。
//!
//! egui 只提供了复制/粘贴事件的输入，没有直接读剪贴板的接口；这里用
//! user32 的剪贴板 API 取一次 CF_UNICODETEXT。

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

const CF_UNICODETEXT: u32 = 13;

pub fn read_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut() as HWND) == 0 {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        let mut out = None;
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let units = GlobalSize(handle) / 2;
                let mut len = 0usize;
                while len < units && *ptr.add(len) != 0 {
                    len += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        out
    }
}

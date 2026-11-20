//! 头像：优先读本地 head_image.db，缺失时用 WinHTTP 从微信 CDN 补拉并解码。

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::model::Avatar;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// 下载一个 https URL（仅用于微信头像 CDN）。
pub fn http_get(url: &str, timeout_ms: u32) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host.is_empty() {
        return None;
    }

    let agent = wide("WechatViewer/1.0");
    let host_w = wide(host);
    let path_w = wide(path);
    let verb = wide("GET");

    unsafe {
        let session = Handle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return None;
        }
        let t = timeout_ms as i32;
        WinHttpSetTimeouts(session.0, t, t, t, t);

        let connect = Handle(WinHttpConnect(
            session.0,
            host_w.as_ptr(),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if connect.0.is_null() {
            return None;
        }

        let request = Handle(WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }

        if WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
            return None;
        }
        if WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 {
            return None;
        }

        let mut status: u32 = 0;
        let mut len: u32 = 4;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        if status != 200 {
            return None;
        }

        let mut data = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read: u32 = 0;
            let ok = WinHttpReadData(
                request.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut read,
            );
            if ok == 0 || read == 0 {
                break;
            }
            data.extend_from_slice(&buf[..read as usize]);
            if data.len() > 10 * 1024 * 1024 {
                break;
            }
        }
        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    }
}

/// 把 JPEG/PNG/WebP/GIF 等原始字节解码成 RGBA。
pub fn decode(bytes: &[u8]) -> Option<Avatar> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    Some(Avatar {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        rgba: rgba.into_raw(),
    })
}

/// 并发下载一组头像；返回 username → 图片字节。
pub fn download_many(
    items: &[(String, String)],
    threads: usize,
    timeout_ms: u32,
) -> Vec<(String, Vec<u8>)> {
    use std::sync::{Arc, Mutex};

    if items.is_empty() {
        return Vec::new();
    }
    let queue = Arc::new(Mutex::new(items.to_vec()));
    let out = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for _ in 0..threads.max(1) {
        let queue = Arc::clone(&queue);
        let out = Arc::clone(&out);
        handles.push(std::thread::spawn(move || loop {
            let item = {
                let mut q = queue.lock().unwrap();
                q.pop()
            };
            let Some((username, url)) = item else {
                break;
            };
            if let Some(bytes) = http_get(&url, timeout_ms) {
                out.lock().unwrap().push((username, bytes));
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    match Arc::try_unwrap(out) {
        Ok(m) => m.into_inner().unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

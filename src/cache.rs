//! 密钥缓存：用 Windows DPAPI 按当前用户加密后落盘。
//!
//! 微信重新登录会更换数据库密钥，因此缓存只在页 1 HMAC 校验通过时才会被使用；
//! 缓存让「微信未运行」时也能查看数据（只要密钥未变）。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

const MAGIC: &[u8] = b"WVK1";

#[derive(Clone, Debug)]
pub struct CachedKey {
    pub account: String,
    pub rel: String,
    pub key: Vec<u8>,
}

fn cache_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("keys.dat")
}

fn protect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        let ok = CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &mut out,
        );
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as *mut core::ffi::c_void);
        Some(v)
    }
}

fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        let ok = CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &mut out,
        );
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as *mut core::ffi::c_void);
        Some(v)
    }
}

pub fn load() -> Vec<CachedKey> {
    let path = cache_path();
    let Ok(raw) = fs::read(&path) else {
        return Vec::new();
    };
    if !raw.starts_with(MAGIC) {
        return Vec::new();
    }
    let Some(plain) = unprotect(&raw[MAGIC.len()..]) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&plain);
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        let (Some(account), Some(rel), Some(hex)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let Some(key) = decode_hex(hex) {
            if key.len() == 32 || key.len() == 48 {
                out.push(CachedKey {
                    account: account.to_string(),
                    rel: rel.to_string(),
                    key,
                });
            }
        }
    }
    out
}

pub fn save(keys: &[CachedKey]) -> bool {
    let mut text = String::new();
    for k in keys {
        text.push_str(&k.account);
        text.push('\t');
        text.push_str(&k.rel);
        text.push('\t');
        for b in &k.key {
            text.push_str(&format!("{b:02x}"));
        }
        text.push('\n');
    }
    let Some(blob) = protect(text.as_bytes()) else {
        return false;
    };
    let path = cache_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let mut file = Vec::with_capacity(MAGIC.len() + blob.len());
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&blob);
    // 先写临时文件再替换，避免中断留下半个缓存。
    let tmp = path.with_extension("dat.tmp");
    if fs::write(&tmp, &file).is_err() {
        return false;
    }
    fs::rename(&tmp, &path).is_ok()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let b = s.as_bytes();
    for pair in b.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

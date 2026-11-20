//! 从 Weixin.exe 内存中提取各数据库的 SQLCipher 密钥。
//!
//! 微信 4.1+ 使用 WCDB 的 `com.Tencent.WCDB.Config.Cipher` 配置对象，
//! 其中保存了一段用固定掩码异或过的文本，文本里是形如 `x'<96 hex>'` 的
//! 「32 字节密钥 + 16 字节 salt」字面量。本模块按同样的结构定位并解出候选密钥，
//! 再由调用方用页 1 HMAC 校验（零误报）。

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};


const CONFIG_CIPHER_NAME: &[u8] = b"com.Tencent.WCDB.Config.Cipher";
const CONFIG_XOR_MASK: [u8; 32] = [
    0xd2, 0xc7, 0x44, 0x24, 0x58, 0x02, 0x00, 0x00, 0x00, 0x48, 0x89, 0x44, 0x24, 0x50, 0x48, 0x8b,
    0x45, 0x00, 0x48, 0x84, 0x4c, 0x24, 0x48, 0x48, 0x89, 0x44, 0x25, 0x40, 0x48, 0x58, 0x4c, 0x24,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Candidate {
    pub key: [u8; 32],
    pub salt: Option<[u8; 16]>,
}

struct Proc(*mut c_void);

impl Proc {
    fn open(pid: u32) -> Option<Self> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if h.is_null() {
            None
        } else {
            Some(Proc(h))
        }
    }

    fn read(&self, addr: usize, size: usize) -> Option<Vec<u8>> {
        if size == 0 {
            return Some(Vec::new());
        }
        let mut buf = vec![0u8; size];
        let mut got: usize = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.0,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                size,
                &mut got,
            )
        };
        if ok == 0 || got == 0 {
            return None;
        }
        buf.truncate(got);
        Some(buf)
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn for_each_region(h: *mut c_void, mut f: impl FnMut(usize, usize)) {
    let mut addr: usize = 0;
    while addr < 0x7fff_ffff_ffff {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let r = unsafe {
            VirtualQueryEx(
                h,
                addr as *const c_void,
                &mut mbi,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if r == 0 {
            break;
        }
        let base = mbi.BaseAddress as usize;
        let size = mbi.RegionSize;
        if size == 0 {
            break;
        }
        let readable = mbi.State == MEM_COMMIT
            && (mbi.Protect & 0xFF) & 0xE6 != 0
            && (mbi.Protect & PAGE_GUARD) == 0
            && size < 0x1000_0000;
        if readable {
            f(base, size);
        }
        let next = base.saturating_add(size);
        if next <= base {
            break;
        }
        addr = next;
    }
}

fn find_bytes(p: &Proc, needle: &[u8]) -> Vec<usize> {
    let finder = memchr::memmem::Finder::new(needle);
    let mut hits = Vec::new();
    for_each_region(p.0, |base, size| {
        if let Some(buf) = p.read(base, size) {
            for i in finder.find_iter(&buf) {
                hits.push(base + i);
            }
        }
    });
    hits
}

/// 单次遍历内存，找出所有「指针 + 长度」结构里指向 needle 的引用。
///
/// 原实现是对每个 needle 各做一次全量内存扫描（字符串出现次数多时会慢到
/// 几十秒）；这里一次扫描全部解决。
fn find_needle_refs(p: &Proc, needle_len: usize, needles: &HashSet<u64>) -> Vec<usize> {
    let mut hits = Vec::new();
    if needles.is_empty() {
        return hits;
    }
    for_each_region(p.0, |base, size| {
        if let Some(buf) = p.read(base, size) {
            let mut off = 0usize;
            while off + 16 <= buf.len() {
                let ptr = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
                if ptr != 0 && needles.contains(&ptr) {
                    let len = u64::from_le_bytes(buf[off + 8..off + 16].try_into().unwrap());
                    if len == needle_len as u64 {
                        hits.push(base + off);
                    }
                }
                off += 8;
            }
        }
    });
    hits
}

fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex_bytes(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.chunks_exact(2) {
        out.push((hex_val(pair[0])? << 4) | hex_val(pair[1])?);
    }
    Some(out)
}

fn probable_key(key: &[u8; 32]) -> bool {
    let mut seen = [false; 256];
    let mut distinct = 0usize;
    for &b in key {
        if !seen[b as usize] {
            seen[b as usize] = true;
            distinct += 1;
        }
    }
    distinct >= 15
}

/// 从异或解码后的文本里解析 `x'<hex>'` 字面量。
fn parse_hex_literals(text: &[u8]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 < text.len() {
        if (text[i] == b'x' || text[i] == b'X') && text[i + 1] == b'\'' {
            let start = i + 2;
            let mut j = start;
            while j < text.len() && text[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j < text.len() && text[j] == b'\'' {
                let run = &text[start..j];
                if (64..=192).contains(&run.len()) && run.len() % 2 == 0 {
                    let len = run.len();
                    let mut starts = vec![0usize];
                    if len > 96 {
                        let mut s = 0usize;
                        while s + 64 <= len {
                            starts.push(s);
                            s += 32;
                        }
                        starts.push(len - 64);
                    }
                    starts.sort_unstable();
                    starts.dedup();
                    for s in starts {
                        if s + 64 > len {
                            continue;
                        }
                        let Some(kb) = hex_bytes(&run[s..s + 64]) else {
                            continue;
                        };
                        let key: [u8; 32] = match kb.try_into() {
                            Ok(k) => k,
                            Err(_) => continue,
                        };
                        if !probable_key(&key) {
                            continue;
                        }
                        let salt = if s + 96 <= len {
                            hex_bytes(&run[s + 64..s + 96])
                                .and_then(|v| v.try_into().ok())
                        } else {
                            None
                        };
                        out.push(Candidate { key, salt });
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn scan_process(p: &Proc) -> Vec<Candidate> {
    let needles = find_bytes(p, CONFIG_CIPHER_NAME);
    if needles.is_empty() {
        return Vec::new();
    }
    let needle_set: HashSet<u64> = needles.iter().map(|&a| a as u64).collect();
    let refs = find_needle_refs(p, CONFIG_CIPHER_NAME.len(), &needle_set);

    let mut out = Vec::new();
    for qaddr in refs {
        {
            let Some(node) = p.read(qaddr.saturating_sub(0x10), 0x50) else {
                continue;
            };
            if node.len() < 0x40 {
                continue;
            }
            let needle_addr = match u64_at(&node, 0x10) {
                Some(a) => a,
                None => continue,
            };
            if !needle_set.contains(&needle_addr)
                || u64_at(&node, 0x18) != Some(CONFIG_CIPHER_NAME.len() as u64)
            {
                continue;
            }
            let Some(config_ptr) = u64_at(&node, 0x28) else {
                continue;
            };
            if !(0x10000..0x8000_0000_0000).contains(&config_ptr) {
                continue;
            }
            let Some(obj) = p.read(config_ptr as usize + 0x88, 0x28) else {
                continue;
            };
            if obj.len() < 0x18 {
                continue;
            }
            let Some(data_ptr) = u64_at(&obj, 0x08) else {
                continue;
            };
            let Some(data_len) = u64_at(&obj, 0x10) else {
                continue;
            };
            if !(0 < data_len && data_len <= 1024)
                || !(0x10000..0x8000_0000_0000).contains(&data_ptr)
            {
                continue;
            }
            let Some(blob) = p.read(data_ptr as usize, data_len as usize) else {
                continue;
            };
            if blob.len() != data_len as usize {
                continue;
            }
            let decoded: Vec<u8> = blob
                .iter()
                .enumerate()
                .map(|(i, &b)| b ^ CONFIG_XOR_MASK[i % CONFIG_XOR_MASK.len()])
                .collect();
            out.extend(parse_hex_literals(&decoded));
        }
    }
    out
}

pub fn weixin_pids() -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut pe);
        while ok != 0 {
            let name = String::from_utf16_lossy(&pe.szExeFile)
                .trim_end_matches('\0')
                .to_string();
            if name.eq_ignore_ascii_case("Weixin.exe") {
                out.push(pe.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut pe);
        }
        CloseHandle(snap);
    }
    out
}

/// 扫描所有 Weixin.exe 进程，返回去重后的候选密钥与诊断日志。
pub fn extract_all() -> (Vec<Candidate>, Vec<String>) {
    let mut logs = Vec::new();
    if std::env::var_os("WV_NO_LIVE").is_some() {
        logs.push("WV_NO_LIVE=1：已跳过内存扫描（仅测试缓存路径）".into());
        return (Vec::new(), logs);
    }
    let pids = weixin_pids();
    if pids.is_empty() {
        logs.push("未检测到 Weixin.exe（微信未运行或未登录）".into());
        return (Vec::new(), logs);
    }
    logs.push(format!("检测到 Weixin.exe 进程: {:?}", pids));

    let mut seen: HashSet<Candidate> = HashSet::new();
    for pid in pids {
        let Some(p) = Proc::open(pid) else {
            logs.push(format!("无法读取进程 {pid} 内存（权限不足？）"));
            continue;
        };
        let cands = scan_process(&p);
        logs.push(format!("进程 {pid}: {} 个候选密钥", cands.len()));
        for c in cands {
            seen.insert(c);
        }
    }
    (seen.into_iter().collect(), logs)
}

//! 微信 4.x（WCDB / SQLCipher 4）数据库页解密与 WAL 合并。
//!
//! 参数：AES-256-CBC，页大小 4096，reserve 80（16 IV + 64 HMAC），
//! HMAC-SHA512。密钥校验：mac_key = PBKDF2-HMAC-SHA512(key, salt^0x3a, 2, 32)，
//! HMAC 覆盖 page[16..4032] 并追加小端页号。这些参数在微信 4.2 上实测通过。

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use hmac::{Hmac, Mac};
use sha2::Sha512;

pub const PAGE_SZ: usize = 4096;
const RESERVE_SZ: usize = 80;
const IV_OFF: usize = PAGE_SZ - RESERVE_SZ; // 4016
const HMAC_OFF: usize = PAGE_SZ - 64; // 4032

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeySpec {
    pub key: [u8; 32],
    /// 仅「明文头」库需要显式 salt；标准库为 None（salt 取文件头 16 字节）。
    pub salt: Option<[u8; 16]>,
}

impl KeySpec {
    pub fn plaintext_header(&self) -> bool {
        self.salt.is_some()
    }
}

fn pbkdf2_sha512_32(pass: &[u8], salt: &[u8], iters: u32) -> [u8; 32] {
    let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(pass).expect("hmac key");
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u = mac.finalize().into_bytes();
    let mut out = u;
    for _ in 1..iters {
        let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(pass).expect("hmac key");
        mac.update(&u);
        u = mac.finalize().into_bytes();
        for i in 0..out.len() {
            out[i] ^= u[i];
        }
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&out[..32]);
    key
}

/// 用页 1 的 HMAC 校验密钥。`explicit_salt` 用于「明文头」库。
pub fn verify_key(key: &[u8; 32], page1: &[u8], explicit_salt: Option<[u8; 16]>) -> bool {
    if page1.len() < PAGE_SZ {
        return false;
    }
    let salt = explicit_salt.unwrap_or_else(|| page1[..16].try_into().unwrap());
    let mac_salt: Vec<u8> = salt.iter().map(|b| b ^ 0x3a).collect();
    let mac_key = pbkdf2_sha512_32(key, &mac_salt, 2);
    let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(&mac_key).expect("hmac key");
    mac.update(&page1[16..HMAC_OFF]);
    mac.update(&1u32.to_le_bytes());
    let tag = mac.finalize().into_bytes();
    tag.as_slice() == &page1[HMAC_OFF..PAGE_SZ]
}

fn cbc_decrypt(key: &[u8; 32], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes256::new(GenericArray::from_slice(key));
    let mut prev = *iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            out.push(block[i] ^ prev[i]);
        }
        prev.copy_from_slice(chunk);
    }
    out
}

/// 解密单页，输出标准 4096 字节 SQLite 页（reserve 区域补 0）。
fn decrypt_page(spec: &KeySpec, page: &[u8; PAGE_SZ], pgno: u64) -> [u8; PAGE_SZ] {
    let iv: [u8; 16] = page[IV_OFF..IV_OFF + 16].try_into().unwrap();
    let enc = if pgno == 1 {
        &page[16..IV_OFF]
    } else {
        &page[..IV_OFF]
    };
    let pt = cbc_decrypt(&spec.key, &iv, enc);
    let mut out = [0u8; PAGE_SZ];
    if pgno == 1 {
        if spec.plaintext_header() {
            out[..16].copy_from_slice(&page[..16]);
        } else {
            out[..16].copy_from_slice(b"SQLite format 3\0");
        }
        out[16..IV_OFF].copy_from_slice(&pt);
    } else {
        out[..IV_OFF].copy_from_slice(&pt);
    }
    out
}

/// 把加密库整库解密到 `dst`（逐页流式读写，避免大库占用大量内存）。
pub fn decrypt_db(src: &Path, dst: &Path, spec: &KeySpec) -> io::Result<()> {
    let mut fin = BufReader::new(File::open(src)?);
    let mut fout = BufWriter::new(File::create(dst)?);
    let mut page = [0u8; PAGE_SZ];
    let mut pgno: u64 = 1;
    loop {
        let mut filled = 0usize;
        while filled < PAGE_SZ {
            let n = fin.read(&mut page[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        for b in &mut page[filled..] {
            *b = 0;
        }
        fout.write_all(&decrypt_page(spec, &page, pgno))?;
        pgno += 1;
        if filled < PAGE_SZ {
            break;
        }
    }
    fout.flush()
}

/// 把 `-wal` 中已提交的帧覆盖进解密后的主库。
///
/// 帧结构（大端）：[0:4] 页号, [4:8] 提交标记, [8:16] salt, [16:24] 校验。
pub fn merge_wal(dst: &Path, wal: &Path, spec: &KeySpec) -> io::Result<usize> {
    let data = fs::read(wal)?;
    if data.len() < 32 {
        return Ok(0);
    }
    let wal_salt = &data[16..24];
    let mut f = OpenOptions::new().read(true).write(true).open(dst)?;
    let mut max_pgno = 0u64;
    let mut applied = 0usize;
    let mut off = 32usize;
    while off + 24 + PAGE_SZ <= data.len() {
        let hdr = &data[off..off + 24];
        let page: &[u8; PAGE_SZ] = data[off + 24..off + 24 + PAGE_SZ].try_into().unwrap();
        off += 24 + PAGE_SZ;
        let pgno = u32::from_be_bytes(hdr[0..4].try_into().unwrap()) as u64;
        if pgno == 0 || &hdr[8..16] != wal_salt {
            continue;
        }
        let pt = decrypt_page(spec, page, pgno);
        if pgno == 1 {
            if pt[..16] != *b"SQLite format 3\0" {
                continue;
            }
        } else if !matches!(pt[0], 0 | 2 | 5 | 10 | 13) {
            continue;
        }
        f.seek(SeekFrom::Start((pgno - 1) * PAGE_SZ as u64))?;
        f.write_all(&pt)?;
        max_pgno = max_pgno.max(pgno);
        applied += 1;
    }
    f.flush()?;

    // 更新页 1 头里的数据库页数，保证 SQLite 认为文件完整。
    let mut hdr = [0u8; 100];
    f.seek(SeekFrom::Start(0))?;
    f.read_exact(&mut hdr)?;
    let cur = u32::from_be_bytes(hdr[28..32].try_into().unwrap()) as u64;
    let file_pages = f.metadata()?.len().div_ceil(PAGE_SZ as u64);
    let new_pages = cur.max(max_pgno).max(file_pages);
    if new_pages != cur {
        hdr[28..32].copy_from_slice(&(new_pages as u32).to_be_bytes());
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&hdr)?;
        f.flush()?;
    }
    Ok(applied)
}

/// 解密主库并在存在 WAL 时合并增量。
pub fn decrypt_with_wal(src: &Path, dst: &Path, spec: &KeySpec) -> io::Result<()> {
    decrypt_db(src, dst, spec)?;
    let wal = src.with_file_name(format!(
        "{}-wal",
        src.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
    ));
    if wal.is_file() {
        let _ = merge_wal(dst, &wal, spec);
    }
    // SQLite 若看到残留的 -wal/-shm 会试图回放，删掉保证读到刚解密的内容。
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(dst.with_file_name(format!(
            "{}{suffix}",
            dst.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
        )));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn page1_of(path: &Path) -> io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut buf = vec![0u8; PAGE_SZ];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

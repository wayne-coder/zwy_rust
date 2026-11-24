//! 开奖号码：澳门 / 香港 / 老澳门，各 6 个平码 + 1 个特码。
//!
//! 保存在 `%LOCALAPPDATA%\WechatViewer\draws.txt`；本程序不联网拉开奖，
//! 由「开奖」弹窗手动录入。录完即用于总账 / 明细的对奖计算。

use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Default, Debug)]
pub struct RegionDraw {
    pub draw: Vec<u32>,
    pub special: Option<u32>,
}

impl RegionDraw {
    pub fn ready(&self) -> bool {
        self.draw.len() == 6 && self.special.is_some()
    }
}

#[derive(Clone, Default, Debug)]
pub struct Draws {
    pub macau: RegionDraw,
    pub hongkong: RegionDraw,
    pub old_macau: RegionDraw,
}

impl Draws {
    pub fn get(&self, region: &str) -> &RegionDraw {
        match region {
            "香港" => &self.hongkong,
            "老澳门" => &self.old_macau,
            _ => &self.macau,
        }
    }
}

const MAGIC: &str = "wxv-draws\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("draws.txt")
}

pub fn load() -> Draws {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return Draws::default();
    };
    parse(&text)
}

pub fn save(draws: &Draws) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(draws)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

fn serialize(draws: &Draws) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for (key, region) in [
        ("macau", &draws.macau),
        ("hongkong", &draws.hongkong),
        ("oldMacau", &draws.old_macau),
    ] {
        if !region.ready() {
            continue;
        }
        let mut nums: Vec<String> = region.draw.iter().map(u32::to_string).collect();
        if let Some(s) = region.special {
            nums.push(s.to_string());
        }
        out.push_str("draw\t");
        out.push_str(key);
        out.push('\t');
        out.push_str(&nums.join(","));
        out.push('\n');
    }
    out
}

fn parse(text: &str) -> Draws {
    let mut draws = Draws::default();
    if !text.starts_with(MAGIC) {
        return draws;
    }
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(3, '\t');
        if parts.next() != Some("draw") {
            continue;
        }
        let Some(key) = parts.next() else { continue };
        let Some(nums) = parts.next() else { continue };
        let vals: Vec<u32> = nums
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        if vals.len() != 7 {
            continue;
        }
        let region = RegionDraw {
            draw: vals[..6].to_vec(),
            special: Some(vals[6]),
        };
        match key {
            "macau" => draws.macau = region,
            "hongkong" => draws.hongkong = region,
            "oldMacau" => draws.old_macau = region,
            _ => {}
        }
    }
    draws
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_draws() {
        let mut d = Draws::default();
        d.macau = RegionDraw {
            draw: vec![1, 2, 3, 4, 5, 6],
            special: Some(7),
        };
        d.old_macau = RegionDraw {
            draw: vec![10, 11, 12, 13, 14, 15],
            special: Some(16),
        };
        let loaded = parse(&serialize(&d));
        assert_eq!(loaded.macau.draw, d.macau.draw);
        assert_eq!(loaded.macau.special, d.macau.special);
        assert_eq!(loaded.old_macau.special, d.old_macau.special);
        assert!(!loaded.hongkong.ready());
    }
}

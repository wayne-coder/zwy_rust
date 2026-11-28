//! 赔率与回水设置：默认一套，分组一套，会员可单独覆盖。
//!
//! 结算优先级：会员个性化 > 所属分组 > 默认。每个层级都有澳门 / 香港 /
//! 老澳门三份，玩法字段与参考项目 `zwy_wails_v1` 的 `OddsSettings` 对齐
//! （本程序未实现的「带砍」等玩法不在此列）。
//!
//! 存储位置：`%LOCALAPPDATA%\WechatViewer\odds.txt`，一行一个「对象 + 地区」：
//!
//! ```text
//! wxv-odds	1
//! default	澳门	0	two_in_two=66	...
//! group	A 组	0	two_in_two=70	...
//! member	张三	0	two_in_two=68	...
//! ```
//!
//! 读取时缺字段一律回落到默认值，未知字段与损坏行忽略。

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

const MAGIC: &str = "wxv-odds\t1";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Odds,
    Rebate,
}

pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    /// 设置页里的玩法分组
    pub group: &'static str,
    pub kind: FieldKind,
    pub default: f64,
}

const fn odd(key: &'static str, label: &'static str, group: &'static str, default: f64) -> FieldDef {
    FieldDef {
        key,
        label,
        group,
        kind: FieldKind::Odds,
        default,
    }
}

const fn rw(key: &'static str, label: &'static str, group: &'static str, default: f64) -> FieldDef {
    FieldDef {
        key,
        label,
        group,
        kind: FieldKind::Rebate,
        default,
    }
}

/// 设置页按这个顺序分组展示。
pub const FIELD_GROUPS: &[&str] = &[
    "组合",
    "特码",
    "平码",
    "平特",
    "连肖",
    "连尾",
    "不中",
    "中一",
    "六肖中一",
];

pub const FIELDS: &[FieldDef] = &[
    // ── 组合 ──
    odd("two_in_two", "二中二赔率", "组合", 66.0),
    odd("two_in_two_dai_kan", "二中二带砍赔率", "组合", 0.0),
    odd("two_all_hit", "二全中赔率", "组合", 66.0),
    odd("three_in_three", "三中三赔率", "组合", 750.0),
    odd("three_in_two_liang", "三中二·中2赔率", "组合", 20.0),
    odd("three_in_two_sa", "三中二·中3赔率", "组合", 100.0),
    // 风险列的名义倍率（与参考项目 xmt 一致）：三中二全中时实际敞口走 sa，
    // 这一列按名义值算，只用于展示。
    odd("three_in_two", "三中二名义赔率", "组合", 25.0),
    odd("special_hit", "特碰赔率", "组合", 160.0),
    rw("rw_two_in_two", "二中二回水(%)", "组合", 15.0),
    rw("rw_two_all_hit", "二全中回水(%)", "组合", 15.0),
    rw("rw_three_in_three", "三中三回水(%)", "组合", 15.0),
    rw("rw_three_in_two", "三中二回水(%)", "组合", 15.0),
    rw("rw_special_hit", "特碰回水(%)", "组合", 15.0),
    // ── 特码 ──
    odd("special_number", "特码赔率", "特码", 47.0),
    odd("special_attr", "特码属性赔率", "特码", 1.9),
    odd("special_attr_49", "特码开49赔率", "特码", 1.0),
    rw("rw_special_number", "特码回水(%)", "特码", 4.0),
    // ── 平码 ──
    odd("flat_number", "平码赔率", "平码", 8.0),
    rw("rw_flat_number", "平码回水(%)", "平码", 0.0),
    // ── 平特 ──
    odd("flat_special_4", "平特4码赔率", "平特", 2.0),
    rw("rw_flat_special_4", "平特4码回水(%)", "平特", 4.0),
    odd("flat_special_5", "平特5码赔率", "平特", 1.8),
    rw("rw_flat_special_5", "平特5码回水(%)", "平特", 0.0),
    // ── 连肖 ──
    odd("lianxiao_2", "二连肖赔率", "连肖", 4.5),
    odd("lianxiao_3", "三连肖赔率", "连肖", 11.0),
    odd("lianxiao_4", "四连肖赔率", "连肖", 33.0),
    odd("lianxiao_5", "五连肖赔率", "连肖", 110.0),
    odd("lianxiao_ma_2", "二连肖含马赔率", "连肖", 4.0),
    odd("lianxiao_ma_3", "三连肖含马赔率", "连肖", 10.0),
    odd("lianxiao_ma_4", "四连肖含马赔率", "连肖", 30.0),
    odd("lianxiao_ma_5", "五连肖含马赔率", "连肖", 90.0),
    rw("rw_lianxiao", "连肖回水(%)", "连肖", 4.0),
    // ── 连尾 ──
    odd("lianwei_2", "二连尾赔率", "连尾", 2.0),
    odd("lianwei_3", "三连尾赔率", "连尾", 6.0),
    odd("lianwei_4", "四连尾赔率", "连尾", 11.0),
    odd("lianwei_zero_2", "二连尾含0尾赔率", "连尾", 3.0),
    odd("lianwei_zero_3", "三连尾含0尾赔率", "连尾", 8.0),
    odd("lianwei_zero_4", "四连尾含0尾赔率", "连尾", 15.0),
    rw("rw_lianwei", "连尾回水(%)", "连尾", 4.0),
    // ── 不中 ──
    odd("buzhong_5", "五不中赔率", "不中", 2.0),
    odd("buzhong_6", "六不中赔率", "不中", 2.0),
    odd("buzhong_7", "七不中赔率", "不中", 2.1),
    odd("buzhong_8", "八不中赔率", "不中", 3.0),
    odd("buzhong_9", "九不中赔率", "不中", 4.0),
    odd("buzhong_10", "十不中赔率", "不中", 5.0),
    rw("rw_buzhong", "不中回水(%)", "不中", 2.0),
    // ── 中一 ──
    odd("zhongyi_3", "三中一赔率", "中一", 2.6),
    rw("rw_zhongyi", "三中一回水(%)", "中一", 4.0),
    odd("zhongyi_4", "四中一赔率", "中一", 2.0),
    rw("rw_zhongyi_4", "四中一回水(%)", "中一", 4.0),
    odd("zhongyi_5", "五中一赔率", "中一", 1.8),
    rw("rw_zhongyi_5", "五中一回水(%)", "中一", 0.0),
    // ── 六肖中一 ──
    odd("liuxiao_zhongyi", "六肖中一赔率", "六肖中一", 1.9),
    odd("liuxiao_zhongyi_49", "六肖中一开49赔率", "六肖中一", 1.0),
    rw("rw_liuxiao", "六肖中一回水(%)", "六肖中一", 4.0),
];

#[derive(Clone, Debug)]
pub struct OddsSet {
    values: Vec<f64>,
}

impl Default for OddsSet {
    fn default() -> Self {
        Self {
            values: FIELDS.iter().map(|f| f.default).collect(),
        }
    }
}

fn field_index(key: &str) -> Option<usize> {
    FIELDS.iter().position(|f| f.key == key)
}

fn round2(v: f64) -> f64 {
    if v.is_finite() {
        (v * 100.0).round() / 100.0
    } else {
        0.0
    }
}

impl OddsSet {
    pub fn value(&self, key: &str) -> f64 {
        field_index(key)
            .and_then(|i| self.values.get(i).copied())
            .unwrap_or(0.0)
    }

    pub fn value_mut(&mut self, key: &str) -> Option<&mut f64> {
        let i = field_index(key)?;
        self.values.get_mut(i)
    }

}

fn default_regions() -> [OddsSet; 3] {
    [
        OddsSet::default(),
        OddsSet::default(),
        OddsSet::default(),
    ]
}

/// 某个对象在每个字段上是否「显式覆盖」。未覆盖的字段跟随上级
/// （分组跟默认、会员跟分组），这样文件里只存差异，界面也能显示来源。
#[derive(Clone, Debug)]
pub struct OwnedMask {
    owned: Vec<bool>,
}

impl Default for OwnedMask {
    fn default() -> Self {
        Self {
            owned: vec![false; FIELDS.len()],
        }
    }
}

impl OwnedMask {
    pub fn all() -> Self {
        Self {
            owned: vec![true; FIELDS.len()],
        }
    }

    pub fn is_owned(&self, key: &str) -> bool {
        field_index(key)
            .and_then(|i| self.owned.get(i).copied())
            .unwrap_or(false)
    }

    pub fn set_owned(&mut self, key: &str, on: bool) {
        if let Some(i) = field_index(key) {
            if let Some(slot) = self.owned.get_mut(i) {
                *slot = on;
            }
        }
    }

    pub fn any(&self) -> bool {
        self.owned.iter().any(|b| *b)
    }

    pub fn clear(&mut self) {
        for b in &mut self.owned {
            *b = false;
        }
    }
}

fn default_masks() -> [OwnedMask; 3] {
    [
        OwnedMask::default(),
        OwnedMask::default(),
        OwnedMask::default(),
    ]
}

/// 三个地区全部字段都算显式覆盖（把一份完整配置套给对象时用）。
pub fn all_masks() -> [OwnedMask; 3] {
    [
        OwnedMask::all(),
        OwnedMask::all(),
        OwnedMask::all(),
    ]
}

/// 把一份「只含覆盖字段」的设置合并到上级设置上。
fn merge_set(base: &OddsSet, own: &OddsSet, mask: &OwnedMask) -> OddsSet {
    let mut out = base.clone();
    for (i, f) in FIELDS.iter().enumerate() {
        if mask.is_owned(f.key) {
            if let Some(v) = own.values.get(i).copied() {
                out.values[i] = v;
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct OddsConfig {
    pub default: [OddsSet; 3],
    pub groups: BTreeMap<String, [OddsSet; 3]>,
    pub members: BTreeMap<String, [OddsSet; 3]>,
    /// 分组 / 会员每个字段是否显式覆盖；未覆盖的字段跟随上级。
    pub group_owned: BTreeMap<String, [OwnedMask; 3]>,
    pub member_owned: BTreeMap<String, [OwnedMask; 3]>,
    /// 命名模板：完整的三地区配置，可一键套给分组 / 会员。
    pub templates: BTreeMap<String, [OddsSet; 3]>,
}

impl Default for OddsConfig {
    fn default() -> Self {
        Self {
            default: default_regions(),
            groups: BTreeMap::new(),
            members: BTreeMap::new(),
            group_owned: BTreeMap::new(),
            member_owned: BTreeMap::new(),
            templates: BTreeMap::new(),
        }
    }
}

impl OddsConfig {
    pub fn has_group(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }

    /// 分组第一次被设置时建立空覆盖（字段仍跟随默认）。
    pub fn ensure_group(&mut self, name: &str) {
        self.groups
            .entry(name.to_string())
            .or_insert_with(default_regions);
        self.group_owned
            .entry(name.to_string())
            .or_insert_with(default_masks);
    }

    /// 新增会员覆盖时建立空覆盖（字段仍跟随分组 / 默认）。
    pub fn ensure_member(&mut self, name: &str, _group: Option<&str>) {
        self.members
            .entry(name.to_string())
            .or_insert_with(default_regions);
        self.member_owned
            .entry(name.to_string())
            .or_insert_with(default_masks);
    }

    /// 分组在某地区的生效值：默认 + 本组显式覆盖。
    pub fn group_effective(&self, group: Option<&str>, region: usize) -> OddsSet {
        let region = region.min(2);
        let mut base = self.default[region].clone();
        let Some(group) = group else {
            return base;
        };
        let Some(sets) = self.groups.get(group) else {
            return base;
        };
        match self.group_owned.get(group).and_then(|m| m.get(region)) {
            Some(mask) => {
                base = merge_set(&base, &sets[region], mask);
                base
            }
            // 没有 mask 的旧数据：整份覆盖（保持旧行为）。
            None => sets[region].clone(),
        }
    }

    /// 会员在某地区的生效值：分组生效值 + 会员显式覆盖。
    fn member_effective(
        &self,
        key: &str,
        group: Option<&str>,
        region: usize,
    ) -> Option<OddsSet> {
        let sets = self.members.get(key)?;
        let base = self.group_effective(group, region);
        match self.member_owned.get(key).and_then(|m| m.get(region)) {
            Some(mask) => Some(merge_set(&base, &sets[region], mask)),
            None => Some(sets[region].clone()),
        }
    }

    /// 结算时取赔率：会员覆盖 → 分组 → 默认。
    /// 返回的是按字段合并后的生效值。
    pub fn resolve(&self, member: &str, group: Option<&str>, region: usize) -> OddsSet {
        self.member_effective(member, group, region)
            .unwrap_or_else(|| self.group_effective(group, region))
    }

    /// 结算时取赔率：先按「会员来源身份」（`m:<会话>:<发送人>`）找覆盖，
    /// 再退回旧的「昵称」覆盖，最后分组 / 默认。
    pub fn resolve_member(
        &self,
        identity: &str,
        nick: &str,
        group: Option<&str>,
        region: usize,
    ) -> OddsSet {
        if let Some(set) = self.member_effective(identity, group, region) {
            return set;
        }
        if let Some(set) = self.member_effective(nick, group, region) {
            return set;
        }
        self.group_effective(group, region)
    }

}

/// 地区下标：0 澳门、1 香港、2 老澳门。
pub fn region_index(lottery: &str) -> usize {
    match lottery {
        "香港" => 1,
        "老澳门" => 2,
        _ => 0,
    }
}

pub const REGION_LABELS: [&str; 3] = ["澳门", "香港", "老澳门"];

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("odds.txt")
}

fn clean_name(name: &str) -> String {
    name.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

pub fn load() -> OddsConfig {
    let Ok(text) = fs::read_to_string(store_path()) else {
        return OddsConfig::default();
    };
    parse(&text)
}

pub fn save(config: &OddsConfig) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(config)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

fn write_set(
    out: &mut String,
    kind: &str,
    name: &str,
    region: usize,
    set: &OddsSet,
    mask: Option<&OwnedMask>,
) {
    out.push_str(kind);
    out.push('\t');
    out.push_str(name);
    out.push('\t');
    out.push_str(&region.to_string());
    for (i, f) in FIELDS.iter().enumerate() {
        if mask.is_some_and(|m| !m.is_owned(f.key)) {
            continue;
        }
        out.push('\t');
        out.push_str(f.key);
        out.push('=');
        let v = set.values.get(i).copied().unwrap_or(f.default);
        out.push_str(&format!("{v}"));
    }
    out.push('\n');
}

pub fn serialize(config: &OddsConfig) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for (region, set) in config.default.iter().enumerate() {
        write_set(&mut out, "default", "默认", region, set, None);
    }
    for (name, sets) in &config.groups {
        let name = clean_name(name);
        if name.is_empty() {
            continue;
        }
        let masks = config.group_owned.get(name.as_str());
        for (region, set) in sets.iter().enumerate() {
            let mask = masks.map(|m| &m[region]);
            if mask.is_some_and(|m| !m.any()) {
                continue;
            }
            write_set(&mut out, "group", &name, region, set, mask);
        }
    }
    for (name, sets) in &config.members {
        let name = clean_name(name);
        if name.is_empty() {
            continue;
        }
        let masks = config.member_owned.get(name.as_str());
        for (region, set) in sets.iter().enumerate() {
            let mask = masks.map(|m| &m[region]);
            if mask.is_some_and(|m| !m.any()) {
                continue;
            }
            write_set(&mut out, "member", &name, region, set, mask);
        }
    }
    for (name, sets) in &config.templates {
        let name = clean_name(name);
        if name.is_empty() {
            continue;
        }
        for (region, set) in sets.iter().enumerate() {
            write_set(&mut out, "template", &name, region, set, None);
        }
    }
    out
}

pub fn parse(text: &str) -> OddsConfig {
    let mut config = OddsConfig::default();
    if !text.starts_with(MAGIC) {
        return config;
    }
    for line in text.lines().skip(1) {
        let mut parts = line.split('\t');
        let Some(kind) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        let Some(region) = parts.next().and_then(|r| r.parse::<usize>().ok()) else {
            continue;
        };
        if region > 2 {
            continue;
        }
        let (target, mut mask): (&mut [OddsSet; 3], Option<&mut OwnedMask>) = match kind {
            "default" => (&mut config.default, None),
            "group" => {
                let name = clean_name(name);
                if name.is_empty() {
                    continue;
                }
                let sets = config.groups.entry(name.clone()).or_insert_with(default_regions);
                let masks = config
                    .group_owned
                    .entry(name)
                    .or_insert_with(default_masks);
                (sets, Some(&mut masks[region]))
            }
            "member" => {
                let name = clean_name(name);
                if name.is_empty() {
                    continue;
                }
                let sets = config.members.entry(name.clone()).or_insert_with(default_regions);
                let masks = config
                    .member_owned
                    .entry(name)
                    .or_insert_with(default_masks);
                (sets, Some(&mut masks[region]))
            }
            "template" => {
                let name = clean_name(name);
                if name.is_empty() {
                    continue;
                }
                let sets = config
                    .templates
                    .entry(name)
                    .or_insert_with(default_regions);
                (sets, None)
            }
            _ => continue,
        };
        let set = &mut target[region];
        for pair in parts {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let Ok(v) = value.trim().parse::<f64>() else {
                continue;
            };
            let key = key.trim();
            if let Some(slot) = set.value_mut(key) {
                *slot = round2(v).max(0.0);
                if let Some(mask) = mask.as_mut() {
                    mask.set_owned(key, true);
                }
            }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_group_and_member_overrides() {
        let mut cfg = OddsConfig::default();
        cfg.ensure_group("A 组");
        for region in 0..3 {
            if let Some(v) = cfg.groups.get_mut("A 组").unwrap()[region]
                .value_mut("two_in_two")
            {
                *v = 70.0;
                cfg.group_owned.get_mut("A 组").unwrap()[region]
                    .set_owned("two_in_two", true);
            }
        }
        cfg.ensure_member("张三", Some("A 组"));
        cfg.members.get_mut("张三").unwrap()[1]
            .value_mut("rw_two_in_two")
            .map(|v| *v = 12.5);
        cfg.member_owned.get_mut("张三").unwrap()[1]
            .set_owned("rw_two_in_two", true);

        let text = serialize(&cfg);
        let back = parse(&text);
        assert_eq!(back.resolve("张三", Some("A 组"), 0).value("two_in_two"), 70.0);
        assert_eq!(back.resolve("张三", Some("A 组"), 1).value("rw_two_in_two"), 12.5);
        // 会员第二地区只在回水上覆盖，赔率仍复制自分组那份
        assert_eq!(back.resolve("张三", Some("A 组"), 1).value("two_in_two"), 70.0);
        assert_eq!(back.resolve("李四", Some("A 组"), 0).value("two_in_two"), 70.0);
        assert_eq!(back.resolve("王五", None, 0).value("two_in_two"), 66.0);
    }

    #[test]
    fn resolve_member_prefers_identity_then_legacy_nick() {
        let mut cfg = OddsConfig::default();
        cfg.ensure_member("张三", None);
        for region in 0..3 {
            if let Some(v) = cfg.members.get_mut("张三").unwrap()[region]
                .value_mut("two_in_two")
            {
                *v = 70.0;
                cfg.member_owned.get_mut("张三").unwrap()[region]
                    .set_owned("two_in_two", true);
            }
        }
        // 旧版昵称覆盖仍然生效。
        assert_eq!(
            cfg.resolve_member("m:群1:wxid_a", "张三", None, 0)
                .value("two_in_two"),
            70.0
        );
        // 来源身份覆盖优先于昵称覆盖。
        cfg.ensure_member("m:群2:wxid_a", None);
        for region in 0..3 {
            if let Some(v) = cfg.members.get_mut("m:群2:wxid_a").unwrap()[region]
                .value_mut("two_in_two")
            {
                *v = 80.0;
                cfg.member_owned.get_mut("m:群2:wxid_a").unwrap()[region]
                    .set_owned("two_in_two", true);
            }
        }
        assert_eq!(
            cfg.resolve_member("m:群2:wxid_a", "张三", None, 0)
                .value("two_in_two"),
            80.0
        );
        assert_eq!(
            cfg.resolve_member("m:群1:wxid_a", "张三", None, 0)
                .value("two_in_two"),
            70.0
        );
        assert_eq!(
            cfg.resolve_member("m:群3:wxid_b", "李四", None, 0)
                .value("two_in_two"),
            66.0
        );
    }

    #[test]
    fn defaults_match_settle_values() {
        let d = OddsSet::default();
        assert_eq!(d.value("two_in_two"), 66.0);
        assert_eq!(d.value("three_in_three"), 750.0);
        assert_eq!(d.value("three_in_two_liang"), 20.0);
        assert_eq!(d.value("three_in_two_sa"), 100.0);
        assert_eq!(d.value("lianxiao_ma_2"), 4.0);
        assert_eq!(d.value("rw_three_in_two"), 15.0);
    }

    #[test]
    fn sparse_overrides_merge_and_serialize() {
        let mut cfg = OddsConfig::default();
        cfg.ensure_group("A");
        cfg.groups.get_mut("A").unwrap()[0]
            .value_mut("two_in_two")
            .map(|v| *v = 70.0);
        cfg.group_owned.get_mut("A").unwrap()[0].set_owned("two_in_two", true);

        cfg.ensure_member("m:g:u", Some("A"));
        cfg.members.get_mut("m:g:u").unwrap()[0]
            .value_mut("three_in_three")
            .map(|v| *v = 800.0);
        cfg.member_owned.get_mut("m:g:u").unwrap()[0]
            .set_owned("three_in_three", true);

        // 会员只覆盖三中三，二中二继续跟随分组。
        let set = cfg.resolve_member("m:g:u", "", Some("A"), 0);
        assert_eq!(set.value("two_in_two"), 70.0);
        assert_eq!(set.value("three_in_three"), 800.0);
        // 同组其它会员只继承分组覆盖。
        assert_eq!(cfg.resolve("", Some("A"), 0).value("three_in_three"), 750.0);

        let text = serialize(&cfg);
        let group_line = text
            .lines()
            .find(|l| l.starts_with("group\tA\t0"))
            .unwrap();
        assert!(group_line.contains("two_in_two="));
        assert!(!group_line.contains("flat_number="));
        let member_line = text
            .lines()
            .find(|l| l.starts_with("member\tm:g:u\t0"))
            .unwrap();
        assert!(member_line.contains("three_in_three="));
        assert!(!member_line.contains("two_in_two="));

        let back = parse(&text);
        let set = back.resolve_member("m:g:u", "", Some("A"), 0);
        assert_eq!(set.value("two_in_two"), 70.0);
        assert_eq!(set.value("three_in_three"), 800.0);
    }

    #[test]
    fn templates_roundtrip() {
        let mut cfg = OddsConfig::default();
        let mut t = [
            OddsSet::default(),
            OddsSet::default(),
            OddsSet::default(),
        ];
        t[0].value_mut("two_in_two").map(|v| *v = 80.0);
        cfg.templates.insert("高赔".to_string(), t);

        let back = parse(&serialize(&cfg));
        assert_eq!(
            back.templates.get("高赔").unwrap()[0].value("two_in_two"),
            80.0
        );
    }

    #[test]
    fn missing_fields_fall_back_to_default() {
        let cfg = parse("wxv-odds\t1\ngroup\tA\t0\ttwo_in_two=70\n");
        assert_eq!(cfg.resolve("", Some("A"), 0).value("two_in_two"), 70.0);
        assert_eq!(cfg.resolve("", Some("A"), 0).value("flat_number"), 8.0);
        assert_eq!(cfg.resolve("", Some("A"), 1).value("two_in_two"), 66.0);
    }
}

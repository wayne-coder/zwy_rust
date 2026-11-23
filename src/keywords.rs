//! 关键词设置：玩法识别、金额识别、区域与其他。
//!
//! 字段与默认值照搬 `zwy_wails_v1` 设置页的三块内容；另附「生肖生僻字映射」。
//! 修改后保存到 `%LOCALAPPDATA%\WechatViewer\keywords.txt`，转换器每次读取当前设置。
//!
//! 三块设置共用一个关键词命名空间：同一个词不能同时出现在两个组里。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Field {
    ThreeInThree,
    TwoInTwo,
    TwoAllHit,
    ThreeInTwo,
    SpecialHit,
    SpecialNumber,
    FlatNumber,
    FlatSpecial,
    LianXiao2,
    LianXiao3,
    LianXiao4,
    LianXiao5,
    LianWei2,
    LianWei3,
    LianWei4,
    BuZhong5,
    BuZhong6,
    BuZhong7,
    BuZhong8,
    BuZhong9,
    BuZhong10,
    ZhongYi3,
    ZhongYi4,
    ZhongYi5,
    LiuXiaoZhongYi,
    StakePrefix,
    TotalStake,
    PerGroupTotalStake,
    StakeSuffix,
    MultiLineStakePrefix,
    HongKong,
    Macau,
    OldMacau,
    BothSides,
    ThreeSides,
    Multiplication,
    Range,
    Intersection,
}

pub const ALL_FIELDS: &[Field] = &[
    Field::ThreeInThree,
    Field::TwoInTwo,
    Field::TwoAllHit,
    Field::ThreeInTwo,
    Field::SpecialHit,
    Field::SpecialNumber,
    Field::FlatNumber,
    Field::FlatSpecial,
    Field::LianXiao2,
    Field::LianXiao3,
    Field::LianXiao4,
    Field::LianXiao5,
    Field::LianWei2,
    Field::LianWei3,
    Field::LianWei4,
    Field::BuZhong5,
    Field::BuZhong6,
    Field::BuZhong7,
    Field::BuZhong8,
    Field::BuZhong9,
    Field::BuZhong10,
    Field::ZhongYi3,
    Field::ZhongYi4,
    Field::ZhongYi5,
    Field::LiuXiaoZhongYi,
    Field::StakePrefix,
    Field::TotalStake,
    Field::PerGroupTotalStake,
    Field::StakeSuffix,
    Field::MultiLineStakePrefix,
    Field::HongKong,
    Field::Macau,
    Field::OldMacau,
    Field::BothSides,
    Field::ThreeSides,
    Field::Multiplication,
    Field::Range,
    Field::Intersection,
];

impl Field {
    pub fn key(self) -> &'static str {
        match self {
            Field::ThreeInThree => "threeInThree",
            Field::TwoInTwo => "twoInTwo",
            Field::TwoAllHit => "twoAllHit",
            Field::ThreeInTwo => "threeInTwo",
            Field::SpecialHit => "specialHit",
            Field::SpecialNumber => "specialNumber",
            Field::FlatNumber => "flatNumber",
            Field::FlatSpecial => "flatSpecial",
            Field::LianXiao2 => "lianXiao2",
            Field::LianXiao3 => "lianXiao3",
            Field::LianXiao4 => "lianXiao4",
            Field::LianXiao5 => "lianXiao5",
            Field::LianWei2 => "lianWei2",
            Field::LianWei3 => "lianWei3",
            Field::LianWei4 => "lianWei4",
            Field::BuZhong5 => "buZhong5",
            Field::BuZhong6 => "buZhong6",
            Field::BuZhong7 => "buZhong7",
            Field::BuZhong8 => "buZhong8",
            Field::BuZhong9 => "buZhong9",
            Field::BuZhong10 => "buZhong10",
            Field::ZhongYi3 => "zhongYi3",
            Field::ZhongYi4 => "zhongYi4",
            Field::ZhongYi5 => "zhongYi5",
            Field::LiuXiaoZhongYi => "liuXiaoZhongYi",
            Field::StakePrefix => "stakePrefix",
            Field::TotalStake => "totalStake",
            Field::PerGroupTotalStake => "perGroupTotalStake",
            Field::StakeSuffix => "stakeSuffix",
            Field::MultiLineStakePrefix => "multiLineStakePrefix",
            Field::HongKong => "hongKong",
            Field::Macau => "macau",
            Field::OldMacau => "oldMacau",
            Field::BothSides => "bothSides",
            Field::ThreeSides => "threeSides",
            Field::Multiplication => "multiplication",
            Field::Range => "range",
            Field::Intersection => "intersection",
        }
    }

    fn from_key(s: &str) -> Option<Field> {
        ALL_FIELDS.iter().copied().find(|f| f.key() == s)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Play,
    Stake,
    Region,
}

impl Section {
    pub const ALL: &'static [Section] = &[Section::Play, Section::Stake, Section::Region];

    pub fn label(self) -> &'static str {
        match self {
            Section::Play => "玩法关键词",
            Section::Stake => "金额识别",
            Section::Region => "区域 & 其他",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Section::Play => "自定义识别投注文本中玩法类型的关键词，修改后需重新录入才生效。",
            Section::Stake => "自定义识别投注金额的前后缀关键词。",
            Section::Region => "区域标记和辅助关键词。",
        }
    }
}

pub struct GroupMeta {
    pub section: Section,
    pub field: Field,
    pub title: &'static str,
    pub description: &'static str,
    pub placeholder: &'static str,
}

macro_rules! g {
    ($sec:ident, $field:ident, $title:literal, $desc:literal, $ph:literal) => {
        GroupMeta {
            section: Section::$sec,
            field: Field::$field,
            title: $title,
            description: $desc,
            placeholder: $ph,
        }
    };
}

pub fn groups() -> &'static [GroupMeta] {
    &[
        g!(Play, ThreeInThree, "三中三关键词", "识别三中三玩法", "例如：三中三"),
        g!(Play, TwoInTwo, "二中二关键词", "识别二中二玩法", "例如：二中二"),
        g!(Play, TwoAllHit, "二全中关键词", "识别二全中玩法（规则同二中二，赔率取「二全中赔率」且不带砍）", "例如：二全中"),
        g!(Play, ThreeInTwo, "三中二关键词", "识别三中二玩法", "例如：三中二"),
        g!(Play, SpecialHit, "特碰关键词", "识别特碰玩法", "例如：特碰"),
        g!(Play, SpecialNumber, "特码关键词", "识别特码玩法（单号匹配特别号）", "例如：特码"),
        g!(Play, FlatNumber, "平码关键词", "识别平码玩法（单号匹配前6位）", "例如：平码"),
        g!(Play, FlatSpecial, "平特关键词", "识别平特玩法（生肖/尾数匹配6+1位，或数字码组——4或5个号码）", "例如：平特"),
        g!(Play, LianXiao2, "连肖-二冲关键词", "识别二连肖（二冲关）玩法", "例如：二冲关"),
        g!(Play, LianXiao3, "连肖-三冲关键词", "识别三连肖玩法", "例如：三冲"),
        g!(Play, LianXiao4, "连肖-四冲关键词", "识别四连肖玩法", "例如：四冲"),
        g!(Play, LianXiao5, "连肖-五冲关键词", "识别五连肖玩法", "例如：五冲"),
        g!(Play, LianWei2, "连尾-二连尾关键词", "识别二连尾玩法（尾数，如 0尾 1尾）", "例如：二连尾"),
        g!(Play, LianWei3, "连尾-三连尾关键词", "识别三连尾玩法（尾数，如 0尾 1尾）", "例如：三连尾"),
        g!(Play, LianWei4, "连尾-四连尾关键词", "识别四连尾玩法（尾数，如 0尾 1尾）", "例如：四连尾"),
        g!(Play, BuZhong5, "不中-五不中关键词", "识别五不中玩法（押5个号码，一个都不开才中）", "例如：五不中"),
        g!(Play, BuZhong6, "不中-六不中关键词", "识别六不中玩法（押6个号码，一个都不开才中）", "例如：六不中"),
        g!(Play, BuZhong7, "不中-七不中关键词", "识别七不中玩法（押7个号码，一个都不开才中）", "例如：七不中"),
        g!(Play, BuZhong8, "不中-八不中关键词", "识别八不中玩法（押8个号码，一个都不开才中）", "例如：八不中"),
        g!(Play, BuZhong9, "不中-九不中关键词", "识别九不中玩法（押9个号码，一个都不开才中）", "例如：九不中"),
        g!(Play, BuZhong10, "不中-十不中关键词", "识别十不中玩法（押10个号码，一个都不开才中）", "例如：十不中"),
        g!(Play, ZhongYi3, "中一-三中一关键词", "识别三中一玩法（押≥3个号码，开中1个以上就算中，按 C(n,3) 拆注）", "例如：三中一"),
        g!(Play, ZhongYi4, "中一-四中一关键词", "识别四中一（独立玩法，必须刚好4个号码，整组一注，有自己的赔率/回水）", "例如：四中一"),
        g!(Play, ZhongYi5, "中一-五中一关键词", "识别五中一（独立玩法，必须刚好5个号码，整组一注，有自己的赔率/回水）", "例如：五中一"),
        g!(Play, LiuXiaoZhongYi, "六肖中一关键词", "识别六肖中一（押正好6个不同的生肖，7个开奖号任意一个生肖出现就算中）", "例如：六肖中一"),
        g!(Stake, StakePrefix, "投注金额前缀词", "设置表示每注金额的前缀关键词（如：各10→每注10）", "例如：各"),
        g!(Stake, TotalStake, "总投注金额关键词", "设置表示总投注金额的关键词（如：合计20→总金额20，自动除以注数）", "例如：合计"),
        g!(Stake, PerGroupTotalStake, "每组总投注金额关键词", "设置表示每个生肖/尾数/号码组各自总投注额的关键词（如：蛇猪鸡各包1000→每个生肖各1000、共3000，按各组的号码数摊成每号金额）", "例如：各包"),
        g!(Stake, StakeSuffix, "投注金额后缀词", "设置表示投注金额的后缀关键词（如：5元、10块）", "例如：元"),
        g!(Stake, MultiLineStakePrefix, "多行投注金额前缀词", "多行输入时共用金额的前缀（如：A10表示所有行各10）", "例如：A"),
        g!(Region, HongKong, "香港关键词", "识别香港开奖的关键词", "例如：香港"),
        g!(Region, Macau, "澳门关键词", "识别澳门开奖的关键词", "例如：澳门"),
        g!(Region, OldMacau, "老澳门关键词", "识别老澳门开奖的关键词（与澳门是独立盘口）", "例如：老澳门"),
        g!(Region, BothSides, "两边关键词", "同时押香港和澳门的关键词（如「三中三 1 2 3 4 各10两边」→ 港、澳各一份注）", "例如：两边"),
        g!(Region, ThreeSides, "三边关键词", "同时押香港、澳门和老澳门的关键词（如「三中三 1 2 3 4 各10三边」→ 港、澳、老各一份注）", "例如：三边"),
        g!(Region, Multiplication, "乘法分隔符", "拖/碰玩法分隔词（如\"12拖13 14\"表示12与13、14组合）", "例如：拖"),
        g!(Region, Range, "号码区间关键词", "设置表示号码区间的关键词（如\"1至25\"展开为1~25每个号码；\"1头至4头\"\"1尾至9尾\"同样展开）", "例如：至"),
        g!(Region, Intersection, "交集关键词", "设置两边号码取交集的关键词（如\"虎牛狗v双\"→取虎、牛、狗覆盖号码中同时是双数的号）", "例如：v"),
    ]
}

#[derive(Clone)]
pub struct KeywordSettings {
    items: HashMap<Field, Vec<String>>,
    pub zodiac_variants: BTreeMap<String, String>,
}

/// 内置默认关键词（当前这台机器上配置好的版本）。
/// 换电脑 / 首次运行时没有本地配置文件，就直接使用这份。
const DEFAULT_KEYWORDS: &str = include_str!("defaults/keywords.txt");

impl KeywordSettings {
    /// 源码里的兜底默认；解析本地 / 内置配置时作为基底。
    fn builtin() -> Self {
        let mut items: HashMap<Field, Vec<String>> = HashMap::new();
        let mut put = |f: Field, v: &[&str]| {
            items.insert(f, v.iter().map(|s| s.to_string()).collect());
        };
        put(Field::ThreeInThree, &["三中三", "三中", "3中", "3中3", "三星", "3星", "三种三"]);
        put(Field::TwoInTwo, &["二中二", "二中", "2中", "2中2", "二星", "2星", "双星", "两星"]);
        put(Field::TwoAllHit, &["二全中", "2全中"]);
        put(Field::ThreeInTwo, &["三中二"]);
        put(Field::SpecialHit, &["特碰", "特串"]);
        put(Field::SpecialNumber, &["特码"]);
        put(Field::FlatNumber, &["平码"]);
        put(Field::FlatSpecial, &["平特", "一肖"]);
        put(Field::LianXiao2, &["二冲", "二肖", "二连肖", "二串", "二冲关", "2冲", "2肖", "2连肖", "2串", "2冲关", "2连", "二连", "两连"]);
        put(Field::LianXiao3, &["三冲", "三肖", "三连肖", "三串", "3冲", "3肖", "3连肖", "3串", "3连", "三连"]);
        put(Field::LianXiao4, &["四冲", "四肖", "四连肖", "四串", "4冲", "4肖", "4连肖", "4串", "4连", "四连"]);
        put(Field::LianXiao5, &["五冲", "五肖", "五连肖", "五串", "5冲", "5肖", "5连肖", "5串", "5连", "五连"]);
        put(Field::LianWei2, &["二连尾", "2连尾"]);
        put(Field::LianWei3, &["三连尾", "3连尾"]);
        put(Field::LianWei4, &["四连尾", "4连尾"]);
        put(Field::BuZhong5, &["五不中", "5不中"]);
        put(Field::BuZhong6, &["六不中", "6不中"]);
        put(Field::BuZhong7, &["七不中", "7不中"]);
        put(Field::BuZhong8, &["八不中", "8不中"]);
        put(Field::BuZhong9, &["九不中", "9不中"]);
        put(Field::BuZhong10, &["十不中", "10不中"]);
        put(Field::ZhongYi3, &["三中一", "3中一"]);
        put(Field::ZhongYi4, &["四中一", "4中一"]);
        put(Field::ZhongYi5, &["五中一", "5中一"]);
        put(Field::LiuXiaoZhongYi, &["六肖中一", "6肖中一"]);
        put(Field::StakePrefix, &["各", "每", "各组", "各数", "各号", "一组", "买", "每组", "=", "一注", "每注", "1组"]);
        put(Field::TotalStake, &["总共", "合计", "共", "总", "总金额", "z", "Z"]);
        put(Field::PerGroupTotalStake, &["各包", "包"]);
        put(Field::StakeSuffix, &["元", "块", "只"]);
        put(Field::MultiLineStakePrefix, &["A", "a"]);
        put(Field::HongKong, &["港", "香", "香港", "昋"]);
        put(Field::Macau, &["澳", "门", "澳门", "奥", "新"]);
        put(Field::OldMacau, &["老澳门", "老澳", "老门", "老"]);
        put(Field::BothSides, &["两边"]);
        put(Field::ThreeSides, &["三边"]);
        put(Field::Multiplication, &["拖", "碰", "t", "T"]);
        put(Field::Range, &["至", "到"]);
        put(Field::Intersection, &["v", "V"]);

        let mut zodiac_variants = BTreeMap::new();
        zodiac_variants.insert("免".to_string(), "兔".to_string());
        zodiac_variants.insert("候".to_string(), "猴".to_string());

        Self {
            items,
            zodiac_variants,
        }
    }
}

impl Default for KeywordSettings {
    fn default() -> Self {
        KeywordSettings::embedded()
    }
}

impl KeywordSettings {
    /// 编译进 exe 的默认配置，以源码内置词表为兜底。
    fn embedded() -> Self {
        Self::parse_with_base(Self::builtin(), DEFAULT_KEYWORDS)
    }

    pub fn get(&self, f: Field) -> &[String] {
        self.items.get(&f).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn set(&mut self, f: Field, v: Vec<String>) {
        self.items.insert(f, v);
    }

    /// 某个词是否已经写在别的组里；是就返回那一组标题。
    #[cfg(test)]
    pub fn owner_elsewhere(&self, word: &str, current_title: &str) -> Option<&'static str> {
        for g in groups() {
            if g.title == current_title {
                continue;
            }
            if self.get(g.field).iter().any(|x| x == word) {
                return Some(g.title);
            }
        }
        None
    }

    /// 在给定基底上解析配置文件。本地文件缺字段时保留基底里的值。
    fn parse_with_base(mut out: KeywordSettings, text: &str) -> KeywordSettings {
        if !text.starts_with(MAGIC) {
            return out;
        }
        let mut seen: HashSet<Field> = HashSet::new();
        let mut zodiac_seen = false;
        for line in text.lines().skip(1) {
            let mut parts = line.splitn(3, '\t');
            match parts.next() {
                Some("kw") => {
                    let Some(f) = parts.next().and_then(Field::from_key) else {
                        continue;
                    };
                    let item = parts.next().map(unescape).unwrap_or_default();
                    if seen.insert(f) {
                        out.set(f, Vec::new());
                    }
                    if !item.is_empty() {
                        if let Some(v) = out.items.get_mut(&f) {
                            v.push(item);
                        }
                    }
                }
                Some("zodiac") => {
                    if !zodiac_seen {
                        out.zodiac_variants.clear();
                        zodiac_seen = true;
                    }
                    if let (Some(v), Some(c)) = (parts.next(), parts.next()) {
                        let v = unescape(v);
                        let c = unescape(c);
                        if !v.is_empty() && !c.is_empty() {
                            out.zodiac_variants.insert(v, c);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

const MAGIC: &str = "wxv-keywords\t1";

fn store_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("WechatViewer").join("keywords.txt")
}

pub fn load() -> KeywordSettings {
    let Ok(text) = fs::read_to_string(store_path()) else {
        // 首次运行 / 换电脑：直接用编译进 exe 的默认配置。
        return KeywordSettings::default();
    };
    if !text.starts_with(MAGIC) {
        return KeywordSettings::default();
    }
    parse(&text)
}

pub fn save(settings: &KeywordSettings) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, serialize(settings)).is_err() {
        return;
    }
    let _ = fs::rename(&tmp, &path);
}

pub fn serialize(settings: &KeywordSettings) -> String {
    let mut out = String::from(MAGIC);
    out.push('\n');
    for f in ALL_FIELDS {
        let items = settings.get(*f);
        if items.is_empty() {
            // 空组也写一行，才能把「删光」保存下来，而不是回落到默认值。
            out.push_str("kw\t");
            out.push_str(f.key());
            out.push('\t');
            out.push('\n');
        }
        for item in items {
            out.push_str("kw\t");
            out.push_str(f.key());
            out.push('\t');
            out.push_str(&escape(item));
            out.push('\n');
        }
    }
    out.push_str("zodiac\n");
    for (variant, correct) in &settings.zodiac_variants {
        out.push_str("zodiac\t");
        out.push_str(&escape(variant));
        out.push('\t');
        out.push_str(&escape(correct));
        out.push('\n');
    }
    out
}

/// 解析本地配置文件：缺字段时继承内置默认配置。
pub fn parse(text: &str) -> KeywordSettings {
    KeywordSettings::parse_with_base(KeywordSettings::embedded(), text)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_reference_keywords() {
        let s = KeywordSettings::default();
        assert!(s.get(Field::ThreeInThree).contains(&"三中三".to_string()));
        assert!(s.get(Field::StakePrefix).contains(&"各".to_string()));
        assert!(s.get(Field::HongKong).contains(&"香港".to_string()));
        assert_eq!(s.zodiac_variants.get("免"), Some(&"兔".to_string()));
    }

    #[test]
    fn default_uses_embedded_user_config() {
        let s = KeywordSettings::default();
        // 当前配置相对源码默认值做过的调整：特码组多了「特」，金额后缀是「米」。
        assert!(s.get(Field::SpecialNumber).contains(&"特".to_string()));
        assert!(s.get(Field::StakeSuffix).contains(&"米".to_string()));
        assert!(!s.get(Field::StakeSuffix).contains(&"只".to_string()));
        assert!(s.get(Field::TotalStake).contains(&"=".to_string()));
        assert!(!s.get(Field::StakePrefix).contains(&"=".to_string()));
    }

    #[test]
    fn roundtrip_keeps_edits_and_empty_groups() {
        let mut s = KeywordSettings::default();
        s.set(Field::SpecialNumber, vec!["单号".to_string()]);
        s.set(Field::FlatNumber, Vec::new());
        s.zodiac_variants.insert("免".to_string(), "兔".to_string());
        s.zodiac_variants.insert("侯".to_string(), "猴".to_string());

        let loaded = parse(&serialize(&s));
        assert_eq!(loaded.get(Field::SpecialNumber), &["单号".to_string()]);
        assert!(loaded.get(Field::FlatNumber).is_empty());
        assert_eq!(loaded.zodiac_variants.get("侯"), Some(&"猴".to_string()));
    }

    #[test]
    fn conflict_is_found_across_groups() {
        let s = KeywordSettings::default();
        assert_eq!(
            s.owner_elsewhere("三中三", "三中三关键词"),
            None
        );
        assert_eq!(
            s.owner_elsewhere("三中三", "二中二关键词"),
            Some("三中三关键词")
        );
    }

    #[test]
    fn parse_requires_magic() {
        assert!(parse("kw\tthreeInThree\tx\n")
            .get(Field::ThreeInThree)
            .contains(&"三中三".to_string()));
    }
}

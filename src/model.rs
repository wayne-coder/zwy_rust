//! 数据模型：与界面无关的纯数据结构。

use std::collections::HashMap;
use std::path::PathBuf;

use crate::decrypt::KeySpec;

#[derive(Clone, Default)]
pub struct Avatar {
    pub width: usize,
    pub height: usize,
    /// RGBA8，逐像素 4 字节
    pub rgba: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct Person {
    pub username: String,
    pub nick_name: String,
    pub remark: String,
    pub alias: String,
    pub display_name: String,
    pub is_owner: bool,
    pub is_self: bool,
    pub avatar: Option<Avatar>,
}

#[derive(Clone, Default)]
pub struct Group {
    pub username: String,
    pub name: String,
    #[allow(dead_code)]
    pub owner: String,
    pub members: Vec<Person>,
    pub has_member_list: bool,
    pub avatar: Option<Avatar>,
}

/// 注单/消息列表的一行。
#[derive(Clone, Debug)]
pub struct MsgRow {
    pub kind: String,
    pub nick: String,
    pub content: String,
    pub time: String,
    /// 用于排序（sort_seq 毫秒或 create_time*1000）
    pub sort: i64,
    /// 会话 username（联系人 wxid 或群 xxx@chatroom）
    pub chat: String,
    /// 发送者 username（清空注单时按会员过滤）
    pub sender: String,
}

pub struct ImportResult {
    pub rows: Vec<MsgRow>,
    pub summary: String,
}

pub struct AppData {
    pub self_wxid: String,
    pub self_name: String,
    pub account_dir: String,
    /// 账号根目录（.../wxid_xxx_xxxx）
    pub account_path: PathBuf,
    /// 解密缓存目录（退出时删除）
    pub workdir: PathBuf,
    /// 该账号所有数据库的 rel → 密钥
    pub keys: HashMap<String, KeySpec>,
    pub data_dir: String,
    pub contacts: Vec<Person>,
    pub groups: Vec<Group>,
    pub cached_avatars: usize,
    pub downloaded_avatars: usize,
    pub missing_avatars: usize,
    pub elapsed_ms: u128,
    pub key_source: String,
}

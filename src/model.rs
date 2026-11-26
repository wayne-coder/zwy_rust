//! 数据模型：与界面无关的纯数据结构。

use std::collections::{HashMap, HashSet};
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

/// 用户自定义会员分组：只包含联系人会话与群成员，没有「整群」维度。
/// 只保存 username，与显示名解耦（改备注或重新读取微信数据后仍能对应）。
#[derive(Clone, Default)]
pub struct MemberGroup {
    pub name: String,
    /// 联系人会话 username
    pub contacts: HashSet<String>,
    /// (群 username, 成员 username)
    pub members: HashSet<(String, String)>,
    /// 上家（联系人会话 username）：他的注单在风险页按减额记。
    pub upline_contacts: HashSet<String>,
    /// 上家（群 username, 成员 username）。
    pub upline_members: HashSet<(String, String)>,
}

impl MemberGroup {
    /// (联系人, 群成员) 数量。
    pub fn counts(&self) -> (usize, usize) {
        (self.contacts.len(), self.members.len())
    }

    /// 用当前选择覆盖分组内容（整群选择没有分组维度，不参与）。
    pub fn replace_selection(
        &mut self,
        contacts: &HashSet<String>,
        members: &HashSet<(String, String)>,
    ) {
        self.contacts = contacts.clone();
        self.members = members.clone();
        // 上家只在仍是本组成员时保留。
        self.upline_contacts.retain(|u| self.contacts.contains(u));
        self.upline_members.retain(|k| self.members.contains(k));
    }

    /// 这条消息是不是本组上家发的（联系人按 username，群成员按 (群, 成员)）。
    pub fn is_upline(&self, chat: &str, sender: &str) -> bool {
        self.upline_contacts.contains(sender)
            || self.upline_contacts.contains(chat)
            || self
                .upline_members
                .iter()
                .any(|(c, m)| c == chat && m == sender)
    }

    /// 判断一条注单是否属于本分组：联系人会话或该群内指定成员发送的消息。
    pub fn matches(&self, row: &MsgRow) -> bool {
        self.contacts.contains(&row.chat)
            || self.members.contains(&(row.chat.clone(), row.sender.clone()))
    }
}

/// 微信「引用」消息里被引用的那一条。
#[derive(Clone, Debug, Default)]
pub struct QuoteRef {
    /// 被引用消息的正文（引用链接 / 文件时是标题）。
    pub content: String,
    /// 被引用消息发送人的显示名。
    pub from: String,
    /// 被引用消息发送人的 username（群里用来精确定位）。
    pub from_usr: String,
    /// 被引用消息的发送时间（秒级时间戳，0 = 未知）。
    pub create_time: i64,
    /// 被引用消息的微信类型（1 = 文字，3 = 图片，49 = appmsg…）。
    pub msg_type: i64,
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
    /// 链接类型消息里的跳转地址（其他类型为空）。
    pub link_url: Option<String>,
    /// 「引用」类型消息里被引用的原文。
    pub quote: Option<QuoteRef>,
}

pub struct ImportResult {
    pub rows: Vec<MsgRow>,
    pub summary: String,
}

pub struct AppData {
    pub self_wxid: String,
    pub self_name: String,
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
    /// 本次读取是否从**正在运行的微信进程内存**里拿到了密钥。
    /// true = 检测时微信确实处于登录状态（缓存路径不算）。
    pub key_live: bool,
}

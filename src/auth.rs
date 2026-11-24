//! 在线鉴权客户端，与 `zwy_wails_v1` 共用同一台服务器（那套后台已经部署好）。
//!
//! 端点（与 `zwy_wails_v1/auth_service.go` 一致）：
//! - `POST /api/v1/login`    `{username, password, product, clientId}` → `{token, account}`
//! - `GET  /api/v1/account`  `Authorization: Bearer <token>` → `{account}`
//! - `POST /api/v1/password` `{oldPassword, newPassword}`（Bearer）→ `{token, account}`
//!
//! 令牌**只在内存**，不落盘；「记住密码」存 `%LOCALAPPDATA%\WechatViewer\.auth-remember`。
//! 服务端地址：`<exe>/ZWY_Data/server.json` → `%LOCALAPPDATA%\WechatViewer\server.json`
//! → 内置默认（与参考项目同一个地址）。
//!
//! 心跳：每 60 秒 `GET /api/v1/account`。只有服务器**明确**说令牌失效（401/403）才清
//! 会话；网络错误不动，避免网络抖一下就把正在录单的用户踢回登录页。

use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, INTERNET_DEFAULT_HTTP_PORT, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

/// 编译进客户端的默认服务端地址（与 `zwy_wails_v1/build_config.go` 相同）。
const DEFAULT_SERVER_URL: &str = "http://111.228.41.20:8081";
/// 登录时声明的产品，服务端据此分栏（`zwy_wails_v1` 的 productID）。
const PRODUCT_ID: &str = "zwy";
const HEARTBEAT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT_MS: u32 = 15_000;

// ── 数据 ─────────────────────────────────────────────────────────────────

/// 服务器返回的账号信息（永远不含 hash/salt）。
#[derive(Clone, Debug, Default)]
pub struct Account {
    pub username: String,
    /// Unix 毫秒；0 = 永不过期
    pub expires_at: i64,
    pub note: String,
}

impl Account {
    fn from_json(v: &Value) -> Option<Self> {
        let username = v.get("username")?.as_str()?.to_string();
        if username.is_empty() {
            return None;
        }
        Some(Self {
            username,
            expires_at: v.get("expiresAt").and_then(Value::as_i64).unwrap_or(0),
            note: v
                .get("note")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// 渲染登录态 / 授权弹窗用的快照。
#[derive(Clone, Debug, Default)]
pub struct StateView {
    pub username: String,
    /// 「365天 12小时 35分钟」/「永不过期」/「已过期」
    pub remaining_text: String,
    pub note: String,
    pub server_url: String,
}

#[derive(Clone, Debug, Default)]
pub struct LoginOutcome {
    pub success: bool,
    /// 失败时的错误码（服务端码 / network / bad_response …）
    pub code: String,
    /// 可以直接显示给用户的中文
    pub message: String,
    /// 仅 too_many_attempts：多少毫秒后可以重试
    pub retry_after_ms: i64,
}

#[derive(Clone, Debug, Default)]
pub struct ChangeOutcome {
    pub success: bool,
    pub message: String,
}

#[derive(Default)]
struct Shared {
    token: String,
    account: Option<Account>,
    /// 登录响应里的属性词映射（`attrMap.words`）；None = 老服务器没下发。
    attr_words: Option<HashMap<String, Vec<u32>>>,
}

#[derive(Clone)]
pub struct Auth {
    server_url: String,
    client_id: String,
    shared: Arc<Mutex<Shared>>,
    /// 每次登录 / 登出自增；心跳线程发现自己的代数过期就退出。
    generation: Arc<AtomicU64>,
}

// ── 服务 ─────────────────────────────────────────────────────────────────

impl Auth {
    pub fn new() -> Self {
        Self {
            server_url: resolve_server_url(),
            client_id: machine_client_id(),
            shared: Arc::new(Mutex::new(Shared::default())),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn authenticated(&self) -> bool {
        let s = self.shared.lock().unwrap();
        if s.token.is_empty() {
            return false;
        }
        match &s.account {
            Some(a) => a.expires_at == 0 || now_ms() <= a.expires_at,
            None => false,
        }
    }

    pub fn state(&self) -> StateView {
        let s = self.shared.lock().unwrap();
        if s.token.is_empty() || s.account.is_none() {
            return StateView {
                server_url: self.server_url.clone(),
                ..Default::default()
            };
        }
        let a = s.account.as_ref().unwrap();
        StateView {
            username: a.username.clone(),
            remaining_text: remaining_text_of(a.expires_at),
            note: a.note.clone(),
            server_url: self.server_url.clone(),
        }
    }

    /// 登录页预填内容：(账号, 密码, 是否记住)。
    pub fn remembered(&self) -> (String, String, bool) {
        let Ok(raw) = fs::read_to_string(remember_path()) else {
            return (String::new(), String::new(), false);
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            let _ = fs::remove_file(remember_path());
            return (String::new(), String::new(), false);
        };
        (
            v.get("username").and_then(Value::as_str).unwrap_or("").to_string(),
            v.get("password").and_then(Value::as_str).unwrap_or("").to_string(),
            true,
        )
    }

    /// 走一次完整登录。阻塞（最坏 15 秒），界面应在后台线程里调用。
    pub fn login(&self, username: &str, password: &str, remember: bool) -> LoginOutcome {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return outcome("bad_credentials", "请输入账号和密码", 0);
        }
        let body = json!({
            "username": username,
            "password": password,
            "product": PRODUCT_ID,
            "clientId": self.client_id,
        });
        let url = format!("{}/api/v1/login", self.server_url);
        let (status, out) = match http_json("POST", &url, "", Some(&body)) {
            Ok(v) => v,
            // 网络层失败**不清**记住的密码：多半是断网 / 服务器没开。
            Err(()) => return outcome("network", default_message("network"), 0),
        };

        if status == 200 {
            let token = out
                .as_ref()
                .and_then(|v| v.get("token"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let account = out
                .as_ref()
                .and_then(|v| v.get("account"))
                .and_then(Account::from_json);
            if token.is_empty() || account.is_none() {
                return outcome("bad_response", default_message("bad_response"), 0);
            }
            let attr_words = out.as_ref().and_then(parse_attr_words);
            {
                let mut s = self.shared.lock().unwrap();
                s.token = token;
                s.account = account;
                // 无条件覆盖：老服务器不发这个字段就是 None，界面退回内置表。
                s.attr_words = attr_words;
            }
            write_remembered(username, password, remember);
            self.start_heartbeat();
            return LoginOutcome {
                success: true,
                code: String::new(),
                message: "登录成功".into(),
                retry_after_ms: 0,
            };
        }

        let (code, message) = api_code_message(out.as_ref(), status);
        // 被明确拒绝（账号删除 / 停用 / 改密）时抹掉记住的密码；429 和 in_use 是临时的。
        if status != 429 && code != "in_use" {
            clear_remembered();
        }
        let retry = out
            .as_ref()
            .and_then(|v| v.get("retryAfterMs"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        outcome(&code, &message, retry)
    }

    /// 退出登录：只清本地，不发请求（无状态令牌撤不掉）。
    pub fn logout(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        {
            let mut s = self.shared.lock().unwrap();
            s.token.clear();
            s.account = None;
            s.attr_words = None;
        }
        clear_remembered();
    }

    /// 主动拉一次账号信息（授权弹窗打开时用）。阻塞，界面应在后台线程里调用。
    pub fn refresh(&self) {
        let token = self.shared.lock().unwrap().token.clone();
        if token.is_empty() {
            return;
        }
        let url = format!("{}/api/v1/account", self.server_url);
        let Ok((status, out)) = http_json("GET", &url, &token, None) else {
            return;
        };
        match status {
            200 => {
                if let Some(a) = out.as_ref().and_then(|v| v.get("account")).and_then(Account::from_json) {
                    let mut s = self.shared.lock().unwrap();
                    if s.token == token {
                        s.account = Some(a);
                    }
                }
            }
            401 | 403 => self.revoke(),
            _ => {}
        }
    }

    /// 改密码。成功后服务器会换发新令牌，这里直接采纳——否则下一次心跳就会被踢。
    pub fn change_password(&self, old_password: &str, new_password: &str) -> ChangeOutcome {
        let token = self.shared.lock().unwrap().token.clone();
        if token.is_empty() {
            return ChangeOutcome {
                success: false,
                message: default_message("not_logged_in").into(),
            };
        }
        if old_password.is_empty() || new_password.is_empty() {
            return ChangeOutcome {
                success: false,
                message: "请填写原密码和新密码".into(),
            };
        }
        let body = json!({ "oldPassword": old_password, "newPassword": new_password });
        let url = format!("{}/api/v1/password", self.server_url);
        let (status, out) = match http_json("POST", &url, &token, Some(&body)) {
            Ok(v) => v,
            Err(()) => {
                return ChangeOutcome {
                    success: false,
                    message: default_message("network").into(),
                };
            }
        };
        if status != 200 {
            let (_code, message) = api_code_message(out.as_ref(), status);
            return ChangeOutcome { success: false, message };
        }
        let new_token = out
            .as_ref()
            .and_then(|v| v.get("token"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if new_token.is_empty() {
            return ChangeOutcome {
                success: false,
                message: "密码可能已修改，但服务器没有返回新凭据，请重新登录".into(),
            };
        }
        let account = out
            .as_ref()
            .and_then(|v| v.get("account"))
            .and_then(Account::from_json);
        {
            let mut s = self.shared.lock().unwrap();
            s.token = new_token;
            if account.is_some() {
                s.account = account;
            }
        }
        // 记住的那份密码也跟着换；原本没记住就什么都不做。
        let (user, _, remember) = self.remembered();
        if remember && !user.is_empty() {
            write_remembered(&user, new_password, true);
        }
        ChangeOutcome {
            success: true,
            message: "密码已修改".into(),
        }
    }

    fn revoke(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let mut s = self.shared.lock().unwrap();
        s.token.clear();
        s.account = None;
        s.attr_words = None;
    }

    fn start_heartbeat(&self) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let me = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(HEARTBEAT);
            if me.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let token = me.shared.lock().unwrap().token.clone();
            if token.is_empty() {
                return;
            }
            let url = format!("{}/api/v1/account", me.server_url);
            let Ok((status, out)) = http_json("GET", &url, &token, None) else {
                // 网络错误什么都不做。
                continue;
            };
            if me.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            match status {
                200 => {
                    if let Some(a) =
                        out.as_ref().and_then(|v| v.get("account")).and_then(Account::from_json)
                    {
                        let mut s = me.shared.lock().unwrap();
                        if s.token == token {
                            s.account = Some(a);
                        }
                    }
                }
                401 | 403 => {
                    me.generation.fetch_add(1, Ordering::SeqCst);
                    let mut s = me.shared.lock().unwrap();
                    s.token.clear();
                    s.account = None;
                    s.attr_words = None;
                    return;
                }
                _ => {}
            }
        });
    }
}

impl Auth {
    /// 本次登录拿到的属性词映射（服务器没下发 / 解析不合法时为 None）。
    pub fn attr_words(&self) -> Option<HashMap<String, Vec<u32>>> {
        self.shared.lock().unwrap().attr_words.clone()
    }
}

/// 解析登录响应里的 `attrMap.words`。**整份校验，任何一条不合格就整份不要**：
/// 半份生效的属性词表会算错钱且不报错，退回内置表反而是安全的一侧。
fn parse_attr_words(body: &Value) -> Option<HashMap<String, Vec<u32>>> {
    let words = body.get("attrMap")?.get("words")?.as_object()?;
    let mut out: HashMap<String, Vec<u32>> = HashMap::new();
    for (key, value) in words {
        let list = value.as_array()?;
        let mut nums = Vec::with_capacity(list.len());
        for item in list {
            let n = item.as_u64()?;
            if !(1..=49).contains(&n) {
                return None;
            }
            nums.push(n as u32);
        }
        if nums.is_empty() {
            return None;
        }
        out.insert(key.clone(), nums);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

fn outcome(code: &str, message: &str, retry_after_ms: i64) -> LoginOutcome {
    LoginOutcome {
        success: false,
        code: code.to_string(),
        message: message.to_string(),
        retry_after_ms,
    }
}

/// 服务端错误码 / 本地错误码的中文兜底（与 `zwy_wails_v1/lib/authMessages.ts` 一致）。
pub fn default_message(code: &str) -> &'static str {
    match code {
        "bad_credentials" => "账号或密码错误，请检查后重试。",
        "disabled" => "账号已被停用，请联系管理员。",
        "expired" => "账号已过期，请联系管理员续期。",
        "in_use" => "该账号正在另一台电脑上使用，请对方退出后再登录。",
        "too_many_attempts" => "尝试次数过多，请稍后再试。",
        "weak_password" => "密码不符合要求，请重新设置。",
        "bad_old_password" => "原密码错误。",
        "token_invalid" | "unauthorized" => "登录已失效，请重新登录。",
        "not_found" => "账号不存在，请联系管理员。",
        "bad_request" => "请求有误，请重试。",
        "server_error" => "服务器出错了，请稍后再试。",
        "network" => "无法连接服务器，请检查网络或联系管理员。",
        "bad_response" => "服务器返回的数据无法识别，请联系管理员。",
        "not_logged_in" => "请先登录。",
        _ => "操作失败，请稍后再试。",
    }
}

/// 语气：warning 用于「等一会 / 重新登录」这类可恢复状态。
pub fn is_warning(code: &str) -> bool {
    matches!(
        code,
        "in_use" | "too_many_attempts" | "token_invalid" | "unauthorized" | "not_logged_in"
    )
}

fn api_code_message(out: Option<&Value>, status: u16) -> (String, String) {
    let mut code = String::new();
    let mut message = String::new();
    if let Some(v) = out {
        if let Some(c) = v.get("code").and_then(Value::as_str) {
            if !c.is_empty() {
                code = c.to_string();
            }
        }
        if let Some(m) = v.get("message").and_then(Value::as_str) {
            if !m.is_empty() {
                message = m.to_string();
            }
        }
    }
    if code.is_empty() {
        code = if status >= 500 { "server_error" } else { "unknown" }.to_string();
    }
    if message.is_empty() {
        message = default_message(&code).to_string();
    }
    (code, message)
}

// ── 剩余时间 ─────────────────────────────────────────────────────────────

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 与参考项目 `remainingTextOf` 同一格式：「365天 12小时 35分钟」。
fn remaining_text_of(expires_at: i64) -> String {
    if expires_at == 0 {
        return "永不过期".to_string();
    }
    let remaining = expires_at - now_ms();
    if remaining <= 0 {
        return "已过期".to_string();
    }
    let total_minutes = remaining / 60_000;
    let days = total_minutes / 1440;
    let hours = (total_minutes % 1440) / 60;
    let minutes = total_minutes % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}天"));
    }
    if hours > 0 {
        parts.push(format!("{hours}小时"));
    }
    if minutes > 0 || parts.is_empty() {
        parts.push(format!("{minutes}分钟"));
    }
    parts.join(" ")
}

// ── 本机身份 ─────────────────────────────────────────────────────────────

/// 主机名的哈希（与参考项目 `clientIDForHost` 一致）。主机名拷不走，便携部署安全。
fn machine_client_id() -> String {
    let host = std::env::var("COMPUTERNAME").unwrap_or_default();
    let host = host.trim().to_lowercase();
    if host.is_empty() {
        return String::new();
    }
    let mut hasher = Sha256::new();
    hasher.update(format!("zwy-client-v1:{host}").as_bytes());
    format!("{:x}", hasher.finalize())
}

// ── 服务端地址 ───────────────────────────────────────────────────────────

fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("WechatViewer")
}

fn remember_path() -> PathBuf {
    data_dir().join(".auth-remember")
}

fn resolve_server_url() -> String {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ZWY_Data").join("server.json"));
        }
    }
    candidates.push(data_dir().join("server.json"));
    for path in candidates {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(url) = v.get("serverUrl").and_then(Value::as_str) {
            let url = url.trim();
            if !url.is_empty() {
                return url.trim_end_matches('/').to_string();
            }
        }
    }
    DEFAULT_SERVER_URL.trim_end_matches('/').to_string()
}

// ── 记住密码 ─────────────────────────────────────────────────────────────

fn write_remembered(username: &str, password: &str, remember: bool) {
    if !remember {
        clear_remembered();
        return;
    }
    let raw = json!({
        "username": username,
        "password": password,
        "savedAt": now_ms(),
    });
    let _ = fs::create_dir_all(data_dir());
    let path = remember_path();
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, raw.to_string()).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

fn clear_remembered() {
    let _ = fs::remove_file(remember_path());
}

// ── HTTP（WinHTTP） ──────────────────────────────────────────────────────

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// 发一个 JSON 请求。`Err(())` 只表示网络层失败；业务错误看状态码和响应体。
fn http_json(
    method: &str,
    url: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, Option<Value>), ()> {
    let (secure, host, port, path) = parse_url(url).ok_or(())?;
    let agent = wide("WechatViewer/1.0");
    let host_w = wide(&host);
    let path_w = wide(&path);
    let verb = wide(method);

    let mut header_text = String::new();
    if body.is_some() {
        header_text.push_str("Content-Type: application/json\r\n");
    }
    if !token.is_empty() {
        header_text.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    let headers = wide(&header_text);
    let body_bytes = body
        .map(|b| serde_json::to_vec(b).unwrap_or_default())
        .unwrap_or_default();

    unsafe {
        let session = Handle(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(());
        }
        let t = REQUEST_TIMEOUT_MS as i32;
        WinHttpSetTimeouts(session.0, t, t, t, t);

        let connect = Handle(WinHttpConnect(session.0, host_w.as_ptr(), port, 0));
        if connect.0.is_null() {
            return Err(());
        }

        let request = Handle(WinHttpOpenRequest(
            connect.0,
            verb.as_ptr(),
            path_w.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            if secure { WINHTTP_FLAG_SECURE } else { 0 },
        ));
        if request.0.is_null() {
            return Err(());
        }

        let header_ptr = if header_text.is_empty() {
            ptr::null()
        } else {
            headers.as_ptr() as *const u16
        };
        let header_len = if header_text.is_empty() {
            0
        } else {
            u32::MAX // -1：WinHTTP 自己按 NUL 结尾算长度
        };
        let body_ptr = if body_bytes.is_empty() {
            ptr::null()
        } else {
            body_bytes.as_ptr() as *const c_void
        };
        let body_len = body_bytes.len() as u32;
        if WinHttpSendRequest(
            request.0,
            header_ptr,
            header_len,
            body_ptr,
            body_len,
            body_len,
            0,
        ) == 0
        {
            return Err(());
        }
        if WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 {
            return Err(());
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
            if data.len() > 1 << 20 {
                break;
            }
        }

        let value = if data.is_empty() {
            None
        } else {
            // 解析失败不算网络错误：对面可能是反向代理，返回的是 HTML。
            serde_json::from_slice::<Value>(&data).ok()
        };
        Ok((status as u16, value))
    }
}

/// `(是否 https, 主机, 端口, 路径)`。只支持 http / https + `host[:port]/path`。
fn parse_url(url: &str) -> Option<(bool, String, u16, String)> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return None;
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (
            host_port,
            if secure {
                INTERNET_DEFAULT_HTTPS_PORT
            } else {
                INTERNET_DEFAULT_HTTP_PORT
            },
        ),
    };
    if host.is_empty() {
        return None;
    }
    Some((secure, host.to_string(), port, path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_urls() {
        let (secure, host, port, path) = parse_url("http://111.228.41.20:8081/api/v1/login").unwrap();
        assert!(!secure);
        assert_eq!(host, "111.228.41.20");
        assert_eq!(port, 8081);
        assert_eq!(path, "/api/v1/login");

        let (secure, host, port, path) = parse_url("https://zwy.example.com/api/v1/account").unwrap();
        assert!(secure);
        assert_eq!(host, "zwy.example.com");
        assert_eq!(port, 443);
        assert_eq!(path, "/api/v1/account");

        assert!(parse_url("ftp://x").is_none());
    }

    #[test]
    fn remaining_text_formats() {
        assert_eq!(remaining_text_of(0), "永不过期");
        assert_eq!(remaining_text_of(now_ms() - 1), "已过期");
        let text = remaining_text_of(now_ms() + 60_000 * 65);
        assert_eq!(text, "1小时 5分钟");
    }

    #[test]
    fn parses_attr_map_words() {
        let body = serde_json::json!({
            "attrMap": {
                "revision": 3,
                "words": { "马": [1, 13, 25, 37, 49], "红波": [1, 2, 7], "0": [10, 20, 30, 40] }
            }
        });
        let words = parse_attr_words(&body).expect("应当解析成功");
        assert_eq!(words["马"], vec![1, 13, 25, 37, 49]);
        assert_eq!(words["0"], vec![10, 20, 30, 40]);

        // 任何一条越界 / 空表都整份不要。
        let bad = serde_json::json!({ "attrMap": { "words": { "马": [0, 50] } } });
        assert!(parse_attr_words(&bad).is_none());
        let empty = serde_json::json!({ "attrMap": { "words": {} } });
        assert!(parse_attr_words(&empty).is_none());
    }

    #[test]
    fn client_id_is_stable_hash() {
        let a = machine_client_id();
        if std::env::var("COMPUTERNAME").map(|h| !h.trim().is_empty()).unwrap_or(false) {
            assert_eq!(a.len(), 64);
            assert_eq!(a, machine_client_id());
        }
    }
}

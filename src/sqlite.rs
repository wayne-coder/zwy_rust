//! 极薄的 SQLite 封装：直接调用 Windows 自带的 winsqlite3.dll。
//!
//! 这样既能完整支持 SQLite 查询，又不需要编译任何 C 代码。

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

use libloading::Library;

type OpenV2 = unsafe extern "C" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
type CloseV2 = unsafe extern "C" fn(*mut c_void) -> c_int;
type Prepare = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    c_int,
    *mut *mut c_void,
    *mut *const c_char,
) -> c_int;
type Step = unsafe extern "C" fn(*mut c_void) -> c_int;
type Finalize = unsafe extern "C" fn(*mut c_void) -> c_int;
type ColumnCount = unsafe extern "C" fn(*mut c_void) -> c_int;
type ColumnType = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type ColumnInt64 = unsafe extern "C" fn(*mut c_void, c_int) -> i64;
type ColumnDouble = unsafe extern "C" fn(*mut c_void, c_int) -> f64;
type ColumnText = unsafe extern "C" fn(*mut c_void, c_int) -> *const u8;
type ColumnBlob = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_void;
type ColumnBytes = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type ErrMsg = unsafe extern "C" fn(*mut c_void) -> *const c_char;

struct Api {
    open_v2: OpenV2,
    close_v2: CloseV2,
    prepare: Prepare,
    step: Step,
    finalize: Finalize,
    column_count: ColumnCount,
    column_type: ColumnType,
    column_int64: ColumnInt64,
    column_double: ColumnDouble,
    column_text: ColumnText,
    column_blob: ColumnBlob,
    column_bytes: ColumnBytes,
    errmsg: ErrMsg,
}

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;
const SQLITE_OPEN_READONLY: c_int = 0x0000_0001;

static API: OnceLock<Api> = OnceLock::new();

fn api() -> Result<&'static Api, String> {
    if let Some(a) = API.get() {
        return Ok(a);
    }
    unsafe {
        let lib: &'static Library = Box::leak(Box::new(
            Library::new("winsqlite3.dll").map_err(|e| format!("加载 winsqlite3.dll 失败: {e}"))?,
        ));
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                *lib.get::<$ty>(concat!($name, "\0").as_bytes())
                    .map_err(|e| format!("winsqlite3.dll 缺少符号: {e}"))?
            }};
        }
        let a = Api {
            open_v2: sym!("sqlite3_open_v2", OpenV2),
            close_v2: sym!("sqlite3_close_v2", CloseV2),
            prepare: sym!("sqlite3_prepare_v2", Prepare),
            step: sym!("sqlite3_step", Step),
            finalize: sym!("sqlite3_finalize", Finalize),
            column_count: sym!("sqlite3_column_count", ColumnCount),
            column_type: sym!("sqlite3_column_type", ColumnType),
            column_int64: sym!("sqlite3_column_int64", ColumnInt64),
            column_double: sym!("sqlite3_column_double", ColumnDouble),
            column_text: sym!("sqlite3_column_text", ColumnText),
            column_blob: sym!("sqlite3_column_blob", ColumnBlob),
            column_bytes: sym!("sqlite3_column_bytes", ColumnBytes),
            errmsg: sym!("sqlite3_errmsg", ErrMsg),
        };
        Ok(API.get_or_init(|| a))
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
pub struct Row(pub Vec<Value>);

impl Row {
    pub fn text(&self, i: usize) -> String {
        match self.0.get(i) {
            Some(Value::Text(s)) => s.clone(),
            Some(Value::Int(n)) => n.to_string(),
            Some(Value::Real(n)) => n.to_string(),
            _ => String::new(),
        }
    }

    pub fn int(&self, i: usize) -> i64 {
        match self.0.get(i) {
            Some(Value::Int(n)) => *n,
            Some(Value::Real(n)) => *n as i64,
            Some(Value::Text(s)) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }

    pub fn blob(&self, i: usize) -> Option<&[u8]> {
        match self.0.get(i) {
            Some(Value::Blob(b)) => Some(b),
            _ => None,
        }
    }

    /// 取字节；TEXT 与 BLOB 都按原始字节返回（消息正文两种存储都会出现）。
    pub fn bytes(&self, i: usize) -> Vec<u8> {
        match self.0.get(i) {
            Some(Value::Blob(b)) => b.clone(),
            Some(Value::Text(t)) => t.as_bytes().to_vec(),
            _ => Vec::new(),
        }
    }
}

pub struct Conn {
    db: *mut c_void,
    api: &'static Api,
}

impl Conn {
    pub fn open_ro(path: &Path) -> Result<Self, String> {
        let api = api()?;
        let cpath = CString::new(path.to_string_lossy().as_bytes().to_vec())
            .map_err(|_| "路径包含非法字符".to_string())?;
        let mut db: *mut c_void = ptr::null_mut();
        let rc = unsafe {
            (api.open_v2)(
                cpath.as_ptr(),
                &mut db,
                SQLITE_OPEN_READONLY,
                ptr::null(),
            )
        };
        if rc != SQLITE_OK || db.is_null() {
            let msg = if db.is_null() {
                format!("打开数据库失败 (code {rc})")
            } else {
                unsafe { errmsg(api, db) }
            };
            if !db.is_null() {
                unsafe {
                    (api.close_v2)(db);
                }
            }
            return Err(msg);
        }
        Ok(Conn { db, api })
    }

    pub fn query(&self, sql: &str) -> Result<Vec<Row>, String> {
        let csql = CString::new(sql).map_err(|_| "SQL 含 NUL".to_string())?;
        let mut stmt: *mut c_void = ptr::null_mut();
        let rc = unsafe {
            (self.api.prepare)(self.db, csql.as_ptr(), -1, &mut stmt, ptr::null_mut())
        };
        if rc != SQLITE_OK {
            return Err(unsafe { errmsg(self.api, self.db) });
        }
        let mut rows = Vec::new();
        loop {
            let rc = unsafe { (self.api.step)(stmt) };
            if rc == SQLITE_ROW {
                let n = unsafe { (self.api.column_count)(stmt) }.max(0) as usize;
                let mut vals = Vec::with_capacity(n);
                for i in 0..n {
                    vals.push(unsafe { column_value(self.api, stmt, i as c_int) });
                }
                rows.push(Row(vals));
            } else if rc == SQLITE_DONE {
                break;
            } else {
                let msg = unsafe { errmsg(self.api, self.db) };
                unsafe {
                    (self.api.finalize)(stmt);
                }
                return Err(msg);
            }
        }
        unsafe {
            (self.api.finalize)(stmt);
        }
        Ok(rows)
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        unsafe {
            (self.api.close_v2)(self.db);
        }
    }
}

unsafe fn errmsg(api: &Api, db: *mut c_void) -> String {
    unsafe {
        let p = (api.errmsg)(db);
        if p.is_null() {
            "SQLite 错误".to_string()
        } else {
            CStr::from_ptr(p).to_string_lossy().to_string()
        }
    }
}

unsafe fn column_value(api: &Api, stmt: *mut c_void, i: c_int) -> Value {
    unsafe {
    match (api.column_type)(stmt, i) {
        1 => Value::Int((api.column_int64)(stmt, i)),
        2 => Value::Real((api.column_double)(stmt, i)),
        3 => {
            let p = (api.column_text)(stmt, i);
            let n = (api.column_bytes)(stmt, i).max(0) as usize;
            if p.is_null() || n == 0 {
                Value::Text(String::new())
            } else {
                let bytes = std::slice::from_raw_parts(p, n);
                Value::Text(String::from_utf8_lossy(bytes).to_string())
            }
        }
        4 => {
            let p = (api.column_blob)(stmt, i) as *const u8;
            let n = (api.column_bytes)(stmt, i).max(0) as usize;
            if p.is_null() || n == 0 {
                Value::Blob(Vec::new())
            } else {
                Value::Blob(std::slice::from_raw_parts(p, n).to_vec())
            }
        }
        _ => Value::Null,
    }
    }
}

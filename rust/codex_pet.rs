#![cfg_attr(not(test), windows_subsystem = "windows")]
//! Native Windows companion. All application logic lives in this file.
//! Artwork: hunmer/aboutme-pets, pets/gpt. The original bytes and attribution
//! are embedded by scripts/prepare-rust-assets.py; no runtime extraction.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    mem::{size_of, zeroed},
    os::windows::{ffi::OsStrExt, io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::core::BOOL;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    Storage::FileSystem::*,
    System::{
        Diagnostics::ToolHelp::*, LibraryLoader::*, Pipes::*, Registry::*, RemoteDesktop::*,
        SystemServices::SS_RIGHT, Threading::*, Time::*,
    },
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const ARCHIVE: &[u8] = include_bytes!("../outputs/rust-assets.bin");
const BASE_IDLE: &str = "待机呼吸休闲";
const UPSTREAM: &str = "https://github.com/hunmer/aboutme-pets/tree/main/pets/gpt";
const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;
const RUN_VALUE: &str = "local.codex.desktop.pet.rust";
const TITLE: &str = "Codex 白龙桌宠 · Rust";
const WIDTH: i32 = 520;
const HEIGHT: i32 = 620;
const PEEK_OPEN: &[u8] = include_bytes!("../assets/edge-peek.webp");
const PEEK_BLINK: &[u8] = include_bytes!("../assets/edge-peek-blink.webp");
const PEEK_TASSEL: &[u8] = include_bytes!("../assets/edge-peek-tassel.webp");

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn option(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|x| x == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn flag(args: &[String], key: &str) -> bool {
    args.iter().any(|s| s == key)
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())
}
fn uuid() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        ^ ((std::process::id() as u128) << 64)
        ^ COUNTER.fetch_add(1, Ordering::Relaxed) as u128)
        .to_le_bytes();
    let mut n = n;
    n[6] = (n[6] & 15) | 64;
    n[8] = (n[8] & 63) | 128;
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        n[0],n[1],n[2],n[3],n[4],n[5],n[6],n[7],n[8],n[9],n[10],n[11],n[12],n[13],n[14],n[15])
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sheet {
    frame_width: usize,
    frame_height: usize,
    columns: usize,
    rows: usize,
    frame_count: usize,
    fps: usize,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Animation {
    id: String,
    name: String,
    category: String,
    spritesheet: Sheet,
    offset: usize,
    length: usize,
    original_length: usize,
}
struct Pack {
    animations: Vec<Animation>,
    body: &'static [u8],
    metadata: Value,
}
impl Pack {
    fn read() -> Result<Self, String> {
        if ARCHIVE.len() < 12 || &ARCHIVE[..8] != b"CDXRUST1" {
            return Err("Invalid embedded artwork".into());
        }
        let length = u32::from_le_bytes(ARCHIVE[8..12].try_into().unwrap()) as usize;
        if length > ARCHIVE.len() - 12 {
            return Err("Truncated artwork index".into());
        }
        let metadata: Value =
            serde_json::from_slice(&ARCHIVE[12..12 + length]).map_err(|e| e.to_string())?;
        let animations: Vec<Animation> =
            serde_json::from_value(metadata["animations"].clone()).map_err(|e| e.to_string())?;
        let body = &ARCHIVE[12 + length..];
        if animations.len() != 92
            || animations.iter().any(|a| {
                a.offset
                    .checked_add(a.length)
                    .is_none_or(|end| end > body.len())
            })
        {
            return Err("Incomplete artwork archive".into());
        }
        Ok(Self {
            animations,
            body,
            metadata,
        })
    }
    fn original(&self, index: usize) -> Result<Vec<u8>, String> {
        let a = self.animations.get(index).ok_or("Unknown animation")?;
        let bytes = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
            &self.body[a.offset..a.offset + a.length],
            a.original_length,
        )
        .map_err(|_| "Damaged compressed artwork")?;
        if bytes.len() != a.original_length {
            return Err("Artwork size mismatch".into());
        }
        Ok(bytes)
    }
    fn base(&self) -> usize {
        self.animations
            .iter()
            .position(|a| a.id == BASE_IDLE)
            .unwrap_or(0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Idle,
    Working,
    Waiting,
    Error,
    Complete,
    Click,
    Drag,
}
fn classify(status: &Value, requests: usize) -> State {
    let kind = status
        .as_str()
        .or_else(|| status.get("type").and_then(Value::as_str))
        .unwrap_or("");
    let waiting = |x: &str| {
        matches!(
            x,
            "waitingOnApproval" | "waitingOnUserInput" | "needsInput" | "waiting_for_input"
        )
    };
    if requests > 0
        || waiting(kind)
        || status["activeFlags"]
            .as_array()
            .is_some_and(|f| f.iter().any(|v| waiting(v.as_str().unwrap_or(""))))
    {
        State::Waiting
    } else if matches!(kind, "systemError" | "failed" | "blocked" | "error") {
        State::Error
    } else if matches!(kind, "active" | "running" | "inProgress") {
        State::Working
    } else {
        State::Idle
    }
}
#[derive(Clone, Debug, Serialize)]
struct Snapshot {
    state: State,
    connected: bool,
    working: usize,
    waiting: usize,
    errors: usize,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            state: State::Idle,
            connected: false,
            working: 0,
            waiting: 0,
            errors: 0,
        }
    }
}
#[derive(Clone)]
struct Shadow {
    status: Value,
    requests: Vec<bool>,
    revision: i64,
    id: String,
    host: String,
}
fn patch_shadow(shadow: &mut Shadow, patches: &Value) {
    if let Some(patches) = patches.as_array() {
        for patch in patches {
            let path: Vec<String> = if let Some(a) = patch["path"].as_array() {
                a.iter()
                    .map(|v| {
                        v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .collect()
            } else {
                patch["path"]
                    .as_str()
                    .unwrap_or("")
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            };
            let Some(key) = path.first().map(String::as_str) else {
                continue;
            };
            let remove = patch["op"] == "remove";
            if key == "threadRuntimeStatus" || key == "status" {
                if path.len() == 1 {
                    shadow.status = patch["value"].clone();
                } else if path.get(1).is_some_and(|p| p == "type") {
                    if !shadow.status.is_object() {
                        shadow.status = json!({});
                    }
                    shadow.status["type"] = patch["value"].clone();
                } else if path.get(1).is_some_and(|p| p == "activeFlags") {
                    if !shadow.status.is_object() {
                        shadow.status = json!({"type":"active"});
                    }
                    if path.len() == 2 {
                        shadow.status["activeFlags"] = if remove {
                            json!([])
                        } else {
                            patch["value"].clone()
                        };
                    } else {
                        let mut flags = shadow.status["activeFlags"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        let at = path[2]
                            .parse::<usize>()
                            .unwrap_or(flags.len())
                            .min(flags.len());
                        if remove {
                            if at < flags.len() {
                                flags.remove(at);
                            }
                        } else if at < 1000 {
                            if at == flags.len() {
                                flags.push(patch["value"].clone());
                            } else {
                                flags[at] = patch["value"].clone();
                            }
                        }
                        shadow.status["activeFlags"] = json!(flags);
                    }
                }
            } else if key == "requests" {
                if path.len() == 1 {
                    shadow.requests =
                        vec![true; patch["value"].as_array().map_or(0, |x| x.len()).min(10000)];
                } else if path.len() == 2 {
                    let at = path[1]
                        .parse::<usize>()
                        .unwrap_or(shadow.requests.len())
                        .min(shadow.requests.len());
                    if remove {
                        if at < shadow.requests.len() {
                            shadow.requests.remove(at);
                        }
                    } else if shadow.requests.len() < 10000 {
                        if patch["op"] == "add" || at == shadow.requests.len() {
                            shadow.requests.insert(at, true);
                        } else {
                            shadow.requests[at] = true;
                        }
                    }
                }
            }
        }
    }
}
fn aggregate(threads: &HashMap<String, Shadow>, complete_until: u64, connected: bool) -> Snapshot {
    let mut result = Snapshot {
        connected,
        ..Snapshot::default()
    };
    for t in threads.values() {
        match classify(&t.status, t.requests.len()) {
            State::Working => result.working += 1,
            State::Waiting => result.waiting += 1,
            State::Error => result.errors += 1,
            _ => {}
        }
    }
    result.state = if result.waiting > 0 {
        State::Waiting
    } else if result.errors > 0 {
        State::Error
    } else if result.working > 0 {
        State::Working
    } else if complete_until > now_ms() {
        State::Complete
    } else {
        State::Idle
    };
    result
}
#[derive(Clone, Debug, Serialize)]
struct QuotaWindow {
    remaining: Option<f64>,
    minutes: Option<u64>,
    resets_at: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
struct Bucket {
    name: String,
    windows: Vec<QuotaWindow>,
    unlimited: bool,
}
#[derive(Clone, Debug, Serialize)]
struct Usage {
    fetched_at: u64,
    from_cli: bool,
    buckets: Vec<Bucket>,
    resets_available: Option<u64>,
}
fn normalize_usage(raw: &Value) -> Usage {
    let buckets: Vec<&Value> = if let Some(by) = raw["rateLimitsByLimitId"]
        .as_object()
        .filter(|o| !o.is_empty())
    {
        by.values().collect()
    } else {
        raw.get("rateLimits")
            .filter(|v| !v.is_null())
            .into_iter()
            .collect()
    };
    Usage {
        fetched_at: now_ms(),
        from_cli: false,
        resets_available: raw["rateLimitResetCredits"]["availableCount"].as_u64(),
        buckets: buckets
            .into_iter()
            .map(|b| Bucket {
                name: b["limitName"].as_str().unwrap_or("Codex").to_string(),
                unlimited: b["credits"]["unlimited"] == true,
                windows: ["primary", "secondary"]
                    .iter()
                    .filter_map(|key| b.get(key).filter(|v| !v.is_null()))
                    .map(|w| QuotaWindow {
                        remaining: w["usedPercent"]
                            .as_f64()
                            .filter(|v| v.is_finite())
                            .map(|v| (100.0 - v).clamp(0.0, 100.0)),
                        minutes: w["windowDurationMins"].as_u64(),
                        resets_at: w["resetsAt"].as_u64(),
                    })
                    .collect(),
            })
            .collect(),
    }
}
fn allowed_tool(name: &str) -> bool {
    matches!(name, "list_threads" | "get_usage_limits")
}

const CLI_READ_LIMIT: u64 = 1024 * 1024;
fn cli_executable(args: &[String]) -> PathBuf {
    option(args, "--codex-exe")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CODEX_CLI_PATH").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("codex.exe"))
}
struct AccountReader(Child);
impl Drop for AccountReader {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn read_cli_usage(executable: &Path) -> Result<Usage, String> {
    let mut process = AccountReader(
        Command::new(executable)
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(".")),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW_FLAG)
            .spawn()
            .map_err(|_| "无法启动 Codex CLI；请检查 CLI 安装或 CODEX_CLI_PATH".to_string())?,
    );
    let mut input = process.0.stdin.take().ok_or("CLI 额度连接写入失败")?;
    let output = process.0.stdout.take().ok_or("CLI 额度连接读取失败")?;
    let (tx, rx) = mpsc::sync_channel(16);
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut bytes = Vec::new();
            match (&mut reader)
                .take(CLI_READ_LIMIT + 1)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) | Err(_) => break,
                Ok(_) if bytes.len() as u64 > CLI_READ_LIMIT => break,
                _ => {}
            }
            if let Ok(message) = serde_json::from_slice::<Value>(&bytes) {
                if tx.send(message).is_err() {
                    break;
                }
            }
        }
    });
    writeln!(input,"{}",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"codex_desktop_pet","title":"Codex Desktop Pet","version":VERSION}}}))
        .map_err(|_|"CLI 额度连接写入失败")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut initialized = false;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let message = rx
            .recv_timeout(remaining)
            .map_err(|_| "CLI 额度读取超时或连接已结束")?;
        let id = message["id"].as_u64();
        if !matches!(id, Some(1 | 2)) {
            continue;
        }
        if message.get("error").is_some() {
            return Err("CLI 无法读取账号额度；请确认已使用 ChatGPT 账号登录".into());
        }
        if !message["result"].is_object() {
            continue;
        }
        if id == Some(1) && !initialized {
            initialized = true;
            writeln!(input, "{}", json!({"method":"initialized"}))
                .map_err(|_| "CLI 额度连接写入失败")?;
            writeln!(
                input,
                "{}",
                json!({"id":2,"method":"account/rateLimits/read"})
            )
            .map_err(|_| "CLI 额度连接写入失败")?;
        } else if id == Some(2) && initialized {
            return Ok(normalize_usage(&message["result"]));
        }
    }
    Err("CLI 额度读取超时".into())
}
fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".codex")
        })
}
fn rollout_time(text: &str) -> Option<u64> {
    let part = |a, b| text.get(a..b)?.parse::<u16>().ok();
    if !text.ends_with('Z') {
        return None;
    }
    let mut st: SYSTEMTIME = unsafe { zeroed() };
    st.wYear = part(0, 4)?;
    st.wMonth = part(5, 7)?;
    st.wDay = part(8, 10)?;
    st.wHour = part(11, 13)?;
    st.wMinute = part(14, 16)?;
    st.wSecond = part(17, 19)?;
    let fraction = text
        .get(19..)?
        .strip_prefix('.')
        .unwrap_or("")
        .trim_end_matches('Z');
    st.wMilliseconds = format!("{fraction:0<3}").get(..3)?.parse().ok()?;
    let mut ft: FILETIME = unsafe { zeroed() };
    if unsafe { SystemTimeToFileTime(&st, &mut ft) } == 0 {
        return None;
    }
    (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64)
        .checked_div(10000)?
        .checked_sub(11644473600000)
}
fn cli_usage(raw: &Value, at: u64) -> Usage {
    let window = |w: &Value| {
        if w.is_object() {
            json!({"usedPercent":w["used_percent"],"windowDurationMins":w["window_minutes"],"resetsAt":w["resets_at"]})
        } else {
            Value::Null
        }
    };
    let mut usage = normalize_usage(
        &json!({"rateLimits":{"limitName":raw["limit_name"],"primary":window(&raw["primary"]),"secondary":window(&raw["secondary"]),"credits":raw["credits"]}}),
    );
    usage.fetched_at = at;
    usage.from_cli = true;
    usage
}
fn is_cli_session(meta: &Value) -> bool {
    match meta["source"].as_str() {
        Some("cli" | "exec") => true,
        // Desktop also uses vscode; only known CLI originators opt in.
        Some("vscode") => matches!(
            meta["originator"].as_str(),
            Some("codex-tui" | "codex_cli_rs" | "codex_exec")
        ),
        _ => false,
    }
}
#[derive(Default)]
struct CliTail {
    offset: u64,
    initialized: bool,
    cli: bool,
    dropping: bool,
    status: Value,
}
struct CliLogs {
    root: PathBuf,
    since: u64,
    files: HashMap<PathBuf, CliTail>,
    usage: Option<Usage>,
    complete_until: u64,
}
impl CliLogs {
    fn new(root: PathBuf, since: u64) -> Self {
        Self {
            root,
            since,
            files: HashMap::new(),
            usage: None,
            complete_until: 0,
        }
    }
    fn consume(&mut self, tail: &mut CliTail, record: &Value, live: bool) {
        if !tail.cli || record["type"] != "event_msg" {
            return;
        }
        let Some(at) = record["timestamp"].as_str().and_then(rollout_time) else {
            return;
        };
        if at < self.since {
            return;
        }
        let p = &record["payload"];
        if p["type"] == "token_count"
            && p["rate_limits"].is_object()
            && self.usage.as_ref().is_none_or(|u| at >= u.fetched_at)
        {
            self.usage = Some(cli_usage(&p["rate_limits"], at));
        }
        let next = match p["type"].as_str().unwrap_or("") {
            "task_started"
            | "turn_started"
            | "exec_command_begin"
            | "patch_apply_begin"
            | "mcp_tool_call_begin" => "active",
            "task_complete" | "turn_complete" => {
                if live && matches!(classify(&tail.status, 0), State::Working | State::Waiting) {
                    self.complete_until = now_ms() + 10000;
                }
                "idle"
            }
            "turn_aborted" | "shutdown_complete" => "idle",
            // Current CLI versions do not persist these transient events.
            "exec_approval_request" | "apply_patch_approval_request" => "waitingOnApproval",
            "request_user_input" => "waitingOnUserInput",
            "error" => "systemError",
            _ => return,
        };
        tail.status = json!(next);
    }
    fn read(&mut self, path: &Path, tail: &mut CliTail) -> std::io::Result<()> {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        if size < tail.offset {
            *tail = CliTail::default();
        }
        let live = tail.initialized;
        if !tail.initialized {
            let mut header = Vec::new();
            (&mut file).take(CLI_READ_LIMIT).read_to_end(&mut header)?;
            let Some(end) = header.iter().position(|b| *b == b'\n') else {
                return Ok(());
            };
            let Ok(meta) = serde_json::from_slice::<Value>(&header[..end]) else {
                return Ok(());
            };
            tail.cli = meta["type"] == "session_meta" && is_cli_session(&meta["payload"]);
            tail.initialized = true;
            tail.offset = (end as u64 + 1).max(size.saturating_sub(CLI_READ_LIMIT));
            tail.dropping = tail.offset > end as u64 + 1;
        }
        if !tail.cli {
            tail.offset = size;
            return Ok(());
        }
        file.seek(SeekFrom::Start(tail.offset))?;
        let mut bytes = Vec::new();
        (&mut file).take(CLI_READ_LIMIT).read_to_end(&mut bytes)?;
        let mut start = 0;
        for (i, b) in bytes.iter().enumerate() {
            if *b != b'\n' {
                continue;
            }
            if !tail.dropping {
                if let Ok(record) = serde_json::from_slice::<Value>(&bytes[start..i]) {
                    self.consume(tail, &record, live);
                }
            }
            tail.dropping = false;
            start = i + 1;
        }
        if start == 0 && bytes.len() as u64 == CLI_READ_LIMIT {
            tail.offset += CLI_READ_LIMIT;
            tail.dropping = true;
        } else {
            tail.offset += start as u64;
        }
        Ok(())
    }
    fn poll(&mut self) -> Snapshot {
        fn walk(dir: &Path, depth: usize, files: &mut Vec<(SystemTime, PathBuf)>) {
            if depth > 4 {
                return;
            }
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for e in entries.filter_map(Result::ok) {
                let Ok(kind) = e.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    walk(&e.path(), depth + 1, files);
                } else if kind.is_file()
                    && e.file_name().to_string_lossy().starts_with("rollout-")
                    && e.path().extension().is_some_and(|v| v == "jsonl")
                {
                    if let Ok(at) = e.metadata().and_then(|m| m.modified()) {
                        files.push((at, e.path()));
                    }
                }
            }
        }
        let mut entries = Vec::new();
        walk(&self.root, 0, &mut entries);
        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.truncate(50);
        let seen: HashSet<_> = entries.iter().map(|(_, p)| p.clone()).collect();
        self.files.retain(|p, _| seen.contains(p));
        for (modified, path) in entries {
            let mut tail = self.files.remove(&path).unwrap_or_default();
            if self.read(&path, &mut tail).is_ok() {
                if modified
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    < self.since as u128
                {
                    tail.status = json!("idle");
                }
                self.files.insert(path, tail);
            }
        }
        let threads = self
            .files
            .iter()
            .filter(|(_, t)| t.cli)
            .map(|(p, t)| {
                (
                    p.to_string_lossy().into_owned(),
                    Shadow {
                        status: t.status.clone(),
                        requests: Vec::new(),
                        revision: 0,
                        id: String::new(),
                        host: "cli".into(),
                    },
                )
            })
            .collect();
        aggregate(&threads, self.complete_until, true)
    }
    fn usage(&self) -> Result<Usage, String> {
        self.usage.clone().ok_or_else(|| {
            "CLI 尚未记录额度；请先完成一次请求（API Key 账号可能不提供额度）".into()
        })
    }
}
struct Pipe {
    file: File,
    buffer: Vec<u8>,
    limit: usize,
}
impl Pipe {
    fn open(name: &str, limit: usize) -> Result<Self, String> {
        let name = format!(r"\\.\pipe\{name}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
            .map_err(|e| e.to_string())?;
        let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
        if unsafe { SetNamedPipeHandleState(file.as_raw_handle(), &mode, null(), null()) } == 0 {
            return Err("Cannot configure Codex pipe".into());
        }
        Ok(Self {
            file,
            buffer: Vec::new(),
            limit,
        })
    }
    fn send(&mut self, value: &Value) -> Result<(), String> {
        let payload = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if payload.len() > self.limit {
            return Err("Outgoing frame too large".into());
        }
        let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(&payload);
        self.file.write_all(&frame).map_err(|e| e.to_string())
    }
    fn take(&mut self) -> Result<Option<Value>, String> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let length = u32::from_le_bytes(self.buffer[..4].try_into().unwrap()) as usize;
        if length == 0 || length > self.limit {
            return Err("Unsupported desktop frame".into());
        }
        if self.buffer.len() < length + 4 {
            return Ok(None);
        }
        let value = serde_json::from_slice(&self.buffer[4..length + 4])
            .map_err(|_| "Invalid desktop frame")?;
        self.buffer.drain(..length + 4);
        Ok(Some(value))
    }
    fn poll(&mut self) -> Result<Option<Value>, String> {
        if let Some(v) = self.take()? {
            return Ok(Some(v));
        }
        let mut bytes = [0u8; 32768];
        for _ in 0..32 {
            let mut available = 0;
            if unsafe {
                PeekNamedPipe(
                    self.file.as_raw_handle(),
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            } == 0
            {
                return Err("Codex pipe closed".into());
            }
            if available == 0 {
                break;
            }
            let count = (available as usize).min(bytes.len());
            match self.file.read(&mut bytes[..count]) {
                Ok(0) => break,
                Ok(n) => {
                    self.buffer.extend_from_slice(&bytes[..n]);
                    if self.buffer.len() > self.limit + 4 {
                        return Err("Desktop frame exceeds limit".into());
                    }
                    if let Some(v) = self.take()? {
                        return Ok(Some(v));
                    }
                }
                Err(e)
                    if e.raw_os_error() == Some(ERROR_NO_DATA as i32)
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    break
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(None)
    }
}
fn recent_thread() -> Option<String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".codex")
        });
    let mut file = File::open(home.join("session_index.jsonl")).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(65536)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .find_map(|line| {
            serde_json::from_str::<Value>(line)
                .ok()?
                .get("id")?
                .as_str()
                .map(String::from)
        })
}
struct Desktop {
    tools: Pipe,
    stream: Pipe,
    client_id: String,
    counter: u64,
    threads: HashMap<String, Shadow>,
    following: HashSet<String>,
    complete_until: u64,
}
impl Desktop {
    fn connect() -> Result<Self, String> {
        let name = fs::read_dir(r"\\.\pipe\")
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with("codex-app-tools-"))
            .ok_or("请先打开 Codex")?;
        let tools = Pipe::open(&name, 8 * 1024 * 1024)?;
        let mut stream = Pipe::open("codex-ipc", 64 * 1024 * 1024)?;
        stream.send(&json!({"type":"request","requestId":uuid(),"sourceClientId":"initializing-client","version":0,"method":"initialize","params":{"clientType":"desktop-pet-rust"}}))?;
        Ok(Self {
            tools,
            stream,
            client_id: String::new(),
            counter: 0,
            threads: HashMap::new(),
            following: HashSet::new(),
            complete_until: 0,
        })
    }
    fn call(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        if !allowed_tool(name) {
            return Err("Read-only desktop bridge".into());
        }
        let context = recent_thread().ok_or("请先在 Codex 中打开一个聊天")?;
        self.counter += 1;
        let id = self.counter;
        self.tools.send(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"namespace":"codex_app","tool":name,"arguments":arguments,"callerSource":"codex","threadId":context,"callId":format!("desktop-pet-rust-{}",uuid()),"turnId":"desktop-pet-observer"}}))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            // Discovery and stream initialization must remain responsive while
            // app tools resolve a request against the active desktop clients.
            self.read_stream()?;
            if let Some(message) = self.tools.poll()? {
                if message["id"] != id {
                    continue;
                }
                let result = &message["result"];
                if message.get("error").is_some() || result["success"] != true {
                    return Err("Codex 读取失败".into());
                }
                let text = result["contentItems"]
                    .as_array()
                    .and_then(|a| a.iter().find(|v| v["type"] == "inputText"))
                    .and_then(|v| v["text"].as_str())
                    .ok_or("Codex 返回格式不兼容")?;
                return serde_json::from_str(text).map_err(|_| "Codex 返回格式不兼容".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err("Codex 响应超时".into())
    }
    fn follow(&mut self, key: &str, enabled: bool) -> Result<(), String> {
        if self.client_id.is_empty() {
            return Ok(());
        }
        let Some(t) = self.threads.get(key) else {
            return Ok(());
        };
        self.stream.send(&json!({"type":"broadcast","method":"thread-stream-following-changed","sourceClientId":self.client_id,"version":1,"params":{"conversationId":t.id,"hostId":t.host,"following":enabled}}))?;
        if enabled {
            self.following.insert(key.to_string());
        } else {
            self.following.remove(key);
        }
        Ok(())
    }
    fn list(&mut self) -> Result<(), String> {
        let data = self.call("list_threads", json!({"limit":50}))?;
        let mut seen = HashSet::new();
        for row in ["pinnedThreads", "threads"]
            .iter()
            .flat_map(|key| data[key].as_array().into_iter().flatten())
        {
            if row["kind"] != "codex" {
                continue;
            }
            let Some(id) = row["id"].as_str() else {
                continue;
            };
            let host = row["hostId"].as_str().unwrap_or("local");
            let key = format!("{host}:{id}");
            if !seen.insert(key.clone()) {
                continue;
            }
            let old = self.threads.get(&key);
            let active = matches!(classify(&row["status"], 0), State::Working | State::Waiting);
            let before = old.map(|t| classify(&t.status, t.requests.len()));
            let next = if active {
                old.cloned().unwrap_or(Shadow {
                    status: row["status"].clone(),
                    requests: Vec::new(),
                    revision: 0,
                    id: id.into(),
                    host: host.into(),
                })
            } else {
                Shadow {
                    status: row["status"].clone(),
                    requests: Vec::new(),
                    revision: 0,
                    id: id.into(),
                    host: host.into(),
                }
            };
            if !active
                && matches!(before, Some(State::Working | State::Waiting))
                && classify(&next.status, 0) == State::Idle
            {
                self.complete_until = now_ms() + 10000;
            }
            self.threads.insert(key.clone(), next);
            if active && !self.following.contains(&key) {
                self.follow(&key, true)?;
            }
            if !active && self.following.contains(&key) {
                self.follow(&key, false)?;
            }
        }
        let removed: Vec<String> = self
            .threads
            .keys()
            .filter(|k| !seen.contains(*k))
            .cloned()
            .collect();
        for key in removed {
            self.follow(&key, false)?;
            self.threads.remove(&key);
        }
        Ok(())
    }
    fn read_stream(&mut self) -> Result<bool, String> {
        let mut changed = false;
        for _ in 0..32 {
            let Some(m) = self.stream.poll()? else { break };
            if m["method"] == "initialize" {
                if let Some(id) = m["result"]["clientId"].as_str() {
                    self.client_id = id.into();
                    self.following.clear();
                    let keys: Vec<String> = self
                        .threads
                        .iter()
                        .filter(|(_, t)| {
                            matches!(
                                classify(&t.status, t.requests.len()),
                                State::Working | State::Waiting
                            )
                        })
                        .map(|(k, _)| k.clone())
                        .collect();
                    for key in keys {
                        self.follow(&key, true)?;
                    }
                }
                continue;
            }
            if m["type"] == "client-discovery-request" {
                self.stream.send(&json!({"type":"client-discovery-response","requestId":m["requestId"],"response":{"canHandle":false}}))?;
                continue;
            }
            if m["type"] != "broadcast" || m["method"] != "thread-stream-state-changed" {
                continue;
            }
            if let Some(targets) = m["targetClientIds"].as_array() {
                if !targets.iter().any(|v| v == &self.client_id) {
                    continue;
                }
            }
            let p = &m["params"];
            let key = format!(
                "{}:{}",
                p["hostId"].as_str().unwrap_or("local"),
                p["conversationId"].as_str().unwrap_or("")
            );
            let Some(t) = self.threads.get_mut(&key) else {
                continue;
            };
            let before = classify(&t.status, t.requests.len());
            let change = &p["change"];
            if change["type"] == "snapshot" {
                let s = &change["conversationState"];
                t.status = s.get("threadRuntimeStatus").unwrap_or(&s["status"]).clone();
                t.requests = vec![true; s["requests"].as_array().map_or(0, |a| a.len()).min(10000)];
                t.revision = change["revision"].as_i64().unwrap_or(0);
            } else if change["type"] == "patches" {
                if t.revision != change["baseRevision"].as_i64().unwrap_or(-1) {
                    self.follow(&key, false)?;
                    self.follow(&key, true)?;
                    continue;
                }
                patch_shadow(t, &change["patches"]);
                t.revision = change["revision"].as_i64().unwrap_or(0);
            }
            if matches!(before, State::Working | State::Waiting)
                && classify(&t.status, t.requests.len()) == State::Idle
            {
                self.complete_until = now_ms() + 10000;
            }
            changed = true;
        }
        Ok(changed)
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        for key in self.following.clone() {
            let _ = self.follow(&key, false);
        }
    }
}
enum BridgeCommand {
    Usage,
    Reconnect,
}
enum Notice {
    State(Snapshot),
    Usage(Result<Usage, String>),
    Atlas(u64, usize, Result<Atlas, String>),
}
fn start_bridge(tx: Sender<Notice>, stop: Arc<AtomicBool>) -> Sender<BridgeCommand> {
    let (commands, rx) = mpsc::channel();
    thread::spawn(move || {
        let args: Vec<String> = std::env::args().collect();
        if flag(&args, "--cli") {
            let since = option(&args, "--codex-since")
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(now_ms);
            let mut logs = CliLogs::new(codex_home().join("sessions"), since);
            let mut next = Instant::now();
            while !stop.load(Ordering::Relaxed) {
                let mut usage = false;
                while let Ok(command) = rx.try_recv() {
                    match command {
                        BridgeCommand::Usage => usage = true,
                        BridgeCommand::Reconnect => {
                            logs = CliLogs::new(codex_home().join("sessions"), now_ms());
                            next = Instant::now();
                        }
                    }
                }
                if usage || Instant::now() >= next {
                    next = Instant::now() + Duration::from_millis(3500);
                    let _ = tx.send(Notice::State(logs.poll()));
                }
                if usage {
                    let _ = tx.send(Notice::Usage(read_cli_usage(&cli_executable(&args))));
                }
                thread::sleep(Duration::from_millis(25));
            }
            return;
        }
        let mut desktop: Option<Desktop> = None;
        let mut next_poll = Instant::now();
        let mut want_usage = false;
        let mut complete = false;
        while !stop.load(Ordering::Relaxed) {
            while let Ok(command) = rx.try_recv() {
                match command {
                    BridgeCommand::Usage => want_usage = true,
                    BridgeCommand::Reconnect => {
                        desktop = None;
                        next_poll = Instant::now();
                    }
                }
            }
            let result = (|| -> Result<(), String> {
                if Instant::now() >= next_poll {
                    next_poll = Instant::now() + Duration::from_millis(3500);
                    if desktop.is_none() {
                        desktop = Some(Desktop::connect()?);
                    }
                    let d = desktop.as_mut().unwrap();
                    d.list()?;
                    let state = aggregate(&d.threads, d.complete_until, true);
                    complete = state.state == State::Complete;
                    let _ = tx.send(Notice::State(state));
                }
                if let Some(d) = desktop.as_mut() {
                    if d.read_stream()? || (complete && d.complete_until <= now_ms()) {
                        let state = aggregate(&d.threads, d.complete_until, true);
                        complete = state.state == State::Complete;
                        let _ = tx.send(Notice::State(state));
                    }
                }
                if want_usage {
                    want_usage = false;
                    let result = desktop
                        .as_mut()
                        .ok_or("请先打开 Codex".to_string())
                        .and_then(|d| d.call("get_usage_limits", json!({})))
                        .map(|v| normalize_usage(&v));
                    let _ = tx.send(Notice::Usage(result));
                }
                Ok(())
            })();
            if result.is_err() {
                desktop = None;
                let _ = tx.send(Notice::State(Snapshot::default()));
                if want_usage {
                    want_usage = false;
                    let _ = tx.send(Notice::Usage(Err("Codex 连接暂不可用".into())));
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
    });
    commands
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn session_id() -> u32 {
    let mut id = 0;
    unsafe {
        ProcessIdToSessionId(GetCurrentProcessId(), &mut id);
    }
    id
}
fn profile_path(args: &[String]) -> PathBuf {
    option(args, "--profile")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
                .join("Codex Desktop Pet Rust")
        })
}
fn identity(profile: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    profile.to_string_lossy().to_lowercase().hash(&mut hash);
    format!("{}.{:x}", session_id(), hash.finish())
}
fn named_event(profile: &Path, kind: &str) -> Handle {
    Handle(unsafe {
        CreateEventW(
            null(),
            1,
            0,
            wide(format!(r"Local\CodexPetRust.{kind}.{}", identity(profile))).as_ptr(),
        )
    })
}
fn signal(profile: &Path, kind: &str) {
    let handle = Handle(unsafe {
        OpenEventW(
            EVENT_MODIFY_STATE,
            0,
            wide(format!(r"Local\CodexPetRust.{kind}.{}", identity(profile))).as_ptr(),
        )
    });
    if !handle.0.is_null() {
        unsafe {
            SetEvent(handle.0);
        }
    }
}
fn singleton(profile: &Path, kind: &str) -> Option<Handle> {
    let handle = Handle(unsafe {
        CreateMutexW(
            null(),
            0,
            wide(format!(r"Local\CodexPetRust.{kind}.{}", identity(profile))).as_ptr(),
        )
    });
    if handle.0.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        None
    } else {
        Some(handle)
    }
}
fn process_handle(pid: u32) -> Option<Handle> {
    let handle =
        Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) });
    if handle.0.is_null() {
        None
    } else {
        Some(handle)
    }
}
fn alive(handle: &Handle) -> bool {
    unsafe { WaitForSingleObject(handle.0, 0) == WAIT_TIMEOUT }
}
unsafe extern "system" fn has_window(hwnd: HWND, param: LPARAM) -> BOOL {
    let pair = &mut *(param as *mut (u32, bool));
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == pair.0 && IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() {
        pair.1 = true;
        return 0;
    }
    1
}
fn find_desktop() -> Option<(u32, Handle)> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
    if snapshot.0 == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while ok != 0 {
        let length = entry
            .szExeFile
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]).to_lowercase();
        if name == "chatgpt.exe" || name == "codex.exe" {
            let mut session = 0;
            unsafe {
                ProcessIdToSessionId(entry.th32ProcessID, &mut session);
            }
            if session == session_id() {
                if let Some(handle) = process_handle(entry.th32ProcessID) {
                    let mut path = [0u16; 32768];
                    let mut size = path.len() as u32;
                    if unsafe {
                        QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut size)
                    } != 0
                        && String::from_utf16_lossy(&path[..size as usize])
                            .to_lowercase()
                            .contains(r"\openai.codex_")
                    {
                        let mut pair = (entry.th32ProcessID, false);
                        unsafe {
                            EnumWindows(Some(has_window), &mut pair as *mut _ as isize);
                        }
                        if pair.1 {
                            return Some((entry.th32ProcessID, handle));
                        }
                    }
                }
            }
        }
        ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    None
}
fn is_cli_command(command: &str) -> bool {
    if command.trim().is_empty() {
        return false;
    }
    let mut count = 0;
    let argv = unsafe { CommandLineToArgvW(wide(command).as_ptr(), &mut count) };
    if argv.is_null() {
        return false;
    }
    let args: Vec<String> = unsafe { std::slice::from_raw_parts(argv, count as usize) }
        .iter()
        .map(|p| {
            let start = *p;
            let mut length = 0;
            unsafe {
                while *start.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16_lossy(std::slice::from_raw_parts(start, length))
            }
        })
        .collect();
    unsafe {
        LocalFree(argv as _);
    }
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            return true;
        }
        if matches!(
            arg,
            "--config"
                | "--enable"
                | "--disable"
                | "--remote"
                | "--remote-auth-token-env"
                | "--image"
                | "--model"
                | "--local-provider"
                | "--profile"
                | "--sandbox"
                | "--cd"
                | "--add-dir"
                | "--ask-for-approval"
                | "-c"
                | "-i"
                | "-m"
                | "-p"
                | "-s"
                | "-C"
                | "-a"
        ) {
            i += 2;
            if i > args.len() {
                return false;
            }
            continue;
        }
        if !arg.starts_with('-') || arg == "-" {
            return !matches!(
                arg,
                "app-server"
                    | "exec-server"
                    | "mcp-server"
                    | "mcp"
                    | "completion"
                    | "login"
                    | "logout"
            );
        }
        i += 1;
    }
    true
}
fn find_cli() -> Option<(u32, Handle)> {
    // WMI supplies the command line, so an app-server daemon is never treated
    // as an interactive CLI. This runs only while the launcher has no owner.
    let script = r#"[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); $s=(Get-Process -Id $PID).SessionId; $rows=@(Get-CimInstance Win32_Process -Filter "Name='codex.exe'" | Where-Object { $_.SessionId -eq $s -and $_.CommandLine -and $_.ExecutablePath -notmatch '\\OpenAI.Codex_' } | Select-Object ProcessId,CommandLine); ConvertTo-Json -InputObject $rows -Compress"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
    rows.iter()
        .filter(|p| p["CommandLine"].as_str().is_some_and(is_cli_command))
        .find_map(|p| {
            let pid = u32::try_from(p["ProcessId"].as_u64()?).ok()?;
            process_handle(pid).map(|h| (pid, h))
        })
}
fn find_codex() -> Option<(u32, Handle)> {
    find_desktop().or_else(find_cli)
}
fn process_since(handle: &Handle) -> u64 {
    let mut created: FILETIME = unsafe { zeroed() };
    let mut exit: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    if unsafe { GetProcessTimes(handle.0, &mut created, &mut exit, &mut kernel, &mut user) } == 0 {
        return now_ms();
    }
    (((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64) / 10000
        - 11644473600000
}
fn set_startup(enabled: bool) -> Result<(), String> {
    let mut key: HKEY = null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Run").as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        )
    };
    if result != ERROR_SUCCESS {
        return Err("无法修改当前用户启动项".into());
    }
    let result = if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let text = wide(format!("\"{}\" --watch", exe.display()));
        unsafe {
            RegSetValueExW(
                key,
                wide(RUN_VALUE).as_ptr(),
                0,
                REG_SZ,
                text.as_ptr() as *const u8,
                (text.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteValueW(key, wide(RUN_VALUE).as_ptr()) }
    };
    unsafe {
        RegCloseKey(key);
    }
    if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err("无法保存启动项".into())
    }
}
fn start_watcher(profile: &Path) -> Result<(), String> {
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--watch")
        .arg("--profile")
        .arg(profile)
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn watcher(args: &[String], profile: &Path) -> Result<(), String> {
    let Some(_guard) = singleton(profile, "Watcher") else {
        return Ok(());
    };
    let stop = named_event(profile, "StopWatcher");
    let fixed = option(args, "--codex-pid")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0);
    let mut owner: Option<(u32, Handle)> = None;
    let mut child: Option<Child> = None;
    let mut dismissed = false;
    let mut last_state = String::new();
    while unsafe { WaitForSingleObject(stop.0, 0) } == WAIT_TIMEOUT {
        if owner.as_ref().is_some_and(|(_, h)| !alive(h)) {
            if let Some(mut p) = child.take() {
                let end = Instant::now() + Duration::from_secs(3);
                while Instant::now() < end && p.try_wait().ok().flatten().is_none() {
                    thread::sleep(Duration::from_millis(50));
                }
                if p.try_wait().ok().flatten().is_none() {
                    let _ = p.kill();
                }
            }
            owner = None;
            dismissed = false;
            if fixed.is_some() {
                break;
            }
        }
        if owner.is_none() {
            owner = if let Some(pid) = fixed {
                process_handle(pid).map(|h| (pid, h))
            } else {
                if flag(args, "--cli") {
                    find_cli()
                } else {
                    find_codex()
                }
            };
        }
        if let Some(p) = child.as_mut() {
            if p.try_wait().map_err(|e| e.to_string())?.is_some() {
                child = None;
                dismissed = true;
            }
        }
        if let Some((pid, handle)) = &owner {
            if child.is_none() && !dismissed {
                let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
                command
                    .arg("--pet")
                    .arg("--codex-pid")
                    .arg(pid.to_string())
                    .arg("--profile")
                    .arg(profile)
                    .creation_flags(CREATE_NO_WINDOW_FLAG);
                if flag(args, "--cli") || find_desktop().is_none_or(|(desktop, _)| desktop != *pid)
                {
                    command
                        .arg("--cli")
                        .arg("--codex-since")
                        .arg(process_since(handle).to_string());
                    let mut path = [0u16; 32768];
                    let mut size = path.len() as u32;
                    if unsafe {
                        QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut size)
                    } != 0
                    {
                        command
                            .arg("--codex-exe")
                            .arg(String::from_utf16_lossy(&path[..size as usize]));
                    }
                }
                if flag(args, "--demo") {
                    command.arg("--demo");
                }
                child = Some(command.spawn().map_err(|e| e.to_string())?);
            }
        }
        let state = if dismissed {
            "dismissed"
        } else if child.is_some() {
            "running"
        } else {
            "waiting"
        };
        let text = format!(
            "{state}:{:?}:{:?}",
            owner.as_ref().map(|p| p.0),
            child.as_ref().map(Child::id)
        );
        if text != last_state {
            let _ = write_json(
                &profile.join("host-state.json"),
                &json!({"status":state,"hostPid":std::process::id(),"codexPid":owner.as_ref().map(|p|p.0),"petPid":child.as_ref().map(Child::id)}),
            );
            last_state = text;
        }
        if fixed.is_some() && owner.is_none() {
            break;
        }
        unsafe {
            WaitForSingleObject(stop.0, 1000);
        }
    }
    let _ = write_json(
        &profile.join("host-state.json"),
        &json!({"status":"stopped","hostPid":std::process::id()}),
    );
    Ok(())
}

// GUI and tests follow below. The atlas contains only the visible frame crops;
// transparent margins are removed in memory and restored at the same offsets.
#[derive(Clone)]
struct Frame {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}
struct DockArt {
    left: Frame,
    right: Frame,
    blink_left: Frame,
    blink_right: Frame,
    eye: BoxRect,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct DockSwayPose {
    wing: f32,
    tail: f32,
}
impl DockSwayPose {
    fn at(ms: u64, motion: &str, percent: u16) -> Self {
        if motion == "still" { return Self::default(); }
        let strength = 0.65 * percent.min(400) as f32 / 100.0;
        let t = (ms / 80 * 80) as f64;
        Self {
            wing: (t * std::f64::consts::TAU / 7800.0).sin() as f32 * strength,
            tail: (t * std::f64::consts::TAU / 10600.0).sin() as f32 * strength,
        }
    }
    fn source(self, x: f32, y: f32, w: f32, h: f32, side: DockSide) -> (f32, f32) {
        if self == Self::default() {
            return (x, y);
        }
        fn smooth(a: f32, b: f32, value: f32) -> f32 {
            let t = ((value - a) / (b - a)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        let u = x / (w - 1.0).max(1.0);
        let u = if side == DockSide::Left { u } else { 1.0 - u };
        let v = y / (h - 1.0).max(1.0);
        // Pin the screen-edge roots. Smooth, overlapping envelopes move only
        // the lower wing and tail; the face, ornament and hands stay exact.
        let anchor = smooth(0.12, 0.70, u);
        let wing = smooth(0.59, 0.64, v) * (1.0 - smooth(0.76, 0.84, v));
        let tail = smooth(0.75, 0.82, v) * (1.0 - smooth(0.94, 1.0, v));
        let amplitude = 1.6;
        let dx = amplitude * anchor * (wing * self.wing * 2.0 + tail * self.tail * 8.0);
        let dy = amplitude * anchor * (wing * self.wing * 7.0 + tail * self.tail * 3.0);
        let direction = if side == DockSide::Left { 1.0 } else { -1.0 };
        (x - dx * direction, y - dy)
    }
}
fn dock_color(mut rgba: [u8; 4]) -> [u8; 4] {
    let (r, g, b) = (rgba[0] as f64, rgba[1] as f64, rgba[2] as f64);
    let chroma = r.max(g).max(b) - r.min(g).min(b);
    // Generated hair and membranes are more lavender than the original silver
    // character. Match only low-chroma cool tones with a small luminance lift.
    // Warm skin, the purple iris and alpha are left untouched.
    if rgba[3] != 0 && chroma <= 36.0 && b + 4.0 >= r && r + 3.0 >= g && b >= g + 2.0 {
        let luminance = r * 0.2126 + g * 0.7152 + b * 0.0722;
        for channel in rgba.iter_mut().take(3) {
            *channel = (luminance + (*channel as f64 - luminance) * 0.6 + 1.5).round().clamp(0.0, 255.0) as u8;
        }
    }
    rgba
}
impl DockArt {
    fn read() -> Result<Self, String> {
        fn decode(bytes: &[u8]) -> Result<Frame, String> {
            let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?;
            let (w, h) = decoder.dimensions();
            if !decoder.has_alpha() || w > 2048 || h > 2048 {
                return Err("Invalid embedded dock artwork".into());
            }
            let mut rgba = vec![0; decoder.output_buffer_size().ok_or("Oversized dock artwork")?];
            decoder.read_image(&mut rgba).map_err(|e| e.to_string())?;
            for pixel in rgba.chunks_exact_mut(4) {
                let color = dock_color([pixel[0], pixel[1], pixel[2], pixel[3]]);
                pixel.copy_from_slice(&color);
            }
            let (mut left, mut top, mut right, mut bottom) = (w as usize, h as usize, 0, 0);
            for y in 0..h as usize {
                for x in 0..w as usize {
                    if rgba[(y * w as usize + x) * 4 + 3] > 16 {
                        left = left.min(x);
                        top = top.min(y);
                        right = right.max(x + 1);
                        bottom = bottom.max(y + 1);
                    }
                }
            }
            if right <= left || bottom <= top {
                return Err("Empty embedded dock artwork".into());
            }
            let mut pixels = Vec::with_capacity((right - left) * (bottom - top) * 4);
            for y in top..bottom {
                pixels.extend_from_slice(
                    &rgba[(y * w as usize + left) * 4..(y * w as usize + right) * 4],
                );
            }
            Ok(Frame { x: 0, y: 0, width: right - left, height: bottom - top, pixels })
        }
        fn mirror(frame: &Frame) -> Frame {
            let mut out = frame.clone();
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let source = (y * frame.width + frame.width - 1 - x) * 4;
                    let target = (y * frame.width + x) * 4;
                    out.pixels[target..target + 4].copy_from_slice(&frame.pixels[source..source + 4]);
                }
            }
            out
        }
        let mut left = decode(PEEK_OPEN)?;
        let closed = decode(PEEK_BLINK)?;
        // Only the eye is composited from the generated blink frame. The approved
        // hair, hands, wing and tail therefore never move or change during a blink.
        let eye = BoxRect { x: 103, y: 423, w: 135, h: 108 };
        if eye.x + eye.w > left.width as i32 || eye.y + eye.h > left.height as i32 {
            return Err("Dock eye region is outside the artwork".into());
        }
        let source = BoxRect {
            x: (eye.x as f64 * closed.width as f64 / left.width as f64).round() as i32,
            y: (eye.y as f64 * closed.height as f64 / left.height as f64).round() as i32,
            w: (eye.w as f64 * closed.width as f64 / left.width as f64).round() as i32,
            h: (eye.h as f64 * closed.height as f64 / left.height as f64).round() as i32,
        };
        let mut blink_left = Frame {
            x: 0, y: 0, width: source.w as usize, height: source.h as usize,
            pixels: Vec::with_capacity((source.w * source.h * 4) as usize),
        };
        for y in 0..source.h {
            for x in 0..source.w {
                let at = (((source.y + y) as usize * closed.width) + (source.x + x) as usize) * 4;
                let mut color = [closed.pixels[at], closed.pixels[at + 1],
                    closed.pixels[at + 2], closed.pixels[at + 3]];
                let edge = x.min(y).min(source.w - 1 - x).min(source.h - 1 - y).clamp(0, 8);
                color[3] = (color[3] as i32 * edge / 8) as u8;
                blink_left.pixels.extend_from_slice(&color);
            }
        }
        // Replace only the dangling accessory, keeping the approved head, colors,
        // hands and lower body outside this feathered patch pixel-for-pixel.
        let tassel = decode(PEEK_TASSEL)?;
        let target = BoxRect { x: 276, y: 403, w: 80, h: 215 };
        let source = BoxRect { x: 284, y: 412, w: 80, h: 215 };
        if target.x + target.w > left.width as i32 || target.y + target.h > left.height as i32
            || source.x + source.w > tassel.width as i32 || source.y + source.h > tassel.height as i32 {
            return Err("Tassel patch is outside the artwork".into());
        }
        for y in 0..target.h {
            for x in 0..target.w {
                let edge = x.min(y).min(target.w - 1 - x).min(target.h - 1 - y).clamp(0, 8) as u32;
                if edge == 0 { continue; }
                let at = (((target.y + y) as usize * left.width) + (target.x + x) as usize) * 4;
                let from = (((source.y + y) as usize * tassel.width) + (source.x + x) as usize) * 4;
                let blend = edge * 255 / 8;
                let old_alpha = left.pixels[at + 3] as u32 * (255 - blend);
                let new_alpha = tassel.pixels[from + 3] as u32 * blend;
                let alpha = old_alpha + new_alpha;
                for channel in 0..3 {
                    left.pixels[at + channel] = if alpha == 0 { 0 } else {
                        ((left.pixels[at + channel] as u32 * old_alpha
                            + tassel.pixels[from + channel] as u32 * new_alpha + alpha / 2) / alpha) as u8
                    };
                }
                left.pixels[at + 3] = ((alpha + 127) / 255) as u8;
            }
        }
        // The hands protrude beyond the straight screen-edge cut in the source.
        // Its alpha bounding box therefore leaves an 18 px strip beside the hair,
        // wing and tail. Clip at that cut plus its 2 px antialiasing fringe before
        // mirroring, so both sides touch the screen at every display scale.
        let inset = 20;
        let width = left.width - inset;
        let pixels = left.pixels.chunks_exact(left.width * 4)
            .flat_map(|row| row[inset * 4..].iter().copied()).collect();
        let left = Frame { width, pixels, ..left };
        let eye = BoxRect { x: eye.x - inset as i32, ..eye };
        Ok(Self { right: mirror(&left), blink_right: mirror(&blink_left), left, blink_left, eye })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DockSide { Left, Right }
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DockPlacement {
    side: DockSide,
    anchor: Position,
}
struct DockBlink {
    next: u64,
    until: u64,
}
impl DockBlink {
    fn new(now: u64) -> Self {
        Self { next: now.saturating_add(4500 + random_index(3500) as u64), until: 0 }
    }
    fn closed(&mut self, now: u64, enabled: bool) -> bool {
        if !enabled {
            self.until = 0;
            self.next = now.saturating_add(4500);
            return false;
        }
        if now >= self.next {
            self.until = now.saturating_add(140);
            self.next = now.saturating_add(4500 + random_index(3500) as u64);
        }
        now < self.until
    }
}
fn dock_side(point: POINT, area: RECT, dpi: f64) -> Option<DockSide> {
    let distance = (28.0 * dpi).round() as i32;
    if point.y < area.top || point.y >= area.bottom {
        None
    } else if point.x <= area.left + distance {
        Some(DockSide::Left)
    } else if point.x >= area.right - distance {
        Some(DockSide::Right)
    } else {
        None
    }
}
fn dock_target(point: POINT, area: RECT, dpi: f64, visible: Option<BoxRect>) -> Option<DockSide> {
    if point.y < area.top || point.y >= area.bottom {
        return None;
    }
    if let Some(side) = dock_side(point, area, dpi) {
        return Some(side);
    }
    let visible = visible?;
    if visible.y >= area.bottom || visible.y + visible.h <= area.top {
        return None;
    }
    let distance = (28.0 * dpi).round() as i32;
    if visible.x <= area.left + distance && visible.x + visible.w > area.left {
        Some(DockSide::Left)
    } else if visible.x + visible.w >= area.right - distance && visible.x < area.right {
        Some(DockSide::Right)
    } else {
        None
    }
}
fn dock_layout(area: RECT, side: DockSide, y: i32, dpi: f64, scale: f64, aspect: f64) -> BoxRect {
    let h = ((190.0 * dpi * scale / 0.8).round() as i32)
        .clamp(1, (area.bottom - area.top).max(1));
    let w = ((h as f64 * aspect).round() as i32)
        .clamp(1, (area.right - area.left).max(1));
    BoxRect {
        x: if side == DockSide::Left { area.left } else { area.right - w },
        y: y.clamp(area.top, (area.bottom - h).max(area.top)),
        w, h,
    }
}
#[derive(Clone, Copy)]
enum DockPhase {
    HideBody { anchor: POINT },
    Peek,
    HidePeek { quota: bool },
    Reveal,
}
struct DockSlide {
    phase: DockPhase,
    side: DockSide,
    started: Instant,
    duration: f64,
    pixels: Vec<u32>,
    w: i32,
    h: i32,
    distance: i32,
    area: RECT,
}
fn slide_fraction(t: f64, reveal: bool) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if reveal { 1.0 - (1.0 - t).powi(3) } else { t * t * (3.0 - 2.0 * t) }
}
struct Atlas {
    index: usize,
    frames: Vec<Frame>,
}
fn decode_atlas(pack: &Pack, index: usize) -> Result<Atlas, String> {
    let original = pack.original(index)?;
    let a = &pack.animations[index];
    let s = &a.spritesheet;
    let mut decoder =
        image_webp::WebPDecoder::new(Cursor::new(&original)).map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    if width as usize != s.frame_width * s.columns || height as usize != s.frame_height * s.rows {
        return Err("Animation dimensions mismatch".into());
    }
    decoder.set_memory_limit(512 * 1024 * 1024);
    let alpha = decoder.has_alpha();
    let channels = if alpha { 4 } else { 3 };
    let mut pixels = vec![0; decoder.output_buffer_size().ok_or("Oversized artwork")?];
    decoder.read_image(&mut pixels).map_err(|e| e.to_string())?;
    let mut frames = Vec::with_capacity(s.frame_count);
    for index in 0..s.frame_count {
        let ox = (index % s.columns) * s.frame_width;
        let oy = (index / s.columns) * s.frame_height;
        let mut left = s.frame_width;
        let mut right = 0;
        let mut top = s.frame_height;
        let mut bottom = 0;
        for y in 0..s.frame_height {
            for x in 0..s.frame_width {
                let at = ((oy + y) * width as usize + ox + x) * channels;
                if !alpha || pixels[at + 3] > 0 {
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        let fw = right.saturating_sub(left);
        let fh = bottom.saturating_sub(top);
        let mut frame = Vec::with_capacity(fw * fh * 4);
        for y in top..bottom {
            for x in left..right {
                let at = ((oy + y) * width as usize + ox + x) * channels;
                frame.extend_from_slice(&[
                    pixels[at],
                    pixels[at + 1],
                    pixels[at + 2],
                    if alpha { pixels[at + 3] } else { 255 },
                ]);
            }
        }
        frames.push(Frame {
            x: left.min(s.frame_width),
            y: top.min(s.frame_height),
            width: fw,
            height: fh,
            pixels: frame,
        });
    }
    Ok(Atlas { index, frames })
}
fn clip_range(sheet: &Sheet) -> (usize, usize, u64) {
    let count = sheet.frame_count.max(1);
    let fps = sheet.fps.max(1);
    let first = (fps * 2).min(count / 5);
    (first, count - 1, ((count - first) * 1000 / fps) as u64)
}
fn frame_at(sheet: &Sheet, elapsed: u64, frozen: bool) -> usize {
    let (first, last, _) = clip_range(sheet);
    if frozen {
        first
    } else {
        (first + (elapsed * sheet.fps.max(1) as u64 / 1000) as usize).min(last)
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Position {
    x: i32,
    y: i32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    scale: f64,
    auto_start: bool,
    position: Option<Position>,
    motion: String,
    appearance: String,
    edge_docking: bool,
    dock_sway_percent: u16,
    dock: Option<DockPlacement>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            scale: 0.8,
            auto_start: true,
            position: None,
            motion: "quiet".into(),
            appearance: "clear".into(),
            edge_docking: true,
            dock_sway_percent: 200,
            dock: None,
        }
    }
}
impl Settings {
    fn load(profile: &Path) -> Self {
        let mut s: Self = fs::read(profile.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if ![0.8, 1.0, 1.2].contains(&s.scale) {
            s.scale = 0.8;
        }
        if !["still", "quiet", "normal", "lively"].contains(&s.motion.as_str()) {
            s.motion = "quiet".into();
        }
        if !["original", "clear", "vivid"].contains(&s.appearance.as_str()) {
            s.appearance = "clear".into();
        }
        s.dock_sway_percent = s.dock_sway_percent.min(400);
        s
    }
    fn save(&self, profile: &Path) {
        let _ = write_json(&profile.join("settings.json"), self);
    }
}
struct IdleSchedule {
    next: u64,
    gesture: bool,
}
impl IdleSchedule {
    fn new(mode: &str, now: u64) -> Self {
        Self {
            next: now.saturating_add(if mode == "normal" {
                60000
            } else if mode == "quiet" {
                180000
            } else {
                u64::MAX - now
            }),
            gesture: false,
        }
    }
}
fn random_index(length: usize) -> usize {
    static SEED: AtomicU64 = AtomicU64::new(0);
    let mut n = SEED.load(Ordering::Relaxed);
    if n == 0 {
        n = now_ms() ^ 0x9e3779b97f4a7c15;
    }
    n ^= n << 13;
    n ^= n >> 7;
    n ^= n << 17;
    SEED.store(n, Ordering::Relaxed);
    if length == 0 {
        0
    } else {
        n as usize % length
    }
}
fn choose(
    pack: &Pack,
    state: State,
    previous: Option<usize>,
    mode: &str,
    schedule: &mut IdleSchedule,
) -> usize {
    let pool: &[&str] = match state {
        State::Working => &["写代码", "忙碌点按", "思考冒泡", "轻快记录", "吃Token"],
        State::Waiting => &["原地踱步张望", "东张西望", "思考冒泡"],
        State::Complete => &["雀跃庆祝", "开心跃动", "元气挥手"],
        State::Error => &["垂头叹气冒汗", "被吓一跳"],
        State::Click => &["元气挥手", "开心跃动", "害羞惊讶", "挠痒咯咯笑"],
        State::Drag => &["被鼠标拖拽悬空反馈"],
        State::Idle => &[],
    };
    if state == State::Idle && mode != "lively" {
        if mode == "still" {
            return pack.base();
        }
        if schedule.gesture {
            schedule.gesture = false;
            return pack.base();
        }
        if now_ms() < schedule.next {
            return pack.base();
        }
        schedule.gesture = true;
        schedule.next = now_ms() + if mode == "normal" { 60000 } else { 180000 };
    }
    let mut choices: Vec<usize> = pack
        .animations
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            if state != State::Idle {
                pool.contains(&a.id.as_str())
            } else if mode == "quiet" {
                ["东张西望", "悠闲哼歌", "哈欠连天"].contains(&a.id.as_str())
            } else {
                !["工作状态", "余额", "拖拽", "转向", "移动", "点击回应"]
                    .contains(&a.category.as_str())
                    && ![BASE_IDLE, "写代码", "轻快记录", "吃Token"].contains(&a.id.as_str())
            }
        })
        .map(|(i, _)| i)
        .collect();
    if choices.len() > 1 {
        choices.retain(|i| Some(*i) != previous);
    }
    if choices.is_empty() {
        pack.base()
    } else {
        choices[random_index(choices.len())]
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct BoxRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl BoxRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}
fn cursor() -> POINT {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut p);
    }
    p
}
fn window_rect(hwnd: HWND) -> RECT {
    let mut r = unsafe { zeroed() };
    unsafe {
        GetWindowRect(hwnd, &mut r);
    }
    r
}
fn work_area(point: POINT) -> RECT {
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    unsafe {
        GetMonitorInfoW(monitor, &mut info);
    }
    info.rcWork
}
fn constrain(p: POINT, area: RECT, w: i32, h: i32, dpi: f64) -> POINT {
    fn axis(v: i32, min: i32, max: i32) -> i32 {
        if max < min {
            (min + max) / 2
        } else {
            v.clamp(min, max)
        }
    }
    POINT {
        x: axis(
            p.x,
            area.left - (100.0 * dpi) as i32,
            area.right - w + (100.0 * dpi) as i32,
        ),
        y: axis(p.y, area.top - (200.0 * dpi) as i32, area.bottom - h),
    }
}
struct Press {
    cursor: POINT,
    origin: POINT,
    last: POINT,
    moved: bool,
    allow_drag: bool,
    edge_drag: bool,
}
impl Press {
    fn sample(
        &mut self,
        point: POINT,
        area: RECT,
        w: i32,
        h: i32,
        dpi: f64,
    ) -> Option<(POINT, bool)> {
        if !self.allow_drag {
            return None;
        }
        let dx = point.x - self.cursor.x;
        let dy = point.y - self.cursor.y;
        if !self.moved && (dx as i64 * dx as i64 + dy as i64 * dy as i64) <= 25 {
            return None;
        }
        let started = !self.moved;
        self.moved = true;
        let mut p = constrain(
            POINT {
                x: self.origin.x + dx,
                y: self.origin.y + dy,
            },
            area,
            w,
            h,
            dpi,
        );
        if self.edge_drag {
            // Let the painted character reach the border, rather than stopping
            // at the transparent window's padding. At least half the window
            // remains reachable until the release either docks or restores it.
            let margin = w / 2;
            p.x = (self.origin.x + dx).clamp(area.left - margin, area.right - w + margin);
        }
        if !started && p.x == self.last.x && p.y == self.last.y {
            return None;
        }
        self.last = p;
        Some((p, started))
    }
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}
impl Surface {
    fn new(w: i32, h: i32) -> Result<Self, String> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return Err("Cannot allocate drawing context".into());
            }
            let mut info: BITMAPINFO = zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = w;
            info.bmiHeader.biHeight = -h;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return Err("Cannot allocate layered bitmap".into());
            }
            let old = SelectObject(dc, bitmap);
            Ok(Self {
                dc,
                bitmap,
                old,
                bits: bits as *mut u32,
                w,
                h,
            })
        }
    }
    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
struct Canvas {
    w: i32,
    h: i32,
    pixels: Vec<u32>,
    clip: Option<BoxRect>,
}
impl Canvas {
    fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            pixels: vec![0; (w * h) as usize],
            clip: None,
        }
    }
    fn blend(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0
            || y < 0
            || x >= self.w
            || y >= self.h
            || self.clip.is_some_and(|r| !r.contains(x, y))
        {
            return;
        }
        let a = color[3] as u32;
        if a == 0 {
            return;
        }
        let index = (y * self.w + x) as usize;
        let old = self.pixels[index];
        let inv = 255 - a;
        let b = (color[2] as u32 * a + 127) / 255 + ((old & 255) * inv + 127) / 255;
        let g = (color[1] as u32 * a + 127) / 255 + (((old >> 8) & 255) * inv + 127) / 255;
        let r = (color[0] as u32 * a + 127) / 255 + (((old >> 16) & 255) * inv + 127) / 255;
        let a = a + (((old >> 24) & 255) * inv + 127) / 255;
        self.pixels[index] = (a << 24) | (r << 16) | (g << 8) | b;
    }
    fn round_rect(&mut self, rect: BoxRect, radius: i32, color: [u8; 4]) {
        let radius = radius.min(rect.w / 2).min(rect.h / 2).max(0) as f32;
        for y in rect.y.max(0)..(rect.y + rect.h).min(self.h) {
            for x in rect.x.max(0)..(rect.x + rect.w).min(self.w) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let cx = px.clamp(rect.x as f32 + radius, (rect.x + rect.w) as f32 - radius);
                let cy = py.clamp(rect.y as f32 + radius, (rect.y + rect.h) as f32 - radius);
                let coverage =
                    (radius + 0.5 - ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()).clamp(0.0, 1.0);
                let mut c = color;
                c[3] = (color[3] as f32 * coverage).round() as u8;
                self.blend(x, y, c);
            }
        }
    }
    fn text(
        &mut self,
        fonts: &mut Fonts,
        rect: BoxRect,
        text: &str,
        height: i32,
        bold: bool,
        color: [u8; 4],
        right: bool,
    ) {
        if rect.w <= 0 || rect.h <= 0 {
            return;
        }
        let Some(mask) = fonts.mask(rect, text, height, bold, right) else {
            return;
        };
        for y in 0..rect.h {
            for x in 0..rect.w {
                let alpha = mask[(y * rect.w + x) as usize] as u32;
                let mut c = color;
                c[3] = (color[3] as u32 * alpha / 255) as u8;
                self.blend(rect.x + x, rect.y + y, c);
            }
        }
    }
    fn picture(&mut self, frame: &Frame, target: BoxRect, appearance: &str) {
        let sheet = Sheet {
            frame_width: frame.width, frame_height: frame.height,
            columns: 1, rows: 1, frame_count: 1, fps: 1,
        };
        self.sprite(frame, target, &sheet, appearance);
    }
    fn dock_picture(&mut self, frame: &Frame, target: BoxRect, appearance: &str,
        pose: DockSwayPose, side: DockSide) {
        let sheet = Sheet {
            frame_width: frame.width, frame_height: frame.height,
            columns: 1, rows: 1, frame_count: 1, fps: 1,
        };
        self.sprite_warp(frame, target, &sheet, appearance, Some((pose, side)));
    }
    fn sprite(&mut self, frame: &Frame, target: BoxRect, sheet: &Sheet, appearance: &str) {
        self.sprite_warp(frame, target, sheet, appearance, None);
    }
    fn sprite_warp(&mut self, frame: &Frame, target: BoxRect, sheet: &Sheet, appearance: &str,
        warp: Option<(DockSwayPose, DockSide)>) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }
        let sx = target.w as f32 / sheet.frame_width as f32;
        let sy = target.h as f32 / sheet.frame_height as f32;
        let left = target.x + (frame.x as f32 * sx).floor() as i32;
        let top = target.y + (frame.y as f32 * sy).floor() as i32;
        let right = target.x + ((frame.x + frame.width) as f32 * sx).ceil() as i32;
        let bottom = target.y + ((frame.y + frame.height) as f32 * sy).ceil() as i32;
        let (contrast, saturation) = match appearance {
            "original" => (1.0, 1.0),
            "vivid" => (1.23, 1.23),
            _ => (1.14, 1.12),
        };
        for y in top.max(0)..bottom.min(self.h) {
            for x in left.max(0)..right.min(self.w) {
                let fx = ((x - target.x) as f32 + 0.5) / sx - 0.5 - frame.x as f32;
                let fy = ((y - target.y) as f32 + 0.5) / sy - 0.5 - frame.y as f32;
                let (fx, fy) = warp.map_or((fx, fy), |(pose, side)|
                    pose.source(fx, fy, frame.width as f32, frame.height as f32, side));
                let ix = fx.floor() as i32;
                let iy = fy.floor() as i32;
                let tx = fx - ix as f32;
                let ty = fy - iy as f32;
                let mut rgba = [0.0f32; 4];
                for (dx, dy, weight) in [
                    (0, 0, (1.0 - tx) * (1.0 - ty)),
                    (1, 0, tx * (1.0 - ty)),
                    (0, 1, (1.0 - tx) * ty),
                    (1, 1, tx * ty),
                ] {
                    let px = ix + dx;
                    let py = iy + dy;
                    if px < 0 || py < 0 || px >= frame.width as i32 || py >= frame.height as i32 {
                        continue;
                    }
                    let at = (py as usize * frame.width + px as usize) * 4;
                    let alpha = frame.pixels[at + 3] as f32 / 255.0;
                    rgba[3] += alpha * weight;
                    for k in 0..3 {
                        rgba[k] += frame.pixels[at + k] as f32 * alpha * weight;
                    }
                }
                if rgba[3] <= 0.0 {
                    continue;
                }
                let alpha = rgba[3];
                for v in rgba.iter_mut().take(3) {
                    *v /= alpha;
                    *v = (*v - 127.5) * contrast + 127.5;
                }
                let luminance = 0.2126 * rgba[0] + 0.7152 * rgba[1] + 0.0722 * rgba[2];
                for v in rgba.iter_mut().take(3) {
                    *v = (luminance + (*v - luminance) * saturation).clamp(0.0, 255.0);
                }
                self.blend(
                    x,
                    y,
                    [
                        rgba[0].round() as u8,
                        rgba[1].round() as u8,
                        rgba[2].round() as u8,
                        (rgba[3] * 255.0).round() as u8,
                    ],
                );
            }
        }
    }
    fn save_bmp(&self, path: &Path) -> Result<(), String> {
        let length = 54 + self.pixels.len() * 4;
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&(length as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&self.w.to_le_bytes());
        bytes.extend_from_slice(&(-self.h).to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 24]);
        for p in &self.pixels {
            let inv = 255 - (p >> 24);
            bytes.extend_from_slice(&[
                ((p & 255) + inv).min(255) as u8,
                (((p >> 8) & 255) + inv).min(255) as u8,
                (((p >> 16) & 255) + inv).min(255) as u8,
                255,
            ]);
        }
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
}
#[derive(Hash, PartialEq, Eq)]
struct TextMaskKey {
    width: i32,
    height: i32,
    font_height: i32,
    medium: bool,
    right: bool,
    text: String,
}
#[derive(Default)]
struct Fonts {
    handles: HashMap<(i32, bool, bool), HFONT>,
    masks: HashMap<TextMaskKey, Arc<Vec<u8>>>,
}
impl Fonts {
    fn get(&mut self, height: i32, medium: bool, latin: bool) -> HFONT {
        *self
            .handles
            .entry((height, medium, latin))
            .or_insert_with(|| unsafe {
                CreateFontW(
                    -height,
                    0,
                    0,
                    0,
                    if medium { 500 } else { 400 },
                    0,
                    0,
                    0,
                    if latin {
                        ANSI_CHARSET as u32
                    } else {
                        GB2312_CHARSET as u32
                    },
                    OUT_OUTLINE_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    ANTIALIASED_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    wide(if latin {
                        "Segoe UI"
                    } else {
                        "Microsoft YaHei UI"
                    })
                    .as_ptr(),
                )
            })
    }
    fn mask(
        &mut self,
        rect: BoxRect,
        text: &str,
        height: i32,
        medium: bool,
        right: bool,
    ) -> Option<Arc<Vec<u8>>> {
        let key = TextMaskKey {
            width: rect.w,
            height: rect.h,
            font_height: height,
            medium,
            right,
            text: text.into(),
        };
        if let Some(mask) = self.masks.get(&key) {
            return Some(mask.clone());
        }
        // Layered windows need grayscale coverage rather than colored LCD
        // subpixels. Rasterize outlines at 3x and average down at the real DPI.
        const SAMPLE: i32 = 3;
        let mut surface = Surface::new(rect.w * SAMPLE, rect.h * SAMPLE).ok()?;
        surface.pixels().fill(0);
        let latin = !text.chars().any(|c| ('\u{2e80}'..='\u{9fff}').contains(&c));
        let font = self.get(height * SAMPLE, medium, latin);
        if font.is_null() {
            return None;
        }
        let old = unsafe { SelectObject(surface.dc, font) };
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: surface.w,
            bottom: surface.h,
        };
        let flags = DT_SINGLELINE
            | DT_VCENTER
            | DT_NOPREFIX
            | DT_END_ELLIPSIS
            | if right { DT_RIGHT } else { DT_LEFT };
        unsafe {
            SetBkColor(surface.dc, 0);
            SetTextColor(surface.dc, 0x00ffffff);
            SetBkMode(surface.dc, TRANSPARENT as i32);
            DrawTextW(surface.dc, wide(text).as_ptr(), -1, &mut bounds, flags);
            GdiFlush();
            SelectObject(surface.dc, old);
        }
        let stride = surface.w;
        let pixels = surface.pixels();
        let mut coverage = vec![0u8; (rect.w * rect.h) as usize];
        for y in 0..rect.h {
            for x in 0..rect.w {
                let mut sum = 0;
                for dy in 0..SAMPLE {
                    for dx in 0..SAMPLE {
                        let p = pixels[((y * SAMPLE + dy) * stride + x * SAMPLE + dx) as usize];
                        sum += (p & 255) + ((p >> 8) & 255) + ((p >> 16) & 255);
                    }
                }
                coverage[(y * rect.w + x) as usize] = ((sum + 13) / 27) as u8;
            }
        }
        let coverage = Arc::new(coverage);
        // Quota timestamps and changing statuses must not grow the cache forever.
        if self.masks.len() >= 128 {
            self.masks.clear();
        }
        self.masks.insert(key, coverage.clone());
        Some(coverage)
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        for f in self.handles.values() {
            unsafe {
                DeleteObject(*f);
            }
        }
    }
}
enum UiEvent {
    Down(POINT),
    Up(POINT),
    Cancel,
    Menu(POINT),
    Wheel(i32),
    Dpi(u32),
    Escape,
    Sway { percent: u16, commit: bool },
    SwayClosed(HWND),
}
thread_local! {static EVENTS:RefCell<VecDeque<UiEvent>>=const{RefCell::new(VecDeque::new())};static HIT:RefCell<(i32,i32,Vec<u8>)>=const{RefCell::new((0,0,Vec::new()))};}
fn queue(event: UiEvent) {
    EVENTS.with(|q| q.borrow_mut().push_back(event));
}
const SWAY_SLIDER: i32 = 5001;
// CommCtrl.h defines this zero-offset message as WM_USER; windows-sys omits it.
const TBM_GETPOS: u32 = WM_USER;
const SWAY_LABEL: i32 = 5002;
const SWAY_RESET: usize = 5003;
const SWAY_DONE: usize = 5004;
unsafe fn sway_update(hwnd: HWND, percent: u16, commit: bool) {
    SetWindowTextW(GetDlgItem(hwnd, SWAY_LABEL), wide(format!("摆幅：{percent}%")).as_ptr());
    queue(UiEvent::Sway { percent, commit });
}
unsafe fn sway_close(hwnd: HWND) {
    let slider = GetDlgItem(hwnd, SWAY_SLIDER);
    let percent = SendMessageW(slider, TBM_GETPOS, 0, 0).clamp(0, 400) as u16;
    sway_update(hwnd, percent, true);
    DestroyWindow(hwnd);
}
unsafe extern "system" fn sway_wndproc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_HSCROLL => {
            if l as HWND == GetDlgItem(hwnd, SWAY_SLIDER) {
                let percent = SendMessageW(l as HWND, TBM_GETPOS, 0, 0).clamp(0, 400) as u16;
                sway_update(hwnd, percent, (w & 0xffff) != TB_THUMBTRACK as usize);
            }
            return 0;
        }
        WM_COMMAND => match w & 0xffff {
            SWAY_RESET => {
                SendMessageW(GetDlgItem(hwnd, SWAY_SLIDER), TBM_SETPOS, 1, 200);
                sway_update(hwnd, 200, true);
                return 0;
            }
            SWAY_DONE => { sway_close(hwnd); return 0; }
            _ => {}
        },
        WM_CLOSE => { sway_close(hwnd); return 0; }
        WM_NCDESTROY => {
            let font = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as HFONT;
            if !font.is_null() { DeleteObject(font); }
            queue(UiEvent::SwayClosed(hwnd));
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, w, l)
}
fn sway_panel(owner: HWND, percent: u16, dpi: f64) -> Result<HWND, String> {
    unsafe {
        let controls = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES };
        if InitCommonControlsEx(&controls) == 0 { return Err("Cannot initialize amplitude slider".into()); }
        let class = wide("CodexPetRust.SwayPanel");
        let instance = GetModuleHandleW(null());
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(sway_wndproc);
        wc.hInstance = instance;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hbrBackground = (COLOR_BTNFACE + 1) as HBRUSH;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        let px = |value: i32| (value as f64 * dpi).round() as i32;
        let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
        let extended = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        let mut bounds = RECT { left: 0, top: 0, right: px(320), bottom: px(176) };
        AdjustWindowRectExForDpi(&mut bounds, style, 0, extended, (96.0 * dpi) as u32);
        let (w, h) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let pet = window_rect(owner);
        let area = work_area(POINT { x: (pet.left + pet.right) / 2, y: (pet.top + pet.bottom) / 2 });
        let x = if pet.left > (area.left + area.right) / 2 { pet.left - w - px(12) }
            else { pet.right + px(12) };
        let hwnd = CreateWindowExW(extended, class.as_ptr(), wide("翅膀与尾巴摆幅").as_ptr(), style,
            x.clamp(area.left, (area.right - w).max(area.left)),
            pet.top.clamp(area.top, (area.bottom - h).max(area.top)), w, h,
            owner, null_mut(), instance, null());
        if hwnd.is_null() { return Err("Cannot create amplitude settings".into()); }
        let font = CreateFontW(-px(13), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET as u32,
            OUT_OUTLINE_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32, wide("Microsoft YaHei UI").as_ptr());
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, font as isize);
        let child = |class: &str, text: &str, extra: u32, id: usize, x, y, w, h| {
            let control = CreateWindowExW(0, wide(class).as_ptr(), wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | extra, px(x), px(y), px(w), px(h), hwnd,
                id as HMENU, instance, null());
            SendMessageW(control, WM_SETFONT, font as usize, 1);
            control
        };
        child("STATIC", &format!("摆幅：{percent}%"), 0, SWAY_LABEL as usize, 16, 12, 288, 24);
        child("STATIC", "仅调节吸附姿势，拖动即时预览", 0, 0, 16, 39, 288, 20);
        let slider = child("msctls_trackbar32", "", WS_TABSTOP | TBS_AUTOTICKS | TBS_TOOLTIPS,
            SWAY_SLIDER as usize, 12, 64, 296, 30);
        SendMessageW(slider, TBM_SETRANGEMIN, 0, 0);
        SendMessageW(slider, TBM_SETRANGEMAX, 1, 400);
        SendMessageW(slider, TBM_SETTICFREQ, 100, 0);
        SendMessageW(slider, TBM_SETLINESIZE, 0, 10);
        SendMessageW(slider, TBM_SETPAGESIZE, 0, 50);
        SendMessageW(slider, TBM_SETPOS, 1, percent as isize);
        child("STATIC", "0% 关闭", 0, 0, 16, 98, 110, 20);
        child("STATIC", "400%", SS_RIGHT, 0, 226, 98, 78, 20);
        child("STATIC", "静态 / 暂停时停止", 0, 0, 16, 134, 144, 22);
        child("BUTTON", "恢复默认", WS_TABSTOP, SWAY_RESET, 166, 130, 80, 28);
        child("BUTTON", "完成", WS_TABSTOP, SWAY_DONE, 254, 130, 50, 28);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
        SetFocus(slider);
        Ok(hwnd)
    }
}
unsafe extern "system" fn wndproc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            let x = (l as u16 as i16) as i32;
            let y = ((l as usize >> 16) as u16 as i16) as i32;
            let r = window_rect(hwnd);
            let x = x - r.left;
            let y = y - r.top;
            return HIT.with(|hit| {
                let hit = hit.borrow();
                if x >= 0
                    && y >= 0
                    && x < hit.0
                    && y < hit.1
                    && hit.2.get((y * hit.0 + x) as usize).is_some_and(|a| *a > 8)
                {
                    HTCLIENT as isize
                } else {
                    HTTRANSPARENT as isize
                }
            });
        }
        WM_LBUTTONDOWN => {
            queue(UiEvent::Down(cursor()));
            SetCapture(hwnd);
            return 0;
        }
        WM_LBUTTONUP => {
            queue(UiEvent::Up(cursor()));
            ReleaseCapture();
            return 0;
        }
        WM_CAPTURECHANGED | WM_KILLFOCUS => {
            queue(UiEvent::Cancel);
            return 0;
        }
        WM_RBUTTONUP => {
            queue(UiEvent::Menu(cursor()));
            return 0;
        }
        WM_MOUSEWHEEL => {
            queue(UiEvent::Wheel(((w >> 16) as u16 as i16) as i32));
            return 0;
        }
        WM_DPICHANGED => {
            queue(UiEvent::Dpi((w & 0xffff) as u32));
            return 0;
        }
        WM_KEYDOWN if w == 27 => {
            queue(UiEvent::Escape);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_TIMER => return 0,
        WM_CLOSE => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, message, w, l)
}
fn reset_text(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "重置时间暂不可用".into();
    };
    let ticks = seconds.saturating_add(11644473600).saturating_mul(10000000);
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = unsafe { zeroed() };
    let mut time: SYSTEMTIME = unsafe { zeroed() };
    if unsafe { FileTimeToLocalFileTime(&ft, &mut local) } == 0
        || unsafe { FileTimeToSystemTime(&local, &mut time) } == 0
    {
        return "重置时间暂不可用".into();
    }
    format!(
        "{}/{} {:02}:{:02} 重置",
        time.wMonth, time.wDay, time.wHour, time.wMinute
    )
}
fn quota_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(10080) => "每周额度".into(),
        Some(1440) => "每日额度".into(),
        Some(n) if n > 0 && n % 60 == 0 => format!("{} 小时额度", n / 60),
        Some(n) if n > 0 => format!("{n} 分钟额度"),
        _ => "使用额度".into(),
    }
}
fn demo_usage() -> Usage {
    normalize_usage(
        &json!({"rateLimits":{"primary":{"usedPercent":21,"windowDurationMins":10080,"resetsAt":now_ms()/1000+86400}},"rateLimitResetCredits":{"availableCount":2}}),
    )
}
struct App {
    hwnd: HWND,
    profile: PathBuf,
    isolated: bool,
    demo: bool,
    pack: Arc<Pack>,
    settings: Settings,
    dpi: f64,
    surface: Surface,
    canvas: Canvas,
    fonts: Fonts,
    snapshot: Snapshot,
    usage: Option<Usage>,
    usage_error: bool,
    busy: bool,
    panel: bool,
    scroll: i32,
    panel_box: BoxRect,
    badge_box: BoxRect,
    press: Option<Press>,
    paused: bool,
    atlas: Option<Atlas>,
    pending: Option<(u64, usize)>,
    generation: Arc<AtomicU64>,
    decoder: Sender<(u64, usize)>,
    started: Instant,
    frame: usize,
    hold_until: u64,
    interaction: State,
    idle: IdleSchedule,
    dirty: bool,
    notices: Receiver<Notice>,
    bridge: Option<Sender<BridgeCommand>>,
    stop: Arc<AtomicBool>,
    last_refresh: Instant,
    position_writes: u64,
    docked: Option<DockSide>,
    dock_art: DockArt,
    dock_blink: DockBlink,
    dock_eye_closed: bool,
    dock_sway_pose: DockSwayPose,
    dock_sway_started: Instant,
    dock_sway_updated: Instant,
    sway_panel: HWND,
    dock_slide: Option<DockSlide>,
}
impl App {
    fn new(hwnd: HWND, profile: PathBuf, isolated: bool, demo: bool) -> Result<Self, String> {
        let pack = Arc::new(Pack::read()?);
        let settings = Settings::load(&profile);
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f64 / 96.0;
        let w = (WIDTH as f64 * dpi).round() as i32;
        let h = (HEIGHT as f64 * dpi).round() as i32;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let bridge = if demo {
            None
        } else {
            Some(start_bridge(tx.clone(), stop.clone()))
        };
        let generation = Arc::new(AtomicU64::new(0));
        let (decoder, decode_rx) = mpsc::channel::<(u64, usize)>();
        let p = pack.clone();
        let g = generation.clone();
        let s = stop.clone();
        thread::spawn(move || {
            while let Ok(mut job) = decode_rx.recv() {
                while let Ok(new) = decode_rx.try_recv() {
                    job = new;
                }
                if s.load(Ordering::Relaxed) {
                    break;
                }
                if g.load(Ordering::Relaxed) != job.0 {
                    continue;
                }
                let result = decode_atlas(&p, job.1);
                if tx.send(Notice::Atlas(job.0, job.1, result)).is_err() {
                    break;
                }
            }
        });
        let mut app = Self {
            hwnd,
            profile,
            isolated,
            demo,
            pack,
            settings: settings.clone(),
            dpi,
            surface: Surface::new(w, h)?,
            canvas: Canvas::new(w, h),
            fonts: Fonts::default(),
            snapshot: if demo {
                Snapshot {
                    state: State::Idle,
                    connected: true,
                    ..Snapshot::default()
                }
            } else {
                Snapshot::default()
            },
            usage: None,
            usage_error: false,
            busy: false,
            panel: false,
            scroll: 0,
            panel_box: BoxRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            badge_box: BoxRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
            press: None,
            paused: false,
            atlas: None,
            pending: None,
            generation,
            decoder,
            started: Instant::now(),
            frame: usize::MAX,
            hold_until: 0,
            interaction: State::Click,
            idle: IdleSchedule::new(&settings.motion, now_ms()),
            dirty: true,
            notices: rx,
            bridge,
            stop,
            last_refresh: Instant::now(),
            position_writes: 0,
            docked: None,
            dock_art: DockArt::read()?,
            dock_blink: DockBlink::new(now_ms()),
            dock_eye_closed: false,
            dock_sway_pose: DockSwayPose::default(),
            dock_sway_started: Instant::now(),
            dock_sway_updated: Instant::now(),
            sway_panel: null_mut(),
            dock_slide: None,
        };
        let area = work_area(POINT { x: 0, y: 0 });
        let position = app
            .settings
            .position
            .as_ref()
            .map(|p| POINT { x: p.x, y: p.y })
            .unwrap_or(POINT {
                x: area.right - w - 24,
                y: area.bottom - h - 12,
            });
        app.move_to(constrain(
            position,
            work_area(POINT {
                x: position.x + w / 2,
                y: position.y + h - 100,
            }),
            w,
            h,
            dpi,
        ));
        app.play(app.pack.base());
        if app.settings.edge_docking {
            if let Some(dock) = app.settings.dock.clone() {
                app.dock_to(dock.side, POINT { x: dock.anchor.x, y: dock.anchor.y });
                app.begin_slide(DockPhase::Peek, dock.side);
            }
        }
        Ok(app)
    }
    fn px(&self, v: f64) -> i32 {
        (v * self.dpi).round() as i32
    }
    fn rect(&self, x: f64, y: f64, w: f64, h: f64) -> BoxRect {
        BoxRect {
            x: self.px(x),
            y: self.px(y),
            w: self.px(w),
            h: self.px(h),
        }
    }
    fn effective(&self) -> State {
        if self.press.as_ref().is_some_and(|p| p.moved) {
            State::Drag
        } else if now_ms() < self.hold_until {
            self.interaction
        } else {
            self.snapshot.state
        }
    }
    fn choose(&mut self, state: State) -> usize {
        choose(
            &self.pack,
            state,
            self.atlas.as_ref().map(|a| a.index),
            &self.settings.motion,
            &mut self.idle,
        )
    }
    fn play(&mut self, index: usize) {
        if self.pending.is_some_and(|(_, i)| i == index) {
            return;
        }
        if self.atlas.as_ref().is_some_and(|a| a.index == index) {
            if self.pending.is_some() {
                self.generation.fetch_add(1, Ordering::Relaxed);
                self.pending = None;
            }
            return;
        }
        let sequence = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.pending = Some((sequence, index));
        let _ = self.decoder.send((sequence, index));
    }
    fn interact(&mut self, index: usize) {
        self.interaction = State::Click;
        self.hold_until = now_ms() + clip_range(&self.pack.animations[index].spritesheet).2;
        self.play(index);
    }
    fn move_to(&mut self, point: POINT) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                point.x,
                point.y,
                self.canvas.w,
                self.canvas.h,
                SWP_NOACTIVATE,
            );
        }
    }
    fn resize_viewport(&mut self, w: i32, h: i32) -> bool {
        if self.canvas.w == w && self.canvas.h == h {
            return true;
        }
        let Ok(surface) = Surface::new(w, h) else { return false; };
        self.surface = surface;
        self.canvas = Canvas::new(w, h);
        self.dirty = true;
        true
    }
    fn dock_to(&mut self, side: DockSide, anchor: POINT) -> bool {
        self.dock_slide = None;
        let area = work_area(anchor);
        let target = dock_layout(
            area, side, anchor.y, self.dpi, self.settings.scale,
            self.dock_art.left.width as f64 / self.dock_art.left.height as f64,
        );
        if self.docked.is_none() {
            self.save_position();
        }
        if !self.resize_viewport(target.w, target.h) {
            return false;
        }
        self.docked = Some(side);
        self.panel = false;
        self.scroll = 0;
        self.hold_until = 0;
        self.dock_eye_closed = false;
        self.dock_blink = DockBlink::new(now_ms());
        self.dock_sway_pose = DockSwayPose::default();
        self.dock_sway_started = Instant::now();
        self.dock_sway_updated = self.dock_sway_started;
        self.move_to(POINT { x: target.x, y: target.y });
        self.save_position();
        self.dirty = true;
        true
    }
    fn begin_slide(&mut self, phase: DockPhase, side: DockSide) {
        self.paint();
        let r = window_rect(self.hwnd);
        let area = work_area(match phase {
            DockPhase::HideBody { anchor } => anchor,
            _ => POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 },
        });
        let mut left = self.canvas.w;
        let mut right = 0;
        for (i, pixel) in self.canvas.pixels.iter().enumerate() {
            if pixel >> 24 > 8 {
                let x = i as i32 % self.canvas.w;
                left = left.min(x);
                right = right.max(x + 1);
            }
        }
        let distance = match side {
            DockSide::Left => -(r.left + right - area.left + 2).max(1),
            DockSide::Right => (area.right - r.left - left + 2).max(1),
        };
        let duration = match phase {
            DockPhase::HideBody { .. } => 0.16,
            DockPhase::Peek => 0.17,
            DockPhase::HidePeek { .. } => 0.10,
            DockPhase::Reveal => 0.18,
        };
        self.dock_slide = Some(DockSlide {
            phase, side, started: Instant::now(), duration,
            pixels: self.canvas.pixels.clone(),
            w: self.canvas.w, h: self.canvas.h, distance, area,
        });
        unsafe { SetTimer(self.hwnd, 1, 16, None); }
        self.dirty = true;
    }
    fn start_docking(&mut self, side: DockSide, anchor: POINT) {
        self.save_position();
        let area = work_area(anchor);
        let target = dock_layout(
            area, side, anchor.y, self.dpi, self.settings.scale,
            self.dock_art.left.width as f64 / self.dock_art.left.height as f64,
        );
        // Persist the intended final location even if Codex closes mid-slide.
        self.settings.dock = Some(DockPlacement {
            side, anchor: Position { x: target.x + target.w / 2, y: target.y },
        });
        self.settings.save(&self.profile);
        self.begin_slide(DockPhase::HideBody { anchor }, side);
    }
    fn start_expanding(&mut self, quota: bool) {
        if let Some(side) = self.docked {
            self.dock_eye_closed = false;
            self.begin_slide(DockPhase::HidePeek { quota }, side);
        }
    }
    fn finish_slide(&mut self) {
        if let Some(slide) = self.dock_slide.take() {
            match slide.phase {
                DockPhase::HideBody { anchor } => { self.dock_to(slide.side, anchor); }
                DockPhase::HidePeek { quota } => { self.expand_from_dock(quota); }
                _ => {}
            }
            unsafe { SetTimer(self.hwnd, 1, 33, None); }
            self.dirty = true;
        }
    }
    fn step_slide(&mut self) -> Result<bool, String> {
        let Some(slide) = self.dock_slide.as_ref() else { return Ok(false); };
        let t = slide.started.elapsed().as_secs_f64() / slide.duration;
        if t >= 1.0 {
            let slide = self.dock_slide.take().unwrap();
            match slide.phase {
                DockPhase::HideBody { anchor } => {
                    self.dock_to(slide.side, anchor);
                    self.begin_slide(DockPhase::Peek, slide.side);
                    return self.step_slide();
                }
                DockPhase::HidePeek { quota } => {
                    self.expand_from_dock(quota);
                    self.begin_slide(DockPhase::Reveal, slide.side);
                    return self.step_slide();
                }
                _ => {
                    unsafe { SetTimer(self.hwnd, 1, 33, None); }
                    self.dirty = true;
                    return Ok(false);
                }
            }
        }
        if slide.w != self.canvas.w || slide.h != self.canvas.h {
            self.finish_slide();
            return Ok(false);
        }
        let reveal = matches!(slide.phase, DockPhase::Peek | DockPhase::Reveal);
        let eased = slide_fraction(t, reveal);
        let offset = (slide.distance as f64 * if reveal { 1.0 - eased } else { eased }).round() as i32;
        let r = window_rect(self.hwnd);
        let left = 0.max(offset).max(slide.area.left - r.left);
        let right = slide.w.min(slide.w + offset).min(slide.area.right - r.left);
        self.canvas.pixels.fill(0);
        if right > left {
            for y in 0..slide.h {
                if r.top + y < slide.area.top || r.top + y >= slide.area.bottom {
                    continue;
                }
                let source = (y * slide.w + left - offset) as usize;
                let target = (y * slide.w + left) as usize;
                let length = (right - left) as usize;
                self.canvas.pixels[target..target + length]
                    .copy_from_slice(&slide.pixels[source..source + length]);
            }
        }
        self.present()?;
        self.dirty = false;
        Ok(true)
    }
    fn expand_from_dock(&mut self, quota: bool) -> bool {
        let Some(side) = self.docked else { return true; };
        let old = window_rect(self.hwnd);
        let area = work_area(POINT {
            x: (old.left + old.right) / 2,
            y: (old.top + old.bottom) / 2,
        });
        let w = self.px(WIDTH as f64);
        let h = self.px(HEIGHT as f64);
        if !self.resize_viewport(w, h) {
            return false;
        }
        self.docked = None;
        self.settings.dock = None;
        self.dock_eye_closed = false;
        let head_y = self.px(HEIGHT as f64 - 43.0 - 480.0 * self.settings.scale * 360.0 / 640.0 * 0.74);
        self.move_to(constrain(
            POINT {
                x: if side == DockSide::Left { area.left } else { area.right - w },
                y: old.top + (old.bottom - old.top) / 4 - head_y,
            },
            area, w, h, self.dpi,
        ));
        self.save_position();
        self.hold_until = 0;
        let animation = self.choose(self.snapshot.state);
        self.play(animation);
        if quota && !self.panel {
            self.toggle_panel();
        }
        self.dirty = true;
        true
    }
    fn restore(&mut self) {
        self.finish_slide();
        self.end_press(true, cursor());
        if !self.expand_from_dock(false) {
            return;
        }
        let area = work_area(POINT { x: 0, y: 0 });
        self.move_to(constrain(
            POINT {
                x: area.right - self.canvas.w - self.px(24.0),
                y: area.bottom - self.canvas.h - self.px(12.0),
            },
            area,
            self.canvas.w,
            self.canvas.h,
            self.dpi,
        ));
        self.save_position();
    }
    fn save_position(&mut self) {
        let r = window_rect(self.hwnd);
        if let Some(side) = self.docked {
            self.settings.dock = Some(DockPlacement {
                side,
                anchor: Position { x: (r.left + r.right) / 2, y: r.top },
            });
            self.settings.save(&self.profile);
            return;
        }
        self.settings.position = Some(Position {
            x: r.left,
            y: r.top,
        });
        self.settings.save(&self.profile);
    }
    fn visible_sprite(&self) -> Option<BoxRect> {
        let atlas = self.atlas.as_ref()?;
        let frame = atlas.frames.get(self.frame)?;
        let sheet = &self.pack.animations[atlas.index].spritesheet;
        let width = 480.0 * self.settings.scale;
        let height = width * 360.0 / 640.0;
        let target = self.rect(
            (WIDTH as f64 - width) / 2.0, HEIGHT as f64 - 43.0 - height, width, height,
        );
        let r = window_rect(self.hwnd);
        Some(BoxRect {
            x: r.left + target.x + (frame.x as f64 * target.w as f64 / sheet.frame_width as f64).floor() as i32,
            y: r.top + target.y + (frame.y as f64 * target.h as f64 / sheet.frame_height as f64).floor() as i32,
            w: (frame.width as f64 * target.w as f64 / sheet.frame_width as f64).ceil() as i32,
            h: (frame.height as f64 * target.h as f64 / sheet.frame_height as f64).ceil() as i32,
        })
    }
    fn sample_drag(&mut self, point: POINT) {
        if let Some(side) = self.docked {
            let Some(press) = self.press.as_ref() else { return; };
            let dx = point.x - press.cursor.x;
            let dy = point.y - press.cursor.y;
            if !press.allow_drag || (!press.moved && (dx as i64 * dx as i64 + dy as i64 * dy as i64) <= 25) {
                return;
            }
            if dx.abs() > self.px(32.0) {
                let previous = self.press.take().unwrap();
                if !self.expand_from_dock(false) {
                    self.press = Some(previous);
                    return;
                }
                let origin = POINT {
                    x: previous.cursor.x - self.canvas.w / 2,
                    y: previous.cursor.y - self.px(
                        HEIGHT as f64 - 43.0 - 480.0 * self.settings.scale * 360.0 / 640.0 * 0.74
                    ),
                };
                let r = window_rect(self.hwnd);
                self.press = Some(Press {
                    cursor: previous.cursor, origin,
                    last: POINT { x: r.left, y: r.top },
                    moved: true, allow_drag: true,
                    edge_drag: self.settings.edge_docking,
                });
                self.hold_until = 0;
                let animation = self.choose(State::Drag);
                self.play(animation);
                self.sample_drag(point);
                return;
            }
            let origin = press.origin;
            let area = work_area(POINT {
                x: origin.x + self.canvas.w / 2,
                y: origin.y + self.canvas.h / 2,
            });
            let target = dock_layout(
                area, side, origin.y + dy, self.dpi, self.settings.scale,
                self.dock_art.left.width as f64 / self.dock_art.left.height as f64,
            );
            if let Some(press) = self.press.as_mut() {
                press.moved = true;
                press.last = POINT { x: target.x, y: target.y };
            }
            let r = window_rect(self.hwnd);
            if r.left != target.x || r.top != target.y {
                self.move_to(POINT { x: target.x, y: target.y });
                self.position_writes += 1;
            }
            return;
        }
        if let Some(press) = self.press.as_mut() {
            if let Some((position, started)) = press.sample(
                point,
                work_area(point),
                self.canvas.w,
                self.canvas.h,
                self.dpi,
            ) {
                if started {
                    self.hold_until = 0;
                    let animation = self.choose(State::Drag);
                    self.play(animation);
                }
                let r = window_rect(self.hwnd);
                if r.left != position.x || r.top != position.y {
                    self.move_to(position);
                    self.position_writes += 1;
                }
            }
        }
    }
    fn end_press(&mut self, cancel: bool, point: POINT) {
        if !cancel {
            self.sample_drag(point);
        }
        if let Some(press) = self.press.take() {
            if press.moved {
                if self.docked.is_some() {
                    self.save_position();
                    self.dirty = true;
                    return;
                }
                if !cancel && self.settings.edge_docking {
                    let area = work_area(point);
                    if let Some(side) = dock_target(point, area, self.dpi, self.visible_sprite()) {
                        let anchor = POINT {
                            x: point.x,
                            y: self.visible_sprite().map_or(
                                point.y - self.px(42.0 * self.settings.scale / 0.8), |r| r.y
                            ),
                        };
                        self.start_docking(side, anchor);
                        self.dirty = true;
                        return;
                    }
                }
                self.save_position();
                self.hold_until = 0;
                let index = self.choose(self.snapshot.state);
                self.play(index);
            } else if !cancel {
                let r = window_rect(self.hwnd);
                self.click(point.x - r.left, point.y - r.top);
            }
            self.dirty = true;
        }
    }
    fn refresh(&mut self) {
        if self.busy {
            return;
        }
        self.last_refresh = Instant::now();
        if self.demo {
            self.usage = Some(demo_usage());
            self.usage_error = false;
        } else if let Some(bridge) = &self.bridge {
            self.busy = true;
            let _ = bridge.send(BridgeCommand::Usage);
        }
        self.dirty = true;
    }
    fn toggle_panel(&mut self) {
        self.panel = !self.panel;
        self.scroll = 0;
        self.dirty = true;
        if self.panel {
            self.refresh();
            let index = self.choose(State::Click);
            self.interact(index);
        }
    }
    fn click(&mut self, x: i32, y: i32) {
        if self.docked.is_some() {
            self.start_expanding(true);
            return;
        }
        if self.panel && self.panel_box.contains(x, y) {
            let r = self.panel_box;
            if x >= r.x + r.w - self.px(32.0) && y < r.y + self.px(28.0) {
                self.panel = false;
            } else if x >= r.x + r.w - self.px(40.0) && y >= r.y + r.h - self.px(32.0) {
                self.refresh();
            }
            self.dirty = true;
        } else {
            self.toggle_panel();
        }
    }
    fn tick(&mut self) -> Result<(), String> {
        let sway_now = Instant::now();
        if self.paused || self.press.is_some() {
            self.dock_sway_started += sway_now.duration_since(self.dock_sway_updated);
        }
        self.dock_sway_updated = sway_now;
        while let Ok(notice) = self.notices.try_recv() {
            match notice {
                Notice::Atlas(sequence, index, result) => {
                    if self.pending != Some((sequence, index)) {
                        continue;
                    }
                    self.pending = None;
                    match result {
                        Ok(atlas) => {
                            self.atlas = Some(atlas);
                            self.started = Instant::now();
                            self.frame = usize::MAX;
                            self.dirty = true;
                        }
                        Err(_) => {
                            if self.atlas.is_none() {
                                return Err("无法解码内嵌动画".into());
                            }
                        }
                    }
                }
                Notice::State(state) => {
                    let before = self.snapshot.state;
                    self.snapshot = state;
                    if before != self.snapshot.state && self.docked.is_none() && self.dock_slide.is_none() {
                        if self.snapshot.state == State::Idle {
                            self.idle = IdleSchedule::new(&self.settings.motion, now_ms());
                        }
                        if matches!(self.snapshot.state, State::Waiting | State::Error) {
                            self.hold_until = 0;
                        }
                        if now_ms() >= self.hold_until {
                            let index = self.choose(self.snapshot.state);
                            self.play(index);
                        }
                    }
                    self.dirty = true;
                }
                Notice::Usage(result) => {
                    self.busy = false;
                    match result {
                        Ok(usage) => {
                            let remaining = usage
                                .buckets
                                .iter()
                                .flat_map(|b| b.windows.iter())
                                .filter_map(|w| w.remaining)
                                .fold(100.0f64, f64::min);
                            self.usage = Some(usage);
                            self.usage_error = false;
                            let id = if remaining <= 0.0 {
                                "分文不剩"
                            } else if remaining <= 10.0 {
                                "袋空如洗"
                            } else if remaining <= 30.0 {
                                "数钱皱眉"
                            } else if remaining <= 60.0 {
                                "钱袋如常"
                            } else {
                                "钱袋满溢"
                            };
                            if self.panel {
                                if let Some(index) =
                                    self.pack.animations.iter().position(|a| a.id == id)
                                {
                                    self.interact(index);
                                }
                            }
                        }
                        Err(_) => self.usage_error = true,
                    }
                    self.dirty = true;
                }
            }
        }
        if self.press.is_some() {
            self.sample_drag(cursor());
        }
        if self.step_slide()? {
            return Ok(());
        }
        if self.docked.is_some() {
            if !self.paused && self.press.is_none() {
                if self.settings.motion == "still" {
                    self.dock_sway_started = Instant::now();
                }
                let pose = DockSwayPose::at(self.dock_sway_started.elapsed().as_millis() as u64,
                    &self.settings.motion, self.settings.dock_sway_percent);
                if pose != self.dock_sway_pose {
                    self.dock_sway_pose = pose;
                    self.dirty = true;
                }
            }
            let closed = self.dock_blink.closed(
                now_ms(), !self.paused && self.settings.motion != "still",
            );
            if closed != self.dock_eye_closed {
                self.dock_eye_closed = closed;
                self.dirty = true;
            }
            if self.dirty {
                self.render()?;
                self.dirty = false;
            }
            return Ok(());
        }
        if self.panel && self.last_refresh.elapsed() >= Duration::from_secs(60) {
            self.refresh();
        }
        if let Some(atlas) = &self.atlas {
            let sheet = &self.pack.animations[atlas.index].spritesheet;
            let duration = clip_range(sheet).2;
            if !self.paused
                && self.started.elapsed().as_millis() as u64 >= duration
                && self.pending.is_none()
            {
                let next = self.choose(self.effective());
                if self.atlas.as_ref().is_some_and(|a| a.index == next) {
                    self.started = Instant::now();
                } else {
                    self.play(next);
                }
            }
        }
        let next_frame = self.atlas.as_ref().map(|a| {
            if self.paused && self.frame != usize::MAX {
                self.frame
            } else {
                frame_at(
                    &self.pack.animations[a.index].spritesheet,
                    self.started.elapsed().as_millis() as u64,
                    self.settings.motion == "still" && self.effective() == State::Idle,
                )
            }
        });
        if next_frame.is_some_and(|f| f != self.frame) {
            self.frame = next_frame.unwrap();
            self.dirty = true;
        }
        if self.dirty && self.atlas.is_some() {
            self.render()?;
            self.dirty = false;
        }
        Ok(())
    }
    fn render(&mut self) -> Result<(), String> {
        if self.dock_slide.is_some() && self.step_slide()? {
            return Ok(());
        }
        self.paint();
        self.present()
    }
    fn paint(&mut self) {
        if self.docked.is_some() {
            self.render_docked();
            return;
        }
        if self.frame == usize::MAX {
            if let Some(atlas) = &self.atlas {
                self.frame = frame_at(
                    &self.pack.animations[atlas.index].spritesheet,
                    self.started.elapsed().as_millis() as u64, false,
                );
            }
        }
        let dpi = self.dpi;
        let px = |v: f64| (v * dpi).round() as i32;
        self.canvas.pixels.fill(0);
        self.canvas.clip = None;
        let sprite_w = 480.0 * self.settings.scale;
        let sprite_h = sprite_w * 360.0 / 640.0;
        let sprite_box = self.rect(
            (WIDTH as f64 - sprite_w) / 2.0,
            HEIGHT as f64 - 43.0 - sprite_h,
            sprite_w,
            sprite_h,
        );
        if let Some(atlas) = &self.atlas {
            if let Some(frame) = atlas.frames.get(self.frame) {
                self.canvas.sprite(
                    frame,
                    sprite_box,
                    &self.pack.animations[atlas.index].spritesheet,
                    &self.settings.appearance,
                );
            }
        }
        let label = if !self.snapshot.connected {
            "等待 Codex".to_string()
        } else {
            match self.snapshot.state {
                State::Working => {
                    if self.snapshot.working > 1 {
                        format!("正在工作 · {} 项", self.snapshot.working)
                    } else {
                        "正在工作".into()
                    }
                }
                State::Waiting => {
                    if self.snapshot.waiting > 1 {
                        format!("等你确认 · {} 项", self.snapshot.waiting)
                    } else {
                        "等你确认".into()
                    }
                }
                State::Error => "遇到问题了".into(),
                State::Complete => "完成啦 ✧".into(),
                _ => "陪你待机".into(),
            }
        };
        let width = (label.chars().count() as f64 * 10.0 + 40.0).clamp(100.0, 240.0);
        self.badge_box = self.rect(
            (WIDTH as f64 - width) / 2.0,
            HEIGHT as f64 - 47.0,
            width,
            31.0,
        );
        let b = self.badge_box;
        self.canvas.round_rect(
            BoxRect {
                x: b.x - px(2.0),
                y: b.y + px(2.0),
                w: b.w + px(4.0),
                h: b.h + px(3.0),
            },
            px(18.0),
            [25, 65, 57, 18],
        );
        self.canvas.round_rect(b, px(16.0), [204, 226, 217, 220]);
        self.canvas.round_rect(
            BoxRect {
                x: b.x + 1,
                y: b.y + 1,
                w: b.w - 2,
                h: b.h - 2,
            },
            px(15.0),
            [250, 255, 252, 250],
        );
        let color = if !self.snapshot.connected {
            [156, 174, 170, 255]
        } else {
            match self.snapshot.state {
                State::Waiting => [224, 172, 73, 255],
                State::Error => [215, 121, 110, 255],
                _ => [142, 203, 187, 255],
            }
        };
        self.canvas.round_rect(
            BoxRect {
                x: b.x + px(12.0),
                y: b.y + px(12.0),
                w: px(7.0),
                h: px(7.0),
            },
            px(4.0),
            color,
        );
        self.canvas.text(
            &mut self.fonts,
            BoxRect {
                x: b.x + px(26.0),
                y: b.y,
                w: b.w - px(33.0),
                h: b.h,
            },
            &label,
            px(11.0),
            true,
            [73, 99, 92, 255],
            false,
        );
        if self.panel {
            self.draw_panel(sprite_h);
        }
    }
    fn render_docked(&mut self) {
        let side = self.docked.unwrap();
        self.canvas.pixels.fill(0);
        self.canvas.clip = None;
        self.panel_box = BoxRect { x: 0, y: 0, w: 0, h: 0 };
        let frame = if side == DockSide::Left { &self.dock_art.left } else { &self.dock_art.right };
        self.canvas.dock_picture(frame, BoxRect { x: 0, y: 0, w: self.canvas.w, h: self.canvas.h },
            &self.settings.appearance, self.dock_sway_pose, side);
        if self.dock_eye_closed {
            let mut eye = self.dock_art.eye;
            if side == DockSide::Right {
                eye.x = frame.width as i32 - eye.x - eye.w;
            }
            let target = BoxRect {
                x: (eye.x as f64 * self.canvas.w as f64 / frame.width as f64).round() as i32,
                y: (eye.y as f64 * self.canvas.h as f64 / frame.height as f64).round() as i32,
                w: (eye.w as f64 * self.canvas.w as f64 / frame.width as f64).round() as i32,
                h: (eye.h as f64 * self.canvas.h as f64 / frame.height as f64).round() as i32,
            };
            let closed = if side == DockSide::Left { &self.dock_art.blink_left } else { &self.dock_art.blink_right };
            self.canvas.picture(closed, target, &self.settings.appearance);
        }
        let dot = self.px(6.0).max(3);
        self.badge_box = BoxRect {
            x: if side == DockSide::Left { self.canvas.w - dot - 1 } else { 1 },
            y: self.px(6.0), w: dot, h: dot,
        };
        let color = if !self.snapshot.connected {
            [156, 174, 170, 255]
        } else {
            match self.snapshot.state {
                State::Waiting => [224, 172, 73, 255],
                State::Error => [215, 121, 110, 255],
                State::Working => [105, 180, 161, 255],
                _ => [162, 202, 189, 230],
            }
        };
        self.canvas.round_rect(self.badge_box, dot / 2, color);
    }
    fn present(&mut self) -> Result<(), String> {
        self.surface.pixels().copy_from_slice(&self.canvas.pixels);
        HIT.with(|hit| {
            let mut hit = hit.borrow_mut();
            hit.0 = self.canvas.w;
            hit.1 = self.canvas.h;
            hit.2 = self.canvas.pixels.iter().map(|p| (p >> 24) as u8).collect();
        });
        let r = window_rect(self.hwnd);
        let point = POINT {
            x: r.left,
            y: r.top,
        };
        let size = SIZE {
            cx: self.canvas.w,
            cy: self.canvas.h,
        };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        if unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                null_mut(),
                &point,
                &size,
                self.surface.dc,
                &origin,
                0,
                &blend,
                ULW_ALPHA,
            )
        } == 0
        {
            return Err(format!("Layered rendering failed: {}", unsafe {
                GetLastError()
            }));
        }
        if self.snapshot.connected || self.demo {
            unsafe {
                ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        } else {
            unsafe {
                ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        Ok(())
    }
    fn draw_panel(&mut self, sprite_h: f64) {
        let dpi = self.dpi;
        let px = |v: f64| (v * dpi).round() as i32;
        let rows = self.usage.as_ref().map_or(0, |u| {
            u.buckets
                .iter()
                .map(|b| b.windows.len().max(1))
                .sum::<usize>()
        });
        let buckets = self.usage.as_ref().map_or(0, |u| {
            if u.buckets.len() > 1 {
                u.buckets.len()
            } else {
                0
            }
        });
        let cli_quota = self.usage.as_ref().is_some_and(|u| u.from_cli);
        let content = (rows * 82
            + buckets * 24
            + if self.usage_error { 24 } else { 0 }
            + if cli_quota { 24 } else { 0 }) as f64;
        let height =
            (content + 48.0).clamp(88.0, (HEIGHT as f64 - 60.0 - sprite_h - 12.0).min(292.0));
        let panel = self.rect(
            130.0,
            HEIGHT as f64 - 60.0 - sprite_h - height,
            260.0,
            height,
        );
        self.panel_box = panel;
        let radius = px(16.0);
        self.canvas.round_rect(
            BoxRect {
                x: panel.x - px(3.0),
                y: panel.y + px(5.0),
                w: panel.w + px(6.0),
                h: panel.h + px(3.0),
            },
            radius,
            [20, 58, 59, 18],
        );
        self.canvas.round_rect(panel, radius, [219, 233, 226, 255]);
        self.canvas.round_rect(
            BoxRect {
                x: panel.x + 1,
                y: panel.y + 1,
                w: panel.w - 2,
                h: panel.h - 2,
            },
            radius,
            [252, 255, 252, 250],
        );
        let close = BoxRect {
            x: panel.x + panel.w - px(27.0),
            y: panel.y + px(5.0),
            w: px(20.0),
            h: px(20.0),
        };
        self.canvas.text(
            &mut self.fonts,
            close,
            "×",
            px(16.0),
            false,
            [148, 168, 157, 255],
            false,
        );
        let viewport = BoxRect {
            x: panel.x + px(18.0),
            y: panel.y + px(20.0),
            w: panel.w - px(36.0),
            h: panel.h - px(52.0),
        };
        let maximum = (px(content) - viewport.h).max(0);
        self.scroll = self.scroll.clamp(0, maximum);
        self.canvas.clip = Some(viewport);
        let mut y = viewport.y - self.scroll;
        if self.usage_error {
            self.canvas.text(
                &mut self.fonts,
                BoxRect {
                    x: viewport.x,
                    y,
                    w: viewport.w,
                    h: px(24.0),
                },
                if self.usage.is_some() {
                    "刷新失败 · 显示上次数据"
                } else {
                    "读取失败，点刷新重试"
                },
                px(11.0),
                false,
                [186, 124, 103, 255],
                false,
            );
            y += px(24.0);
        }
        if let Some(usage) = &self.usage {
            if usage.from_cli {
                self.canvas.text(
                    &mut self.fonts,
                    BoxRect {
                        x: viewport.x,
                        y,
                        w: viewport.w,
                        h: px(24.0),
                    },
                    &format!(
                        "CLI 日志快照 · {}",
                        reset_text(Some(usage.fetched_at / 1000)).trim_end_matches(" 重置")
                    ),
                    px(10.0),
                    false,
                    [125, 152, 140, 255],
                    false,
                );
                y += px(24.0);
            }
            for bucket in &usage.buckets {
                if usage.buckets.len() > 1 {
                    self.canvas.text(
                        &mut self.fonts,
                        BoxRect {
                            x: viewport.x,
                            y,
                            w: viewport.w,
                            h: px(24.0),
                        },
                        &bucket.name,
                        px(11.0),
                        false,
                        [125, 152, 140, 255],
                        false,
                    );
                    y += px(24.0);
                }
                for row in &bucket.windows {
                    self.canvas.text(
                        &mut self.fonts,
                        BoxRect {
                            x: viewport.x,
                            y,
                            w: px(110.0),
                            h: px(28.0),
                        },
                        &quota_label(row.minutes),
                        px(11.0),
                        false,
                        [85, 113, 103, 255],
                        false,
                    );
                    let number = row.remaining.map_or("—".into(), |v| format!("{:.0}", v));
                    let color = if row.remaining.is_some_and(|v| v <= 20.0) {
                        [182, 133, 55, 255]
                    } else {
                        [45, 104, 86, 255]
                    };
                    self.canvas.text(
                        &mut self.fonts,
                        BoxRect {
                            x: viewport.x + px(111.0),
                            y,
                            w: viewport.w
                                - px(if row.remaining.is_some() {
                                    128.0
                                } else {
                                    113.0
                                }),
                            h: px(28.0),
                        },
                        &number,
                        px(20.0),
                        false,
                        color,
                        true,
                    );
                    if row.remaining.is_some() {
                        self.canvas.text(
                            &mut self.fonts,
                            BoxRect {
                                x: viewport.x + viewport.w - px(12.0),
                                y: y + px(4.0),
                                w: px(12.0),
                                h: px(24.0),
                            },
                            "%",
                            px(11.0),
                            false,
                            [125, 152, 140, 255],
                            true,
                        );
                    }
                    let track = BoxRect {
                        x: viewport.x,
                        y: y + px(35.0),
                        w: viewport.w,
                        h: px(5.0),
                    };
                    self.canvas.round_rect(track, px(3.0), [230, 238, 232, 255]);
                    if let Some(remaining) = row.remaining {
                        let fill = BoxRect {
                            w: (track.w as f64 * remaining / 100.0).round() as i32,
                            ..track
                        };
                        if fill.w > 0 {
                            self.canvas.round_rect(
                                fill,
                                px(3.0),
                                if remaining <= 20.0 {
                                    [217, 181, 110, 255]
                                } else {
                                    [129, 189, 164, 255]
                                },
                            );
                        }
                    }
                    self.canvas.text(
                        &mut self.fonts,
                        BoxRect {
                            x: viewport.x,
                            y: y + px(45.0),
                            w: viewport.w,
                            h: px(24.0),
                        },
                        &reset_text(row.resets_at),
                        px(10.5),
                        false,
                        [149, 167, 157, 255],
                        false,
                    );
                    y += px(82.0);
                }
                if bucket.windows.is_empty() {
                    self.canvas.text(
                        &mut self.fonts,
                        BoxRect {
                            x: viewport.x,
                            y,
                            w: viewport.w,
                            h: px(30.0),
                        },
                        if bucket.unlimited {
                            "无限额度"
                        } else {
                            "额度暂不可用"
                        },
                        px(12.0),
                        false,
                        [124, 145, 135, 255],
                        false,
                    );
                    y += px(82.0);
                }
            }
            if usage.buckets.is_empty() {
                self.canvas.text(
                    &mut self.fonts,
                    viewport,
                    "额度暂不可用",
                    px(12.0),
                    false,
                    [124, 145, 135, 255],
                    false,
                );
            }
        } else if !self.usage_error {
            self.canvas.text(
                &mut self.fonts,
                viewport,
                if self.busy {
                    "读取中…"
                } else {
                    "点刷新查看额度"
                },
                px(12.0),
                false,
                [124, 145, 135, 255],
                false,
            );
        }
        self.canvas.clip = None;
        let resets = self.usage.as_ref().and_then(|usage| usage.resets_available);
        let reset_count = resets.map_or_else(
            || "重置次数 —".to_string(),
            |count| format!("可重置 {count} 次"),
        );
        self.canvas.text(
            &mut self.fonts,
            BoxRect {
                x: panel.x + px(18.0),
                y: panel.y + panel.h - px(28.0),
                w: panel.w - px(68.0),
                h: px(22.0),
            },
            &reset_count,
            px(10.0),
            false,
            [139, 160, 149, 255],
            false,
        );
        let refresh = BoxRect {
            x: panel.x + panel.w - px(33.0),
            y: panel.y + panel.h - px(28.0),
            w: px(22.0),
            h: px(22.0),
        };
        self.canvas.text(
            &mut self.fonts,
            refresh,
            if self.busy { "…" } else { "↻" },
            px(17.0),
            false,
            [103, 157, 133, 255],
            false,
        );
    }
    fn menu(&mut self, point: POINT) {
        unsafe {
            let menu = CreatePopupMenu();
            fn item(menu: HMENU, id: usize, label: &str, checked: bool) {
                unsafe {
                    AppendMenuW(
                        menu,
                        MF_STRING | if checked { MF_CHECKED } else { 0 },
                        id,
                        wide(label).as_ptr(),
                    );
                }
            }
            item(menu, 1, "查看剩余额度", false);
            item(menu, 2, "随机换个动作", false);
            item(
                menu,
                3,
                if self.paused {
                    "继续动画"
                } else {
                    "暂停动画"
                },
                false,
            );
            let motion = CreatePopupMenu();
            for (i, label) in [
                "静态 · 待机不动",
                "安静 · 约 3 分钟一个小动作",
                "适中 · 约 1 分钟一个动作",
                "活泼 · 连续随机动作",
            ]
            .iter()
            .enumerate()
            {
                item(
                    motion,
                    100 + i,
                    label,
                    self.settings.motion == ["still", "quiet", "normal", "lively"][i],
                );
            }
            AppendMenuW(menu, MF_POPUP, motion as usize, wide("待机动态").as_ptr());
            item(menu, 13, &format!("翅膀与尾巴摆幅 · {}%…", self.settings.dock_sway_percent), false);
            let appearance = CreatePopupMenu();
            for (i, label) in ["原色", "清晰（默认）", "鲜明"].iter().enumerate() {
                item(
                    appearance,
                    200 + i,
                    label,
                    self.settings.appearance == ["original", "clear", "vivid"][i],
                );
            }
            AppendMenuW(
                menu,
                MF_POPUP,
                appearance as usize,
                wide("画面效果").as_ptr(),
            );
            let size = CreatePopupMenu();
            for (i, label) in ["小", "标准", "大"].iter().enumerate() {
                item(
                    size,
                    300 + i,
                    label,
                    self.settings.scale == [0.8, 1.0, 1.2][i],
                );
            }
            AppendMenuW(menu, MF_POPUP, size as usize, wide("桌宠大小").as_ptr());
            let animations = CreatePopupMenu();
            let mut categories: Vec<(String, HMENU)> = Vec::new();
            for (index, a) in self.pack.animations.iter().enumerate() {
                let sub =
                    if let Some((_, m)) = categories.iter().find(|(name, _)| name == &a.category) {
                        *m
                    } else {
                        let sub = CreatePopupMenu();
                        AppendMenuW(
                            animations,
                            MF_POPUP,
                            sub as usize,
                            wide(&a.category).as_ptr(),
                        );
                        categories.push((a.category.clone(), sub));
                        sub
                    };
                item(sub, 1000 + index, &a.name, false);
            }
            AppendMenuW(
                menu,
                MF_POPUP,
                animations as usize,
                wide("动作图鉴 · 92").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            item(menu, 4, "恢复到屏幕内", false);
            item(menu, 11, "侧边吸附", self.settings.edge_docking);
            if self.docked.is_some() {
                item(menu, 12, "展开桌宠", false);
            }
            item(menu, 5, "随 Codex 启停", self.settings.auto_start);
            item(menu, 6, "重新连接", false);
            item(menu, 7, "素材来源 · hunmer/aboutme-pets", false);
            item(menu, 8, "关于与第三方说明", false);
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            item(menu, 9, "退出桌宠（本次 Codex）", false);
            item(menu, 10, "退出并关闭后台监听", false);
            SetForegroundWindow(self.hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            );
            DestroyMenu(menu);
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            if command > 0 {
                self.command(command as usize);
            }
        }
    }
    fn command(&mut self, id: usize) {
        self.finish_slide();
        match id{
        1=>{if !self.panel{self.toggle_panel();}else{self.refresh();}},2=>{let mut schedule=IdleSchedule::new("lively",0);let index=choose(&self.pack,State::Idle,self.atlas.as_ref().map(|a|a.index),"lively",&mut schedule);self.interact(index);},
        3=>{self.paused=!self.paused;if !self.paused{self.started=Instant::now();self.frame=usize::MAX;}},4=>self.restore(),
        5=>{self.settings.auto_start=!self.settings.auto_start;if !self.isolated{if let Err(error)=set_startup(self.settings.auto_start){message_box(&error);}if self.settings.auto_start{let _=start_watcher(&self.profile);}else{signal(&self.profile,"StopWatcher");}}self.settings.save(&self.profile);},
        6=>{if let Some(bridge)=&self.bridge{let _=bridge.send(BridgeCommand::Reconnect);}},7=>unsafe{ShellExecuteW(self.hwnd,wide("open").as_ptr(),wide(UPSTREAM).as_ptr(),null(),null(),SW_SHOWNORMAL);},
        8=>message_box(&format!("Codex 白龙桌宠 Rust {VERSION}\n\n动画素材：hunmer/aboutme-pets · pets/gpt\n92 张原始 WebP 无损封装于本 EXE。角色与动画权利归原权利人，不另行授予素材许可。\n\n原生 Win32 + Rust，无 Electron 或 WebView 依赖。\nRust 依赖采用 MIT / Apache-2.0 等许可，完整说明可通过 --licenses 文件路径 导出。\n独立社区项目，与 OpenAI 无隶属关系。")),
        9=>unsafe{DestroyWindow(self.hwnd);},10=>{signal(&self.profile,"StopWatcher");unsafe{DestroyWindow(self.hwnd);}},
        11=>{self.settings.edge_docking=!self.settings.edge_docking;if !self.settings.edge_docking{self.expand_from_dock(false);self.settings.dock=None;}self.settings.save(&self.profile);},
        12=>{self.start_expanding(false);},
        13=>{if unsafe{IsWindow(self.sway_panel)}!=0{unsafe{SetForegroundWindow(self.sway_panel);}}else{match sway_panel(self.hwnd,self.settings.dock_sway_percent,self.dpi){Ok(hwnd)=>self.sway_panel=hwnd,Err(error)=>message_box(&error)}}},
        100..=103=>{self.settings.motion=["still","quiet","normal","lively"][id-100].into();self.idle=IdleSchedule::new(&self.settings.motion,now_ms());self.settings.save(&self.profile);if self.snapshot.state==State::Idle{self.hold_until=0;let index=self.choose(State::Idle);self.play(index);}},
        200..=202=>{self.settings.appearance=["original","clear","vivid"][id-200].into();self.settings.save(&self.profile);},
        300..=302=>{self.settings.scale=[0.8,1.0,1.2][id-300];self.settings.save(&self.profile);if let Some(side)=self.docked{let r=window_rect(self.hwnd);self.dock_to(side,POINT{x:(r.left+r.right)/2,y:r.top});}},
        1000.. =>{let index=id-1000;if index<self.pack.animations.len(){self.interact(index);}},_=>{}
    }
        self.dirty = true;
    }
    fn event(&mut self, event: UiEvent) -> Result<(), String> {
        match event {
            UiEvent::Sway { percent, commit } => {
                self.settings.dock_sway_percent = percent.min(400);
                if !self.paused {
                    self.dock_sway_pose = DockSwayPose::at(
                        self.dock_sway_started.elapsed().as_millis() as u64,
                        &self.settings.motion, self.settings.dock_sway_percent);
                }
                if commit { self.settings.save(&self.profile); }
                self.dirty = true;
            }
            UiEvent::SwayClosed(hwnd) => {
                if self.sway_panel == hwnd { self.sway_panel = null_mut(); }
            }
            UiEvent::Down(point) => {
                let hiding = self.dock_slide.as_ref().is_some_and(|s|
                    matches!(s.phase, DockPhase::HideBody { .. } | DockPhase::HidePeek { .. }));
                self.finish_slide();
                if hiding {
                    self.press = None;
                    unsafe { ReleaseCapture(); }
                    return Ok(());
                }
                let r = window_rect(self.hwnd);
                let x = point.x - r.left;
                let y = point.y - r.top;
                self.press = Some(Press {
                    cursor: point,
                    origin: POINT {
                        x: r.left,
                        y: r.top,
                    },
                    last: POINT {
                        x: r.left,
                        y: r.top,
                    },
                    moved: false,
                    allow_drag: !(self.panel && self.panel_box.contains(x, y))
                        && !self.badge_box.contains(x, y),
                    edge_drag: self.settings.edge_docking,
                });
            }
            UiEvent::Up(point) => self.end_press(false, point),
            UiEvent::Cancel => self.end_press(true, cursor()),
            UiEvent::Escape => {
                self.finish_slide();
                self.end_press(true, cursor());
                self.panel = false;
                self.dirty = true;
            }
            UiEvent::Menu(point) => {
                self.finish_slide();
                self.end_press(true, point);
                self.menu(point);
            }
            UiEvent::Wheel(delta) => {
                if self.panel {
                    self.scroll = (self.scroll - self.px(delta as f64 / 3.0)).max(0);
                    self.dirty = true;
                }
            }
            UiEvent::Dpi(dpi) => {
                self.finish_slide();
                let dpi = dpi.max(96) as f64 / 96.0;
                if (dpi - self.dpi).abs() > 0.01 {
                    self.dpi = dpi;
                    if let Some(side) = self.docked {
                        let r = window_rect(self.hwnd);
                        self.dock_to(side, POINT { x: (r.left + r.right) / 2, y: r.top });
                        self.dirty = true;
                        return Ok(());
                    }
                    let w = self.px(WIDTH as f64);
                    let h = self.px(HEIGHT as f64);
                    self.surface = Surface::new(w, h)?;
                    self.canvas = Canvas::new(w, h);
                    let r = window_rect(self.hwnd);
                    self.move_to(constrain(
                        POINT {
                            x: r.left,
                            y: r.top,
                        },
                        work_area(POINT {
                            x: r.left + w / 2,
                            y: r.top + h - 100,
                        }),
                        w,
                        h,
                        dpi,
                    ));
                    self.dirty = true;
                }
            }
        }
        Ok(())
    }
}
fn settle_dock_slide(app: &mut App, dir: &Path, tag: &str) -> Result<(), String> {
    if app.dock_slide.is_none() {
        return Err(format!("{tag}: transition did not start"));
    }
    let started = Instant::now();
    let mut frames = 0;
    let mut changed = 0;
    let mut previous = Vec::new();
    while app.dock_slide.is_some() {
        if started.elapsed() > Duration::from_secs(2) {
            return Err(format!("{tag}: transition did not finish"));
        }
        app.tick()?;
        if app.canvas.pixels != previous {
            changed += 1;
        }
        if frames == 4 {
            app.canvas.save_bmp(&dir.join(format!("{tag}-transition.bmp")))?;
        }
        previous = app.canvas.pixels.clone();
        frames += 1;
        thread::sleep(Duration::from_millis(16));
    }
    if changed < 3 {
        return Err(format!("{tag}: transition skipped intermediate frames"));
    }
    Ok(())
}
fn dock_smoke(app: &mut App, dir: &Path) -> Result<(), String> {
    if !app.demo || !app.isolated {
        return Err("--dock-smoke requires --demo and an isolated --profile".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let area = work_area(POINT { x: 0, y: 0 });
    let mut cases = Vec::<Value>::new();
    for side in [DockSide::Left, DockSide::Right] {
        let tag = if side == DockSide::Left { "left" } else { "right" };
        if !app.dock_to(side, POINT {
            x: if side == DockSide::Left { area.left + 1 } else { area.right - 1 },
            y: area.top + (area.bottom - area.top) / 3,
        }) {
            return Err("Dock viewport could not be allocated".into());
        }
        app.render()?;
        let r = window_rect(app.hwnd);
        if r.left < area.left || r.right > area.right || r.top < area.top || r.bottom > area.bottom
            || (side == DockSide::Left && r.left != area.left)
            || (side == DockSide::Right && r.right != area.right)
        {
            return Err(format!("{tag}: dock left the monitor work area"));
        }
        app.canvas.save_bmp(&dir.join(format!("{tag}-open.bmp")))?;
        let edge_x = if side == DockSide::Left { 0 } else { app.canvas.w - 1 };
        for fraction in [0.15, 0.25, 0.35, 0.65, 0.8, 0.95] {
            let y = (fraction * app.canvas.h as f64) as i32;
            if app.canvas.pixels[(y * app.canvas.w + edge_x) as usize] >> 24 < 240 {
                return Err(format!("{tag}: artwork left a transparent seam at the edge"));
            }
        }
        let open = app.canvas.pixels.clone();
        let writes_before_sway = app.position_writes;
        app.dock_sway_pose = DockSwayPose { wing: 0.65, tail: -0.65 };
        app.render()?;
        app.canvas.save_bmp(&dir.join(format!("{tag}-sway.bmp")))?;
        let mut wing_changes = 0;
        let mut tail_changes = 0;
        for (i, pixel) in app.canvas.pixels.iter().enumerate() {
            if *pixel != open[i] {
                let x = i as i32 % app.canvas.w;
                let y = i as i32 / app.canvas.w;
                let edge = if side == DockSide::Left { x < 3 } else { x >= app.canvas.w - 3 };
                if (y as f64) < app.canvas.h as f64 * 0.58 || edge {
                    return Err(format!("{tag}: sway moved the face, hands or screen-edge roots"));
                }
                if (y as f64) < app.canvas.h as f64 * 0.75 { wing_changes += 1; }
                if (y as f64) > app.canvas.h as f64 * 0.85 { tail_changes += 1; }
            }
        }
        let sway_bounds = window_rect(app.hwnd);
        if wing_changes == 0 || tail_changes == 0 || app.position_writes != writes_before_sway
            || (r.left, r.top, r.right, r.bottom) !=
                (sway_bounds.left, sway_bounds.top, sway_bounds.right, sway_bounds.bottom) {
            return Err(format!("{tag}: sway was missing or moved the window"));
        }
        let frozen = app.dock_sway_pose;
        app.paused = true;
        app.tick()?;
        if app.dock_sway_pose != frozen {
            return Err(format!("{tag}: pause did not freeze sway"));
        }
        app.paused = false;
        let motion = app.settings.motion.clone();
        app.settings.motion = "still".into();
        app.tick()?;
        if app.dock_sway_pose != DockSwayPose::default() {
            return Err(format!("{tag}: static mode did not stop sway"));
        }
        app.settings.motion = motion;
        let saved_percent = app.settings.dock_sway_percent;
        for percent in [400, 0, saved_percent] {
            app.event(UiEvent::Sway { percent, commit: true })?;
            if Settings::load(&app.profile).dock_sway_percent != percent {
                return Err(format!("{tag}: independent sway value was not saved"));
            }
            if percent == 0 && app.dock_sway_pose != DockSwayPose::default() {
                return Err(format!("{tag}: zero amplitude did not stop sway"));
            }
        }
        app.dock_sway_pose = DockSwayPose::default();
        app.render()?;
        app.dock_eye_closed = true;
        app.render()?;
        app.canvas.save_bmp(&dir.join(format!("{tag}-blink.bmp")))?;
        let art = &app.dock_art.left;
        let mut eye = app.dock_art.eye;
        if side == DockSide::Right {
            eye.x = art.width as i32 - eye.x - eye.w;
        }
        let eye_region = BoxRect {
            x: (eye.x as f64 * app.canvas.w as f64 / art.width as f64).floor() as i32 - 2,
            y: (eye.y as f64 * app.canvas.h as f64 / art.height as f64).floor() as i32 - 2,
            w: (eye.w as f64 * app.canvas.w as f64 / art.width as f64).ceil() as i32 + 4,
            h: (eye.h as f64 * app.canvas.h as f64 / art.height as f64).ceil() as i32 + 4,
        };
        let mut changed = 0;
        for (i, pixel) in app.canvas.pixels.iter().enumerate() {
            if *pixel != open[i] {
                changed += 1;
                if !eye_region.contains(i as i32 % app.canvas.w, i as i32 / app.canvas.w) {
                    return Err(format!("{tag}: blink moved artwork outside the eye"));
                }
            }
        }
        if changed == 0 {
            return Err(format!("{tag}: blink did not change the eye"));
        }
        app.dock_eye_closed = false;
        app.render()?;
        let p = POINT { x: r.left + app.canvas.w / 2, y: r.top + app.canvas.h / 4 };
        app.event(UiEvent::Down(p))?;
        let writes = app.position_writes;
        for _ in 0..128 {
            app.sample_drag(p);
        }
        let held = window_rect(app.hwnd);
        if (r.left, r.top, r.right, r.bottom) != (held.left, held.top, held.right, held.bottom)
            || app.position_writes != writes
        {
            return Err(format!("{tag}: a stationary hold moved the pet"));
        }
        app.end_press(false, p);
        settle_dock_slide(app, dir, &format!("{tag}-expand"))?;
        if app.docked.is_some() || !app.panel
            || app.canvas.w != app.px(WIDTH as f64) || app.canvas.h != app.px(HEIGHT as f64)
        {
            return Err(format!("{tag}: clicking did not expand and open quota"));
        }
        app.panel = false;
        app.dock_to(side, p);
        let r = window_rect(app.hwnd);
        let p = POINT { x: r.left + app.canvas.w / 2, y: r.top + app.canvas.h / 4 };
        app.event(UiEvent::Down(p))?;
        let vertical = POINT { x: p.x, y: p.y + app.px(24.0) };
        app.sample_drag(vertical);
        app.end_press(false, vertical);
        if app.docked != Some(side) || Settings::load(&app.profile).dock.is_none() {
            return Err(format!("{tag}: vertical docking or persistence failed"));
        }
        let r = window_rect(app.hwnd);
        let p = POINT { x: r.left + app.canvas.w / 2, y: r.top + app.canvas.h / 4 };
        app.event(UiEvent::Down(p))?;
        let inward = POINT {
            x: p.x + app.px(if side == DockSide::Left { 180.0 } else { -180.0 }),
            y: p.y,
        };
        app.sample_drag(inward);
        app.end_press(false, inward);
        if app.docked.is_some() || app.canvas.w != app.px(WIDTH as f64) {
            return Err(format!("{tag}: dragging inward did not expand"));
        }
        cases.push(json!({
            "side":side, "stable":true, "boundedToWorkArea":true, "holdSamples":128,
            "artworkTouchesScreenEdge":true,
            "wingAndTailSway":true, "swayKeepsFaceHandsAndEdgeFixed":true,
            "swayDoesNotMoveWindow":true,
            "pauseAndStaticStopSway":true,
            "independentSwaySettingSaved":true,
            "blinkChangedOnlyEye":true, "blinkChangedPixels":changed,
            "clickOpenedQuota":true, "verticalDragStayedDocked":true,
            "expandTransitionHadIntermediateFrames":true,
            "dragInwardExpanded":true, "savedDockRestorable":true,
        }));
    }
    for side in [DockSide::Left, DockSide::Right] {
        let r = window_rect(app.hwnd);
        let p = POINT {
            x: r.left + app.canvas.w / 2,
            y: r.top + app.px(420.0),
        };
        app.event(UiEvent::Down(p))?;
        let edge = POINT {
            x: if side == DockSide::Left { area.left + 1 } else { area.right - 1 },
            y: area.top + (area.bottom - area.top) / 2,
        };
        app.sample_drag(edge);
        app.end_press(false, edge);
        settle_dock_slide(app, dir, &format!("{side:?}-dock"))?;
        if app.docked != Some(side) {
            return Err(format!("{side:?}: releasing near the edge did not dock"));
        }
        app.expand_from_dock(false);
    }
    app.dock_to(DockSide::Left, POINT { x: area.left + 1, y: area.top + 100 });
    app.command(11);
    if app.settings.edge_docking || app.docked.is_some() {
        return Err("Disabling side docking did not restore the pet".into());
    }
    app.command(11);
    app.restore();
    cases.push(json!({"case":"automatic-edge-release-and-disable","stable":true,
        "dockTransitionHadIntermediateFrames":true}));
    write_json(&dir.join("dock-smoke.json"), &json!({
        "version":VERSION, "passed":true, "cases":cases,
        "dpi":app.dpi, "animations":app.pack.animations.len(),
        "singleExecutable":true, "realCodexWasNotClosed":true,
    }))
}
impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.sway_panel) != 0 { DestroyWindow(self.sway_panel); }
        }
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn message_box(text: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide(TITLE).as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
fn gui(args: &[String], profile: PathBuf) -> Result<(), String> {
    let Some(_guard) = singleton(&profile, "Pet") else {
        return Ok(());
    };
    let quit = named_event(&profile, "QuitPet");
    let demo = flag(args, "--demo");
    let isolated = option(args, "--profile").is_some();
    let owner = option(args, "--codex-pid")
        .and_then(|v| v.parse::<u32>().ok())
        .and_then(process_handle);
    if option(args, "--codex-pid").is_some() && owner.is_none() {
        return Ok(());
    }
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let class = wide("CodexPetRust.NativeWindow");
    let instance = unsafe { GetModuleHandleW(null()) };
    let mut wc: WNDCLASSW = unsafe { zeroed() };
    wc.lpfnWndProc = Some(wndproc);
    wc.hInstance = instance;
    wc.hIcon = unsafe { LoadIconW(instance, 1usize as *const u16) };
    wc.lpszClassName = class.as_ptr();
    wc.hCursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    unsafe {
        RegisterClassW(&wc);
    }
    // QA can expose a taskbar entry so desktop automation can discover the
    // otherwise intentionally hidden tool window. Normal runs stay trayless.
    let ui_test = flag(args, "--ui-test");
    let extended_style = WS_EX_LAYERED
        | WS_EX_TOPMOST
        | if ui_test {
            WS_EX_APPWINDOW
        } else {
            WS_EX_TOOLWINDOW
        };
    let hwnd = unsafe {
        CreateWindowExW(
            extended_style,
            class.as_ptr(),
            wide(if ui_test { "Codex 白龙桌宠 · 吸附测试" } else { TITLE }).as_ptr(),
            WS_POPUP,
            100,
            50,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err("Cannot create transparent window".into());
    }
    // Consume a launcher's initial SW_HIDE before showing the layered image.
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
    let mut app = App::new(hwnd, profile, isolated, demo)?;
    if ui_test && flag(args, "--sway-settings") { app.command(13); }
    let smoke = option(args, "--smoke").map(PathBuf::from);
    let dock_test = option(args, "--dock-smoke").map(PathBuf::from);
    let since = Instant::now();
    let mut smoke_stage = 0;
    let mut diagnostics_at = Instant::now();
    let mut evidence = Vec::<Value>::new();
    unsafe {
        SetTimer(hwnd, 1, if app.dock_slide.is_some() { 16 } else { 33 }, None);
    }
    let mut message: MSG = unsafe { zeroed() };
    loop {
        let got = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if got == 0 {
            break;
        }
        if got < 0 {
            return Err("Windows event loop failed".into());
        }
        if unsafe { IsWindow(app.sway_panel) } != 0 {
            if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize
                && (message.hwnd == app.sway_panel || unsafe { IsChild(app.sway_panel, message.hwnd) } != 0) {
                unsafe { SendMessageW(app.sway_panel, WM_CLOSE, 0, 0); }
                continue;
            }
            if unsafe { IsDialogMessageW(app.sway_panel, &mut message) } != 0 { continue; }
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        while let Some(event) = EVENTS.with(|q| q.borrow_mut().pop_front()) {
            app.event(event)?;
        }
        if unsafe { IsWindow(hwnd) } == 0 {
            break;
        }
        if unsafe { WaitForSingleObject(quit.0, 0) } == WAIT_OBJECT_0
            || owner.as_ref().is_some_and(|h| !alive(h))
        {
            unsafe {
                DestroyWindow(hwnd);
            }
            continue;
        }
        app.tick()?;
        if let Some(dir) = &dock_test {
            if app.atlas.is_some() {
                dock_smoke(&mut app, dir)?;
                unsafe { DestroyWindow(hwnd); }
                continue;
            }
            if since.elapsed() > Duration::from_secs(30) {
                return Err("Dock smoke test timed out loading the original animation".into());
            }
        }
        if ui_test && diagnostics_at.elapsed() >= Duration::from_millis(500) {
            let r = window_rect(hwnd);
            let _ = write_json(
                &app.profile.join("native-state.json"),
                &json!({"visible":unsafe{IsWindowVisible(hwnd)}!=0,"bounds":{"x":r.left,"y":r.top,"width":r.right-r.left,"height":r.bottom-r.top},"dpi":app.dpi,"panel":app.panel,"frame":app.frame,"animation":app.atlas.as_ref().map(|a|&app.pack.animations[a.index].id),"motion":app.settings.motion,"appearance":app.settings.appearance,"edgeDocking":app.settings.edge_docking,"docked":app.docked,"dockBlinkClosed":app.dock_eye_closed,"positionWrites":app.position_writes,"state":app.snapshot}),
            );
            diagnostics_at = Instant::now();
        }
        if let Some(dir) = &smoke {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            if app.atlas.is_some() && smoke_stage == 0 {
                app.canvas.save_bmp(&dir.join("idle.bmp"))?;
                let r = window_rect(hwnd);
                let before = (r.left, r.top, r.right, r.bottom);
                let p = POINT {
                    x: r.left + app.canvas.w / 2,
                    y: r.top + app.canvas.h - 100,
                };
                app.event(UiEvent::Down(p))?;
                for _ in 0..128 {
                    app.sample_drag(p);
                }
                let after = window_rect(hwnd);
                let stable = before == (after.left, after.top, after.right, after.bottom)
                    && app.position_writes == 0;
                app.end_press(true, p);
                evidence.push(json!({"case":"stationary-hold","samples":128,"stable":stable,"positionWrites":app.position_writes}));
                app.command(1);
                smoke_stage = 1;
            }
            if smoke_stage == 1 && since.elapsed() > Duration::from_secs(6) && app.pending.is_none()
            {
                app.tick()?;
                app.canvas.save_bmp(&dir.join("quota.bmp"))?;
                evidence.push(json!({"case":"embedded-assets","animations":app.pack.animations.len(),"frame":app.frame,"panel":app.panel,"usage":app.usage,"state":app.snapshot}));
                evidence.push(json!({"case":"visible-native-window","stable":unsafe{IsWindowVisible(hwnd)}!=0}));
                write_json(
                    &dir.join("smoke.json"),
                    &json!({"version":VERSION,"passed":evidence.iter().all(|v|v.get("stable").is_none_or(|v|v==true)),"cases":evidence,"singleExecutable":true,"runtimeExtraction":false}),
                )?;
                unsafe {
                    DestroyWindow(hwnd);
                }
                smoke_stage = 2;
            }
            if since.elapsed() > Duration::from_secs(30) {
                return Err("Native smoke test timed out".into());
            }
        }
    }
    Ok(())
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let profile = profile_path(&args);
    if let Some(file) = option(&args, "--licenses") {
        let pack = Pack::read()?;
        let text = format!(
            "Codex 白龙桌宠 Rust {VERSION}\n素材来源：{UPSTREAM}\n\n{}\n\n{}\n\n{}",
            pack.metadata["notices"].as_str().unwrap_or(""),
            pack.metadata["rustLicenses"].as_str().unwrap_or(""),
            pack.metadata["upstreamReadme"].as_str().unwrap_or("")
        );
        fs::write(file, text).map_err(|e| e.to_string())?;
        return Ok(());
    }
    if let Some(file) = option(&args, "--owner-probe") {
        let owner = if flag(&args, "--cli") {
            find_cli()
        } else {
            find_codex()
        };
        write_json(
            Path::new(&file),
            &json!({"found":owner.is_some(),"codexPid":owner.as_ref().map(|(pid,_)|pid),"sessionId":session_id()}),
        )?;
        return Ok(());
    }
    if let Some(file) = option(&args, "--probe") {
        if flag(&args, "--cli") {
            let since = option(&args, "--codex-since")
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(now_ms);
            let mut logs = CliLogs::new(codex_home().join("sessions"), since);
            let state = logs.poll();
            let usage = read_cli_usage(&cli_executable(&args));
            write_json(
                Path::new(&file),
                &json!({"source":"cli","state":state,"usage":usage.as_ref().ok(),"usageError":usage.as_ref().err(),"logUsage":logs.usage().ok(),"animationCount":Pack::read()?.animations.len()}),
            )?;
            return usage.map(|_| ());
        }
        let result = (|| -> Result<Value, String> {
            let mut d = Desktop::connect()?;
            d.list()?;
            for _ in 0..40 {
                let _ = d.read_stream()?;
                thread::sleep(Duration::from_millis(25));
            }
            let usage = normalize_usage(&d.call("get_usage_limits", json!({}))?);
            Ok(
                json!({"connected":true,"state":aggregate(&d.threads,d.complete_until,true),"usage":usage,"animationCount":Pack::read()?.animations.len()}),
            )
        })();
        let report = match &result {
            Ok(v) => v.clone(),
            Err(e) => json!({"connected":false,"error":e}),
        };
        write_json(Path::new(&file), &report)?;
        return result.map(|_| ());
    }
    if flag(&args, "--quit") {
        signal(&profile, "QuitPet");
        return Ok(());
    }
    if flag(&args, "--stop") {
        signal(&profile, "StopWatcher");
        signal(&profile, "QuitPet");
        return Ok(());
    }
    if flag(&args, "--disable-autostart") {
        set_startup(false)?;
        let mut settings = Settings::load(&profile);
        settings.auto_start = false;
        settings.save(&profile);
        signal(&profile, "StopWatcher");
        return Ok(());
    }
    if flag(&args, "--pet")
        || flag(&args, "--demo") && !flag(&args, "--watch")
        || option(&args, "--smoke").is_some()
    {
        return gui(&args, profile);
    }
    if flag(&args, "--install") || !flag(&args, "--watch") {
        let mut settings = Settings::load(&profile);
        if flag(&args, "--install") {
            settings.auto_start = true;
        }
        if settings.auto_start {
            set_startup(true)?;
        }
        settings.save(&profile);
    }
    watcher(&args, &profile)
}
fn main() {
    if let Err(error) = run() {
        let args: Vec<String> = std::env::args().collect();
        if let Some(dir) = option(&args, "--smoke").or_else(|| option(&args, "--dock-smoke")) {
            let _ = write_json(
                &PathBuf::from(dir).join("error.json"),
                &json!({"error":error}),
            );
        } else if option(&args, "--probe").is_none() {
            message_box(&error);
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dock_sway_is_continuous_and_has_an_independent_amplitude_control() {
        assert_eq!(DockSwayPose::at(0, "quiet", 200), DockSwayPose::default());
        assert_eq!(DockSwayPose::at(4000, "still", 400), DockSwayPose::default());
        assert_eq!(DockSwayPose::at(4000, "quiet", 0), DockSwayPose::default());
        let quiet = DockSwayPose::at(1000, "quiet", 100);
        assert_eq!(quiet, DockSwayPose::at(1000, "lively", 100));
        let strong = DockSwayPose::at(1000, "quiet", 300);
        assert!((strong.wing - quiet.wing * 3.0).abs() < 0.00001);
        assert!((strong.tail - quiet.tail * 3.0).abs() < 0.00001);
        assert!((quiet.wing - quiet.tail).abs() > 0.01);
        let mut previous = DockSwayPose::default();
        for ms in (0..40000).step_by(80) {
            let pose = DockSwayPose::at(ms, "quiet", 400);
            assert!(pose.wing.abs() <= 2.6 && pose.tail.abs() <= 2.6);
            assert!((pose.wing - previous.wing).abs() < 0.18);
            assert!((pose.tail - previous.tail).abs() < 0.18);
            previous = pose;
        }
    }
    #[test]
    fn dock_sway_changes_wing_and_tail_but_pins_face_hands_and_both_screen_edges() {
        let art = DockArt::read().unwrap();
        for (w, h) in [(74, 190), (92, 238), (148, 380)] {
            for side in [DockSide::Left, DockSide::Right] {
                let frame = if side == DockSide::Left { &art.left } else { &art.right };
                let rect = BoxRect { x: 0, y: 0, w, h };
                let mut base = Canvas::new(w, h);
                base.picture(frame, rect, "clear");
                let mut still = Canvas::new(w, h);
                still.dock_picture(frame, rect, "clear", DockSwayPose::default(), side);
                assert_eq!(base.pixels, still.pixels);
                let mut moving = Canvas::new(w, h);
                moving.dock_picture(frame, rect, "clear",
                    DockSwayPose { wing: 2.6, tail: -2.6 }, side);
                let mut wing = 0;
                let mut tail = 0;
                for i in 0..base.pixels.len() {
                    let x = i as i32 % w;
                    let y = i as i32 / w;
                    if (y as f64) < h as f64 * 0.58
                        || if side == DockSide::Left { x < 3 } else { x >= w - 3 } {
                        assert_eq!(base.pixels[i], moving.pixels[i], "{side:?} at {x},{y}");
                    } else if base.pixels[i] != moving.pixels[i] {
                        if (y as f64) < h as f64 * 0.75 { wing += 1; }
                        if (y as f64) > h as f64 * 0.85 { tail += 1; }
                    }
                }
                assert!(wing > 0 && tail > 0, "{side:?}: wing={wing}, tail={tail}");
            }
        }
    }
    #[test]
    fn dock_slide_easing_has_endpoints_and_monotonic_intermediate_frames() {
        for reveal in [false, true] {
            assert_eq!(slide_fraction(-1.0, reveal), 0.0);
            assert_eq!(slide_fraction(2.0, reveal), 1.0);
            let mut previous = 0.0;
            for frame in 1..60 {
                let current = slide_fraction(frame as f64 / 60.0, reveal);
                assert!(current > previous && current < 1.0);
                previous = current;
            }
        }
    }
    #[test]
    fn side_docking_stays_inside_each_monitor_at_all_supported_scales() {
        for area in [
            RECT { left: 0, top: 0, right: 1920, bottom: 1040 },
            RECT { left: -1920, top: -1080, right: 0, bottom: -40 },
            RECT { left: 1920, top: -200, right: 3200, bottom: 800 },
        ] {
            for dpi in [1.0, 1.25, 1.5, 2.0] {
                for scale in [0.8, 1.0, 1.2] {
                    for side in [DockSide::Left, DockSide::Right] {
                        for y in [-1000000, area.top + 100, 1000000] {
                            let r = dock_layout(area, side, y, dpi, scale, 466.0 / 1149.0);
                            assert!(r.w > 0 && r.h > 0);
                            assert!(r.x >= area.left && r.x + r.w <= area.right);
                            assert!(r.y >= area.top && r.y + r.h <= area.bottom);
                            assert_eq!(
                                if side == DockSide::Left { r.x } else { r.x + r.w },
                                if side == DockSide::Left { area.left } else { area.right },
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn side_docking_is_triggered_only_near_a_horizontal_edge() {
        let area = RECT { left: -1920, top: 0, right: 0, bottom: 1040 };
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            assert_eq!(dock_side(POINT { x: -1919, y: 400 }, area, dpi), Some(DockSide::Left));
            assert_eq!(dock_side(POINT { x: -1, y: 400 }, area, dpi), Some(DockSide::Right));
            assert_eq!(dock_side(POINT { x: -960, y: 1 }, area, dpi), None);
            assert_eq!(dock_side(POINT { x: -960, y: 1039 }, area, dpi), None);
            assert_eq!(dock_side(POINT { x: -1919, y: -1 }, area, dpi), None);
        }
    }
    #[test]
    fn painted_character_reaches_the_edge_before_the_drag_cursor() {
        let area = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
        assert_eq!(dock_target(
            POINT { x: 145, y: 400 }, area, 1.0,
            Some(BoxRect { x: 20, y: 310, w: 180, h: 220 }),
        ), Some(DockSide::Left));
        assert_eq!(dock_target(
            POINT { x: 1775, y: 400 }, area, 1.0,
            Some(BoxRect { x: 1720, y: 310, w: 180, h: 220 }),
        ), Some(DockSide::Right));
        assert_eq!(dock_target(
            POINT { x: 200, y: 400 }, area, 1.0,
            Some(BoxRect { x: 150, y: 310, w: 180, h: 220 }),
        ), None);
        let mut press = Press {
            cursor: POINT { x: 800, y: 400 },
            origin: POINT { x: 540, y: 50 },
            last: POINT { x: 540, y: 50 },
            moved: false, allow_drag: true, edge_drag: true,
        };
        let (position, _) = press.sample(POINT { x: 90, y: 400 }, area, 520, 620, 1.0).unwrap();
        assert_eq!(position.x, -170);
        assert_eq!(position.y, 50);
        for _ in 0..128 {
            assert!(press.sample(POINT { x: 90, y: 400 }, area, 520, 620, 1.0).is_none());
        }
    }
    #[test]
    fn dock_blink_is_short_infrequent_and_respects_pause() {
        let mut blink = DockBlink { next: 1000, until: 0 };
        assert!(!blink.closed(999, true));
        assert!(blink.closed(1000, true));
        assert!(blink.closed(1139, true));
        assert!(!blink.closed(1140, true));
        assert!((5500..9000).contains(&blink.next));
        assert!(!blink.closed(1200, false));
        assert!(!blink.closed(1201, true));
        assert!(!blink.closed(5000, false));
        assert!(!blink.closed(5001, true));
    }
    #[test]
    fn dock_art_has_alpha_and_blinks_without_replacing_the_approved_body() {
        let art = DockArt::read().unwrap();
        assert!(art.left.width > 300 && art.left.height > art.left.width);
        assert_eq!((art.left.width, art.left.height), (art.right.width, art.right.height));
        for y in [0, art.left.height / 2, art.left.height - 1] {
            for x in [0, art.left.width / 2, art.left.width - 1] {
                let a = (y * art.left.width + x) * 4;
                let b = (y * art.right.width + art.right.width - 1 - x) * 4;
                assert_eq!(&art.left.pixels[a..a + 4], &art.right.pixels[b..b + 4]);
            }
        }
        assert!(art.left.pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(art.left.pixels.chunks_exact(4).any(|p| p[3] == 255));
        assert!(art.blink_left.width < art.left.width / 2);
        assert!(art.blink_left.height < art.left.height / 4);
        assert!(art.blink_left.pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(art.blink_left.pixels.chunks_exact(4).any(|p| p[3] >= 250));
    }
    #[test]
    fn dock_art_touches_both_edges_at_every_supported_scale_and_dpi() {
        let art = DockArt::read().unwrap();
        let area = RECT { left: 0, top: 0, right: 3840, bottom: 2160 };
        for dpi in [1.0, 1.25, 1.5, 2.0] {
            for scale in [0.8, 1.0, 1.2] {
                for side in [DockSide::Left, DockSide::Right] {
                    let frame = if side == DockSide::Left { &art.left } else { &art.right };
                    let r = dock_layout(area, side, 100, dpi, scale,
                        frame.width as f64 / frame.height as f64);
                    let mut canvas = Canvas::new(r.w, r.h);
                    canvas.picture(frame, BoxRect { x: 0, y: 0, w: r.w, h: r.h }, "clear");
                    let x = if side == DockSide::Left { 0 } else { r.w - 1 };
                    for fraction in [0.15, 0.25, 0.35, 0.65, 0.8, 0.95] {
                        let y = (fraction * r.h as f64) as i32;
                        assert!(canvas.pixels[(y * r.w + x) as usize] >> 24 >= 240,
                            "{side:?} scale={scale} dpi={dpi} row={y}: transparent seam");
                    }
                }
            }
        }
    }
    #[test]
    fn dock_palette_matches_silver_hair_without_desaturating_skin_or_iris() {
        let hair = [235, 221, 245, 253];
        let silver = dock_color(hair);
        assert!(silver[0] - silver[1] < hair[0] - hair[1]);
        assert!(silver[2] - silver[1] < hair[2] - hair[1]);
        assert_eq!(silver[3], hair[3]);
        assert_eq!(dock_color([255, 225, 217, 254]), [255, 225, 217, 254]);
        assert_eq!(dock_color([161, 119, 207, 253]), [161, 119, 207, 253]);
        assert_eq!(dock_color([213, 193, 235, 253]), [213, 193, 235, 253]);
        assert_eq!(dock_color([255, 255, 255, 0]), [255, 255, 255, 0]);
    }
    #[test]
    fn old_settings_keep_their_position_and_support_dock_persistence() {
        let mut settings: Settings = serde_json::from_value(json!({
            "scale":0.8, "position":{"x":1395,"y":305}, "autoStart":false,
            "motion":"quiet", "appearance":"clear"
        })).unwrap();
        assert!(settings.edge_docking);
        assert_eq!(settings.dock_sway_percent, 200);
        assert!(!settings.auto_start);
        assert_eq!(settings.position.as_ref().unwrap().x, 1395);
        settings.dock = Some(DockPlacement {
            side: DockSide::Right, anchor: Position { x: -1, y: 270 },
        });
        let restored: Settings = serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();
        assert_eq!(restored.dock.as_ref().unwrap().side, DockSide::Right);
        assert_eq!(restored.dock.as_ref().unwrap().anchor.x, -1);
        assert_eq!(restored.position.as_ref().unwrap().y, 305);
    }
    #[test]
    fn independent_sway_setting_persists_and_clamps_without_changing_idle_motion() {
        let profile = std::env::temp_dir().join(format!("codex-pet-sway-{}", uuid()));
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("settings.json"),
            br#"{"scale":1.2,"autoStart":false,"motion":"quiet","dockSwayPercent":900}"#).unwrap();
        let mut settings = Settings::load(&profile);
        assert_eq!(settings.dock_sway_percent, 400);
        assert_eq!(settings.scale, 1.2);
        assert!(!settings.auto_start);
        settings.dock_sway_percent = 75;
        settings.save(&profile);
        let restored = Settings::load(&profile);
        assert_eq!(restored.dock_sway_percent, 75);
        assert_eq!(restored.motion, "quiet");
        fs::remove_dir_all(profile).unwrap();
    }
    #[test]
    fn cli_session_origins_exclude_desktop_and_unknown_vscode_clients() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/cli-origins.json")).unwrap();
        let root = std::env::temp_dir().join(format!("codex-pet-origins-{}", uuid()));
        fs::create_dir_all(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            for case in cases {
                let metadata = &case["metadata"];
                let header = json!({"type":"session_meta","payload":metadata});
                let event = json!({"type":"event_msg","timestamp":"2026-11-12T00:00:01Z","payload":{"type":"task_started"}});
                fs::write(
                    root.join("rollout-origin.jsonl"),
                    format!("{header}\n{event}\n"),
                )
                .unwrap();
                let mut logs = CliLogs::new(root.clone(), 0);
                assert_eq!(
                    logs.poll().working,
                    usize::from(case["expected"] == true),
                    "{metadata}"
                );
            }
        });
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }
    #[test]
    fn cli_process_classification_uses_windows_argv_and_the_top_level_command() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/cli-command-lines.json")).unwrap();
        for case in cases {
            let command = format!(
                "\"C:\\mock bin\\codex.exe\" {}",
                case["arguments"].as_str().unwrap()
            );
            assert_eq!(
                is_cli_command(&command),
                case["expected"] == true,
                "{command}"
            );
        }
        assert!(!is_cli_command(""));
    }
    #[test]
    fn cli_events_keep_only_status_and_timestamped_quota() {
        let since = rollout_time("2026-11-12T00:00:00Z").unwrap();
        assert_eq!(rollout_time("2026-11-12T00:00:01.123Z"), Some(since + 1123));
        assert_eq!(rollout_time("bad"), None);
        let mut logs = CliLogs::new(PathBuf::new(), since);
        let mut tail = CliTail {
            cli: true,
            ..CliTail::default()
        };
        let event = |kind| json!({"timestamp":"2026-11-12T00:00:01.123Z","type":"event_msg","payload":{"type":kind,"last_agent_message":"private"}});
        logs.consume(&mut tail, &event("task_started"), true);
        assert_eq!(classify(&tail.status, 0), State::Working);
        logs.consume(&mut tail, &event("task_complete"), true);
        assert_eq!(classify(&tail.status, 0), State::Idle);
        assert!(logs.complete_until > now_ms());
        logs.consume(&mut tail, &event("turn_started"), true);
        logs.consume(&mut tail, &event("turn_aborted"), true);
        assert_eq!(classify(&tail.status, 0), State::Idle);
        let mut old = event("task_started");
        old["timestamp"] = json!("2026-11-11T00:00:00Z");
        logs.consume(&mut tail, &old, true);
        assert_eq!(classify(&tail.status, 0), State::Idle);
        let usage = cli_usage(
            &json!({"primary":{"used_percent":21,"window_minutes":300},"secondary":{"used_percent":null}}),
            since,
        );
        assert_eq!(usage.fetched_at, since);
        assert!(usage.from_cli);
        assert_eq!(usage.buckets[0].windows[0].remaining, Some(79.0));
        assert_eq!(usage.buckets[0].windows[1].remaining, None);
        assert_eq!(usage.buckets[0].windows[0].minutes, Some(300));
    }
    #[test]
    fn cli_rollouts_recover_from_partial_records_and_ignore_other_sources() {
        let root = std::env::temp_dir().join(format!("codex-pet-cli-test-{}", uuid()));
        fs::create_dir_all(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            let file = root.join("rollout-test.jsonl");
            fs::write(
                &file,
                b"{\"type\":\"session_meta\",\"payload\":{\"source\":\"cli\"}}\n",
            )
            .unwrap();
            let mut logs = CliLogs::new(root.clone(), 0);
            assert_eq!(logs.poll().state, State::Idle);
            let line="{\"type\":\"event_msg\",\"timestamp\":\"2026-11-12T00:00:01Z\",\"payload\":{\"type\":\"task_started\"}}\n";
            let mut writer = OpenOptions::new().append(true).open(&file).unwrap();
            writer.write_all(&line.as_bytes()[..40]).unwrap();
            assert_eq!(logs.poll().state, State::Idle);
            writer.write_all(&line.as_bytes()[40..]).unwrap();
            assert_eq!(logs.poll().state, State::Working);
            writer
                .write_all(&vec![b'x'; 2 * CLI_READ_LIMIT as usize])
                .unwrap();
            writer.write_all(b"\n").unwrap();
            writer
                .write_all(line.replace("task_started", "task_complete").as_bytes())
                .unwrap();
            for _ in 0..4 {
                logs.poll();
            }
            assert!(logs.complete_until > now_ms());
            drop(writer);
            fs::write(
                &file,
                b"{\"type\":\"session_meta\",\"payload\":{\"source\":\"mcp\"}}\n",
            )
            .unwrap();
            logs.complete_until = 0;
            assert_eq!(logs.poll().state, State::Idle);
            fs::remove_file(&file).unwrap();
            assert_eq!(logs.poll().working, 0);
        });
        fs::remove_dir_all(&root).unwrap();
        result.unwrap();
    }
    fn sheet(count: usize) -> Sheet {
        Sheet {
            frame_width: 640,
            frame_height: 360,
            columns: 8,
            rows: count.div_ceil(8),
            frame_count: count,
            fps: 12,
        }
    }
    fn shadow() -> Shadow {
        Shadow {
            status: json!({"type":"active","activeFlags":[]}),
            requests: Vec::new(),
            revision: 1,
            id: "test".into(),
            host: "local".into(),
        }
    }
    #[test]
    fn embedded_webps_are_byte_for_byte_originals() {
        let pack = Pack::read().unwrap();
        assert_eq!(pack.animations.len(), 92);
        for (i, a) in pack.animations.iter().enumerate() {
            let original = pack.original(i).unwrap();
            assert_eq!(
                original,
                fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../assets/spritesheet")
                        .join(format!("{}.webp", a.id))
                )
                .unwrap(),
                "{}",
                a.id
            );
        }
    }
    #[test]
    fn framing_handles_fragments_and_multiple_messages() {
        let file = File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
        let mut pipe = Pipe {
            file,
            buffer: Vec::new(),
            limit: 1024,
        };
        let values = [json!({"message":"额度 79%"}), json!({"active":true})];
        let mut bytes = Vec::new();
        for value in &values {
            let b = serde_json::to_vec(value).unwrap();
            bytes.extend_from_slice(&(b.len() as u32).to_le_bytes());
            bytes.extend(b);
        }
        pipe.buffer.extend_from_slice(&bytes[..2]);
        assert!(pipe.take().unwrap().is_none());
        pipe.buffer.extend_from_slice(&bytes[2..9]);
        assert!(pipe.take().unwrap().is_none());
        pipe.buffer.extend_from_slice(&bytes[9..]);
        assert_eq!(pipe.take().unwrap().unwrap(), values[0]);
        assert_eq!(pipe.take().unwrap().unwrap(), values[1]);
        assert!(pipe.take().unwrap().is_none());
    }
    #[test]
    fn malformed_frames_are_rejected() {
        let file = File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
        let mut pipe = Pipe {
            file,
            buffer: 2000u32.to_le_bytes().to_vec(),
            limit: 100,
        };
        assert!(pipe.take().is_err());
        pipe.buffer = vec![0; 4];
        assert!(pipe.take().is_err());
    }
    #[test]
    fn bridge_only_allows_read_tools() {
        assert!(allowed_tool("list_threads"));
        assert!(allowed_tool("get_usage_limits"));
        for t in [
            "delete_thread",
            "create_thread",
            "automation_update",
            "send_message_to_thread",
        ] {
            assert!(!allowed_tool(t));
        }
    }
    #[test]
    fn waiting_and_errors_outrank_concurrent_work() {
        let mut threads = HashMap::new();
        threads.insert("a".into(), shadow());
        let mut wait = shadow();
        wait.status = json!({"type":"active","activeFlags":["waitingOnApproval"]});
        threads.insert("b".into(), wait);
        let result = aggregate(&threads, 0, true);
        assert_eq!(result.state, State::Waiting);
        assert_eq!(result.working, 1);
        assert_eq!(result.waiting, 1);
        threads.get_mut("b").unwrap().status = json!("systemError");
        assert_eq!(aggregate(&threads, 0, true).state, State::Error);
    }
    #[test]
    fn completion_expires_and_never_overrides_active_work() {
        let mut threads = HashMap::new();
        assert_eq!(
            aggregate(&threads, now_ms() + 10000, true).state,
            State::Complete
        );
        assert_eq!(aggregate(&threads, 0, true).state, State::Idle);
        threads.insert("a".into(), shadow());
        assert_eq!(
            aggregate(&threads, now_ms() + 10000, true).state,
            State::Working
        );
    }
    #[test]
    fn quota_keeps_missing_values_and_clamps_percent() {
        let data = normalize_usage(
            &json!({"rateLimitsByLimitId":{"a":{"primary":{"usedPercent":21},"secondary":{"usedPercent":null}},"b":{"primary":{"usedPercent":120},"secondary":{"usedPercent":-5}}}}),
        );
        assert_eq!(data.buckets[0].windows[0].remaining, Some(79.0));
        assert_eq!(data.buckets[0].windows[1].remaining, None);
        assert_eq!(data.buckets[1].windows[0].remaining, Some(0.0));
        assert_eq!(data.buckets[1].windows[1].remaining, Some(100.0));
        assert!(normalize_usage(&Value::Null).buckets.is_empty());
        assert_eq!(data.resets_available, None);
        assert_eq!(
            normalize_usage(&json!({"rateLimitResetCredits":{"availableCount":2}}))
                .resets_available,
            Some(2)
        );
        assert_eq!(
            normalize_usage(&json!({"rateLimitResetCredits":{"availableCount":0}}))
                .resets_available,
            Some(0)
        );
        assert_eq!(
            normalize_usage(&json!({"rateLimitResetCredits":{"availableCount":null}}))
                .resets_available,
            None
        );
        assert_eq!(
            normalize_usage(&json!({"rateLimitResetCredits":{"availableCount":-1}}))
                .resets_available,
            None
        );
    }
    #[test]
    fn stream_shadow_discards_transcripts_and_request_arguments() {
        let mut t = shadow();
        patch_shadow(
            &mut t,
            &json!([{"op":"add","path":["requests",0],"value":{"params":{"prompt":"private"}}},{"op":"replace","path":["turns",0,"content"],"value":"private"}]),
        );
        assert_eq!(t.requests, vec![true]);
        assert_eq!(classify(&t.status, t.requests.len()), State::Waiting);
        assert!(!t.status.to_string().contains("private"));
        patch_shadow(
            &mut t,
            &json!([{"op":"remove","path":["requests",0]},{"op":"replace","path":["threadRuntimeStatus","activeFlags"],"value":["waitingOnUserInput"]}]),
        );
        assert_eq!(classify(&t.status, t.requests.len()), State::Waiting);
        patch_shadow(
            &mut t,
            &json!([{"op":"remove","path":["threadRuntimeStatus","activeFlags",0]},{"op":"replace","path":["threadRuntimeStatus","type"],"value":"idle"}]),
        );
        assert_eq!(classify(&t.status, 0), State::Idle);
    }
    #[test]
    fn stable_color_playback_never_rewinds_to_pale_frame_zero() {
        let s = sheet(120);
        assert_eq!(clip_range(&s), (24, 119, 8000));
        assert_eq!(frame_at(&s, 0, false), 24);
        assert_eq!(frame_at(&s, 1000, false), 36);
        assert_eq!(frame_at(&s, 8000, false), 119);
        assert_eq!(frame_at(&s, 999999, true), 24);
    }
    #[test]
    fn short_clips_keep_at_least_eighty_percent_of_frames() {
        for n in [1, 5, 12, 30, 60, 120] {
            let (first, last, _) = clip_range(&sheet(n));
            assert!(last >= first);
            assert!((n - first) * 5 >= n * 4);
        }
    }
    #[test]
    fn idle_schedule_has_no_catchup_burst() {
        let pack = Pack::read().unwrap();
        let mut schedule = IdleSchedule {
            next: 0,
            gesture: false,
        };
        let first = choose(&pack, State::Idle, None, "quiet", &mut schedule);
        assert_ne!(first, pack.base());
        assert_eq!(
            choose(&pack, State::Idle, Some(first), "quiet", &mut schedule),
            pack.base()
        );
        assert_eq!(
            choose(&pack, State::Idle, None, "quiet", &mut schedule),
            pack.base()
        );
        assert!(schedule.next > now_ms());
    }
    #[test]
    fn static_mode_still_permits_task_feedback() {
        let pack = Pack::read().unwrap();
        let mut schedule = IdleSchedule::new("still", 0);
        assert_eq!(
            choose(&pack, State::Idle, None, "still", &mut schedule),
            pack.base()
        );
        for state in [
            State::Working,
            State::Waiting,
            State::Complete,
            State::Error,
            State::Click,
            State::Drag,
        ] {
            assert_ne!(
                choose(&pack, state, None, "still", &mut schedule),
                pack.base()
            );
        }
    }
    #[test]
    fn stationary_press_never_moves_window() {
        let p = POINT { x: 960, y: 520 };
        let mut press = Press {
            cursor: p,
            origin: POINT { x: 700, y: 70 },
            last: POINT { x: 700, y: 70 },
            moved: false,
            allow_drag: true,
            edge_drag: false,
        };
        let area = RECT {
            left: 0,
            top: 0,
            right: 1536,
            bottom: 824,
        };
        for _ in 0..1000 {
            assert!(press.sample(p, area, 520, 620, 1.0).is_none());
        }
        assert!(press
            .sample(POINT { x: 963, y: 524 }, area, 520, 620, 1.0)
            .is_none());
        assert!(!press.moved);
    }
    #[test]
    fn repeated_drag_uses_absolute_native_coordinates() {
        let area = RECT {
            left: 0,
            top: 0,
            right: 1536,
            bottom: 824,
        };
        let mut press = Press {
            cursor: POINT { x: 960, y: 520 },
            origin: POINT { x: 700, y: 70 },
            last: POINT { x: 700, y: 70 },
            moved: false,
            allow_drag: true,
            edge_drag: false,
        };
        for i in 0..1000 {
            let x = if i % 2 == 0 { 980 } else { 990 };
            let (p, _) = press
                .sample(POINT { x, y: 530 }, area, 520, 620, 1.0)
                .unwrap();
            assert_eq!((p.x, p.y), (700 + x - 960, 80));
        }
    }
    #[test]
    fn native_bounds_stay_visible_across_negative_monitor_coordinates() {
        for area in [
            RECT {
                left: 0,
                top: 0,
                right: 1536,
                bottom: 824,
            },
            RECT {
                left: -1536,
                top: -864,
                right: 0,
                bottom: -40,
            },
        ] {
            let bottom = constrain(
                POINT {
                    x: 1000000,
                    y: 1000000,
                },
                area,
                520,
                620,
                1.0,
            );
            assert_eq!(bottom.y + 620, area.bottom);
            let top = constrain(
                POINT {
                    x: -1000000,
                    y: -1000000,
                },
                area,
                520,
                620,
                1.0,
            );
            assert!(top.y + 620 - 260 >= area.top);
        }
    }
    #[test]
    fn native_canvas_uses_premultiplied_alpha() {
        let mut c = Canvas::new(3, 3);
        c.blend(1, 1, [200, 100, 50, 128]);
        let p = c.pixels[4];
        assert_eq!((p >> 24) & 255, 128);
        assert_eq!((p >> 16) & 255, 100);
        assert_eq!((p >> 8) & 255, 50);
        assert_eq!(p & 255, 25);
        c.blend(1, 1, [0, 0, 0, 0]);
        assert_eq!(c.pixels[4], p);
    }
}

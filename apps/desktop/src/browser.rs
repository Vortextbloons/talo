//! Browser service has its own process containment and outlives Pi and the WebView.
use serde_json::{json, Value};
use std::{collections::HashMap, io::{BufRead, BufReader, Write}, path::Path, process::{Child, ChildStdin, Command, Stdio}, sync::{Arc, Mutex, mpsc::{self, Receiver, SyncSender}}, time::{Duration, Instant}};

pub struct Browser {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
    ui: Arc<Mutex<HashMap<String, SyncSender<Result<Value, String>>>>>,
    pub state: Arc<Mutex<Value>>,
    pending: HashMap<String, (Instant, u64)>,
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}
impl Browser {
    pub fn spawn(root: &Path, data: &Path) -> Result<Self, String> {
        let mut command = Command::new(root.join("runtime/node/node.exe"));
        command.arg(root.join("bundle/services/browser/src/main.js")).current_dir(root.join("bundle"))
            .env("TALO_DATA_DIR", data).env("TALO_VENDOR_DIR", root).env("TALO_HOST_EXE", std::env::current_exe().map_err(|e| e.to_string())?)
            .env_remove("NODE_OPTIONS").env_remove("NODE_PATH").env_remove("DEBUG")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let job = match crate::job::Job::new().and_then(|job| { job.assign(&child)?; Ok(job) }) {
            Ok(job) => Some(job), Err(e) => { crate::supervisor::kill_tree(&mut child); return Err(e.to_string()); }
        };
        let stdin = child.stdin.take().ok_or("Browser input unavailable")?;
        let stdout = child.stdout.take().ok_or("Browser output unavailable")?;
        let (tx, incoming) = mpsc::channel();
        let ui: Arc<Mutex<HashMap<String, SyncSender<Result<Value, String>>>>> = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(Mutex::new(json!({"state":"starting","owner":"agent","tabs":[],"busy":false})));
        let replies = ui.clone(); let cached = state.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let mut limited = std::io::Read::take(&mut reader, 256 * 1024 + 1);
                match limited.read_until(b'\n', &mut bytes) { Ok(0) | Err(_) => break, _ if bytes.len() > 256 * 1024 => break, _ => {} }
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
                if value["type"] == "browser_state" { if let Ok(mut state) = cached.lock() { *state = value["state"].clone(); } continue; }
                let id = value["id"].as_str().unwrap_or("");
                if let Some(sender) = replies.lock().ok().and_then(|mut p| p.remove(id)) {
                    let result = if value.get("error").is_some() { Err(value["error"]["message"].as_str().unwrap_or("Browser request failed").to_string()) } else { Ok(value["result"].clone()) };
                    let _ = sender.send(result);
                } else if tx.send(value).is_err() { break; }
            }
            if let Ok(mut state) = cached.lock() { *state = json!({"state":"crashed","owner":"paused","tabs":[],"busy":false,"error":"Browser service disconnected. Open Browser to recover."}); }
            if let Ok(mut pending) = replies.lock() { for (_, sender) in pending.drain() { let _ = sender.send(Err("Browser service stopped".into())); } }
        });
        Ok(Self { child, stdin, incoming, ui, state, pending: HashMap::new(), #[cfg(windows)] job })
    }
    pub fn write(&mut self, value: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{value}").and_then(|_| self.stdin.flush()).map_err(|_| "Browser connection unavailable".into())
    }
    pub fn request_ui(&mut self, action: &str) -> Result<Receiver<Result<Value,String>>, String> {
        let id = format!("ui-{}", uuid::Uuid::new_v4()); let (tx, rx) = mpsc::sync_channel(1);
        self.ui.lock().map_err(|_| "Browser state unavailable")?.insert(id.clone(), tx);
        if let Err(error) = self.write(&json!({"id":id,"method":"control","params":{"action":action}})) { self.ui.lock().unwrap().remove(&id); return Err(error); }
        Ok(rx)
    }
    pub fn send(&mut self, message: Value, epoch: u64) -> Result<(), String> {
        if message.to_string().len() > 60 * 1024 { return Err("Browser request exceeds 60 KiB. Narrow the request.".into()); }
        if let Some(id) = message["id"].as_str() { if self.pending.len() >= 8 { return Err("Browser request limit reached".into()); } self.pending.insert(id.into(), (Instant::now(), epoch)); }
        self.write(&message)
    }
    pub fn poll(&mut self, epoch: u64) -> Vec<Value> {
        let mut replies = Vec::new();
        while let Ok(value) = self.incoming.try_recv() {
            if let Some((_, owner)) = value["id"].as_str().and_then(|id| self.pending.remove(id)) { if owner == epoch { replies.push(value); } }
        }
        let failed = self.child.try_wait().ok().flatten().is_some();
        let expired: Vec<_> = self.pending.iter().filter(|(_, (at, owner))| failed || *owner != epoch || at.elapsed() > Duration::from_secs(50)).map(|(id, _)| id.clone()).collect();
        for id in expired {
            let (_, owner) = self.pending.remove(&id).unwrap(); let _ = self.write(&json!({"method":"cancel","params":{"requestId":id}}));
            if owner == epoch { replies.push(json!({"id":id,"error":{"message":"Browser stopped or timed out. Retry after checking its status."}})); }
        }
        replies
    }
    pub fn running(&mut self) -> bool { self.child.try_wait().ok().flatten().is_none() }
}
impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.write(&json!({"method":"shutdown"}));
        for _ in 0..30 { if self.child.try_wait().ok().flatten().is_some() { return; } std::thread::sleep(Duration::from_millis(100)); }
        #[cfg(windows)] { self.job.take(); }
        crate::supervisor::kill_tree(&mut self.child);
    }
}

/// Windows disables hardware input to all Chromium windows during agent ownership;
/// CDP remains usable. No overlay that page content could remove or bypass.
pub fn window_helper() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--browser-window") { return false; }
    #[cfg(windows)] {
        use windows_sys::Win32::{Foundation::{HWND, LPARAM}, UI::WindowsAndMessaging::*};
        #[link(name="user32")] unsafe extern "system" { fn EnableWindow(hwnd: HWND, enable: i32) -> i32; }
        struct Options { pid: u32, enable: bool, focus: bool, count: u32 }
        unsafe extern "system" fn each(hwnd: HWND, value: LPARAM) -> i32 {
            let options = &mut *(value as *mut Options); let mut pid = 0; GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == options.pid && IsWindowVisible(hwnd) != 0 {
                EnableWindow(hwnd, options.enable as i32); options.count += 1;
                if options.focus { ShowWindow(hwnd, SW_RESTORE); SetForegroundWindow(hwnd); }
            } 1
        }
        let mut options = Options { pid: args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0), enable: args.get(3).map(String::as_str) == Some("enable"), focus: args.get(4).map(String::as_str) == Some("focus"), count: 0 };
        if options.pid == 0 { std::process::exit(2); }
        unsafe { EnumWindows(Some(each), &mut options as *mut _ as isize); }
        std::process::exit(if options.count > 0 { 0 } else { 3 });
    }
    #[cfg(not(windows))] { true }
}

#[tauri::command]
pub async fn browser_control(app: tauri::AppHandle, action: String) -> Result<Value, String> {
    if !["open","watch","take_over","return","pause","stop"].contains(&action.as_str()) { return Err("Unknown browser control".into()); }
    crate::run_blocking(app, move |app| {
        use tauri::Manager;
        let receiver = {
            let shared = app.state::<crate::Shared>(); let mut host = shared.lock().map_err(|e| e.to_string())?;
            if ["open","take_over","pause","stop"].contains(&action.as_str()) {
                let pending = std::mem::take(&mut host.browser_approvals);
                for (id, (message, _, _)) in pending {
                    let crate::Host { db, permissions, .. } = &mut *host;
                    let _ = permissions.resolve(db, &id, false);
                    host.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser approval cancelled by user control."}}));
                }
            }
            if action == "stop" && !host.supervisor.browser_running() { return Ok(json!({"state":"stopped","owner":"paused","tabs":[],"busy":false})); }
            host.supervisor.ensure_browser()?.request_ui(&action)?
        };
        receiver.recv_timeout(Duration::from_secs(55)).map_err(|_| "Browser control timed out. Check the browser status.".to_string())?
    }).await
}

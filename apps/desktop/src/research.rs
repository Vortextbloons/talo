//! Isolated, lazy MCP worker. The supervisor assigns its entire process tree to its Job.
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub struct Research {
    #[cfg(windows)]
    job: Option<crate::job::Job>,
    pub child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
    pending: HashMap<String, Instant>,
    queued: VecDeque<Value>,
    initialized: bool,
    started: Instant,
    last_activity: Instant,
}

pub fn command(root: &Path, data: &Path) -> Result<Command, String> {
    let cache = data.join("research");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let model_dir = cache.join("rerank");
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;
    for file in ["model_quantized.onnx", "tokenizer.json"] {
        // Always seed from the installer; never let missing state trigger model downloads.
        std::fs::copy(
            root.join("bundle/research-models/rerank").join(file),
            model_dir.join(file),
        )
        .map_err(|e| format!("Packaged research model unavailable: {e}"))?;
    }
    let mut cmd = Command::new(root.join("bundle/node_modules/donsetch/binaries/donsetch.exe"));
    // Allowlist environment: never inherit provider keys, proxies, personal browser profiles,
    // DonSeTch user config, imported cookies or key stores.
    cmd.env_clear();
    for name in ["SystemRoot", "WINDIR", "SystemDrive", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd.arg("mcp")
        .current_dir(&cache)
        .env("HOME", &cache)
        .env("USERPROFILE", &cache)
        .env("APPDATA", &cache)
        .env("LOCALAPPDATA", &cache)
        .env("DONSETCH_CACHE_DIR", &cache)
        .env("DONSETCH_NO_CONFIG_FILE", "1")
        .env("DONSETCH_FETCH__ALLOW_PRIVATE_EGRESS", "false")
        .env("DONSETCH_FETCH__OCR", "false")
        .env("DONSETCH_FETCH__PDF_MAX_MB", "10")
        .env("DONSETCH_SEARCH__RERANK_TOPUP", "false")
        .env("DONSETCH_STATE__COOKIE_VAULT", "false")
        .env("DONSETCH_PROXY__FROM_ENVIRONMENT", "false")
        .env("DONSETCH_BROWSER__CLOAK_AUTO_DOWNLOAD", "false")
        .env(
            "DONSETCH_BROWSER__CHROMIUM_PATH",
            root.join("runtime/chromium/chrome-win64/chrome.exe"),
        )
        .env("DONSETCH_BROWSER__BACKEND", "headless")
        .env("DONSETCH_MCP__URL_HANDLES", "false")
        .env("DONSETCH_BYPASS__ENABLED", "false")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    Ok(cmd)
}

impl Research {
    pub fn new(mut child: Child) -> Result<Self, String> {
        #[cfg(windows)]
        let job = match crate::job::Job::new().and_then(|job| {
            job.assign(&child)?;
            Ok(job)
        }) {
            Ok(job) => Some(job),
            Err(error) => {
                super::supervisor::kill_tree(&mut child);
                return Err(format!("Unable to contain web process tree: {error}"));
            }
        };
        let stdin = child.stdin.take().ok_or("Web worker stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("Web worker stdout unavailable")?;
        let (tx, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            // Read with a hard frame limit before allocating an unbounded line.
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let mut limited = std::io::Read::take(&mut reader, 24 * 1024 * 1024 + 1);
                match limited.read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => break,
                    _ if bytes.len() > 24 * 1024 * 1024 => break,
                    _ => {
                        if let Ok(value) = serde_json::from_slice(&bytes) {
                            if tx.send(value).is_err() {
                                break;
                            }
                            crate::supervisor::wake_pump();
                        }
                    }
                }
            }
        });
        let mut worker = Self {
            #[cfg(windows)]
            job,
            child,
            stdin,
            incoming,
            pending: HashMap::new(),
            queued: VecDeque::new(),
            initialized: false,
            started: Instant::now(),
            last_activity: Instant::now(),
        };
        worker.write(&json!({"jsonrpc":"2.0","id":"talo-web-init","method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"Talo","version":"0.1.0"}}}))?;
        Ok(worker)
    }
    fn write(&mut self, value: &Value) -> Result<(), String> {
        writeln!(self.stdin, "{value}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| e.to_string())
    }
    pub fn send(&mut self, value: Value) -> Result<(), String> {
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "notifications/cancelled" {
            // Keep cancelled calls active until the server acknowledges or is reaped.
            let id = value
                .pointer("/params/requestId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let queued = self
                .queued
                .iter()
                .any(|v| v.get("id").and_then(Value::as_str) == Some(id));
            self.queued
                .retain(|v| v.get("id").and_then(Value::as_str) != Some(id));
            if queued {
                self.pending.remove(id);
                return Ok(());
            }
        } else if method != "tools/call" {
            return Err("Unsupported research operation".into());
        }
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            if self.pending.len() >= 8 {
                return Err("Too many active research requests".into());
            }
            self.pending.insert(id.into(), Instant::now());
        }
        self.last_activity = Instant::now();
        if self.initialized {
            self.write(&value)
        } else {
            self.queued.push_back(value);
            Ok(())
        }
    }
    pub fn poll(&mut self) -> (Vec<Value>, bool) {
        let mut replies = Vec::new();
        let mut failed = false;
        while let Ok(value) = self.incoming.try_recv() {
            if value.get("id").and_then(Value::as_str) == Some("talo-web-init") {
                if value.get("error").is_some() {
                    failed = true;
                    break;
                }
                self.initialized = true;
                if self
                    .write(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                    .is_err()
                {
                    failed = true;
                    break;
                }
                while let Some(request) = self.queued.pop_front() {
                    if self.write(&request).is_err() {
                        failed = true;
                        break;
                    }
                }
            } else if let Some(id) = value.get("id").and_then(Value::as_str) {
                if self.pending.remove(id).is_some() {
                    replies.push(value);
                    self.last_activity = Instant::now();
                }
            }
        }
        failed |= self.child.try_wait().ok().flatten().is_some();
        failed |= !self.initialized && self.started.elapsed() > Duration::from_secs(20);
        // A stuck/cancelled request is not idle. Fail it explicitly before recovery.
        failed |= self
            .pending
            .values()
            .any(|at| at.elapsed() > Duration::from_secs(70));
        let idle =
            self.pending.is_empty() && self.last_activity.elapsed() >= Duration::from_secs(120);
        if failed {
            for (id, _) in self.pending.drain() {
                replies.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"Web worker stopped or exceeded its deadline. Retry the request."}}));
            }
        }
        (replies, failed || idle)
    }
}

impl Drop for Research {
    fn drop(&mut self) {
        // This nested Job also kills renderers after their parent worker has crashed.
        #[cfg(windows)]
        {
            self.job.take();
        }
        super::supervisor::kill_tree(&mut self.child);
    }
}

fn open_external(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize <= 32 {
            return Err("Unable to open the source or artifact".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("Opening sources is supported on Windows".into())
    }
}

#[tauri::command]
pub async fn research_source_open(app: tauri::AppHandle, url: String) -> Result<(), String> {
    crate::run_blocking(app, move |_| {
        let parsed = tauri::Url::parse(&url).map_err(|_| "Invalid source URL")?;
        if !["https", "http"].contains(&parsed.scheme())
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Only HTTP(S) source URLs without credentials can be opened".into());
        }
        open_external(parsed.as_str())
    })
    .await
}

#[tauri::command]
pub async fn research_artifact_open(app: tauri::AppHandle, name: String, browser: Option<bool>) -> Result<(), String> {
    crate::run_blocking(app, move |app| {
        use tauri::Manager;
        let (id, extension) = name.rsplit_once('.').ok_or("Invalid artifact name")?;
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid artifact identifier")?;
        if !["json", "png"].contains(&extension) {
            return Err("Unsupported artifact format".into());
        }
        let root = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join(if browser.unwrap_or(false) { "artifacts/browser" } else { "artifacts/web" })
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let file = root.join(&name).canonicalize().map_err(|e| e.to_string())?;
        if file.parent() != Some(root.as_path()) {
            return Err("Artifact is outside the research directory".into());
        }
        open_external(&file.to_string_lossy())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }
    fn fixture() -> Research {
        let root = root();
        let runtime = crate::runtime::Runtime::require(&root.join("vendor")).unwrap();
        let mut cmd = runtime.command(&root.join("tests/fixtures/web-mcp.mjs"));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        Research::new(cmd.spawn().unwrap()).unwrap()
    }
    fn request(id: &str, tool: &str) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":{}}})
    }
    fn response(web: &mut Research, id: &str) -> Value {
        let until = Instant::now() + Duration::from_secs(65);
        loop {
            let (messages, stop) = web.poll();
            if let Some(value) = messages.into_iter().find(|v| v["id"] == id) {
                return value;
            }
            assert!(!stop, "worker stopped before replying");
            assert!(Instant::now() < until, "worker reply deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn research_initialization_idle_active_cancellation_and_crash() {
        let mut web = fixture();
        web.send(request("first", "ok")).unwrap();
        assert!(response(&mut web, "first")["result"]["content"].is_array());
        web.send(request("active", "hang")).unwrap();
        web.last_activity = Instant::now() - Duration::from_secs(121);
        assert!(
            !web.poll().1,
            "idle timeout must not stop an active operation"
        );
        web.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"active"}})).unwrap();
        assert_eq!(response(&mut web, "active")["result"]["isError"], true);
        web.last_activity = Instant::now() - Duration::from_secs(121);
        assert!(web.poll().1, "empty worker becomes idle after two minutes");
        drop(web);
        let mut restarted = fixture();
        restarted.send(request("crash", "crash")).unwrap();
        assert!(response(&mut restarted, "crash")["error"].is_object());
        drop(restarted);
        let mut recovered = fixture();
        recovered.send(request("recovered", "ok")).unwrap();
        assert!(response(&mut recovered, "recovered")["result"].is_object());
    }

    #[test]
    fn research_environment_is_isolated_and_models_are_local() {
        let data = std::env::temp_dir().join(format!("talo-web-env-{}", uuid::Uuid::new_v4()));
        let cmd = command(&root().join("vendor"), &data).unwrap();
        let env: HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_string(),
                    v.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();
        assert_eq!(
            env["DONSETCH_FETCH__ALLOW_PRIVATE_EGRESS"].as_deref(),
            Some("false")
        );
        assert_eq!(env["DONSETCH_FETCH__OCR"].as_deref(), Some("false"));
        assert_eq!(env["DONSETCH_NO_CONFIG_FILE"].as_deref(), Some("1"));
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(data.join("research/rerank/model_quantized.onnx").is_file());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn research_job_removes_renderers_even_after_worker_crash() {
        let mut web = fixture();
        web.send(request("spawn", "spawn-child")).unwrap();
        let pid = response(&mut web, "spawn")["result"]["pid"]
            .as_u64()
            .unwrap()
            .to_string();
        web.child.kill().unwrap();
        web.child.wait().unwrap();
        drop(web);
        let mut command = Command::new("tasklist.exe");
        use std::os::windows::process::CommandExt;
        command
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(0x08000000);
        let output = command.output().unwrap();
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")),
            "renderer must not survive worker cleanup"
        );
    }

    #[test]
    #[ignore = "live public network and bundled Chromium; run explicitly for release verification"]
    fn bundled_donsetch_live_four_tools_and_redirect_guard() {
        let data = std::env::temp_dir().join(format!("talo-web-live-{}", uuid::Uuid::new_v4()));
        let mut web = Research::new(
            command(&root().join("vendor"), &data)
                .unwrap()
                .spawn()
                .unwrap(),
        )
        .unwrap();
        for (id, tool, args) in [
            (
                "search",
                "web_search",
                json!({"query":"Rust official documentation","max_results":3,"deadline_ms":45000}),
            ),
            (
                "fetch",
                "web_fetch",
                json!({"url":"https://example.com/","max_chars":8000,"deadline_ms":45000}),
            ),
            (
                "crawl",
                "web_crawl",
                json!({"url":"https://example.com/","max_pages":2,"max_depth":1,"max_total_chars":8000,"deadline_s":45,"respect_robots":true}),
            ),
            (
                "screenshot",
                "web_screenshot",
                json!({"url":"https://example.com/","full_page":false}),
            ),
        ] {
            web.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":args}})).unwrap();
            let value = response(&mut web, id);
            assert!(value.get("error").is_none(), "{tool}: {value}");
            assert_ne!(value["result"]["isError"], true, "{tool}: {value}");
            if id == "screenshot" {
                assert!(value["result"]["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["type"] == "image"));
            } else {
                assert!(
                    value.to_string().contains("https://"),
                    "{tool} lacks citation URLs"
                );
            }
            println!("Live {tool} passed");
        }
        web.send(json!({"jsonrpc":"2.0","id":"redirect","method":"tools/call","params":{"name":"web_fetch","arguments":{"url":"https://httpbin.org/redirect-to?url=http://127.0.0.1/","deadline_ms":20000}}})).unwrap();
        let redirected = response(&mut web, "redirect");
        assert_eq!(
            redirected["result"]["code"], "guard.ssrf",
            "private redirect must be blocked by the egress guard: {redirected}"
        );
        drop(web);
        let _ = std::fs::remove_dir_all(data);
    }
}

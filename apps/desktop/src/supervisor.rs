//! Lifecycle and protocol bridge for the bundled Node/Pi worker.
//!
//! The worker is started lazily, speaks newline-delimited JSON on stdin/stdout, and is
//! contained by a Windows Job Object. Requests are correlated by id on a reader thread, so a
//! command can wait for a response without also having to run the event pump. Chat events are
//! queued for the host pump, which persists them and emits Tauri events under one lock.
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Idle sleep is disabled while a generation is running. Otherwise the worker is stopped
/// after roughly two minutes without app activity.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const READY_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug)]
pub enum Error {
    NotRunning,
    Timeout,
    Worker(String),
    Io(String),
}

impl Error {
    pub fn is_timeout(&self) -> bool {
        matches!(self, Error::Timeout)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotRunning => write!(f, "The Pi worker is not running"),
            Error::Timeout => write!(f, "The Pi worker did not respond in time"),
            Error::Worker(message) => write!(f, "{message}"),
            Error::Io(message) => write!(f, "Pi worker connection failed: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
pub enum WorkerSignal {
    Ready,
    Sleeping,
    Error(String),
    Chat(Value),
    /// The worker's stdout closed. The pump calls `refresh` to reap the process.
    Eof,
}

#[derive(Debug)]
pub enum Lifecycle {
    Started,
    Exited {
        epoch: u64,
        success: bool,
        error: Option<String>,
    },
}

enum Reply {
    Result(Value),
    Error(String),
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    ready: Arc<AtomicBool>,
    signals: Arc<Mutex<VecDeque<WorkerSignal>>>,
    pending: Arc<Mutex<HashMap<String, SyncSender<Reply>>>>,
    last_activity: Instant,
    eof_at: Option<Instant>,
}

struct ServiceChild {
    child: Child,
    stdin: Option<ChildStdin>,
}

pub struct Supervisor {
    pub root: PathBuf,
    pub data: PathBuf,
    pi: Option<Worker>,
    services: HashMap<String, ServiceChild>,
    failures: HashMap<String, String>,
    /// Runtime files cannot appear while the host runs, so probe them once.
    dependencies: Option<Value>,
    /// Signals preserved when a worker exits before the pump drains its queue.
    deferred: VecDeque<WorkerSignal>,
    lifecycle: VecDeque<Lifecycle>,
    /// Incremented for every spawned worker so chat session bindings cannot outlive it.
    worker_epoch: u64,
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}

impl Supervisor {
    pub fn new(root: PathBuf, data: PathBuf) -> Self {
        Self {
            root,
            data,
            pi: None,
            services: HashMap::new(),
            failures: HashMap::new(),
            dependencies: None,
            deferred: VecDeque::new(),
            lifecycle: VecDeque::new(),
            worker_epoch: 0,
            #[cfg(windows)]
            job: crate::job::Job::new().ok(),
        }
    }

    /// Reap exited services. A Pi exit is reported through `drain_lifecycle`; the pump uses
    /// that to fail any in-flight generation and clear stale session bindings.
    pub fn refresh(&mut self) {
        let epoch = self.worker_epoch;
        if let Some(worker) = self.pi.as_mut() {
            match worker.child.try_wait() {
                Ok(Some(status)) => {
                    let success = status.success();
                    let error = if success {
                        None
                    } else {
                        Some(format!("Pi worker exited with {status}"))
                    };
                    if let Ok(mut signals) = worker.signals.lock() {
                        self.deferred.extend(signals.drain(..));
                    }
                    fail_pending(&worker.pending, "Pi worker stopped before replying");
                    worker.ready.store(false, Ordering::SeqCst);
                    self.pi = None;
                    if success {
                        self.failures.remove("pi");
                    } else {
                        self.failures.insert("pi".into(), status.to_string());
                    }
                    self.lifecycle.push_back(Lifecycle::Exited {
                        epoch,
                        success,
                        error,
                    });
                }
                Ok(None) => {
                    // A closed stdout pipe without a timely process exit means the worker is
                    // wedged. Terminate it so crash recovery can restart from SQLite.
                    if worker
                        .eof_at
                        .is_some_and(|at| at.elapsed() > Duration::from_secs(3))
                    {
                        let _ = worker.child.kill();
                    }
                }
                Err(error) => {
                    self.failures.insert("pi".into(), error.to_string());
                }
            }
        }
        self.services
            .retain(|id, service| match service.child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        self.failures.remove(id);
                    } else {
                        self.failures.insert(id.clone(), status.to_string());
                    }
                    false
                }
                _ => true,
            });
    }

    pub fn status(&mut self) -> Value {
        self.refresh();
        let state = |id: &str| {
            let running = if id == "pi" {
                self.pi.is_some()
            } else {
                self.services.contains_key(id)
            };
            if running {
                "running"
            } else if self.failures.contains_key(id) {
                "failed"
            } else {
                "stopped"
            }
        };
        let services = json!({"pi":state("pi"),"browser":state("browser"),"web":state("web")});
        let root = &self.root;
        let dependencies = self.dependencies.get_or_insert_with(|| {
            json!({
            "node":root.join("runtime/node/node.exe").is_file(),
            "chromium":root.join("runtime/chromium/chrome-win64/chrome.exe").is_file(),
            "donsetch":root.join("bundle/node_modules/donsetch/binaries/donsetch.exe").is_file(),
            "pi":root.join("bundle/node_modules/@earendil-works/pi-coding-agent/package.json").is_file(),
            "stagehand":root.join("bundle/node_modules/@browserbasehq/stagehand/package.json").is_file(),
            "mcpAdapter":root.join("bundle/node_modules/pi-mcp-adapter/package.json").is_file()
            })
        });
        json!({"database":"ready", "services":services, "dependencies":dependencies.clone()})
    }

    pub fn start(&mut self, id: &str) -> Result<(), String> {
        self.refresh();
        match id {
            "pi" => {
                if self.pi.is_some() {
                    return Ok(());
                }
                self.spawn_pi()
            }
            "browser" => {
                if self.services.contains_key(id) {
                    return Ok(());
                }
                self.spawn_service(id)
            }
            _ => Err("Unknown managed service".into()),
        }
    }

    /// Start the Pi worker if needed and wait until it announces readiness. This is the lazy
    /// entry point used by chat and provider commands; startup never happens at app launch.
    pub fn ensure_worker(&mut self) -> Result<(), String> {
        self.refresh();
        if self.pi.is_none() {
            self.spawn_pi()?;
        }
        self.wait_ready(READY_TIMEOUT)
    }

    pub fn worker_epoch(&self) -> u64 {
        self.worker_epoch
    }

    pub fn worker_ready(&self) -> bool {
        self.pi
            .as_ref()
            .is_some_and(|worker| worker.ready.load(Ordering::SeqCst))
    }

    fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.pi.is_none() {
                return Err("Pi worker stopped while starting".into());
            }
            if self.worker_ready() {
                return Ok(());
            }
            self.refresh();
            if self.pi.is_none() {
                let detail = self
                    .failures
                    .get("pi")
                    .cloned()
                    .unwrap_or_else(|| "Pi worker stopped while starting".into());
                return Err(detail);
            }
            if Instant::now() >= deadline {
                let _ = self.stop_pi();
                return Err("Pi worker did not become ready in time".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn spawn_pi(&mut self) -> Result<(), String> {
        let node = self.root.join("runtime/node/node.exe");
        if !node.is_file() {
            return Err("Bundled Node runtime is missing; run npm run vendor".into());
        }
        let script = self.root.join("bundle/workers/pi/src/main.js");
        if !script.is_file() {
            return Err("Bundled Pi worker is missing; run npm run prepare:bundle".into());
        }
        let logs = self.data.join("logs");
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join("pi.jsonl"))
            .map_err(|error| error.to_string())?;
        let err = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join("pi.stderr.log"))
            .map_err(|error| error.to_string())?;
        let mut command = Command::new(&node);
        command
            .arg(&script)
            .current_dir(self.root.join("bundle"))
            .env("TALO_DATA_DIR", &self.data)
            .env("TALO_VENDOR_DIR", &self.root)
            .env("TALO_HOST_MODE", "1")
            .env("TALO_IDLE_MS", IDLE_TIMEOUT.as_millis().to_string())
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(err));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        #[cfg(windows)]
        if self.job.is_none() {
            return Err("Unable to create worker process containment job".into());
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        #[cfg(windows)]
        if let Err(error) = self.job.as_ref().unwrap().assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Pi worker stdout is unavailable".to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Pi worker stdin is unavailable".to_string())?;
        let ready = Arc::new(AtomicBool::new(false));
        let signals = Arc::new(Mutex::new(VecDeque::new()));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        self.deferred.clear();
        self.failures.remove("pi");
        self.worker_epoch = self.worker_epoch.wrapping_add(1);
        let reader_ready = Arc::clone(&ready);
        let reader_signals = Arc::clone(&signals);
        let reader_pending = Arc::clone(&pending);
        std::thread::spawn(move || {
            read_worker(stdout, log, reader_ready, reader_signals, reader_pending);
        });
        self.pi = Some(Worker {
            child,
            stdin,
            ready,
            signals,
            pending,
            last_activity: Instant::now(),
            eof_at: None,
        });
        self.lifecycle.push_back(Lifecycle::Started);
        Ok(())
    }

    fn spawn_service(&mut self, id: &str) -> Result<(), String> {
        let script = match id {
            "browser" => "services/browser/src/main.js",
            _ => return Err("Unknown managed service".into()),
        };
        let node = self.root.join("runtime/node/node.exe");
        if !node.is_file() {
            return Err("Bundled Node runtime is missing; run npm run vendor".into());
        }
        let logs = self.data.join("logs");
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join(format!("{id}.jsonl")))
            .map_err(|error| error.to_string())?;
        let err = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join(format!("{id}.stderr.log")))
            .map_err(|error| error.to_string())?;
        let mut command = Command::new(&node);
        command
            .arg(self.root.join("bundle").join(script))
            .current_dir(self.root.join("bundle"))
            .env("TALO_DATA_DIR", &self.data)
            .env("TALO_VENDOR_DIR", &self.root)
            .env("TALO_HOST_MODE", "1")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        #[cfg(windows)]
        if self.job.is_none() {
            return Err("Unable to create worker process containment job".into());
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        #[cfg(windows)]
        if let Err(error) = self.job.as_ref().unwrap().assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        let stdin = child.stdin.take();
        self.failures.remove(id);
        self.services
            .insert(id.into(), ServiceChild { child, stdin });
        Ok(())
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        if id == "pi" {
            return self.stop_pi();
        }
        if let Some(mut service) = self.services.remove(id) {
            if let Some(input) = service.stdin.as_mut() {
                let _ = writeln!(input, "{{\"id\":\"host-stop\",\"method\":\"shutdown\"}}");
                let _ = input.flush();
            }
            for _ in 0..30 {
                if service
                    .child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_some()
                {
                    self.failures.remove(id);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            kill_tree(&mut service.child);
        }
        self.failures.remove(id);
        Ok(())
    }

    /// Intentional Pi shutdown. It does not create a failure lifecycle event.
    pub fn stop_pi(&mut self) -> Result<(), String> {
        let Some(mut worker) = self.pi.take() else {
            return Ok(());
        };
        worker.ready.store(false, Ordering::SeqCst);
        fail_pending(&worker.pending, "Pi worker was stopped before replying");
        let _ = writeln!(
            worker.stdin,
            "{{\"id\":\"host-stop\",\"method\":\"shutdown\"}}"
        );
        let _ = worker.stdin.flush();
        for _ in 0..30 {
            if worker
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                self.failures.remove("pi");
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill_tree(&mut worker.child);
        self.failures.remove("pi");
        Ok(())
    }

    pub fn stop_all(&mut self) {
        let _ = self.stop_pi();
        for id in self.services.keys().cloned().collect::<Vec<_>>() {
            let _ = self.stop(&id);
        }
    }

    /// Stop an idle worker. The caller passes `busy = true` while any generation is live;
    /// idle sleep must never interrupt an in-flight response.
    pub fn idle_stop(&mut self, busy: bool) -> bool {
        if busy {
            return false;
        }
        let idle = self.pi.as_ref().is_some_and(|worker| {
            worker.ready.load(Ordering::SeqCst) && worker.last_activity.elapsed() >= IDLE_TIMEOUT
        });
        if idle {
            let _ = self.stop_pi();
            true
        } else {
            false
        }
    }

    pub fn drain_signals(&mut self) -> Vec<WorkerSignal> {
        let mut drained = VecDeque::new();
        if let Some(worker) = self.pi.as_mut() {
            if let Ok(mut signals) = worker.signals.lock() {
                while let Some(signal) = signals.pop_front() {
                    if matches!(signal, WorkerSignal::Eof) {
                        worker.eof_at = Some(Instant::now());
                    }
                    drained.push_back(signal);
                }
            }
            if !drained.is_empty() {
                worker.last_activity = Instant::now();
            }
        }
        while let Some(signal) = self.deferred.pop_front() {
            drained.push_back(signal);
        }
        drained.into()
    }

    pub fn drain_lifecycle(&mut self) -> Vec<Lifecycle> {
        self.lifecycle.drain(..).collect()
    }

    /// Send a correlated request to the worker. Params are wiped after serialization because
    /// `apiKey` is allowed in the private worker contract and must be ephemeral.
    pub fn request(
        &mut self,
        method: &str,
        mut params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let worker = self.pi.as_mut().ok_or(Error::NotRunning)?;
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let pending = Arc::clone(&worker.pending);
            let mut pending = pending
                .lock()
                .map_err(|_| Error::Io("Request table is unavailable".into()))?;
            pending.insert(id.clone(), sender);
        }
        let mut line = json!({"id": id, "method": method, "params": &params}).to_string();
        line.push('\n');
        let write = worker
            .stdin
            .write_all(line.as_bytes())
            .and_then(|_| worker.stdin.flush());
        wipe_line(&mut line);
        wipe_param_secrets(&mut params);
        if let Err(error) = write {
            if let Ok(mut pending) = worker.pending.lock() {
                pending.remove(&id);
            }
            return Err(Error::Io(error.to_string()));
        }
        worker.last_activity = Instant::now();
        match receiver.recv_timeout(timeout) {
            Ok(Reply::Result(value)) => Ok(value),
            Ok(Reply::Error(message)) => Err(Error::Worker(message)),
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(mut pending) = worker.pending.lock() {
                    pending.remove(&id);
                }
                Err(Error::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(Error::Worker("Pi worker stopped before replying".into()))
            }
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn fail_pending(pending: &Mutex<HashMap<String, SyncSender<Reply>>>, message: &str) {
    if let Ok(mut pending) = pending.lock() {
        for (_, sender) in pending.drain() {
            let _ = sender.send(Reply::Error(message.to_string()));
        }
    }
}

fn read_worker(
    stdout: std::process::ChildStdout,
    mut log: fs::File,
    ready: Arc<AtomicBool>,
    signals: Arc<Mutex<VecDeque<WorkerSignal>>>,
    pending: Arc<Mutex<HashMap<String, SyncSender<Reply>>>>,
) {
    let push = |signal: WorkerSignal| {
        if let Ok(mut signals) = signals.lock() {
            signals.push_back(signal);
        }
    };
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.len() > 4 * 1024 * 1024 {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let _ = writeln!(log, "{}", describe(&value));
        let _ = log.flush();
        match value.get("type").and_then(Value::as_str) {
            Some("response") => {
                let Some(id) = value.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let reply = match value.get("error").and_then(Value::as_str) {
                    Some(error) => Reply::Error(error.to_string()),
                    None => Reply::Result(value.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Ok(mut pending) = pending.lock() {
                    if let Some(sender) = pending.remove(id) {
                        let _ = sender.send(reply);
                    }
                }
            }
            Some("ready") => {
                ready.store(true, Ordering::SeqCst);
                push(WorkerSignal::Ready);
            }
            Some("sleeping") => {
                ready.store(false, Ordering::SeqCst);
                push(WorkerSignal::Sleeping);
            }
            Some("error") => {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Pi worker reported an error")
                    .to_string();
                push(WorkerSignal::Error(message));
            }
            Some("chat") => {
                if let Some(event) = value.get("event") {
                    if !event.is_null() {
                        push(WorkerSignal::Chat(event.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    ready.store(false, Ordering::SeqCst);
    fail_pending(&pending, "Pi worker connection closed before replying");
    push(WorkerSignal::Eof);
}

/// Log only protocol shape/state, never assistant text, prompts or credentials.
fn describe(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("response") => json!({
            "type":"response",
            "id":value.get("id"),
            "ok":value.get("error").is_none()
        }),
        Some("chat") => {
            let event = value.get("event").unwrap_or(&Value::Null);
            json!({
                "type":"chat",
                "event":event.get("type"),
                "state":event.get("state"),
                "generationId":event.get("generationId")
            })
        }
        Some(kind) => json!({"type":kind}),
        None => json!({"type":"unknown"}),
    }
}

fn wipe_line(line: &mut String) {
    unsafe {
        line.as_bytes_mut().fill(0);
    }
    line.clear();
}

fn wipe_param_secrets(params: &mut Value) {
    if let Value::Object(map) = params {
        for key in ["apiKey", "api_key", "token", "authorization"] {
            if let Some(Value::String(secret)) = map.get_mut(key) {
                unsafe {
                    secret.as_bytes_mut().fill(0);
                }
                secret.clear();
            }
        }
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_log_description_omits_content_and_credentials() {
        let value = json!({"type":"chat","event":{"type":"generation","state":"generating","text":"private words","generationId":"g1"}});
        let described = describe(&value).to_string();
        assert!(described.contains("generating"));
        assert!(described.contains("g1"));
        assert!(!described.contains("private words"));
        let response =
            json!({"type":"response","id":"1","error":"Authorization: Bearer sk-secret"});
        let described = describe(&response).to_string();
        assert!(!described.contains("sk-secret"));
        assert!(described.contains("\"ok\":false"));
    }

    #[test]
    fn private_params_are_wiped_after_serialization() {
        let mut params = json!({"provider":"openrouter","apiKey":"sk-secret-value"});
        let line = params.to_string();
        assert!(line.contains("sk-secret-value"));
        wipe_param_secrets(&mut params);
        assert_eq!(params.get("apiKey").and_then(Value::as_str), Some(""));
    }

    #[test]
    fn idle_stop_never_interrupts_generating_work() {
        let mut supervisor = Supervisor::new(PathBuf::new(), PathBuf::new());
        assert!(!supervisor.idle_stop(true));
    }
}

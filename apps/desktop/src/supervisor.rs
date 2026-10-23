//! Lifecycle and protocol bridge for the bundled Pi worker.
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
        Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

/// Idle sleep is disabled while a generation is running. Otherwise the worker is stopped
/// after roughly two minutes without app activity.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const READY_TIMEOUT: Duration = Duration::from_secs(25);
/// Matches the worker's own write budget. A frame over this is rejected, not silently dropped.
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

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

/// Event-driven pump wake. Reader threads notify after enqueueing output the pump consumes,
/// and the pump waits with a fallback tick instead of spinning on a fixed timer. A queued
/// flag makes the wake edge-triggered: a notify that lands while the pump is already
/// draining is consumed on the next wait, so nothing can be lost.
struct Wake {
    flag: Mutex<bool>,
    cond: Condvar,
}

impl Wake {
    fn new() -> Self {
        Self { flag: Mutex::new(false), cond: Condvar::new() }
    }
    fn notify(&self) {
        if let Ok(mut flag) = self.flag.lock() {
            *flag = true;
        }
        self.cond.notify_one();
    }
    fn wait(&self, fallback: Duration) {
        let Ok(mut flag) = self.flag.lock() else {
            std::thread::sleep(fallback);
            return;
        };
        if *flag {
            *flag = false;
            return;
        }
        if let Ok((mut flag, _)) = self.cond.wait_timeout(flag, fallback) {
            *flag = false;
        }
    }
}

/// Exactly one pump exists per process, so a process-wide wake is its honest shape.
static PUMP_WAKE: OnceLock<Wake> = OnceLock::new();

/// Reader threads call this after enqueueing anything the pump consumes.
pub(crate) fn wake_pump() {
    if let Some(wake) = PUMP_WAKE.get() {
        wake.notify();
    }
}

/// Block the pump until output arrives or the fallback tick expires, whichever is first.
pub(crate) fn pump_wait(fallback: Duration) {
    PUMP_WAKE.get_or_init(Wake::new).wait(fallback);
}

#[derive(Debug)]
pub enum WorkerSignal {
    Tools(Value),
    Agent(Value),
    Delegation(Value),
    Web(Value),
    Browser(Value),
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
    #[cfg(windows)]
    _job: crate::job::Job,
    child: Child,
    stdin: ChildStdin,
    ready: Arc<AtomicBool>,
    signals: Arc<Mutex<VecDeque<WorkerSignal>>>,
    pending: Arc<Mutex<HashMap<String, SyncSender<Reply>>>>,
    last_activity: Instant,
    eof_at: Option<Instant>,
}

pub struct Supervisor {
    browser: Option<crate::browser::Browser>,
    research: Option<crate::research::Research>,
    pub root: PathBuf,
    pub data: PathBuf,
    pi: Option<Worker>,
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
    pub fn pi_pid(&self) -> Option<u32> { self.pi.as_ref().map(|worker|worker.child.id()) }
    pub fn new(root: PathBuf, data: PathBuf) -> Self {
        Self {
            browser: None,
            research: None,
            root,
            data,
            pi: None,
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

    }

    pub fn status(&mut self) -> Value {
        self.refresh();
        let state = |id: &str| {
            let running = if id == "pi" {
                self.pi.is_some()
            } else {
                self.browser.as_ref().is_some_and(|b| b.state.lock().ok().is_some_and(|s| s["state"] != "crashed" && s["state"] != "stopped"))
            };
            if id == "browser" && self.browser.as_ref().is_some_and(|b| b.state.lock().ok().is_some_and(|s| s["state"] == "crashed")) { "failed" } else if running {
                "running"
            } else if self.failures.contains_key(id) {
                "failed"
            } else {
                "stopped"
            }
        };
        let services = json!({"pi":state("pi"),"browser":state("browser"),"web":if self.research.is_some() { "running" } else { "stopped" }});
        let root = &self.root;
        let dependencies = self.dependencies.get_or_insert_with(|| {
            json!({
            "runtime":crate::runtime::Runtime::locate(root).is_some(),
            "chromium":root.join("runtime/chromium/chrome-win64/chrome.exe").is_file(),
            "donsetch":root.join("bundle/node_modules/donsetch/binaries/donsetch.exe").is_file(),
            "pi":root.join("bundle/node_modules/@earendil-works/pi-coding-agent/package.json").is_file(),
            "stagehand":root.join("bundle/node_modules/@browserbasehq/stagehand/package.json").is_file(),
            "mcpAdapter":root.join("bundle/node_modules/pi-mcp-adapter/package.json").is_file()
            })
        });
        json!({"database":"ready", "services":services, "dependencies":dependencies.clone(),"browserSessions":self.browser.as_ref().and_then(|b| b.sessions.lock().ok().map(|s|s.clone())).unwrap_or(json!({})),"browser":self.browser.as_ref().and_then(|b| b.state.lock().ok().map(|s| s.clone())).unwrap_or(json!({"state":"stopped","owner":"paused","tabs":[],"busy":false}))})
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
            "browser" => { self.ensure_browser()?.request_ui("open")?; Ok(()) }
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
        let runtime = crate::runtime::Runtime::require(&self.root)?;
        let script = self.root.join("bundle/workers/pi/src/main.js");
        if !script.is_file() {
            return Err("Bundled Pi worker is missing; run bun run prepare:bundle".into());
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
        let mut command = runtime.command(&script);
        command
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
        #[cfg(windows)]
        let worker_job = crate::job::Job::new().map_err(|_| "Unable to contain Pi process tree")?;
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        #[cfg(windows)]
        if let Err(error) = self.job.as_ref().unwrap().assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        #[cfg(windows)]
        if let Err(error) = worker_job.assign(&child) {
            let _ = child.kill();let _ = child.wait();return Err(error.to_string());
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
            #[cfg(windows)]
            _job: worker_job,
            child,
            stdin,
            ready,
            signals,
            pending,
            last_activity: Instant::now(),
            eof_at: None,
        });
        let config = crate::integrations::load(&self.data)?;
        self.request("tools.configure", json!({"servers":config["servers"],"disabled":config["disabled"]}), Duration::from_secs(15)).map_err(|e|e.to_string())?;
        self.lifecycle.push_back(Lifecycle::Started);
        Ok(())
    }

    pub fn browser_running(&mut self) -> bool { self.browser.as_mut().is_some_and(|b| b.running()) }
    pub fn take_browser_frame(&self) -> Option<Value> { self.browser.as_ref().and_then(|browser| browser.take_frame()) }
    pub fn ensure_browser(&mut self) -> Result<&mut crate::browser::Browser, String> {
        if !self.browser_running() || self.browser.as_ref().is_some_and(|b| b.state.lock().ok().is_some_and(|s| s["state"] == "crashed")) { self.browser = None; self.browser = Some(crate::browser::Browser::spawn(&self.root, &self.data)?); }
        Ok(self.browser.as_mut().unwrap())
    }
    pub fn relay_browser(&mut self, message: Value) {
        if let Some(pi) = self.pi.as_mut() { let _ = writeln!(pi.stdin, "{}", json!({"type":"browser_response","message":message})); let _ = pi.stdin.flush(); }
    }
    pub fn relay_tools(&mut self, message: Value) {
        if let Some(pi) = self.pi.as_mut() { let _ = writeln!(pi.stdin, "{}", json!({"type":"tools_response","message":message})); let _ = pi.stdin.flush(); }
    }
    pub fn relay_agent(&mut self, message: Value) {
        if let Some(pi) = self.pi.as_mut() { let _ = writeln!(pi.stdin, "{}", json!({"type":"agent_response","message":message})); let _ = pi.stdin.flush(); }
    }
    pub fn relay_delegation(&mut self, message: Value) {
        if let Some(pi) = self.pi.as_mut() { let _ = writeln!(pi.stdin, "{}", json!({"type":"delegation_response","message":message})); let _ = pi.stdin.flush(); }
    }
    pub fn browser_request(&mut self, message: Value) {
        if message["method"] == "cancel" && self.browser.is_none() { return; }
        let id = message["id"].clone(); let epoch = self.worker_epoch;
        let result = self.ensure_browser().and_then(|browser| browser.send(message, epoch));
        if let Err(error) = result { self.relay_browser(json!({"id":id,"error":{"message":error}})); }
    }

    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        if id == "browser" { self.browser = None; return Ok(()); }
        if id == "pi" {
            return self.stop_pi();
        }
        Err("Unknown managed service".into())
    }

    /// Intentional Pi shutdown. It does not create a failure lifecycle event.
    pub fn stop_pi(&mut self) -> Result<(), String> {
        let Some(mut worker) = self.pi.take() else {
            return Ok(());
        };
        // Revoke outstanding browser requests and approvals owned by the stopped worker.
        self.worker_epoch += 1;
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
        self.browser = None;
        self.research = None;
        let _ = self.stop_pi();
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

    fn relay_web(&mut self, message: Value) {
        if let Some(pi) = self.pi.as_mut() {
            let _ = writeln!(pi.stdin, "{}", json!({"type":"web_response","message":message}));
            let _ = pi.stdin.flush();
        }
    }

    fn web_request(&mut self, message: Value) {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let result = (|| -> Result<(), String> {
            if self.research.is_none() {
                if message.get("method").and_then(Value::as_str) != Some("tools/call") { return Ok(()); }
                let mut command = crate::research::command(&self.root, &self.data)?;
                #[cfg(windows)]
                if self.job.is_none() { return Err("Unable to contain research worker".into()); }
                let mut child = command.spawn().map_err(|e| e.to_string())?;
                #[cfg(windows)]
                if let Err(error) = self.job.as_ref().unwrap().assign(&child) {
                    kill_tree(&mut child);
                    return Err(error.to_string());
                }
                self.research = Some(crate::research::Research::new(child)?);
            }
            self.research.as_mut().unwrap().send(message)
        })();
        if let Err(error) = result { self.relay_web(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}})); }
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
        let mut visible = Vec::new();
        for signal in drained {
            if let WorkerSignal::Web(message) = signal { self.web_request(message); }
            else { visible.push(signal); }
        }
        if let Some(web) = self.research.as_mut() {
            let (replies, stop) = web.poll();
            if stop { self.research = None; }
            for reply in replies { self.relay_web(reply); }
        }
        if let Some(browser) = self.browser.as_mut() {
            let replies = browser.poll(self.worker_epoch);
            for reply in replies { self.relay_browser(reply); }
        }
        visible
    }

    pub fn drain_lifecycle(&mut self) -> Vec<Lifecycle> {
        self.lifecycle.drain(..).collect()
    }

    /// Send a correlated request to the worker. Params are wiped after serialization because
    /// `apiKey` is allowed in the private worker contract and must be ephemeral.
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        self.dispatch(method, params)?.wait(timeout)
    }

    /// Send a request without waiting for its reply. The caller can drop the host mutex, wait
    /// on the returned handle, and reacquire it to apply the result, so one chat's session
    /// startup never stalls the event pump or another chat's cancellation.
    pub fn dispatch(&mut self, method: &str, mut params: Value) -> Result<PendingReply, Error> {
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
        Ok(PendingReply {
            id,
            receiver,
            pending: Arc::clone(&worker.pending),
        })
    }
}

/// A dispatched request whose reply has not arrived yet. It borrows nothing from the
/// supervisor, so waiting for it can happen outside the host mutex.
pub struct PendingReply {
    id: String,
    receiver: mpsc::Receiver<Reply>,
    pending: Arc<Mutex<HashMap<String, SyncSender<Reply>>>>,
}

impl PendingReply {
    pub fn wait(self, timeout: Duration) -> Result<Value, Error> {
        match self.receiver.recv_timeout(timeout) {
            Ok(Reply::Result(value)) => Ok(value),
            Ok(Reply::Error(message)) => Err(Error::Worker(message)),
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&self.id);
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
        wake_pump();
    };
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.len() > MAX_FRAME_BYTES {
            // Rejecting the frame fails the waiting request instead of letting it time out.
            let _ = writeln!(log, "{}", json!({"type":"oversized_frame","bytes":line.len()}));
            let _ = log.flush();
            fail_pending(
                &pending,
                "Pi worker sent a message larger than the 4 MiB protocol limit",
            );
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let _ = writeln!(log, "{}", describe(&value));
        let _ = log.flush();
        match value.get("type").and_then(Value::as_str) {
            Some("tools") => {
                if let Some(message) = value.get("message") { push(WorkerSignal::Tools(message.clone())); }
            }
            Some("agent") => {
                if let Some(message) = value.get("message") { push(WorkerSignal::Agent(message.clone())); }
            }
            Some("delegation") => {
                if let Some(message) = value.get("message") { push(WorkerSignal::Delegation(message.clone())); }
            }
            Some("browser") => {
                if let Some(message) = value.get("message") { push(WorkerSignal::Browser(message.clone())); }
            }
            Some("web") => {
                if let Some(message) = value.get("message") { push(WorkerSignal::Web(message.clone())); }
            }
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
        for key in ["apiKey", "api_key", "token", "authorization", "credential", "access", "refresh", "idToken"] {
            if let Some(Value::String(secret)) = map.get_mut(key) {
                unsafe {
                    secret.as_bytes_mut().fill(0);
                }
                secret.clear();
            }
        }
        for value in map.values_mut(){wipe_param_secrets(value);}
    }else if let Value::Array(items)=params{
        for item in items{wipe_param_secrets(item);}
    }
}

pub(crate) fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x08000000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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

    #[cfg(windows)]
    #[test]
    fn pi_shutdown_and_crash_remove_mcp_descendants() {
        use windows_sys::Win32::System::Threading::{OpenProcess,GetExitCodeProcess,PROCESS_QUERY_LIMITED_INFORMATION};
        let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor");
        let fixture=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/integration-mcp.mjs");
        let data=std::env::temp_dir().join(format!("talo-mcp-job-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&data).unwrap();
        for crash in [false,true]{
            let pid_file=data.join(if crash{"crash.pid"}else{"stop.pid"});
            let mut supervisor=Supervisor::new(root.clone(),data.clone());
            supervisor.ensure_worker().unwrap();
            let runtime=crate::runtime::Runtime::require(&root).unwrap();
            supervisor.request("tools.configure",json!({"servers":[{"id":"fixture","transport":"stdio","command":runtime.path().to_string_lossy(),"args":[fixture.to_string_lossy(),"--spawn-child",pid_file.to_string_lossy()],"enabled":true,"trusted":true}]}),Duration::from_secs(10)).unwrap();
            supervisor.request("tools.test",json!({"server":"fixture"}),Duration::from_secs(10)).unwrap();
            let until=Instant::now()+Duration::from_secs(15);
            while !pid_file.exists(){assert!(Instant::now()<until,"MCP child did not start");std::thread::sleep(Duration::from_millis(30));}
            let pid=fs::read_to_string(&pid_file).unwrap().parse::<u32>().unwrap();
            let process=unsafe{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid)};
            assert!(!process.is_null());
            if crash {
                supervisor.pi.as_mut().unwrap().child.kill().unwrap();
                while supervisor.pi.is_some(){supervisor.refresh();std::thread::sleep(Duration::from_millis(20));}
            }else{supervisor.stop_pi().unwrap();}
            let until=Instant::now()+Duration::from_secs(5);
            loop{
                let mut code=0;unsafe{GetExitCodeProcess(process,&mut code);}
                if code!=259{break;}
                assert!(Instant::now()<until,"MCP descendant survived Pi shutdown");std::thread::sleep(Duration::from_millis(30));
            }
            unsafe{windows_sys::Win32::Foundation::CloseHandle(process);}
        }
        fs::remove_dir_all(data).unwrap();
    }
}

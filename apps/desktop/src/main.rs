#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod chat;
mod primary;
mod telegram;
mod chatgpt;
mod agent;
mod automation;
mod diagnostics;
mod delegation;
mod google;
mod power;
mod attachments;
mod browser;
mod credentials;
#[cfg(windows)]
mod job;
mod library;
mod permissions;
mod integrations;
#[path = "../../../services/scheduler/mod.rs"]
mod scheduler;
#[path = "../../../services/storage/store.rs"]
mod storage;
mod supervisor;
mod research;
mod skills;
mod runtime;
use permissions::Permissions;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, TryLockError,
    },
    time::{Duration, Instant},
};
use supervisor::Supervisor;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

struct Host {
    telegram: telegram::Telegram,
    google: google::Google,
    integrations: integrations::Integrations,
    db: Connection,
    supervisor: Supervisor,
    permissions: Permissions,
    chat: chat::ChatState,
    last_maintenance: Instant,
    browser_approvals: std::collections::HashMap<String, (Value, Instant, u64)>,

    agent_asks: std::collections::HashMap<String, agent::PendingAsk>,
}

static WEBVIEW_SUSPEND_GENERATION: AtomicU64 = AtomicU64::new(0);
static KEEP_WEBVIEW_IN_TRAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const WEBVIEW_SUSPEND_DELAY: Duration = Duration::from_secs(30);
type Shared = Arc<Mutex<Host>>;

#[cfg(windows)]
fn log_webview_suspend(message: &str) {
    if let Ok(mut log) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("talo-webview-suspend.log"))
    {
        let _ = writeln!(log, "[DEBUG-suspend30] {message}");
    }
}



#[derive(Clone)]
pub(crate) struct RuntimeStatusCache(Arc<Mutex<Value>>);

impl RuntimeStatusCache {
    fn new(initial: Value) -> Self {
        Self(Arc::new(Mutex::new(initial)))
    }

    fn get(&self) -> Value {
        self.0
            .lock()
            .map(|value| value.clone())
            .unwrap_or(Value::Null)
    }

    fn set(&self, status: &Value) {
        if let Ok(mut cached) = self.0.lock() {
            *cached = status.clone();
        }
    }
}



pub(crate) async fn run_blocking<T, F>(app: tauri::AppHandle, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || task(&app))
        .await
        .map_err(|error| format!("Talo task failed: {error}"))?
}

impl Host {



    fn browser_message(&mut self, app: &tauri::AppHandle, mut message: Value) {
        if message["method"] == "notifications/cancelled" {
            let request = message["params"]["requestId"].clone();
            let ids: Vec<_> = self.browser_approvals.iter().filter(|(_, (m,_,_))| m["id"] == request).map(|(id,_)| id.clone()).collect();
            for id in ids { self.browser_approvals.remove(&id); let _ = self.permissions.resolve(&self.db, &id, false); }
            self.supervisor.browser_request(json!({"method":"cancel","params":{"requestId":request}})); return;
        }
        if message["method"] != "tools/call" { return; }
        let name = message["params"]["name"].as_str().unwrap_or("").to_string();
        if !["browser_open","browser_tabs","browser_snapshot","browser_interact","browser_ai","browser_screenshot","browser_download","browser_handoff"].contains(&name.as_str()) {
            self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Unknown browser tool"}})); return;
        }


        let session = message["params"]["sessionId"].as_str().unwrap_or("");
        let exists = self.db.query_row("SELECT COUNT(*) FROM chat_conversations WHERE id=?1", [session], |row| row.get::<_, i64>(0)).unwrap_or(0) > 0;
        if !exists {
            self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser requires a valid chat thread."}})); return;
        }
        let generation = message["params"]["generationId"].as_str().unwrap_or("").to_string();
        let remote = telegram::owns(self,session,&generation);
        if remote && ["browser_interact", "browser_ai", "browser_download"].contains(&name.as_str()) {
            if name != "browser_download" {
                let state = self.supervisor.ensure_browser().ok().and_then(|browser| browser.sessions.lock().ok().map(|states| states[session].clone())).unwrap_or(Value::Null);
                let tab = state["tabs"].as_array().and_then(|tabs| tabs.iter().find(|tab| tab["id"]==state["activeTab"]));
                let Some(tab) = tab.filter(|tab|tab["id"].is_string() && tab["revision"].is_string()) else {
                    self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Open and read the browser page before requesting an action."}}));
                    return;
                };
                message["params"]["target"] = json!({"tabId":tab["id"],"revision":tab["revision"]});
                message["params"]["page"] = tab["url"].clone();
            }
            let resource = json!({"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"target":message["params"]["target"]}).to_string();
            let id = self.permissions.request("browser.action".into(),resource);
            self.browser_approvals.insert(id,(message,Instant::now(),self.supervisor.worker_epoch()));
            return;
        }
        if let Some(run) = automation::current(self) {
            if ["browser_interact", "browser_ai", "browser_download"].contains(&name.as_str()) {
                let resource = json!({"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"target":message["params"]["target"]}).to_string();
                let id = self.permissions.request("browser.action".into(),resource);
                self.browser_approvals.insert(id,(message,Instant::now(),self.supervisor.worker_epoch()));
                automation::notify(app,&run,"A scheduled browser action needs approval. Open Talo within 85 seconds.");
                return;
            }
        }
        message["params"]["approved"] = json!(true);
        if let Some(params) = message["params"].as_object_mut() { params.remove("target"); }
        self.supervisor.browser_request(message);
    }
    fn pump(&mut self, app: &tauri::AppHandle) {
        if power::take_interrupt() {
            automation::event(self,None,"windows_suspend_resume");
            if let Some(run)=automation::current(self) {
                let generation=self.db.query_row::<String,_,_>("SELECT generation_id FROM automation_runs WHERE id=?1",[&run],|r|r.get(0)).ok();
                if let Some(generation)=generation { let _=automation::stop_run(self,app,generation); }
                let _=self.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[&run]);
                let _=self.db.execute("UPDATE automation_runs SET status='uncertain',finished_at=unixepoch(),error='Windows suspended or resumed during execution. Review progress before resuming.' WHERE id=?1",[&run]);
                automation::notify(app,&run,"Scheduled task interrupted by Windows sleep; review progress");
            }
        }
        if let Some(frame) = self.supervisor.take_browser_frame() { let _ = app.emit_to("main", "browser-frame", frame); }
        for signal in self.supervisor.drain_signals() {
            if let supervisor::WorkerSignal::Tools(message) = signal { integrations::message(self, app, message); }
            else if let supervisor::WorkerSignal::Agent(message) = signal { agent::message(self, app, message); }
            else if let supervisor::WorkerSignal::Delegation(message) = signal { delegation::message(self, app, message); }
            else if let supervisor::WorkerSignal::Browser(message) = signal { self.browser_message(app, message); }
            else { self.chat.on_worker_signal(&mut self.db, app, signal); }
        }
        if integrations::expire(self){let _=app.emit("tool-approvals-changed",());}
        google::tick(self, app);
        agent::expire(self, app);
        let epoch = self.supervisor.worker_epoch();
        let expired: Vec<_> = self.browser_approvals.iter().filter(|(_, (_, at, owner))| *owner != epoch || at.elapsed() > Duration::from_secs(90)).map(|(id, _)| id.clone()).collect();
        for id in expired {
            if let Some((message, _, owner)) = self.browser_approvals.remove(&id) {
                let _ = self.permissions.resolve(&self.db, &id, false);
                if owner == epoch { self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser approval expired or activity was cancelled."}})); }
            }
        }
        let pending: Vec<_> = self.browser_approvals.iter().map(|(id,(message,_,_))| json!({"id":id,"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"page":message["params"]["page"],"conversationId":message["params"]["sessionId"],"generationId":message["params"]["generationId"]})).collect();
        if self.last_maintenance.elapsed() >= Duration::from_secs(2) { let _ = app.emit("browser-approvals", pending); }
        self.supervisor.refresh();
        for lifecycle in self.supervisor.drain_lifecycle() {
            automation::event(self,None,match &lifecycle { supervisor::Lifecycle::Started=>"worker_started", supervisor::Lifecycle::Exited{..}=>"worker_exited" });
            self.chat.on_lifecycle(&mut self.db, app, lifecycle);
        }
        delegation::tick(self, app);
        telegram::tick(self, app);
        let busy = self.chat.is_generating() || self.chat.any_active() || delegation::has_pending(&self.db);
        if self.supervisor.idle_stop(busy) {
            automation::event(self,None,"worker_hibernated");
            self.chat.on_worker_intentionally_stopped(app);
        }
        if self.last_maintenance.elapsed() >= Duration::from_secs(2) {
            self.chat.retry_pending(&mut self.db, app);
            if let Err(_) = automation::tick(self, app) {
                automation::event(self, None, "scheduler_error");
            }
            self.last_maintenance = Instant::now();
        }
    }
}
fn warm_worker(app: &tauri::AppHandle) {
    let shared = Arc::clone(app.state::<Shared>().inner());
    std::thread::spawn(move || {
        if let Ok(mut host) = shared.lock() {
            let _ = host.supervisor.start("pi");
        }
    });
}

#[tauri::command]
fn warm_worker_after_first_frame(app: tauri::AppHandle) {
    warm_worker(&app);
}

#[cfg(windows)]
fn suspend_webview(window: &tauri::WebviewWindow, generation: u64) {
    use webview2_com::{
        Microsoft::Web::WebView2::Win32::ICoreWebView2_3,
        TrySuspendCompletedHandler,
    };
    use windows::core::Interface;

    let visibility_window = window.clone();
    let queued = window.with_webview(move |webview| {
        let visible = visibility_window.is_visible().unwrap_or(true);
        let current_generation = WEBVIEW_SUSPEND_GENERATION.load(Ordering::SeqCst);
        log_webview_suspend(&format!(
            "suspend callback generation={generation} current={current_generation} visible={visible}"
        ));
        if current_generation != generation || visible {
            return;
        }
        unsafe {
            let controller = webview.controller();
            let mut controller_visible = windows::core::BOOL(0);
            let visibility = controller.IsVisible(&mut controller_visible);
            log_webview_suspend(&format!(
                "controller visibility before suspend {:?} visible={}",
                visibility,
                controller_visible.as_bool()
            ));
            let visibility = controller.SetIsVisible(false);
            log_webview_suspend(&format!("controller hide before suspend {visibility:?}"));
            if let Err(error) = visibility {
                log_webview_suspend(&format!("controller hide failed {error:?}"));
                return;
            }
            match controller
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_3>()) {
                Ok(core) => {
                    let handler = TrySuspendCompletedHandler::create(Box::new(|result, suspended| {
                        log_webview_suspend(&format!(
                            "suspend completed result={result:?} suspended={suspended}"
                        ));
                        Ok(())
                    }));
                    let result = core.TrySuspend(&handler);
                    log_webview_suspend(&format!("TrySuspend returned {result:?}"));
                }
                Err(error) => log_webview_suspend(&format!("CoreWebView2_3 unavailable {error:?}")),
            }
        }
    });
    log_webview_suspend(&format!("suspend callback queued {queued:?}"));
}

#[cfg(not(windows))]
fn suspend_webview(_window: &tauri::WebviewWindow, _generation: u64) {}

#[cfg(windows)]
fn resume_webview(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_3;
    use windows::core::Interface;

    let _ = window.with_webview(|webview| unsafe {
        let controller = webview.controller();
        let visible = controller.SetIsVisible(true);
        log_webview_suspend(&format!("controller show on resume {visible:?}"));
        if let Ok(core) = controller
            .CoreWebView2()
            .and_then(|core| core.cast::<ICoreWebView2_3>())
        {
            let result = core.Resume();
            log_webview_suspend(&format!("Resume returned {result:?}"));
        } else {
            log_webview_suspend("ICoreWebView2_3 unavailable during resume");
        }
    });
}

#[cfg(not(windows))]
fn resume_webview(_window: &tauri::WebviewWindow) {}

fn schedule_webview_suspend(app: tauri::AppHandle, generation: u64) {
    std::thread::spawn(move || {
        log_webview_suspend(&format!("suspend timer started generation={generation}"));
        std::thread::sleep(WEBVIEW_SUSPEND_DELAY);
        if WEBVIEW_SUSPEND_GENERATION.load(Ordering::SeqCst) != generation {
            log_webview_suspend(&format!("suspend timer cancelled generation={generation}"));
            return;
        }
        let Some(window) = app.get_webview_window("main") else {
            log_webview_suspend(&format!("suspend timer missing window generation={generation}"));
            return;
        };
        if window.is_visible().unwrap_or(true) {
            log_webview_suspend(&format!("suspend timer found visible window generation={generation}"));
            return;
        }
        log_webview_suspend(&format!("suspend timer firing generation={generation}"));
        suspend_webview(&window, generation);
    });
}


fn open_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        WEBVIEW_SUSPEND_GENERATION.fetch_add(1, Ordering::SeqCst);
        let _ = window.show();
        resume_webview(&window);
        let _ = window.set_focus();
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let config = handle
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == "main")
            .cloned();
        let Some(config) = config else { return };
        match tauri::WebviewWindowBuilder::from_config(&handle, &config)
            .and_then(|builder| builder.build())
        {
            Ok(window) => {
                let _ = window.show();
                resume_webview(&window);
                let _ = window.set_focus();
            }

            Err(error) => match handle.get_webview_window("main") {
                Some(window) => {
                    let _ = window.show();
                    resume_webview(&window);
                    let _ = window.set_focus();
                }
                None => eprintln!("Unable to restore the Talo window: {error}"),
            },
        }
    });
}

#[tauri::command]
async fn set_window_background(app: tauri::AppHandle, color: String) -> Result<(), String> {
    run_blocking(app, move |app| {
        let parsed: tauri::window::Color = color.trim().parse().map_err(|_| "Invalid color".to_string())?;
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_background_color(Some(parsed))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn runtime_status(
    shared: tauri::State<'_, Shared>,
    cache: tauri::State<'_, RuntimeStatusCache>,
) -> Result<Value, String> {


    let shared = Arc::clone(shared.inner());
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let status = match shared.try_lock() {
            Ok(mut host) => {

                let status = host.supervisor.status();
                cache.set(&status);
                status
            }


            Err(TryLockError::WouldBlock) => cache.get(),
            Err(TryLockError::Poisoned(_)) => return Err("Talo state is unavailable".into()),
        };
        Ok(status)
    })
    .await
    .map_err(|error| format!("Talo task failed: {error}"))?
}

#[tauri::command]
async fn load_settings(app: tauri::AppHandle) -> Result<Value, String> {
    run_blocking(app, |app| {
        let shared = app.state::<Shared>();
        let h = shared.lock().map_err(|e| e.to_string())?;
        saved_settings(&h.db)
    })
    .await
}

fn saved_settings(db: &Connection) -> Result<Value, String> {
    let value: Result<String, _> = db.query_row(
        "SELECT value FROM settings WHERE key='onboarding'",
        [],
        |row| row.get(0),
    );
    match value {
        Ok(value) => serde_json::from_str(&value).map_err(|error| error.to_string()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Value::Null),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
async fn startup_snapshot(app: tauri::AppHandle) -> Result<Value, String> {
    run_blocking(app, |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        let chat = chat::chat_bootstrap_inner(&mut host)?;
        let mut errors = Vec::new();
        let settings = saved_settings(&host.db).unwrap_or_else(|error| {
            errors.push(format!("Could not load saved settings: {error}"));
            Value::Null
        });
        let agent = agent::state(&host.db, &host);
        let library = library::startup_state(&host.db).unwrap_or_else(|error| {
            errors.push(format!("Could not load projects: {error}"));
            Value::Null
        });
        let runtime = host.supervisor.status();
        let tools: Vec<_> = host
            .integrations
            .pending
            .iter()
            .map(|(id, (message, _, _))| {
                json!({
                    "id": id,
                    "action": message["params"]["action"],
                    "scope": message["params"]["scope"],
                    "summary": message["params"]["summary"]
                })
            })
            .collect();
        let google = google::approvals(&host);
        let browser: Vec<_> = host
            .browser_approvals
            .iter()
            .map(|(id, (message, _, _))| {
                json!({
                    "id": id,
                    "tool": message["params"]["name"],
                    "arguments": message["params"]["arguments"],
                    "page": message["params"]["page"],
                    "conversationId": message["params"]["sessionId"],
                    "generationId": message["params"]["generationId"]
                })
            })
            .collect();
        Ok(json!({
            "chat": chat,
            "settings": settings,
            "agent": agent,
            "library": library,
            "runtime": runtime,
            "errors": errors,
            "approvals": {"tools": tools, "google": google, "browser": browser}
        }))
    })
    .await
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, settings: Value) -> Result<(), String> {
    run_blocking(app, move |app| {

        let allowed = [
            "name",
            "agentName",
            "personality",
            "provider",
            "model",
            "baseUrl",
            "reasoning",
            "showReasoning",
            "routing",
            "performance",
            "memory",
            "complete",
            "keepWebviewInTray",
        ];
        let obj = settings.as_object().ok_or("Expected settings object")?;
        if obj.keys().any(|k| !allowed.contains(&k.as_str())) || settings.to_string().len() > 8192 {
            return Err("Invalid settings".into());
        }
        if let Some(name) = obj.get("agentName") {
            let name = name.as_str().ok_or("Invalid agent name")?;
            if name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
                return Err("Choose an agent name between 1 and 80 characters".into());
            }
        }
        let provider = match obj.get("provider") {
            Some(Value::String(value)) if value.len() <= 64 => value.as_str(),
            Some(_) => return Err("Invalid provider".into()),
            None => "opencode",
        };

        if !["opencode", "opencode-go", "openrouter", "nvidia", "custom"].contains(&provider) {
            return Err("Unknown provider".into());
        }
        let model = match obj.get("model") {
            Some(Value::String(value)) if value.len() <= 200 => value.as_str(),
            Some(_) => return Err("Invalid model".into()),
            None => "",
        };
        let base_url = match obj.get("baseUrl") {
            Some(Value::String(value)) if value.len() <= 2048 => value.as_str(),
            Some(_) => return Err("Invalid base URL".into()),
            None => "",
        };
        let reasoning = match obj.get("reasoning") {
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("Invalid reasoning flag".into()),
            None => false,
        };
        let keep_webview_in_tray = match obj.get("keepWebviewInTray") {
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err("Invalid tray WebView setting".into()),
            None => false,
        };
        let shared = app.state::<Shared>();
        let h = shared.lock().map_err(|e| e.to_string())?;
        let previous_memory = library::setting(&h.db,"onboarding","{}");
        let previous_memory = serde_json::from_str::<Value>(&previous_memory).unwrap_or(Value::Null);
        if let Some(memory)=obj.get("memory").and_then(Value::as_str) {
            if !["off","workspace","global"].contains(&memory) {return Err("Invalid memory preference".into());}
            if previous_memory.get("memory").and_then(Value::as_str)!=Some(memory) {
                h.db.execute("INSERT INTO settings(key,value) VALUES('memory_enabled',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[if memory=="off"{"false"}else{"true"}]).map_err(|e|e.to_string())?;
            }
        }
        h.db.execute("INSERT INTO settings(key,value) VALUES('onboarding',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[settings.to_string()]).map_err(|e|e.to_string())?;
        KEEP_WEBVIEW_IN_TRAY.store(keep_webview_in_tray, Ordering::SeqCst);

        let projection =
            json!({"provider":provider,"model":model,"baseUrl":base_url,"reasoning":reasoning});
        std::fs::write(
            h.supervisor.data.join("provider.json"),
            projection.to_string(),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn request_permission(
    app: tauri::AppHandle,
    action: String,
    resource: String,
) -> Result<String, String> {
    run_blocking(app, move |app| {
        if !["browser.start", "autostart.change"].contains(&action.as_str()) {
            return Err("Action is not implemented in this scaffold".into());
        }
        let shared = app.state::<Shared>();
        let permission = shared
            .lock()
            .map_err(|e| e.to_string())?
            .permissions
            .request(action, resource);
        Ok(permission)
    })
    .await
}

pub(crate) fn resolve_permission_inner(h: &mut Host, id: &str, allowed: bool) -> Result<(), String> {
        let epoch_now = h.supervisor.worker_epoch();
        let fresh = |at: Instant, epoch: u64| at.elapsed() < Duration::from_secs(90) && epoch == epoch_now;
        let current = h.browser_approvals.get(id).is_none_or(|(m, at, epoch)| fresh(*at, *epoch) && h.db.query_row("SELECT EXISTS(SELECT 1 FROM chat_generations WHERE id=?1 AND conversation_id=?2 AND state='generating')",rusqlite::params![m["params"]["generationId"].as_str().unwrap_or(""),m["params"]["sessionId"].as_str().unwrap_or("")],|r|r.get::<_,bool>(0)).unwrap_or(false));
        let Host {
            db, permissions, ..
        } = h;
        permissions.resolve(db, &id, allowed && current)?;
        if let Some((mut message, at, epoch)) = h.browser_approvals.remove(id) {
            let valid = allowed && current && fresh(at, epoch);
            if valid {
                let resource = json!({"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"target":message["params"]["target"]}).to_string();
                h.permissions.consume(&id, "browser.action", &resource)?;
                if !automation::reserve(h,"browser.action",&resource) {
                    h.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Duplicate scheduled browser action suppressed. Review the conversation."}}));
                    return Ok(());
                }
                message["params"]["approved"] = json!(true);
                h.supervisor.browser_request(message);
            } else { h.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser action denied, expired or cancelled."}})); }
        }
    Ok(())
}

#[tauri::command]
async fn resolve_permission(
    app: tauri::AppHandle,
    id: String,
    allowed: bool,
) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut h = shared.lock().map_err(|e| e.to_string())?;
        resolve_permission_inner(&mut h, &id, allowed)?;
        Ok(())
    })
    .await
}

#[tauri::command]
async fn service_control(
    app: tauri::AppHandle,
    service: String,
    start: bool,
    _grant: Option<String>,
) -> Result<Value, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut h = shared.lock().map_err(|e| e.to_string())?;
        if !["pi", "browser"].contains(&service.as_str()) {
            return Err("Unsupported service".into());
        }
        if !start && service == "pi" && h.chat.any_active() {
            return Err(
                "Pi is generating a response; stop the response before shutting it down".into(),
            );
        }

        if start {
            h.supervisor.start(&service)?;
        } else {
            h.supervisor.stop(&service)?;
            if service == "pi" {
                h.chat.on_worker_intentionally_stopped(app);
            }
        }
        let status = h.supervisor.status();
        drop(h);
        if let Some(cache) = app.try_state::<RuntimeStatusCache>() {
            cache.set(&status);
        }

        let _ = app.emit("runtime-status", &status);
        Ok(status)
    })
    .await
}




#[tauri::command]
async fn prepare_update(app: tauri::AppHandle) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared.lock().map_err(|e| e.to_string())?;
        let Host {
            chat, db, supervisor, ..
        } = &mut *host;
        chat.fail_active_on_shutdown(db);
        supervisor.stop_all();
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_autostart(app: tauri::AppHandle, enabled: bool, grant: String) -> Result<(), String> {
    run_blocking(app, move |app| {
        use tauri_plugin_autostart::ManagerExt;
        {
            let shared = app.state::<Shared>();
            shared
                .lock()
                .map_err(|e| e.to_string())?
                .permissions
                .consume(
                    &grant,
                    "autostart.change",
                    if enabled { "enable" } else { "disable" },
                )?;
        }
        if enabled {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        }
        .map_err(|e| e.to_string())
    })
    .await
}

fn main() {
    #[cfg(windows)]
    unsafe {
        let args:Vec<u16>="--tray\0".encode_utf16().collect();
        windows_sys::Win32::System::Recovery::RegisterApplicationRestart(args.as_ptr(),0);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            open_main(app);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--tray"])
                .build(),
        )
        .plugin(tauri_plugin_notification::init())

        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            let Some(main) = window.app_handle().get_webview_window("main") else {
                return;
            };
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    let generation =
                        WEBVIEW_SUSPEND_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
                    log_webview_suspend(&format!("close requested generation={generation}"));
                    if KEEP_WEBVIEW_IN_TRAY.load(Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = main.hide();
                        schedule_webview_suspend(window.app_handle().clone(), generation);
                    }
                }
                tauri::WindowEvent::Focused(true) => {
                    WEBVIEW_SUSPEND_GENERATION.fetch_add(1, Ordering::SeqCst);
                    log_webview_suspend("window focused");
                    resume_webview(&main);
                }
                _ => {}
            }
        })
        .register_asynchronous_uri_scheme_protocol("attachment", |context, request, responder| {
            let app = context.app_handle().clone();
            let allowed = context.webview_label() == "main";
            tauri::async_runtime::spawn_blocking(move || {
                let result = if allowed {
                    let shared = app.state::<Shared>();
                    let host = shared.lock().unwrap();
                    attachments::preview(&host.db, request.uri().path().trim_start_matches('/'))
                } else { None };
                let response = match result {
                    Some((mime, bytes)) => tauri::http::Response::builder().header("Content-Type", mime).body(bytes),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![
            automation::automation_action,
            integrations::tools_action,
            google::google_action,
            telegram::telegram_action,
            agent::agent_action,
            delegation::delegation_action,
            runtime_status,
            startup_snapshot,
            warm_worker_after_first_frame,
            browser::browser_control,
            browser::browser_view,
            browser::browser_input,
            research::research_source_open,
            research::research_artifact_open,
            load_settings,
            save_settings,
            request_permission,
            resolve_permission,
            service_control,
            set_autostart,
            prepare_update,
            chat::chat_bootstrap,
            library::library_action,
            library::memory_save,
            library::memory_search,
            library::memory_list,
            library::memory_update,
            library::memory_delete,
            chat::conversation_create,
            chat::conversation_open,
            chat::conversation_refresh,
            chat::conversation_history,
            chat::conversation_select_model,
            chat::conversation_thinking,
            chat::chat_send,
            chat::chat_retry,
            chat::chat_stop,
            chat::provider_list,
            chat::provider_connect,
            chatgpt::chatgpt_begin,
            chatgpt::chatgpt_poll,
            chat::provider_disconnect,
            chat::provider_models,
            chat::theme_draft,
            set_window_background
        ])
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let mut db = Connection::open(data.join("talo.db"))?;
            db.busy_timeout(Duration::from_secs(5))?;


            db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
            db.execute_batch(include_str!("../../../services/memory/schema.sql"))?;
            KEEP_WEBVIEW_IN_TRAY.store(
                saved_settings(&db)?
                    .get("keepWebviewInTray")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                Ordering::SeqCst,
            );


            storage::init(&db)?;
            primary::ensure(&db)?;
            telegram::recover(&db)?;
            google::init(&db)?;
            scheduler::init(&db)?;
            agent::init(&db)?;
            skills::init(&db)?;
            delegation::init(&db)?;
            scheduler::recover(&db)?;
            delegation::recover(&db)?;

            db.execute(
                "UPDATE tasks SET state='awaiting_permission' WHERE state='running'",
                [],
            )?;

            storage::recover_interrupted(&mut db)?;
            let root = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor")
            } else {
                app.path().resource_dir()?
            };
            crate::runtime::set_root(root.clone());
            let status_cache = RuntimeStatusCache::new(json!({
                "database":"ready",
                "services":{"pi":"stopped","browser":"stopped","web":"stopped"},
                "dependencies":{}
            }));
            app.manage(status_cache.clone());
            let skill_data = data.clone();
            app.manage(Arc::new(Mutex::new(Host {
                telegram: telegram::Telegram::default(),
                google: google::Google::default(),
                integrations: integrations::Integrations::default(),
                db,
                supervisor: Supervisor::new(root, data),
                permissions: Permissions::default(),
                chat: chat::ChatState::default(),
                last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
            agent_asks: Default::default(),
            })));
            std::thread::spawn(move || {
                if let Ok(db) = Connection::open(skill_data.join("talo.db")) {
                    let _ = db.busy_timeout(Duration::from_secs(5));
                    skills::sync(&db, &skill_data);
                }
            });
            diagnostics::start(app.handle().clone());
            telegram::start(app.handle().clone());
            #[cfg(windows)]
            app.manage(power::Registration::new()?);

            {
                use tauri_plugin_notification::NotificationExt;
                let handle=app.handle().clone();
                app.notification().on_action(move |action| {
                    if action.action_id() != "tap" { return; }
                    let run=action.notification().and_then(|n|n.extra().get("runId")).and_then(Value::as_str).map(str::to_owned);
                    if let Some(run)=run {
                        if let Ok(h)=handle.state::<Shared>().lock() {
                            let _=h.db.execute("INSERT INTO settings(key,value) VALUES('notification_run',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[run]);
                        }
                    }
                    open_main(&handle);
                    let _=handle.emit("automation-open",());
                })?;
            }
            let show = MenuItem::with_id(app, "show", "Open Talo", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Talo", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause_automation", "Pause Automation", true, None::<&str>)?;
            let resume = MenuItem::with_id(app, "resume_automation", "Resume Automation", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &pause, &resume, &quit])?;
            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/icon.png"
                ))?)
                .tooltip("Talo is here")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => open_main(app),
                    "pause_automation" | "resume_automation" => {
                        let paused=event.id.as_ref()=="pause_automation";
                        let app=app.clone();
                        std::thread::spawn(move||{
                            if let Ok(h)=app.state::<Shared>().lock(){
                                let _=automation::set_paused(&h,paused);
                                let _=app.emit("automation-changed",());
                            }
                        });
                    }
                    "quit" => {
                        if let Ok(mut host) = app.state::<Shared>().lock() {
                            let Host {
                                chat,
                                db,
                                supervisor,
                                ..
                            } = &mut *host;
                            chat.fail_active_on_shutdown(db);
                            supervisor.stop_all();
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            let handle = app.handle().clone();




            std::thread::spawn(move || loop {
                {


                    let shared = Arc::clone(handle.state::<Shared>().inner());
                    if let Ok(mut h) = shared.lock() {
                        h.pump(&handle);
                    };
                }
                supervisor::pump_wait(Duration::from_secs(1));
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut published = String::new();
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let status = {
                        let shared = handle.state::<Shared>();
                        let Ok(mut h) = shared.lock() else {
                            continue;
                        };
                        h.supervisor.status()
                    };
                    if let Some(cache) = handle.try_state::<RuntimeStatusCache>() {
                        cache.set(&status);
                    }
                    let serialized = status.to_string();
                    if serialized != published {
                        published = serialized;
                        let _ = handle.emit("runtime-status", &status);
                    }
                }
            });

            let headless = std::env::args().any(|a| a == "--tray" || a == "--smoke-test");
            if !headless {
                open_main(app.handle());
            }
            if std::env::args().any(|a| a == "--smoke-test") {
                let shared = app.state::<Shared>();
                let mut h = shared.lock().unwrap();
                h.supervisor.start("pi").map_err(std::io::Error::other)?;
                println!(
                    "{}",
                    json!({"smoke":"ready","runtime":h.supervisor.status()})
                );
                drop(h);
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(8));
                    if let Ok(mut host) = handle.state::<Shared>().lock() {
                        let Host {
                            chat,
                            db,
                            supervisor,
                            ..
                        } = &mut *host;
                        chat.fail_active_on_shutdown(db);
                        supervisor.stop_all();
                    }
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Unable to launch Talo")
        .run(|_, event| {

            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod dispatch_regression {
    use super::*;
    use serde_json::json;
    use std::sync::Barrier;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::webview::InvokeRequest;

    fn test_host() -> Host {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        storage::init(&db).unwrap();
        google::init(&db).unwrap();
        Host {
            telegram: telegram::Telegram::default(),
            google: google::Google::default(),
            integrations: integrations::Integrations::default(),
            db,
            supervisor: Supervisor::new(
                std::path::PathBuf::from("missing-vendor"),
                std::env::temp_dir(),
            ),
            permissions: Permissions::default(),
            chat: chat::ChatState::default(),
            last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
            agent_asks: Default::default(),
        }
    }

    fn request(command: &str) -> InvokeRequest {
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost".parse().unwrap()
            } else {
                "tauri://localhost".parse().unwrap()
            },
            body: InvokeBody::default(),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        }
    }

    #[cfg(windows)]
    #[test]
    fn agent_reads_do_not_emit_changes_but_mutations_do() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tauri::Listener;

        let host = test_host();
        agent::init(&host.db).unwrap();
        host.db
            .execute(
                "INSERT INTO library_workspaces(id,name,draft) VALUES('draft-1','Review invoices',1)",
                [],
            )
            .unwrap();
        let app = tauri::Builder::default()
            .any_thread()
            .manage(Arc::new(Mutex::new(host)))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let changes = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&changes);
        app.listen("agent-changed", move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
        });

        for operation in ["state", "refresh"] {
            let state = tauri::async_runtime::block_on(agent::agent_action(
                app.handle().clone(),
                operation.into(),
                None,
            ))
            .unwrap();
            assert_eq!(state["drafts"].as_array().unwrap().len(), 1);
            assert_eq!(
                changes.load(Ordering::SeqCst),
                0,
                "{operation} triggered another refresh"
            );
        }

        tauri::async_runtime::block_on(agent::agent_action(
            app.handle().clone(),
            "space_accept".into(),
            Some(json!({"id":"draft-1"})),
        ))
        .unwrap();
        assert_eq!(changes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn google_state_is_safe_and_setup_aware_without_credentials() {
        let app = tauri::Builder::default()
            .any_thread()
            .manage(Arc::new(Mutex::new(test_host())))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let state = tauri::async_runtime::block_on(google::google_action(
            app.handle().clone(),
            "state".into(),
            None,
        ))
        .unwrap();
        assert!(state["configured"].is_boolean());
        assert_eq!(state["connected"], false);
        assert_eq!(state["authorizing"], false);
        let scopes = state["scopes"].as_array().unwrap();
        assert_eq!(scopes.len(), 5);
        assert!(scopes.iter().all(|scope| scope["granted"] == false));
        let serialized = state.to_string().to_lowercase();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("token"));

        let approvals = tauri::async_runtime::block_on(google::google_action(
            app.handle().clone(),
            "approvals".into(),
            None,
        ))
        .unwrap();
        assert!(approvals.as_array().unwrap().is_empty());
        assert!(tauri::async_runtime::block_on(google::google_action(
            app.handle().clone(),
            "unknown_operation".into(),
            None,
        ))
        .is_err());
    }



    #[test]
    fn runtime_status_dispatch_is_not_stalled_by_a_held_host_mutex() {
        let cache = RuntimeStatusCache::new(json!({
            "database":"ready",
            "services":{"pi":"stopped","browser":"stopped","web":"stopped"},
            "dependencies":{}
        }));
        let app = tauri::test::mock_builder()
            .manage(Arc::new(Mutex::new(test_host())))
            .manage(cache)
            .invoke_handler(tauri::generate_handler![runtime_status])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let acquired = Arc::new(Barrier::new(2));
        let signal = Arc::clone(&acquired);
        let handle = app.handle().clone();
        let holder = std::thread::spawn(move || {
            let state = handle.state::<Shared>();
            let guard = state.lock().unwrap();
            signal.wait();
            std::thread::sleep(Duration::from_millis(1500));
            drop(guard);
        });
        acquired.wait();

        let started = Instant::now();
        let response = tauri::test::get_ipc_response(&webview, request("runtime_status"))
            .expect("runtime_status should dispatch while the host mutex is held");
        let elapsed = started.elapsed();
        holder.join().unwrap();
        let status: Value = response.deserialize().unwrap();
        assert_eq!(status["database"], "ready");
        assert!(
            elapsed < Duration::from_millis(500),
            "runtime_status blocked the IPC dispatch thread for {elapsed:?}; \
             polling must not wait on the host mutex"
        );
    }
    #[tauri::command]
    async fn test_slow_probe(
        started: tauri::State<'_, Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<(), String> {
        let started = Arc::clone(started.inner());
        tauri::async_runtime::spawn_blocking(move || {
            started.store(true, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(1200));
        })
        .await
        .map_err(|error| error.to_string())
    }

    #[tauri::command]
    fn test_heartbeat() -> &'static str {
        "pong"
    }



    #[test]
    fn slow_command_keeps_ipc_dispatch_responsive() {
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let app = tauri::test::mock_builder()
            .manage(Arc::clone(&started))
            .manage(Arc::new(Mutex::new(test_host())))
            .manage(RuntimeStatusCache::new(json!({"database":"ready"})))
            .invoke_handler(tauri::generate_handler![
                runtime_status,
                test_slow_probe,
                test_heartbeat
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();

        let slow_webview = webview.clone();
        let slow = std::thread::spawn(move || {
            tauri::test::get_ipc_response(&slow_webview, request("test_slow_probe"))
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !started.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(
                Instant::now() < deadline,
                "slow probe never entered its blocking section"
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        let began = Instant::now();
        let heartbeat = tauri::test::get_ipc_response(&webview, request("test_heartbeat"))
            .expect("heartbeat should dispatch while the probe is blocked");
        let elapsed = began.elapsed();
        assert_eq!(heartbeat.deserialize::<String>().unwrap(), "pong");
        assert!(
            elapsed < Duration::from_millis(500),
            "IPC dispatch stalled for {elapsed:?} behind a blocking command"
        );
        slow.join().unwrap().unwrap();
    }
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod chat;
mod attachments;
mod browser;
mod credentials;
#[cfg(windows)]
mod job;
mod library;
mod permissions;
#[path = "../../../services/scheduler/mod.rs"]
mod scheduler;
#[path = "../../../services/storage/store.rs"]
mod storage;
mod supervisor;
mod research;
use permissions::Permissions;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex, TryLockError},
    time::{Duration, Instant},
};
use supervisor::Supervisor;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

struct Host {
    db: Connection,
    supervisor: Supervisor,
    permissions: Permissions,
    chat: chat::ChatState,
    last_maintenance: Instant,
    browser_approvals: std::collections::HashMap<String, (Value, Instant, u64)>,
}
type Shared = Arc<Mutex<Host>>;

/// Runtime status is published outside the host mutex so polling can never wait behind a
/// provider probe or another blocking command.
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

/// Run a command body off the Tauri event loop. Commands that lock `Host` must use this:
/// worker probes can take tens of seconds, and a synchronous command would freeze WebView2.
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
    /// Drain worker protocol messages, persist/emit chat transitions and enforce idle sleep.
    /// Running this under the same mutex as commands is what makes snapshots and events
    /// race-safe: a generation cannot advance while a command is mutating it.
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
        // Model-supplied flags can never authorize a browser operation.
        if let Some(params) = message["params"].as_object_mut() { params.remove("approved"); }
        if ["browser_interact","browser_ai","browser_download"].contains(&name.as_str()) {
            if self.browser_approvals.len() >= 8 { self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Too many pending browser approvals"}})); return; }
            if let Ok(browser) = self.supervisor.ensure_browser() {
                if let Ok(state) = browser.state.lock() {
                    if let Some(tab) = state["tabs"].as_array().and_then(|tabs| tabs.iter().find(|tab| tab["id"] == state["activeTab"])) {
                        message["params"]["target"] = json!({"tabId":tab["id"],"revision":tab["revision"]});
                        message["params"]["page"] = json!({"url":tab["url"],"title":tab["title"]});
                    }
                }
            }
            let resource = json!({"tool":name,"arguments":message["params"]["arguments"],"target":message["params"]["target"]}).to_string();
            let id = self.permissions.request("browser.action".into(), resource);
            self.browser_approvals.insert(id, (message, Instant::now(), self.supervisor.worker_epoch()));
            open_main(app);
        } else { self.supervisor.browser_request(message); }
    }
    fn pump(&mut self, app: &tauri::AppHandle) {
        for signal in self.supervisor.drain_signals() {
            if let supervisor::WorkerSignal::Browser(message) = signal { self.browser_message(app, message); }
            else { self.chat.on_worker_signal(&mut self.db, app, signal); }
        }
        let epoch = self.supervisor.worker_epoch();
        let expired: Vec<_> = self.browser_approvals.iter().filter(|(_, (_, at, owner))| *owner != epoch || at.elapsed() > Duration::from_secs(90)).map(|(id, _)| id.clone()).collect();
        for id in expired {
            if let Some((message, _, owner)) = self.browser_approvals.remove(&id) {
                let _ = self.permissions.resolve(&self.db, &id, false);
                if owner == epoch { self.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser approval expired or activity was cancelled."}})); }
            }
        }
        let pending: Vec<_> = self.browser_approvals.iter().map(|(id,(message,_,_))| json!({"id":id,"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"page":message["params"]["page"]})).collect();
        if self.last_maintenance.elapsed() >= Duration::from_secs(2) { let _ = app.emit("browser-approvals", pending); }
        self.supervisor.refresh();
        for lifecycle in self.supervisor.drain_lifecycle() {
            self.chat.on_lifecycle(&mut self.db, app, lifecycle);
        }
        let busy = self.chat.is_generating();
        if self.supervisor.idle_stop(busy) {
            self.chat.on_worker_intentionally_stopped(app);
        }
        if self.last_maintenance.elapsed() >= Duration::from_secs(2) {
            let _ = scheduler::tick(&self.db);
            self.last_maintenance = Instant::now();
        }
    }
}
/// Show the main window, rebuilding it when the user closed it to the tray.
/// Building a WebView2 window on the event-loop thread deadlocks on Windows, so it runs elsewhere.
fn open_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
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
                let _ = window.set_focus();
            }
            // A concurrent request may have won the label; show whatever exists.
            Err(error) => match handle.get_webview_window("main") {
                Some(window) => {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                None => eprintln!("Unable to restore the Talo window: {error}"),
            },
        }
    });
}
#[tauri::command]
async fn runtime_status(
    shared: tauri::State<'_, Shared>,
    cache: tauri::State<'_, RuntimeStatusCache>,
) -> Result<Value, String> {
    // Clone the Arc-backed state before awaiting: no Tauri State guard crosses the await,
    // and the blocked-mutex fallback below never parks the IPC dispatch thread.
    let shared = Arc::clone(shared.inner());
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let status = match shared.try_lock() {
            Ok(mut host) => {
                // Refresh can reap a worker, but never waits on the network.
                let status = host.supervisor.status();
                cache.set(&status);
                status
            }
            // A slow provider probe owns the host mutex. Return the last published status
            // immediately instead of parking the IPC dispatch thread behind it.
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
        let value: Result<String, _> = h.db.query_row(
            "SELECT value FROM settings WHERE key='onboarding'",
            [],
            |r| r.get(0),
        );
        match value {
            Ok(v) => serde_json::from_str(&v).map_err(|e| e.to_string()),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Value::Null),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
}

#[tauri::command]
async fn save_settings(app: tauri::AppHandle, settings: Value) -> Result<(), String> {
    run_blocking(app, move |app| {
        // No credentials are accepted or persisted in settings.
        let allowed = [
            "name",
            "personality",
            "provider",
            "model",
            "baseUrl",
            "reasoning",
            "showReasoning",
            "memory",
            "complete",
        ];
        let obj = settings.as_object().ok_or("Expected settings object")?;
        if obj.keys().any(|k| !allowed.contains(&k.as_str())) || settings.to_string().len() > 8192 {
            return Err("Invalid settings".into());
        }
        let provider = match obj.get("provider") {
            Some(Value::String(value)) if value.len() <= 64 => value.as_str(),
            Some(_) => return Err("Invalid provider".into()),
            None => "opencode",
        };
        // Mirrors the provider IDs in packages/contracts/src/providers.ts.
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
        // Non-secret provider preferences are projected for the Pi worker, which owns models.json.
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

#[tauri::command]
async fn resolve_permission(
    app: tauri::AppHandle,
    id: String,
    allowed: bool,
) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut h = shared.lock().map_err(|e| e.to_string())?;
        let current = h.browser_approvals.get(&id).is_none_or(|(_, at, epoch)| at.elapsed() < Duration::from_secs(90) && *epoch == h.supervisor.worker_epoch());
        let Host {
            db, permissions, ..
        } = &mut *h;
        permissions.resolve(db, &id, allowed && current)?;
        if let Some((mut message, at, epoch)) = h.browser_approvals.remove(&id) {
            let valid = allowed && at.elapsed() < Duration::from_secs(90) && epoch == h.supervisor.worker_epoch();
            if valid {
                let resource = json!({"tool":message["params"]["name"],"arguments":message["params"]["arguments"],"target":message["params"]["target"]}).to_string();
                h.permissions.consume(&id, "browser.action", &resource)?;
                message["params"]["approved"] = json!(true);
                h.supervisor.browser_request(message);
            } else { h.supervisor.relay_browser(json!({"id":message["id"],"error":{"message":"Browser action denied, expired or cancelled."}})); }
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn service_control(
    app: tauri::AppHandle,
    service: String,
    start: bool,
    grant: Option<String>,
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
        if start && service == "browser" {
            h.permissions.consume(
                grant.as_deref().unwrap_or(""),
                "browser.start",
                "persistent-profile",
            )?;
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
        // The window reflects this immediately instead of waiting for a poll.
        let _ = app.emit("runtime-status", &status);
        Ok(status)
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
    if browser::window_helper() { return; }
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
        .invoke_handler(tauri::generate_handler![
            runtime_status,
            browser::browser_control,
            research::research_source_open,
            research::research_artifact_open,
            load_settings,
            save_settings,
            request_permission,
            resolve_permission,
            service_control,
            set_autostart,
            chat::chat_bootstrap,
            library::library_action,
            library::memory_save,
            library::memory_search,
            library::memory_list,
            library::memory_update,
            library::memory_delete,
            chat::conversation_create,
            chat::conversation_open,
            chat::conversation_select_model,
            chat::conversation_thinking,
            chat::chat_send,
            chat::chat_retry,
            chat::chat_stop,
            chat::provider_list,
            chat::provider_connect,
            chat::provider_disconnect,
            chat::provider_models
        ])
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let mut db = Connection::open(data.join("talo.db"))?;
            db.busy_timeout(Duration::from_secs(5))?;
            db.execute_batch(include_str!("../../../services/memory/schema.sql"))?;
            // Chat/provider metadata is added alongside the memory schema. The DDL is
            // idempotent so existing v1 databases upgrade in place.
            storage::init(&db)?;
            // Interrupted executions require approval again after restart.
            db.execute(
                "UPDATE tasks SET state='awaiting_permission' WHERE state='running'",
                [],
            )?;
            // A generation that was streaming when the host stopped is terminal now.
            storage::recover_interrupted(&mut db)?;
            let root = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor")
            } else {
                app.path().resource_dir()?
            };
            let status_cache = RuntimeStatusCache::new(json!({
                "database":"ready",
                "services":{"pi":"stopped","browser":"stopped","web":"stopped"},
                "dependencies":{}
            }));
            app.manage(status_cache.clone());
            app.manage(Arc::new(Mutex::new(Host {
                db,
                supervisor: Supervisor::new(root, data),
                permissions: Permissions::default(),
                chat: chat::ChatState::default(),
                last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
            })));
            let show = MenuItem::with_id(app, "show", "Open Talo", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Talo", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/icon.png"
                ))?)
                .tooltip("Talo is here")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => open_main(app),
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
            // Streaming and crash recovery need a faster clock than the runtime-status poll.
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(30));
                let shared = handle.state::<Shared>();
                let Ok(mut h) = shared.lock() else {
                    continue;
                };
                h.pump(&handle);
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
            // A quiet launch-into-tray start should not keep a WebView2 tree alive.
            let headless = std::env::args().any(|a| a == "--tray" || a == "--smoke-test");
            if headless {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.destroy();
                }
            } else if let Some(window) = app.get_webview_window("main") {
                window.show()?;
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
            // Closing the window destroys its WebView2 tree; only the tray keeps Talo alive.
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
        Host {
            db,
            supervisor: Supervisor::new(
                std::path::PathBuf::from("missing-vendor"),
                std::env::temp_dir(),
            ),
            permissions: Permissions::default(),
            chat: chat::ChatState::default(),
            last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
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

    /// The native event loop must keep dispatching while a command holds the host mutex.
    /// This reproduces the NVIDIA NIM freeze without a key or a real network request.
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

    /// An async command that enters a long blocking operation must leave the IPC dispatcher
    /// free for a heartbeat. This is the provider_connect shape without a network or key.
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

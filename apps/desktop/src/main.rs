#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(windows)]
mod job;
mod permissions;
#[path = "../../../services/scheduler/mod.rs"]
mod scheduler;
mod supervisor;
use permissions::Permissions;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{sync::Mutex, time::Duration};
use supervisor::Supervisor;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, State,
};

struct Host {
    db: Connection,
    supervisor: Supervisor,
    permissions: Permissions,
}
type Shared = Mutex<Host>;
#[tauri::command]
fn runtime_status(host: State<Shared>) -> Result<Value, String> {
    Ok(host.lock().map_err(|e| e.to_string())?.supervisor.status())
}
#[tauri::command]
fn load_settings(host: State<Shared>) -> Result<Value, String> {
    let h = host.lock().map_err(|e| e.to_string())?;
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
}
#[tauri::command]
fn save_settings(host: State<Shared>, settings: Value) -> Result<(), String> {
    // No credentials are accepted or persisted in settings.
    let allowed = [
        "name",
        "personality",
        "provider",
        "model",
        "baseUrl",
        "reasoning",
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
    let h = host.lock().map_err(|e| e.to_string())?;
    h.db.execute("INSERT INTO settings(key,value) VALUES('onboarding',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[settings.to_string()]).map_err(|e|e.to_string())?;
    // Non-secret provider preferences are projected for the Pi worker, which owns models.json.
    let projection = json!({"provider":provider,"model":model,"baseUrl":base_url,"reasoning":reasoning});
    std::fs::write(h.supervisor.data.join("provider.json"), projection.to_string())
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
fn request_permission(
    host: State<Shared>,
    action: String,
    resource: String,
) -> Result<String, String> {
    if !["browser.start", "autostart.change"].contains(&action.as_str()) {
        return Err("Action is not implemented in this scaffold".into());
    }
    Ok(host
        .lock()
        .map_err(|e| e.to_string())?
        .permissions
        .request(action, resource))
}
#[tauri::command]
fn resolve_permission(host: State<Shared>, id: String, allowed: bool) -> Result<(), String> {
    let mut h = host.lock().map_err(|e| e.to_string())?;
    let Host {
        db, permissions, ..
    } = &mut *h;
    permissions.resolve(db, &id, allowed)
}
#[tauri::command]
fn service_control(
    host: State<Shared>,
    service: String,
    start: bool,
    grant: Option<String>,
) -> Result<Value, String> {
    let mut h = host.lock().map_err(|e| e.to_string())?;
    if !["pi", "browser"].contains(&service.as_str()) {
        return Err("Unsupported service".into());
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
    }
    Ok(h.supervisor.status())
}
#[tauri::command]
fn set_autostart(
    app: tauri::AppHandle,
    host: State<Shared>,
    enabled: bool,
    grant: String,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    host.lock()
        .map_err(|e| e.to_string())?
        .permissions
        .consume(
            &grant,
            "autostart.change",
            if enabled { "enable" } else { "disable" },
        )?;
    if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|e| e.to_string())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--tray"])
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            runtime_status,
            load_settings,
            save_settings,
            request_permission,
            resolve_permission,
            service_control,
            set_autostart
        ])
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let db = Connection::open(data.join("talo.db"))?;
            db.busy_timeout(Duration::from_secs(5))?;
            db.execute_batch(include_str!("../../../services/memory/schema.sql"))?;
            // Interrupted executions require approval again after restart.
            db.execute(
                "UPDATE tasks SET state='awaiting_permission' WHERE state='running'",
                [],
            )?;
            let root = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor")
            } else {
                app.path().resource_dir()?
            };
            app.manage(Mutex::new(Host {
                db,
                supervisor: Supervisor::new(root, data),
                permissions: Permissions::default(),
            }));
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
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.state::<Shared>().lock().unwrap().supervisor.stop_all();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(2));
                if let Ok(mut h) = handle.state::<Shared>().lock() {
                    h.supervisor.refresh();
                    let _ = scheduler::tick(&h.db);
                }
            });
            if std::env::args().any(|a| a == "--tray") {
                if let Some(w) = app.get_webview_window("main") {
                    w.hide()?;
                }
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
                    handle
                        .state::<Shared>()
                        .lock()
                        .unwrap()
                        .supervisor
                        .stop_all();
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("Unable to launch Talo");
}

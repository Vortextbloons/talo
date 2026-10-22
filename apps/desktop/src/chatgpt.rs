use crate::{credentials, run_blocking, storage, supervisor::Supervisor, Shared};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const TIMEOUT: Duration = Duration::from_secs(30);

fn credential(connection: &storage::ConnectionRow) -> Result<Value, String> {
    let secret = connection
        .secret
        .as_ref()
        .ok_or("Connect your ChatGPT account again.")?;
    let secret = credentials::unprotect(secret)?;
    serde_json::from_str(secret.as_str()?)
        .map_err(|_| "Invalid ChatGPT connection. Connect again.".into())
}

fn seal(value: &Value) -> Result<Vec<u8>, String> {
    let secret = credentials::Secret::new(value.to_string());
    credentials::protect(secret.as_str()?.as_bytes())
}

pub(crate) fn access(
    db: &rusqlite::Connection,
    supervisor: &mut Supervisor,
    connection: &storage::ConnectionRow,
) -> Result<credentials::Secret, String> {
    let mut value = credential(connection)?;
    let expires = value.get("expires").and_then(Value::as_i64).unwrap_or(0);
    if expires <= chrono::Utc::now().timestamp_millis() + 300_000 {
        supervisor.ensure_worker()?;
        value = supervisor
            .request("chatgpt.refresh", json!({"credential": value}), TIMEOUT)
            .map_err(|_| {
                "Could not renew ChatGPT access. Connect your account again.".to_string()
            })?;
        let sealed = seal(&value)?;
        db.execute(
            "UPDATE provider_connections SET secret=?2 WHERE id=?1",
            rusqlite::params![connection.id, sealed],
        )
        .map_err(|_| "Could not save renewed ChatGPT access.".to_string())?;
    }
    let access = value
        .get("access")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("Connect your ChatGPT account again.")?;
    Ok(credentials::Secret::new(access.to_string()))
}

pub(crate) fn disconnect(
    db: &rusqlite::Connection,
    supervisor: &mut Supervisor,
    connection: &storage::ConnectionRow,
) -> Result<bool, String> {
    let mut value = credential(connection)?;
    let revoked = supervisor.ensure_worker().is_ok()
        && supervisor
            .request("chatgpt.revoke", json!({"credential": value}), TIMEOUT)
            .ok()
            .and_then(|value| value.as_bool())
            == Some(true);
    for field in ["access", "refresh", "idToken"] {
        value[field] = json!("");
    }
    let sealed = seal(&value)?;
    db.execute(
        "UPDATE provider_connections SET connected=0,secret=?2 WHERE id=?1",
        rusqlite::params![connection.id, sealed],
    )
    .map_err(|_| "Could not disconnect ChatGPT.".to_string())?;
    Ok(revoked)
}

#[tauri::command]
pub async fn chatgpt_begin(
    app: AppHandle,
    connection_id: Option<String>,
) -> Result<String, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared.lock().map_err(|_| "Talo state is unavailable")?;
        let mut host_id = crate::library::setting(&host.db, "chatgpt_host_id", "");
        if host_id.is_empty() {
            host_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
            host.db
                .execute(
                    "INSERT INTO settings(key,value) VALUES('chatgpt_host_id',?1)",
                    [&host_id],
                )
                .map_err(|_| "Could not save Talo's installation identifier")?;
        }
        let previous = connection_id
            .map(|id| {
                let row = storage::get_connection(&host.db, &id)
                    .map_err(|_| "ChatGPT connection not found")?;
                if row.provider_id != "chatgpt" {
                    return Err("Choose a ChatGPT connection".to_string());
                }
                credential(&row)
            })
            .transpose()?;
        host.supervisor.ensure_worker()?;
        let result = host
            .supervisor
            .request(
                "chatgpt.begin",
                json!({"hostId":host_id,"credential":previous}),
                TIMEOUT,
            )
            .map_err(|error| error.to_string())?;
        let id = result
            .get("attemptId")
            .and_then(Value::as_str)
            .ok_or("ChatGPT sign-in did not start")?;
        let url = result
            .get("url")
            .and_then(Value::as_str)
            .ok_or("ChatGPT sign-in did not start")?;
        if !url.starts_with("https://auth.openai.com/api/accounts/authorize?") {
            return Err("Invalid ChatGPT sign-in address".into());
        }
        if let Err(error) = crate::research::open_external(url) {
            let _ = host.supervisor.request(
                "chatgpt.poll",
                json!({"attemptId":id,"cancel":true}),
                TIMEOUT,
            );
            return Err(error);
        }
        Ok(id.to_string())
    })
    .await
}

#[tauri::command]
pub async fn chatgpt_poll(
    app: AppHandle,
    attempt_id: String,
    cancel: bool,
) -> Result<Option<storage::ConnectionInfo>, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared.lock().map_err(|_| "Talo state is unavailable")?;
        let result = host
            .supervisor
            .request(
                "chatgpt.poll",
                json!({"attemptId":attempt_id,"cancel":cancel}),
                TIMEOUT,
            )
            .map_err(|error| error.to_string())?;
        let Some(value) = result.get("credential") else {
            return Ok(None);
        };
        let client = value
            .get("clientId")
            .and_then(Value::as_str)
            .ok_or("Invalid ChatGPT registration")?;
        let email = value
            .get("email")
            .and_then(Value::as_str)
            .filter(|email| !email.is_empty())
            .unwrap_or("ChatGPT account");
        let mut saved_name = None;
        let accounts =
            storage::list_connections(&host.db).map_err(|_| "Could not load ChatGPT accounts")?;
        let mut count = 0;
        for account in accounts
            .iter()
            .filter(|account| account.provider_id == "chatgpt")
        {
            count += 1;
            let row = storage::get_connection(&host.db, &account.id)
                .map_err(|_| "Could not load ChatGPT account")?;
            let saved = credential(&row)?;
            if saved.get("clientId").and_then(Value::as_str) == Some(client)
                && saved.get("subject") == value.get("subject")
            {
                saved_name = Some(account.name.clone());
            }
        }
        let name = saved_name.unwrap_or_else(|| format!("{email} · ChatGPT {}", count + 1));
        let sealed = seal(value)?;
        let id =
            storage::upsert_connection(&mut host.db, "chatgpt", &name, None, None, Some(&sealed))
                .map_err(|_| "Could not save ChatGPT connection")?;
        Ok(Some(
            storage::get_connection(&host.db, &id)
                .map_err(|_| "Could not load ChatGPT connection")?
                .info(),
        ))
    })
    .await
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn sign_out_clears_tokens_but_preserves_the_protected_account_registration() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        storage::init(&db).unwrap();
        let value = json!({"access":"test-plan-token","refresh":"test-refresh-token","idToken":"test-identity-token",
            "clientId":"oaiapp_test","subject":"account-one","email":"person@example.com",
            "expires":chrono::Utc::now().timestamp_millis()+3_600_000,"scopes":["chatgpt.tokens.use.direct"]});
        let sealed = seal(&value).unwrap();
        assert!(!sealed
            .windows(15)
            .any(|window| window == b"test-plan-token"));
        let id =
            storage::upsert_connection(&mut db, "chatgpt", "Personal", None, None, Some(&sealed))
                .unwrap();
        let missing = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let mut supervisor = Supervisor::new(missing.clone(), missing);
        let row = storage::get_connection(&db, &id).unwrap();
        assert_eq!(
            access(&db, &mut supervisor, &row)
                .unwrap()
                .as_str()
                .unwrap(),
            "test-plan-token"
        );
        assert!(!disconnect(&db, &mut supervisor, &row).unwrap());
        let row = storage::get_connection(&db, &id).unwrap();
        assert!(!row.connected);
        let saved = credential(&row).unwrap();
        assert_eq!(saved["clientId"], "oaiapp_test");
        assert_eq!(saved["subject"], "account-one");
        for field in ["access", "refresh", "idToken"] {
            assert_eq!(saved[field], "");
        }
        let public = serde_json::to_string(&row.info()).unwrap();
        assert!(!public.contains("test-plan-token"));
        assert!(!public.contains("test-refresh-token"));
    }
}

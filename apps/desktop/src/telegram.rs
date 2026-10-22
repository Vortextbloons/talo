use crate::{credentials, primary, run_blocking, Host, Shared};
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    io::Read,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct Telegram {
    pairing: Option<(String, Instant)>,
    candidate: Option<(i64, i64, String)>,
    online: bool,
    error: Option<String>,
    answers: HashMap<String, Map<String, Value>>,
    writing: Option<(String, usize)>,
    last_tick: Option<Instant>,
}

struct Link {
    id: String,
    secret: Vec<u8>,
    username: String,
    user: Option<i64>,
    chat: Option<i64>,
    name: Option<String>,
    offset: i64,
}

fn link(db: &Connection) -> rusqlite::Result<Option<Link>> {
    db.query_row("SELECT id,secret,bot_username,user_id,chat_id,account_name,next_update FROM telegram_connection LIMIT 1",[],|r| Ok(Link {
        id:r.get(0)?,secret:r.get(1)?,username:r.get(2)?,user:r.get(3)?,chat:r.get(4)?,name:r.get(5)?,offset:r.get(6)?,
    })).optional()
}

fn status(h: &Host) -> Result<Value, String> {
    let link = link(&h.db).map_err(|_| "Cannot read Telegram connection")?;
    let uncertain: bool =
        h.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM telegram_outbox WHERE status='uncertain')",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "Cannot read reply delivery status")?;
    let error=h.telegram.error.clone().or_else(||uncertain.then(||"A Telegram reply could not be confirmed. Read your primary chat for the result; the work will not be repeated.".to_string()));
    let pairing = h
        .telegram
        .pairing
        .as_ref()
        .filter(|(_, at)| at.elapsed() < Duration::from_secs(300));
    Ok(json!({
        "connected":link.is_some(),"paired":link.as_ref().is_some_and(|l|l.user.is_some()),
        "botUsername":link.as_ref().map(|l|&l.username),"accountName":link.as_ref().and_then(|l|l.name.as_ref()),
        "pairingUrl":link.as_ref().and_then(|l|pairing.map(|(code,_)|format!("https://t.me/{}?start={code}",l.username))),
        "pendingAccount":h.telegram.candidate.as_ref().filter(|_|pairing.is_some()).map(|(_,_,name)|json!({"name":name})),
        "online":h.telegram.online,"error":error,
    }))
}

fn pair(h: &mut Host) {
    h.telegram.pairing = Some((uuid::Uuid::new_v4().simple().to_string(), Instant::now()));
    h.telegram.candidate = None;
}

enum ApiError {
    Connect,
    Rate(i64),
    Rejected,
    Uncertain,
}

fn api(client: &Client, token: &str, method: &str, payload: &Value) -> Result<Value, ApiError> {
    let response = client
        .post(format!("https://api.telegram.org/bot{token}/{method}"))
        .json(payload)
        .send()
        .map_err(|e| {
            if e.is_connect() {
                ApiError::Connect
            } else {
                ApiError::Uncertain
            }
        })?;
    let mut bytes = Vec::new();
    response
        .take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| ApiError::Uncertain)?;
    if bytes.len() > 2_000_000 {
        return Err(ApiError::Uncertain);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError::Uncertain)?;
    if body["ok"] == true {
        return Ok(body["result"].clone());
    }
    if body["error_code"] == 429 {
        return Err(ApiError::Rate(
            body["parameters"]["retry_after"]
                .as_i64()
                .unwrap_or(5)
                .clamp(1, 3600),
        ));
    }
    if body["error_code"].as_i64().is_some_and(|code| code >= 500) {
        return Err(ApiError::Uncertain);
    }
    Err(ApiError::Rejected)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot start Telegram connection".into())
}

#[tauri::command]
pub async fn telegram_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app,move|app|{
        let shared=app.state::<Shared>();
        if operation=="connect" {
            let mut payload=payload.unwrap_or(Value::Null);
            let token=credentials::Secret::new(payload["token"].take().as_str().unwrap_or("").trim().to_string());
            let text=token.as_str()?;
            let (id,key)=text.split_once(':').ok_or("Enter the bot token from BotFather")?;
            if id.is_empty() || !id.bytes().all(|b|b.is_ascii_digit()) || key.len()<20 || key.len()>150 || !key.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-') {
                return Err("Enter the bot token from BotFather".into());
            }
            let client=client()?;
            let bot=api(&client,text,"getMe",&json!({})).map_err(|_|"Cannot verify this bot. Check the token and your internet connection.")?;
            let hook=api(&client,text,"getWebhookInfo",&json!({})).map_err(|_|"Cannot check this bot's connection")?;
            if hook["url"].as_str().is_some_and(|s|!s.is_empty()) {return Err("This bot is connected to another service. Create a dedicated bot for Talo in BotFather.".into());}
            let username=bot["username"].as_str().ok_or("Telegram returned an invalid bot")?;
            if bot["is_bot"]!=true {return Err("Connect a Telegram bot".into());}
            let sealed=credentials::protect(text.as_bytes())?;
            let mut h=shared.lock().map_err(|_|"Telegram is unavailable")?;
            if link(&h.db).map_err(|_|"Cannot read connection")?.is_some() {return Err("Disconnect the current bot before connecting another".into());}
            h.db.execute("INSERT INTO telegram_connection(id,secret,bot_username) VALUES(?1,?2,?3)",params![uuid::Uuid::new_v4().to_string(),sealed,username]).map_err(|_|"Cannot save Telegram connection")?;
            h.telegram=Telegram::default();pair(&mut h);
            let _=app.emit("telegram-changed",());
            return status(&h);
        }
        let mut h=shared.lock().map_err(|_|"Telegram is unavailable")?;
        match operation.as_str() {
            "state"=>return status(&h),
            "open_botfather"=>{crate::research::open_external("https://t.me/BotFather")?;},
            "open_pair"=>{
                let url=status(&h)?["pairingUrl"].as_str().ok_or("Create a new pairing link first")?.to_string();
                crate::research::open_external(&url)?;
            },
            "pair"=>{
                let current=link(&h.db).map_err(|_|"Cannot read connection")?.ok_or("Connect a bot first")?;
                if current.user.is_some(){return Err("This bot is already paired".into());}
                pair(&mut h);
            },
            "confirm"=>{
                if !h.telegram.pairing.as_ref().is_some_and(|(_,at)|at.elapsed()<Duration::from_secs(300)){return Err("Pairing expired. Create a new pairing link.".into());}
                let (user,chat,name)=h.telegram.candidate.clone().ok_or("Open the pairing link in Telegram first")?;
                if h.db.execute("UPDATE telegram_connection SET user_id=?1,chat_id=?2,account_name=?3 WHERE user_id IS NULL",params![user,chat,name]).map_err(|_|"Cannot save paired account")?!=1{return Err("This bot is already paired".into());}
                h.telegram.pairing=None;h.telegram.candidate=None;
                let current=link(&h.db).map_err(|_|"Cannot read connection")?.ok_or("Connection removed")?;
                enqueue(&h.db,&current.id,"paired","sendMessage",json!({"chat_id":chat,"text":"Connected to your primary Talo chat. Send a request here. Talo must be running on your PC. Use /status for progress or /stop to stop the current primary-chat reply."}),None)?;
            },
            "disconnect"=>{
                h.db.execute_batch("BEGIN IMMEDIATE; DELETE FROM telegram_connection; DELETE FROM telegram_inbox; DELETE FROM telegram_origins; DELETE FROM telegram_outbox; COMMIT;").map_err(|_|"Cannot disconnect Telegram")?;
                h.telegram=Telegram::default();
            },
            _=>return Err("Unknown Telegram operation".into()),
        }
        let _=app.emit("telegram-changed",());
        status(&h)
    }).await
}

pub fn recover(db: &Connection) -> rusqlite::Result<()> {
    db.execute(
        "UPDATE telegram_outbox SET status='uncertain' WHERE status='sending'",
        [],
    )?;
    db.execute("UPDATE telegram_inbox SET status='interrupted' WHERE status='dispatching' AND NOT EXISTS(SELECT 1 FROM chat_generations WHERE id=telegram_inbox.generation_id) AND NOT EXISTS(SELECT 1 FROM chat_turn_queue WHERE generation_id=telegram_inbox.generation_id)",[])?;
    db.execute(
        "UPDATE telegram_inbox SET status='accepted' WHERE status='dispatching'",
        [],
    )?;
    Ok(())
}

pub fn track_task(db: &Connection, task: &str, parent: &str) -> rusqlite::Result<()> {
    db.execute("INSERT OR IGNORE INTO telegram_origins(id,connection_id,kind) SELECT ?1,o.connection_id,'task' FROM chat_generations g JOIN telegram_origins o ON o.id=g.id AND o.kind='generation' WHERE g.conversation_id=?2 AND g.state='generating'",params![task,parent])?;
    Ok(())
}

pub fn owns(h: &Host, conversation: &str, generation: &str) -> bool {
    h.db.query_row("SELECT EXISTS(SELECT 1 FROM chat_generations g JOIN telegram_connection c ON c.user_id IS NOT NULL WHERE g.id=?1 AND g.conversation_id=?2 AND g.state='generating' AND (EXISTS(SELECT 1 FROM telegram_origins o WHERE o.id=g.id AND o.connection_id=c.id AND o.kind='generation') OR EXISTS(SELECT 1 FROM agent_delegations d JOIN telegram_origins o ON o.id=d.id AND o.kind='task' AND o.connection_id=c.id WHERE d.child_conversation_id=g.conversation_id AND d.generation_id=g.id)))",params![generation,conversation],|r|r.get(0)).unwrap_or(false)
}

fn enqueue(
    db: &Connection,
    connection: &str,
    id: &str,
    method: &str,
    payload: Value,
    card: Option<(&str, &str)>,
) -> Result<(), String> {
    db.execute("INSERT OR IGNORE INTO telegram_outbox(id,connection_id,method,payload,card_kind,card_id) VALUES(?1,?2,?3,?4,?5,?6)",params![format!("{connection}:{id}"),connection,method,payload.to_string(),card.map(|c|c.0),card.map(|c|c.1)]).map_err(|_|"Cannot save Telegram reply")?;
    Ok(())
}

fn chunks(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut units = 0;
    for ch in text.chars() {
        let len = ch.len_utf16();
        if units + len > 3800 {
            parts.push(std::mem::take(&mut part));
            units = 0;
        }
        part.push(ch);
        units += len;
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn say(db: &Connection, current: &Link, id: &str, text: &str) -> Result<(), String> {
    for (index, part) in chunks(text).into_iter().enumerate() {
        enqueue(
            db,
            &current.id,
            &format!("{id}:{index}"),
            "sendMessage",
            json!({"chat_id":current.chat,"text":part}),
            None,
        )?;
    }
    Ok(())
}

fn allowed(current: &Link, from: &Value, chat: &Value) -> bool {
    current.user.is_some()
        && from["id"].as_i64() == current.user
        && chat["id"].as_i64() == current.chat
        && chat["type"] == "private"
        && from["is_bot"] != true
}

fn persist_incoming(
    db: &mut Connection,
    current: &Link,
    update: i64,
    text: &str,
) -> Result<(), String> {
    let tx = db
        .transaction()
        .map_err(|_| "Cannot save Telegram message")?;
    tx.execute("INSERT OR IGNORE INTO telegram_inbox(connection_id,update_id,generation_id,text) VALUES(?1,?2,?3,?4)",params![current.id,update,uuid::Uuid::new_v4().to_string(),text]).map_err(|_|"Cannot save Telegram message")?;
    tx.execute(
        "UPDATE telegram_connection SET next_update=max(next_update,?2) WHERE id=?1",
        params![current.id, update + 1],
    )
    .map_err(|_| "Cannot acknowledge message")?;
    tx.commit().map_err(|_| "Cannot save Telegram message")?;
    Ok(())
}

fn ingest(
    h: &mut Host,
    app: &tauri::AppHandle,
    current: &Link,
    update: &Value,
) -> Result<(), String> {
    let update_id = update["update_id"]
        .as_i64()
        .ok_or("Invalid Telegram update")?;
    if update_id < current.offset {
        return Ok(());
    }
    let fresh = link(&h.db)
        .map_err(|_| "Cannot read Telegram connection")?
        .is_some_and(|l| l.id == current.id && l.offset <= update_id);
    if !fresh {
        return Ok(());
    }
    let key = format!("update:{update_id}");
    let m = &update["message"];
    if let Some(text) = m["text"].as_str() {
        if current.user.is_none() && m["chat"]["type"] == "private" && m["from"]["is_bot"] != true {
            if let Some((code, at)) = &h.telegram.pairing {
                if at.elapsed() < Duration::from_secs(300)
                    && text == format!("/start {code}")
                    && h.telegram.candidate.is_none()
                {
                    if let (Some(user), Some(chat)) =
                        (m["from"]["id"].as_i64(), m["chat"]["id"].as_i64())
                    {
                        let name = m["from"]["first_name"]
                            .as_str()
                            .unwrap_or("Telegram account")
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(80)
                            .collect::<String>();
                        h.telegram.candidate =
                            Some((user, chat, format!("{name} (Telegram ID {user})")));
                        let _ = app.emit("telegram-changed", ());
                    }
                }
            }
        } else if allowed(current, &m["from"], &m["chat"]) {
            let age = chrono::Utc::now().timestamp() - m["date"].as_i64().unwrap_or(0);
            if age > 600 {
                say(&h.db,current,&key,"This message arrived more than 10 minutes late. Please send it again so I know you still want it done.")?;
            } else if text == "/status" {
                let count:i64=h.db.query_row("SELECT count(*) FROM agent_delegations d JOIN telegram_origins o ON o.id=d.id AND o.connection_id=?1 WHERE d.state IN ('queued','running','waiting')",[&current.id],|r|r.get(0)).unwrap_or(0);
                let waiting = h
                    .integrations
                    .pending
                    .values()
                    .filter(|(m, _, _)| {
                        owns(
                            h,
                            m["params"]["conversationId"].as_str().unwrap_or(""),
                            m["params"]["generationId"].as_str().unwrap_or(""),
                        )
                    })
                    .count();
                say(&h.db,current,&key,&format!("Talo is online. Primary chat: {}. {count} Telegram tasks active. {waiting} tool approvals waiting.",if h.chat.has_active_for(primary::ID){"working"}else{"ready"}))?;
            } else if text == "/stop" {
                let generation:Option<String>=h.db.query_row("SELECT id FROM chat_generations WHERE conversation_id=?1 AND state IN ('generating','cancelling')",[primary::ID],|r|r.get(0)).optional().map_err(|_|"Cannot find current reply")?;
                if let Some(generation) = generation {
                    crate::chat::chat_stop_inner(app, h, generation)?;
                }
                say(&h.db,current,&key,"Stopped the current primary-chat reply. Other chats and child tasks were left running.")?;
            } else if text.starts_with("/stop_task ") {
                let task = text.trim_start_matches("/stop_task ").trim();
                let own:bool=h.db.query_row("SELECT EXISTS(SELECT 1 FROM telegram_origins WHERE id=?1 AND connection_id=?2 AND kind='task')",params![task,current.id],|r|r.get(0)).unwrap_or(false);
                let response = if own {
                    crate::delegation::cancel(h, app, task)
                        .map(|_| "Task stopped.")
                        .unwrap_or("That task could not be stopped.")
                } else {
                    "That task does not belong to this Telegram connection."
                };
                say(&h.db, current, &key, response)?;
            } else if text == "/cancel_answer" {
                h.telegram.writing = None;
                say(&h.db, current, &key, "Custom answer cancelled.")?;
            } else if text.starts_with("/start") {
                say(&h.db,current,&key,"You are connected to your primary Talo chat. Send a request, /status, or /stop.")?;
            } else if let Some((ask, index)) = h.telegram.writing.take() {
                let result = record_answer(h, app, &ask, index, json!({"other":text}));
                say(
                    &h.db,
                    current,
                    &key,
                    result.as_deref().unwrap_or_else(|e| e),
                )?;
            } else {
                let pending:i64=h.db.query_row("SELECT count(*) FROM telegram_inbox WHERE connection_id=?1 AND status IN ('received','dispatching')",[&current.id],|r|r.get(0)).unwrap_or(0);
                let queued: i64 =
                    h.db.query_row(
                        "SELECT count(*) FROM chat_turn_queue WHERE conversation_id=?1",
                        [primary::ID],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                if pending + queued >= 20 {
                    say(
                        &h.db,
                        current,
                        &key,
                        "There are too many messages waiting. Let Talo finish before sending more.",
                    )?;
                } else {
                    persist_incoming(&mut h.db, current, update_id, text)?;
                    return Ok(());
                }
            }
        }
    } else if !update["callback_query"].is_null() {
        let callback = &update["callback_query"];
        if allowed(current, &callback["from"], &callback["message"]["chat"]) {
            let result = callback_action(h, app, callback["data"].as_str().unwrap_or(""));
            let response = match result {
                Ok(text) => text,
                Err(text) => text,
            };
            enqueue(
                &h.db,
                &current.id,
                &key,
                "answerCallbackQuery",
                json!({"callback_query_id":callback["id"],"text":response.chars().take(180).collect::<String>()}),
                None,
            )?;
        }
    } else if allowed(current, &m["from"], &m["chat"]) {
        say(
            &h.db,
            current,
            &key,
            "Telegram currently supports text messages. Please describe what you want in text.",
        )?;
    }
    h.db.execute(
        "UPDATE telegram_connection SET next_update=?2 WHERE id=?1",
        params![current.id, update_id + 1],
    )
    .map_err(|_| "Cannot acknowledge message")?;
    Ok(())
}

fn ask_live(h: &Host, id: &str) -> bool {
    h.agent_asks.get(id).is_some_and(|p| {
        p.epoch == h.supervisor.worker_epoch()
            && p.at.elapsed() < crate::agent::ASK_TIMEOUT
            && conversation_owned(h, p.ask["conversationId"].as_str().unwrap_or(""))
    })
}

fn conversation_owned(h: &Host, conversation: &str) -> bool {
    let generation: String =
        h.db.query_row(
            "SELECT id FROM chat_generations WHERE conversation_id=?1 AND state='generating'",
            [conversation],
            |r| r.get(0),
        )
        .unwrap_or_default();
    owns(h, conversation, &generation)
}

fn record_answer(
    h: &mut Host,
    app: &tauri::AppHandle,
    id: &str,
    index: usize,
    answer: Value,
) -> Result<String, String> {
    if !ask_live(h, id) {
        return Err("This question has already been answered, cancelled, or expired.".into());
    }
    let questions = h.agent_asks[id].ask["questions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let question = questions.get(index).ok_or("Question unavailable")?;
    if answer.is_object() && question["other"] != true {
        return Err("Choose one of the listed answers.".into());
    }
    let question_id = question["id"].as_str().ok_or("Question unavailable")?;
    if h.telegram
        .answers
        .get(id)
        .is_some_and(|answers| answers.contains_key(question_id))
    {
        return Err("This question already has your answer.".into());
    }
    let one = Map::from_iter([(question_id.to_string(), answer.clone())]);
    crate::agent::resolve_answers(&[question.clone()], &one)?;
    h.telegram
        .answers
        .entry(id.to_string())
        .or_default()
        .insert(question_id.to_string(), answer);
    let answers = &h.telegram.answers[id];
    if questions
        .iter()
        .all(|q| answers.contains_key(q["id"].as_str().unwrap_or("")))
    {
        crate::agent::answer(h, app, &json!({"id":id,"answers":answers}))?;
        h.telegram.answers.remove(id);
        Ok("Answer sent to Talo.".into())
    } else {
        Ok("Answer saved. Please answer the remaining questions.".into())
    }
}

fn tool_approval_live(h: &Host, id: &str) -> bool {
    h.integrations
        .pending
        .get(id)
        .is_some_and(|(m, at, epoch)| {
            at.elapsed() < Duration::from_secs(85)
                && *epoch == h.supervisor.worker_epoch()
                && owns(
                    h,
                    m["params"]["conversationId"].as_str().unwrap_or(""),
                    m["params"]["generationId"].as_str().unwrap_or(""),
                )
        })
}

fn callback_action(h: &mut Host, app: &tauri::AppHandle, data: &str) -> Result<String, String> {
    let parts: Vec<_> = data.split(':').collect();
    match parts.as_slice() {
        ["t", id, decision] => {
            if !tool_approval_live(h, id) {
                return Err("This action has already been answered, stopped, or expired.".into());
            }
            crate::integrations::resolve_pending(
                h,
                app,
                id,
                if *decision == "y" { "once" } else { "deny" },
            )?;
            Ok(if *decision == "y" {
                "Approved once."
            } else {
                "Denied."
            }
            .into())
        }
        ["b", id, decision] => {
            let pending = h
                .browser_approvals
                .get(*id)
                .ok_or("Approval already answered or expired")?;
            if pending.1.elapsed() >= Duration::from_secs(90)
                || pending.2 != h.supervisor.worker_epoch()
                || !owns(
                    h,
                    pending.0["params"]["sessionId"].as_str().unwrap_or(""),
                    pending.0["params"]["generationId"].as_str().unwrap_or(""),
                )
            {
                return Err("This action is no longer running.".into());
            }
            crate::resolve_permission_inner(h, id, *decision == "y")?;
            let _=app.emit("browser-approvals",h.browser_approvals.iter().map(|(id,(m,_,_))|json!({"id":id,"tool":m["params"]["name"],"arguments":m["params"]["arguments"]})).collect::<Vec<_>>());
            Ok(if *decision == "y" {
                "Approved once."
            } else {
                "Denied."
            }
            .into())
        }
        ["q", id, index, option] => {
            if !ask_live(h, id) {
                return Err("Question already answered or expired.".into());
            }
            let index = index.parse::<usize>().map_err(|_| "Invalid question")?;
            let option = option.parse::<usize>().map_err(|_| "Invalid answer")?;
            let answer = h.agent_asks[*id].ask["questions"][index]["options"][option]["id"].clone();
            record_answer(h, app, id, index, answer)
        }
        ["w", id, index] => {
            if !ask_live(h, id) {
                return Err("Question already answered or expired.".into());
            }
            let index = index.parse::<usize>().map_err(|_| "Invalid question")?;
            if h.agent_asks[*id].ask["questions"][index]["other"] != true {
                return Err("Choose one of the listed answers.".into());
            }
            h.telegram.writing = Some((id.to_string(), index));
            let current = link(&h.db)
                .map_err(|_| "Connection unavailable")?
                .ok_or("Connection unavailable")?;
            say(&h.db,&current,&uuid::Uuid::new_v4().to_string(),"Send your answer as your next text message (up to 400 characters), or /cancel_answer.")?;
            Ok("Send your answer as a text message.".into())
        }
        _ => Err("This control is unavailable.".into()),
    }
}

pub fn tick(h: &mut Host, app: &tauri::AppHandle) {
    if h.telegram
        .last_tick
        .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
    {
        return;
    }
    h.telegram.last_tick = Some(Instant::now());
    if let Err(error) = tick_inner(h, app) {
        h.telegram.error = Some(error);
        let _ = app.emit("telegram-changed", ());
    }
}

fn tick_inner(h: &mut Host, app: &tauri::AppHandle) -> Result<(), String> {
    let Some(current) = link(&h.db).map_err(|_| "Cannot read Telegram connection")? else {
        return Ok(());
    };
    if current.user.is_none() {
        return Ok(());
    }
    let incoming:Option<(i64,String,String,i64)>=h.db.query_row("SELECT update_id,generation_id,text,created_at FROM telegram_inbox WHERE connection_id=?1 AND status='received' ORDER BY update_id LIMIT 1",[&current.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|"Cannot read Telegram messages")?;
    if let Some((update, generation, text, created)) = incoming {
        if chrono::Utc::now().timestamp() - created > 600 {
            say(
                &h.db,
                &current,
                &format!("stale:{update}"),
                "A queued request became too old while Talo was offline. Please resend it.",
            )?;
            h.db.execute(
                "UPDATE telegram_inbox SET status='interrupted' WHERE generation_id=?1",
                [&generation],
            )
            .map_err(|_| "Cannot update message")?;
        } else {
            let tx = h.db.transaction().map_err(|_| "Cannot accept message")?;
            tx.execute(
                "UPDATE telegram_inbox SET status='dispatching' WHERE generation_id=?1",
                [&generation],
            )
            .map_err(|_| "Cannot accept message")?;
            tx.execute("INSERT OR IGNORE INTO telegram_origins(id,connection_id,kind) VALUES(?1,?2,'generation')",params![generation,current.id]).map_err(|_|"Cannot route message")?;
            tx.commit().map_err(|_| "Cannot accept message")?;
            let result = crate::chat::chat_send_inner(
                app,
                h,
                primary::ID.into(),
                generation.clone(),
                text,
                vec![],
            );
            let state = if result.is_ok() { "accepted" } else { "failed" };
            h.db.execute(
                "UPDATE telegram_inbox SET status=?2 WHERE generation_id=?1",
                params![generation, state],
            )
            .map_err(|_| "Cannot update message")?;
            if let Err(error) = result {
                say(
                    &h.db,
                    &current,
                    &format!("failed:{generation}"),
                    &crate::chat::sanitize_error(&error),
                )?;
            } else {
                say(
                    &h.db,
                    &current,
                    &format!("accepted:{generation}"),
                    if h.db
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM chat_turn_queue WHERE generation_id=?1)",
                            [&generation],
                            |r| r.get::<_, bool>(0),
                        )
                        .unwrap_or(false)
                    {
                        "Message queued behind the current reply."
                    } else {
                        "I'm working on it. I'll send the result here."
                    },
                )?;
            }
            let _ = app.emit("telegram-message", json!({"conversationId":primary::ID}));
        }
    }
    let interrupted: Vec<String> = {
        let mut s=h.db.prepare("SELECT generation_id FROM telegram_inbox WHERE connection_id=?1 AND status='interrupted'").map_err(|_|"Cannot read interrupted messages")?;
        let rows = s
            .query_map([&current.id], |r| r.get(0))
            .map_err(|_| "Cannot read interrupted messages")?;
        rows.filter_map(Result::ok).collect()
    };
    for generation in interrupted {
        say(&h.db,&current,&format!("interrupted:{generation}"),"Talo restarted while accepting a request. It was not replayed. Review your primary chat before sending it again.")?;
    }
    deliver_results(&mut h.db, &current)?;
    let tasks = crate::delegation::list(&h.db, primary::ID).unwrap_or_default();
    for task in tasks {
        let id = task["id"].as_str().unwrap_or("");
        let remote:bool=h.db.query_row("SELECT EXISTS(SELECT 1 FROM telegram_origins WHERE id=?1 AND connection_id=?2 AND kind='task' AND last_state<>?3)",params![id,current.id,task["state"].as_str().unwrap_or("")],|r|r.get(0)).unwrap_or(false);
        if remote {
            let state = task["state"].as_str().unwrap_or("working");
            say(
                &h.db,
                &current,
                &format!("task:{id}:{state}"),
                &format!(
                    "{}: {state}\nStop this task: /stop_task {id}",
                    task["title"].as_str().unwrap_or("Task")
                ),
            )?;
            h.db.execute(
                "UPDATE telegram_origins SET last_state=?2 WHERE id=?1",
                params![id, state],
            )
            .map_err(|_| "Cannot save task progress")?;
        }
    }
    let mut cards = Vec::new();
    for (id, (m, _, _)) in &h.integrations.pending {
        if owns(
            h,
            m["params"]["conversationId"].as_str().unwrap_or(""),
            m["params"]["generationId"].as_str().unwrap_or(""),
        ) {
            let text = format!(
                "Approve this action once?\n{}\n{}\nExpires after 85 seconds.",
                m["params"]["summary"].as_str().unwrap_or("Tool action"),
                m["params"]["scope"].as_str().unwrap_or("")
            );
            let (text, buttons) = approval_card(text, "t", id);
            cards.push(("tool", id.clone(), text, buttons));
        }
    }
    for (id, (m, _, _)) in &h.browser_approvals {
        if owns(
            h,
            m["params"]["sessionId"].as_str().unwrap_or(""),
            m["params"]["generationId"].as_str().unwrap_or(""),
        ) {
            let text = format!(
                "Approve this browser action once?\n{}\nPage: {}\n{}\nExpires after 90 seconds.",
                m["params"]["name"].as_str().unwrap_or("Browser action"),
                m["params"]["page"].as_str().unwrap_or("Download URL shown below"),
                m["params"]["arguments"]
            );
            let (text, buttons) = approval_card(text, "b", id);
            cards.push(("browser", id.clone(), text, buttons));
        }
    }
    for (id, pending) in &h.agent_asks {
        if !ask_live(h, id) {
            continue;
        }
        for (index, q) in pending.ask["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mut buttons:Vec<Value>=q["options"].as_array().into_iter().flatten().enumerate().map(|(option,v)|json!([{"text":v["label"].as_str().unwrap_or("Choose").chars().take(60).collect::<String>(),"callback_data":format!("q:{id}:{index}:{option}")}])).collect();
            if q["other"] == true {
                buttons.push(
                    json!([{"text":"Write an answer","callback_data":format!("w:{id}:{index}")}]),
                );
            }
            cards.push((
                "ask",
                format!("{id}/{index}"),
                format!(
                    "{}\n{}",
                    pending.ask["header"].as_str().unwrap_or("A question"),
                    q["question"]
                        .as_str()
                        .or_else(|| q["prompt"].as_str())
                        .unwrap_or("")
                )
                .chars()
                .take(3600)
                .collect::<String>(),
                json!(buttons),
            ));
        }
    }
    for (kind, id, text, buttons) in cards {
        enqueue(
            &h.db,
            &current.id,
            &format!("card:{kind}:{id}"),
            "sendMessage",
            json!({"chat_id":current.chat,"text":text,"reply_markup":{"inline_keyboard":buttons}}),
            Some((kind, &id)),
        )?;
    }
    close_cards(h, &current)?;
    Ok(())
}

fn deliver_results(db: &mut Connection, current: &Link) -> Result<(), String> {
    let replies: Vec<(String, String, String, Option<String>)> = {
        let mut s=db.prepare("SELECT g.id,g.state,COALESCE(m.text,''),g.error FROM chat_generations g JOIN telegram_origins o ON o.id=g.id AND o.kind='generation' LEFT JOIN chat_messages m ON m.generation_id=g.id AND m.role='assistant' WHERE o.connection_id=?1 AND o.delivered=0 AND g.state IN ('completed','cancelled','failed')").map_err(|_|"Cannot read Telegram results")?;
        let rows = s
            .query_map([&current.id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(|_| "Cannot read Telegram results")?;
        rows.filter_map(Result::ok).collect()
    };
    for (generation, state, text, error) in replies {
        let reply = if state == "completed" && !text.trim().is_empty() {
            text
        } else if state == "completed" {
            "Finished. See your primary chat for details.".into()
        } else {
            format!(
                "{}{}",
                if state == "cancelled" {
                    "Stopped."
                } else {
                    "The request did not finish."
                },
                error
                    .map(|e| format!(" {}", crate::chat::sanitize_error(&e)))
                    .unwrap_or_default()
            )
        };
        let tx = db.transaction().map_err(|_| "Cannot save reply")?;
        say(&tx, current, &format!("result:{generation}"), &reply)?;
        tx.execute(
            "UPDATE telegram_origins SET delivered=1 WHERE id=?1",
            [generation],
        )
        .map_err(|_| "Cannot save reply")?;
        tx.commit().map_err(|_| "Cannot save reply")?;
    }
    Ok(())
}

fn approval_card(text: String, kind: &str, id: &str) -> (String, Value) {
    if text.encode_utf16().count() > 3800 {
        return ("An action needs your approval. Its details are too long for Telegram. Open Talo to review and approve the complete action.".into(),json!([]));
    }
    (
        text,
        json!([[{"text":"Approve once","callback_data":format!("{kind}:{id}:y")},{"text":"Deny","callback_data":format!("{kind}:{id}:n")}]]),
    )
}

fn card_live(h: &Host, kind: &str, id: &str) -> bool {
    match kind {
        "tool" => tool_approval_live(h, id),
        "browser" => h.browser_approvals.get(id).is_some_and(|(m, at, epoch)| {
            at.elapsed() < Duration::from_secs(90)
                && *epoch == h.supervisor.worker_epoch()
                && owns(
                    h,
                    m["params"]["sessionId"].as_str().unwrap_or(""),
                    m["params"]["generationId"].as_str().unwrap_or(""),
                )
        }),
        "ask" => {
            let (ask, index) = id.split_once('/').unwrap_or((id, "0"));
            ask_live(h, ask)
                && !h.telegram.answers.get(ask).is_some_and(|answers| {
                    h.agent_asks.get(ask).is_some_and(|p| {
                        answers.contains_key(
                            p.ask["questions"][index.parse::<usize>().unwrap_or(0)]["id"]
                                .as_str()
                                .unwrap_or(""),
                        )
                    })
                })
        }
        _ => false,
    }
}

fn close_cards(h: &Host, current: &Link) -> Result<(), String> {
    let cards: Vec<(String, String, String, String, Option<i64>, String)> = {
        let mut s=h.db.prepare("SELECT id,card_kind,card_id,payload,message_id,status FROM telegram_outbox WHERE connection_id=?1 AND card_kind IS NOT NULL AND resolved=0").map_err(|_|"Cannot read approvals")?;
        let rows = s
            .query_map([&current.id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(|_| "Cannot read approvals")?;
        rows.filter_map(Result::ok).collect()
    };
    for (id, kind, card, payload, message, state) in cards {
        if card_live(h, &kind, &card) {
            continue;
        }
        if state == "sending" {
            continue;
        }
        if let Some(message) = message {
            let payload: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
            enqueue(
                &h.db,
                &current.id,
                &format!("closed:{id}"),
                "editMessageText",
                json!({"chat_id":current.chat,"message_id":message,"text":format!("{}\n\nAnswered, expired, or cancelled.",payload["text"].as_str().unwrap_or("Request")),"reply_markup":{"inline_keyboard":[]}}),
                None,
            )?;
        } else {
            h.db.execute(
                "UPDATE telegram_outbox SET status='cancelled' WHERE id=?1 AND status='pending'",
                [&id],
            )
            .map_err(|_| "Cannot close approval")?;
        }
        h.db.execute("UPDATE telegram_outbox SET resolved=1 WHERE id=?1", [id])
            .map_err(|_| "Cannot close approval")?;
    }
    Ok(())
}

pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let Ok(client) = client() else {
            return;
        };
        loop {
            let current = {
                let shared = app.state::<Shared>();
                let Ok(h) = shared.lock() else {
                    return;
                };
                link(&h.db).ok().flatten()
            };
            let Some(current) = current else {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            };
            let Ok(token) = credentials::unprotect(&current.secret) else {
                connection_error(
                    &app,
                    false,
                    Some("Cannot unlock the bot token. Reconnect Telegram."),
                );
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let Ok(token) = token.as_str() else {
                continue;
            };
            for _ in 0..4 {
                let job = {
                    let shared = app.state::<Shared>();
                    let Ok(h) = shared.lock() else {
                        return;
                    };
                    if !link(&h.db)
                        .ok()
                        .flatten()
                        .is_some_and(|l| l.id == current.id)
                    {
                        break;
                    }
                    let job:Option<(String,String,String)>=h.db.query_row("SELECT id,method,payload FROM telegram_outbox WHERE connection_id=?1 AND status='pending' AND retry_at<=unixepoch() ORDER BY CASE WHEN method='answerCallbackQuery' THEN 0 WHEN card_kind IS NOT NULL THEN 1 ELSE 2 END,rowid LIMIT 1",[&current.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().ok().flatten();
                    if let Some((id, _, _)) = &job {
                        let card:Option<(String,String)>=h.db.query_row("SELECT card_kind,card_id FROM telegram_outbox WHERE id=?1 AND card_kind IS NOT NULL",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().ok().flatten();
                        if card.is_some_and(|(kind, card)| !card_live(&h, &kind, &card)) {
                            let _=h.db.execute("UPDATE telegram_outbox SET status='cancelled',resolved=1 WHERE id=?1",[id]);
                            continue;
                        }
                        if h.db
                            .execute(
                                "UPDATE telegram_outbox SET status='sending' WHERE id=?1",
                                [id],
                            )
                            .is_err()
                        {
                            break;
                        }
                    }
                    job
                };
                let Some((id, method, payload)) = job else {
                    break;
                };
                let payload: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
                let result = api(&client, token, &method, &payload);
                let shared = app.state::<Shared>();
                let Ok(mut h) = shared.lock() else {
                    return;
                };
                match result {
                    Ok(value) => {
                        let _ = h.db.execute(
                            "UPDATE telegram_outbox SET status='sent',message_id=?2 WHERE id=?1",
                            params![id, value["message_id"].as_i64()],
                        );
                    }
                    Err(ApiError::Connect) => {
                        let _=h.db.execute("UPDATE telegram_outbox SET status='pending',retry_at=unixepoch()+5 WHERE id=?1",[id]);
                        break;
                    }
                    Err(ApiError::Rate(seconds)) => {
                        let _=h.db.execute("UPDATE telegram_outbox SET status='pending',retry_at=unixepoch()+?2 WHERE id=?1",params![id,seconds]);
                        break;
                    }
                    Err(_) => {
                        let _ = h.db.execute(
                            "UPDATE telegram_outbox SET status='uncertain' WHERE id=?1",
                            [id],
                        );
                        h.telegram.error=Some("A Telegram reply could not be confirmed. Read your primary chat for the result; the work will not be repeated.".into());
                        let _ = app.emit("telegram-changed", ());
                    }
                }
                drop(h);
                std::thread::sleep(Duration::from_millis(1100));
            }
            let offset = {
                let shared = app.state::<Shared>();
                let Ok(h) = shared.lock() else {
                    return;
                };
                let Some(link) = link(&h.db).ok().flatten().filter(|l| l.id == current.id) else {
                    continue;
                };
                link.offset
            };
            match api(
                &client,
                token,
                "getUpdates",
                &json!({"offset":offset,"timeout":8,"limit":30,"allowed_updates":["message","callback_query"]}),
            ) {
                Ok(updates) => {
                    connection_error(&app, true, None);
                    for update in updates.as_array().into_iter().flatten() {
                        let shared = app.state::<Shared>();
                        let Ok(mut h) = shared.lock() else {
                            return;
                        };
                        let Some(latest) =
                            link(&h.db).ok().flatten().filter(|l| l.id == current.id)
                        else {
                            break;
                        };
                        if let Err(error) = ingest(&mut h, &app, &latest, update) {
                            h.telegram.error = Some(error);
                            let _ = app.emit("telegram-changed", ());
                            break;
                        }
                    }
                }
                Err(error) => {
                    connection_error(&app,false,Some("Telegram is offline or this bot is being used elsewhere. Check your connection and use this bot only with Talo."));
                    std::thread::sleep(Duration::from_secs(match error {
                        ApiError::Rate(seconds) => seconds.min(60) as u64,
                        _ => 5,
                    }));
                }
            }
        }
    });
}

fn connection_error(app: &tauri::AppHandle, online: bool, error: Option<&str>) {
    let shared = app.state::<Shared>();
    let Ok(mut h) = shared.lock() else {
        return;
    };
    let changed = h.telegram.online != online
        || error.is_some_and(|e| h.telegram.error.as_deref() != Some(e));
    h.telegram.online = online;
    if let Some(error) = error {
        h.telegram.error = Some(error.into());
    } else if changed {
        h.telegram.error = None;
    }
    if changed {
        let _ = app.emit("telegram-changed", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        crate::storage::init(&db).unwrap();
        crate::delegation::init(&db).unwrap();
        primary::ensure(&db).unwrap();
        db.execute("INSERT INTO telegram_connection(id,secret,bot_username,user_id,chat_id,account_name) VALUES('connection',x'00','test_bot',11,22,'Owner')",[]).unwrap();
        Host {
            telegram: Telegram::default(),
            db,
            integrations: Default::default(),
            supervisor: crate::supervisor::Supervisor::new(
                "missing-vendor".into(),
                std::env::temp_dir(),
            ),
            permissions: Default::default(),
            chat: Default::default(),
            last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
            agent_asks: Default::default(),
        }
    }

    fn generation(h: &mut Host, id: &str, conversation: &str, remote: bool) {
        if conversation != primary::ID {
            crate::storage::create_conversation(&h.db, conversation, "Side chat", None).unwrap();
        }
        crate::storage::start_generation(
            &mut h.db,
            crate::storage::NewGeneration {
                id,
                conversation_id: conversation,
                assistant_id: &format!("assistant-{id}"),
                text: "Request",
                title: None,
            },
        )
        .unwrap();
        if remote {
            h.db.execute("INSERT INTO telegram_origins(id,connection_id,kind) VALUES(?1,'connection','generation')",[id]).unwrap();
        }
    }

    #[test]
    fn telegram_requires_both_paired_ids_and_a_private_chat() {
        let h = host();
        let link = link(&h.db).unwrap().unwrap();
        assert!(allowed(
            &link,
            &json!({"id":11}),
            &json!({"id":22,"type":"private"})
        ));
        assert!(!allowed(
            &link,
            &json!({"id":12}),
            &json!({"id":22,"type":"private"})
        ));
        assert!(!allowed(
            &link,
            &json!({"id":11}),
            &json!({"id":23,"type":"private"})
        ));
        assert!(!allowed(
            &link,
            &json!({"id":11}),
            &json!({"id":22,"type":"group"})
        ));
        assert!(!allowed(
            &link,
            &json!({"id":11,"is_bot":true}),
            &json!({"id":22,"type":"private"})
        ));
    }

    #[test]
    fn telegram_updates_are_durable_and_accepted_once() {
        let mut h = host();
        let link = link(&h.db).unwrap().unwrap();
        let update = json!({"update_id":42,"message":{"date":chrono::Utc::now().timestamp(),"text":"Research this","from":{"id":11},"chat":{"id":22,"type":"private"}}});
        persist_incoming(
            &mut h.db,
            &link,
            update["update_id"].as_i64().unwrap(),
            update["message"]["text"].as_str().unwrap(),
        )
        .unwrap();
        persist_incoming(
            &mut h.db,
            &link,
            update["update_id"].as_i64().unwrap(),
            update["message"]["text"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            h.db.query_row("SELECT count(*) FROM telegram_inbox", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(super::link(&h.db).unwrap().unwrap().offset, 43);
        assert_eq!(
            h.db.query_row("SELECT text FROM telegram_inbox", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "Research this"
        );
    }

    #[test]
    fn telegram_recovery_does_not_replay_uncertain_dispatch_or_delivery() {
        let h = host();
        h.db.execute("INSERT INTO telegram_inbox(connection_id,update_id,generation_id,text,status) VALUES('connection',1,'uncertain','Do it','dispatching'),('connection',2,'queued','Next','dispatching')",[]).unwrap();
        crate::storage::enqueue_turn(&h.db, "queued", primary::ID, "user", "Next").unwrap();
        enqueue(
            &h.db,
            "connection",
            "reply",
            "sendMessage",
            json!({"text":"Done"}),
            None,
        )
        .unwrap();
        h.db.execute("UPDATE telegram_outbox SET status='sending'", [])
            .unwrap();
        recover(&h.db).unwrap();
        assert_eq!(
            h.db.query_row(
                "SELECT status FROM telegram_inbox WHERE generation_id='uncertain'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "interrupted"
        );
        assert_eq!(
            h.db.query_row(
                "SELECT status FROM telegram_inbox WHERE generation_id='queued'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "accepted"
        );
        assert_eq!(
            h.db.query_row("SELECT status FROM telegram_outbox", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "uncertain"
        );
    }

    #[test]
    fn telegram_approval_ownership_ends_with_the_generation() {
        let mut h = host();
        generation(&mut h, "remote", primary::ID, true);
        generation(&mut h, "desktop", "other", false);
        assert!(owns(&h, primary::ID, "remote"));
        assert!(!owns(&h, "other", "desktop"));
        assert!(!owns(&h, "other", "remote"));
        h.integrations.pending.insert("approval".into(),(json!({"id":"call","params":{"conversationId":primary::ID,"generationId":"remote","action":"files.write","scope":"file","summary":"Write file"}}),Instant::now(),h.supervisor.worker_epoch()));
        h.db.execute(
            "UPDATE chat_generations SET state='cancelled' WHERE id='remote'",
            [],
        )
        .unwrap();
        assert!(!owns(&h, primary::ID, "remote"));
        assert!(!tool_approval_live(&h, "approval"));
        assert_eq!(
            h.db.query_row(
                "SELECT state FROM chat_generations WHERE id='desktop'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "generating"
        );
        assert_eq!(
            h.db.query_row("SELECT count(*) FROM permission_audit", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn telegram_results_are_queued_once_and_desktop_replies_stay_local() {
        let mut h = host();
        generation(&mut h, "remote", primary::ID, true);
        generation(&mut h, "desktop", "other", false);
        h.db.execute("UPDATE chat_generations SET state='completed'", [])
            .unwrap();
        h.db.execute(
            "UPDATE chat_messages SET text='Finished',status='completed' WHERE role='assistant'",
            [],
        )
        .unwrap();
        let current = link(&h.db).unwrap().unwrap();
        deliver_results(&mut h.db, &current).unwrap();
        deliver_results(&mut h.db, &current).unwrap();
        assert_eq!(
            h.db.query_row(
                "SELECT count(*) FROM telegram_outbox WHERE id LIKE '%result:%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            h.db.query_row(
                "SELECT delivered FROM telegram_origins WHERE id='remote'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap(),
            true
        );
    }

    #[test]
    fn telegram_long_answers_split_without_cutting_unicode_or_approval_details() {
        let text = "😀".repeat(4000);
        let parts = chunks(&text);
        assert_eq!(parts.concat(), text);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= 3800));
        let (text, buttons) = approval_card("x".repeat(4000), "t", "id");
        assert!(text.contains("Open Talo"));
        assert_eq!(buttons, json!([]));
    }

    #[test]
    fn telegram_primary_identity_survives_initialization_without_merging_history() {
        let h = host();
        primary::ensure(&h.db).unwrap();
        let main = crate::storage::get_conversation(&h.db, primary::ID).unwrap();
        assert_eq!(main.workspace_id, "general");
        assert!(!main.archived);
        assert_eq!(crate::storage::list_conversations(&h.db).unwrap().len(), 1);
    }
}

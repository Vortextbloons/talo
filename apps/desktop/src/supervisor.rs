use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::PathBuf,
    process::{Child, Command, Stdio},
};

pub struct Supervisor {
    pub root: PathBuf,
    pub data: PathBuf,
    children: HashMap<String, Child>,
    failures: HashMap<String, String>,
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}
impl Supervisor {
    pub fn new(root: PathBuf, data: PathBuf) -> Self {
        Self {
            root,
            data,
            children: HashMap::new(),
            failures: HashMap::new(),
            #[cfg(windows)]
            job: crate::job::Job::new().ok(),
        }
    }
    pub fn refresh(&mut self) {
        self.children.retain(|id, child| match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
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
            if self.children.contains_key(id) {
                "running"
            } else if self.failures.contains_key(id) {
                "failed"
            } else {
                "stopped"
            }
        };
        json!({"database":"ready", "services":{"pi":state("pi"),"browser":state("browser"),"web":state("web")},"dependencies":{
            "node":self.root.join("runtime/node/node.exe").is_file(),
            "chromium":self.root.join("runtime/chromium/chrome-win64/chrome.exe").is_file(),
            "donsetch":self.root.join("bundle/node_modules/donsetch/binaries/donsetch.exe").is_file(),
            "pi":self.root.join("bundle/node_modules/@earendil-works/pi-coding-agent/package.json").is_file(),
            "stagehand":self.root.join("bundle/node_modules/@browserbasehq/stagehand/package.json").is_file(),
            "mcpAdapter":self.root.join("bundle/node_modules/pi-mcp-adapter/package.json").is_file()
        }})
    }
    pub fn start(&mut self, id: &str) -> Result<(), String> {
        self.refresh();
        if self.children.contains_key(id) {
            return Ok(());
        }
        let script = match id {
            "pi" => "workers/pi/src/main.js",
            "browser" => "services/browser/src/main.js",
            _ => return Err("Unknown managed service".into()),
        };
        let logs = self.data.join("logs");
        fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join(format!("{id}.jsonl")))
            .map_err(|e| e.to_string())?;
        let err = OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join(format!("{id}.stderr.log")))
            .map_err(|e| e.to_string())?;
        let mut command = Command::new(self.root.join("runtime/node/node.exe"));
        command
            .arg(self.root.join("bundle").join(script))
            .current_dir(self.root.join("bundle"))
            .env("TALO_DATA_DIR", &self.data)
            .env("TALO_VENDOR_DIR", &self.root)
            .env("TALO_HOST_MODE", "1")
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .stdin(Stdio::piped())
            .stdout(out)
            .stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        #[cfg(windows)]
        if self.job.is_none() {
            return Err("Unable to create worker process containment job".into());
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        if let Err(e) = self.job.as_ref().unwrap().assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.to_string());
        }
        self.children.insert(id.into(), child);
        self.failures.remove(id);
        Ok(())
    }
    pub fn stop(&mut self, id: &str) -> Result<(), String> {
        if let Some(mut child) = self.children.remove(id) {
            use std::io::Write;
            if let Some(input) = child.stdin.as_mut() {
                let _ = writeln!(input, "{{\"id\":\"host-stop\",\"method\":\"shutdown\"}}");
            }
            for _ in 0..30 {
                if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
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
        Ok(())
    }
    pub fn stop_all(&mut self) {
        for id in self.children.keys().cloned().collect::<Vec<_>>() {
            let _ = self.stop(&id);
        }
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop_all();
    }
}

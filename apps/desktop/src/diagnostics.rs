
use crate::{automation, Shared};
use serde_json::json;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{Emitter, Manager};
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut system = System::new();
        let mut previous = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(10));
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_memory().with_cpu(),
            );
            let shared = app.state::<Shared>();
            let Ok(mut h) = shared.lock() else { continue };
            if previous.elapsed() > Duration::from_secs(30) {
                automation::event(&h, None, "host_resumed_or_delayed");
            }
            previous = Instant::now();
            let mut managed = HashSet::from([Pid::from_u32(std::process::id())]);
            let mut pi = HashSet::new();
            if let Some(pid) = h.supervisor.pi_pid() {
                pi.insert(Pid::from_u32(pid));
            }
            for _ in 0..16 {
                let before = managed.len() + pi.len();
                for (id, p) in system.processes() {
                    if p.parent().is_some_and(|parent| managed.contains(&parent)) {
                        managed.insert(*id);
                    }
                    if p.parent().is_some_and(|parent| pi.contains(&parent)) {
                        pi.insert(*id);
                    }
                }
                if before == managed.len() + pi.len() {
                    break;
                }
            }
            let memory: u64 = managed
                .iter()
                .filter_map(|id| system.process(*id))
                .map(|p| p.memory())
                .sum();
            let cpu: f32 = managed
                .iter()
                .filter_map(|id| system.process(*id))
                .map(|p| p.cpu_usage())
                .sum();
            let pi_memory: u64 = pi
                .iter()
                .filter_map(|id| system.process(*id))
                .map(|p| p.memory())
                .sum();
            let sample = json!({"event":"resources","memoryBytes":memory,"cpuPercent":cpu,"piMemoryBytes":pi_memory,"processes":managed.len()});
            automation::event(&h, None, &sample.to_string());
            let _ = app.emit("automation-resources", sample);
            let _=h.db.execute("DELETE FROM automation_events WHERE id < (SELECT max(id)-10000 FROM automation_events)",[]);
            if pi_memory > 2 * 1024 * 1024 * 1024 {
                if let Some(run) = automation::current(&h) {
                    let generation =
                        h.db.query_row::<String, _, _>(
                            "SELECT generation_id FROM automation_runs WHERE id=?1",
                            [&run],
                            |r| r.get(0),
                        )
                        .ok();
                    if let Some(generation) = generation {
                        let _ = automation::stop_run(&mut h, &app, generation);
                        let _=h.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[&run]);
                        let _=h.db.execute("UPDATE automation_runs SET status='uncertain',finished_at=unixepoch(),error='Pi exceeded the 2 GiB memory limit. Review progress before resuming.' WHERE id=?1",[&run]);
                        automation::notify(
                            &app,
                            &run,
                            "Scheduled task stopped at its memory limit",
                        );
                    }
                }
            }
        }
    });
}

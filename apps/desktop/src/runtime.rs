//! Locates the packaged JavaScript runtime that runs Talo's stdio services.
//!
//! Bun is the runtime the installer ships. A vendored Node runtime is still accepted so a
//! checkout that predates the switch keeps working; `scripts/vendor.mjs` only downloads Bun.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

static INSTALLED_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The running app's vendor root. Debug builds and tests fall back to the repo's `vendor/`.
pub fn set_root(root: PathBuf) {
    let _ = INSTALLED_ROOT.set(root);
}

pub fn root() -> PathBuf {
    INSTALLED_ROOT
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor"))
}

const BUN: &str = "runtime/bun/bun.exe";
const NODE: &str = "runtime/node/node.exe";

pub struct Runtime {
    path: PathBuf,
    bun: bool,
}

impl Runtime {
    pub fn locate(root: &Path) -> Option<Self> {
        [BUN, NODE].into_iter().find_map(|relative| {
            let path = root.join(relative);
            path.is_file().then(|| Self { path, bun: relative == BUN })
        })
    }

    pub fn require(root: &Path) -> Result<Self, String> {
        Self::locate(root).ok_or_else(|| "Bundled runtime is missing; run bun run vendor".to_string())
    }

    /// A command that runs `script` on this runtime. A packaged app has no network, so Bun
    /// must never try to resolve a missing module by installing it.
    pub fn command(&self, script: &Path) -> Command {
        let mut command = Command::new(&self.path);
        if self.bun {
            command.arg("--no-install");
        }
        command.arg(script);
        command
    }

    /// The runtime executable, for services that need to launch it themselves.
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

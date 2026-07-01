// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::sync::Arc;

/// Remove each socket file, logging the outcome. Missing files are silently ignored.
fn remove_sockets(paths: &[PathBuf]) {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => log::info!("removed vsock socket: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("failed to remove vsock socket {}: {}", path.display(), e),
        }
    }
}

/// Guard that cleans up vsock socket files when dropped (normal exit) and
/// also installs a Ctrl-C handler so the sockets are removed on SIGINT.
pub struct VsockCleanupGuard {
    paths: Arc<Vec<PathBuf>>,
}

impl VsockCleanupGuard {
    /// Create the guard **and** register the SIGINT handler.
    pub fn new(paths: Vec<PathBuf>) -> Result<Self, anyhow::Error> {
        let paths = Arc::new(paths);
        let handler_paths = Arc::clone(&paths);
        ctrlc::set_handler(move || {
            remove_sockets(&handler_paths);
            std::process::exit(0);
        })?;
        Ok(Self { paths })
    }
}

impl Drop for VsockCleanupGuard {
    fn drop(&mut self) {
        remove_sockets(&self.paths);
    }
}

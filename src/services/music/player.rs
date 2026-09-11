use std::fs::OpenOptions;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::{Child, Command};

use crate::config::MusicPlayerConfig;

pub const SIGNAL_KILL: i32 = 9;
pub const SIGNAL_TERM: i32 = 15;
pub const SIGNAL_CONT: i32 = 18;
pub const SIGNAL_STOP: i32 = 19;

pub struct LocalPlayer {
    config: MusicPlayerConfig,
}

impl LocalPlayer {
    pub fn new(config: MusicPlayerConfig) -> Self {
        Self { config }
    }

    pub fn spawn(&self, url: &str) -> Result<Child> {
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.config.log_path)
            .with_context(|| format!("failed to open player log {}", self.config.log_path))?;
        let stderr = log.try_clone()?;
        Command::new(&self.config.path)
            .arg("-f")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to start {}", self.config.path))
    }

    pub fn signal(pid: u32, signal: i32) -> Result<()> {
        #[cfg(target_family = "unix")]
        {
            let rc = unsafe { libc::kill(pid as i32, signal) };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error().into())
            }
        }
        #[cfg(not(target_family = "unix"))]
        {
            let _ = (pid, signal);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "process signals require Unix",
            )
            .into())
        }
    }
}

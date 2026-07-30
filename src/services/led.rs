use std::time::Duration;

use tokio::task::JoinHandle;

use crate::config::LedConfig;
use crate::services::media::{MediaBus, MusicLedState};

pub struct LedController {
    task: JoinHandle<()>,
}

impl LedController {
    pub fn start(config: LedConfig, media: MediaBus) -> Option<Self> {
        if !config.enabled {
            println!("[led] disabled by config");
            return None;
        }
        let task = tokio::spawn(run(config, media));
        Some(Self { task })
    }
}

impl Drop for LedController {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(config: LedConfig, media: MediaBus) {
    let hz = config.update_hz.clamp(1, 20);
    let mut interval = tokio::time::interval(Duration::from_millis(1_000 / u64::from(hz)));
    let mut smoothed_db = config.min_db;
    let mut last_level = None;
    let mut airplay_was_active = false;
    let mut last_music_state = None;

    loop {
        interval.tick().await;
        if config.suspend_during_wakeup && media.wake_active() {
            last_level = None;
            continue;
        }

        let airplay_active = media.airplay_active();
        if airplay_active && !config.level_commands.is_empty() {
            let raw_db = media.airplay_level_db().clamp(config.min_db, config.max_db);
            let factor = if raw_db > smoothed_db {
                config.attack
            } else {
                config.release
            }
            .clamp(0.0, 1.0);
            smoothed_db += (raw_db - smoothed_db) * factor;
            let span = (config.max_db - config.min_db).max(1.0);
            let normalized = ((smoothed_db - config.min_db) / span).clamp(0.0, 1.0);
            let level = (normalized * (config.level_commands.len() - 1) as f32).round() as usize;
            if last_level != Some(level) {
                run_command(&config.level_commands[level]).await;
                last_level = Some(level);
            }
            airplay_was_active = true;
            continue;
        }

        if airplay_was_active {
            run_command(&config.off_command).await;
            airplay_was_active = false;
            last_level = None;
            last_music_state = None;
        }

        let music_state = media.music_state();
        if last_music_state != Some(music_state) {
            let command = match music_state {
                MusicLedState::Playing => &config.music_start_command,
                MusicLedState::Paused => &config.music_pause_command,
                MusicLedState::Idle => &config.music_stop_command,
            };
            run_command(command).await;
            last_music_state = Some(music_state);
        }
    }
}

async fn run_command(script: &str) {
    if script.trim().is_empty() {
        return;
    }
    match tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[led] command exited with {status}"),
        Err(err) => eprintln!("[led] command failed: {err}"),
    }
}

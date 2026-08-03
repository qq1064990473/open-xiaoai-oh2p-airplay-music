use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;

use crate::config::{LedConfig, NativeEventsConfig};
use crate::services::media::{MediaBus, MusicLedState};
use crate::services::music::{MusicCommand, MusicService};

pub struct NativeEventService {
    task: JoinHandle<()>,
}

impl NativeEventService {
    pub fn start(
        config: NativeEventsConfig,
        led: LedConfig,
        native_stop_command: String,
        music: MusicService,
        media: MediaBus,
    ) -> Option<Self> {
        if !config.enabled {
            println!("[native-events] disabled by config");
            return None;
        }
        if config.monitor_command.trim().is_empty() {
            eprintln!("[native-events] monitor_command is empty; service disabled");
            return None;
        }

        let task = tokio::spawn(run(config, led, native_stop_command, music, media));
        Some(Self { task })
    }
}

impl Drop for NativeEventService {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeSignal {
    PlayKeySound,
    CommonToggle,
    LedShut,
    Other,
}

async fn run(
    config: NativeEventsConfig,
    led: LedConfig,
    native_stop_command: String,
    music: MusicService,
    media: MediaBus,
) {
    loop {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(&config.monitor_command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("[native-events] failed to start monitor: {err}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let Some(stdout) = child.stdout.take() else {
            eprintln!("[native-events] monitor stdout is unavailable");
            let _ = child.kill().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };

        println!("[native-events] monitoring OH2P touchpad and LED UBUS traffic");
        let mut lines = BufReader::new(stdout).lines();
        let mut play_key_sound_at = None;
        let mut last_button_at = None;
        let mut last_led_restore_at = None;

        while let Ok(Some(line)) = lines.next_line().await {
            let now = Instant::now();
            match classify_line(&line, config.led_effect_id, config.led_restore_on_any_shut) {
                NativeSignal::PlayKeySound => play_key_sound_at = Some(now),
                NativeSignal::CommonToggle => {
                    let follows_touchpad_sound = play_key_sound_at.is_some_and(|at| {
                        now.duration_since(at)
                            <= Duration::from_millis(config.button_sequence_window_ms)
                    });
                    play_key_sound_at = None;
                    let debounced = last_button_at.is_some_and(|at| {
                        now.duration_since(at) < Duration::from_millis(config.button_debounce_ms)
                    });
                    if !follows_touchpad_sound || debounced {
                        continue;
                    }

                    let action = match media.music_state() {
                        MusicLedState::Playing => Some(MusicCommand::Pause),
                        MusicLedState::Paused => Some(MusicCommand::Resume),
                        MusicLedState::Idle => None,
                    };
                    if let Some(action) = action {
                        println!("[native-events] hardware play key -> {action:?}");
                        music.execute(action);
                        quench_native_player(
                            native_stop_command.clone(),
                            config.native_quench_delay_ms,
                        );
                        last_button_at = Some(now);
                    }
                }
                NativeSignal::LedShut => {
                    let debounced = last_led_restore_at
                        .is_some_and(|at| now.duration_since(at) < Duration::from_millis(200));
                    if debounced || media.music_state() != MusicLedState::Playing {
                        continue;
                    }
                    last_led_restore_at = Some(now);
                    restore_music_led(
                        led.music_start_command.clone(),
                        config.led_restore_delay_ms,
                        media.clone(),
                    );
                }
                NativeSignal::Other => {}
            }
        }

        let status = child.wait().await;
        eprintln!("[native-events] monitor ended ({status:?}); restarting");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn classify_line(line: &str, led_effect_id: u32, restore_on_any_led_shut: bool) -> NativeSignal {
    if line.contains("\"method\":\"play\"") && line.contains("key_prev_next.opus") {
        NativeSignal::PlayKeySound
    } else if line.contains("\"method\":\"player_play_operation\"")
        && line.contains("\"action\":\"toggle\"")
        && line.contains("\"media\":\"common\"")
    {
        NativeSignal::CommonToggle
    } else if line.contains("\"method\":\"shut\"")
        && (restore_on_any_led_shut || line.contains(&format!("\"L\":{led_effect_id}")))
    {
        NativeSignal::LedShut
    } else {
        NativeSignal::Other
    }
}

fn quench_native_player(command: String, delay_ms: u64) {
    if command.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        run_command("native player cleanup", &command).await;
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        run_command("delayed native player cleanup", &command).await;
    });
}

fn restore_music_led(command: String, delay_ms: u64, media: MediaBus) {
    if command.trim().is_empty() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        if media.music_state() == MusicLedState::Playing && !media.airplay_active() {
            println!("[native-events] restoring music LED after native shut");
            run_command("music LED restore", &command).await;
        }
    });
}

async fn run_command(label: &str, script: &str) {
    match Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("[native-events] {label} exited with {status}"),
        Err(err) => eprintln!("[native-events] {label} failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_line, NativeSignal};

    #[test]
    fn classifies_oh2p_touchpad_and_led_messages() {
        assert_eq!(
            classify_line(
                r#"<- ab81bef1 #bb317333 invoke: {"method":"play","data":{"play":"\/usr\/share\/common_sound\/key_prev_next.opus"}}"#,
                14,
                true
            ),
            NativeSignal::PlayKeySound
        );
        assert_eq!(
            classify_line(
                r#"<- 0e098a1f #3ff96d1a invoke: {"method":"player_play_operation","data":{"action":"toggle","media":"common"}}"#,
                14,
                true
            ),
            NativeSignal::CommonToggle
        );
        assert_eq!(
            classify_line(
                r#"<- 2110d776 #f050681e invoke: {"method":"shut","data":{"L":14}}"#,
                14,
                true
            ),
            NativeSignal::LedShut
        );
        assert_eq!(
            classify_line(
                r#"<- 2110d776 #f050681e invoke: {"method":"shut","data":{"L":4}}"#,
                14,
                true
            ),
            NativeSignal::LedShut
        );
        assert_eq!(
            classify_line(
                r#"<- 2110d776 #f050681e invoke: {"method":"shut","data":{"L":4}}"#,
                14,
                false
            ),
            NativeSignal::Other
        );
    }
}

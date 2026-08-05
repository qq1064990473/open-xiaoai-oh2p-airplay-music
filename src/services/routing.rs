use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::base::AppError;
use crate::services::home_assistant::{ask_xiaoai, speak_text, HomeAssistantService, RouteOutcome};
use crate::services::media::MediaBus;
use crate::services::monitor::file::FileMonitorEvent;
use crate::services::music::{MusicCommand, MusicCommandParser, MusicService};

#[derive(Clone)]
pub struct RoutingService {
    music: MusicService,
    parser: MusicCommandParser,
    media: MediaBus,
    native_stop_command: String,
    home_assistant: Option<HomeAssistantService>,
    dialogs: Arc<Mutex<DialogCache>>,
}

#[derive(Default)]
struct DialogCache {
    states: HashMap<String, DialogState>,
    order: VecDeque<String>,
}

#[derive(Default)]
struct DialogState {
    routed_asr: bool,
    claimed_music: bool,
    external_pending: bool,
}

impl RoutingService {
    pub fn new(
        music: MusicService,
        parser: MusicCommandParser,
        media: MediaBus,
        native_stop_command: String,
        home_assistant: Option<HomeAssistantService>,
    ) -> Self {
        Self {
            music,
            parser,
            media,
            native_stop_command,
            home_assistant,
            dialogs: Arc::new(Mutex::new(DialogCache::default())),
        }
    }

    pub async fn process(&self, event: &FileMonitorEvent) -> Result<(), AppError> {
        let FileMonitorEvent::NewLine(line) = event else {
            self.dialogs.lock().expect("dialog cache poisoned").clear();
            return Ok(());
        };
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        let header = &message["header"];
        let namespace = header["namespace"].as_str().unwrap_or("");
        let name = header["name"].as_str().unwrap_or("");
        let dialog_id = header["dialog_id"].as_str().unwrap_or("");
        if dialog_id.is_empty() {
            return Ok(());
        }

        if namespace == "SpeechRecognizer" && name == "RecognizeResult" {
            self.media.set_wake_active(true);
            self.music.wake_started();
            if let Some(text) = final_asr_text(&message) {
                let already_routed = {
                    let mut cache = self.dialogs.lock().expect("dialog cache poisoned");
                    let state = cache.state_mut(dialog_id);
                    let routed = state.routed_asr;
                    state.routed_asr = true;
                    routed
                };
                if !already_routed {
                    if let Some(command) = self.parser.parse(&text, self.music.is_active()) {
                        {
                            let mut cache = self.dialogs.lock().expect("dialog cache poisoned");
                            cache.state_mut(dialog_id).claimed_music = true;
                        }
                        println!("[routing] claimed music ASR: {text:?} -> {command:?}");
                        // Stop the native path for both queue creation and controls.
                        // A native PlaybackController directive may otherwise advance
                        // Xiaomi's queue after the local command has already run.
                        self.stop_native_music();
                        self.music.execute(command);
                    } else if self
                        .home_assistant
                        .as_ref()
                        .is_some_and(HomeAssistantService::routing_enabled)
                    {
                        self.dialogs
                            .lock()
                            .expect("dialog cache poisoned")
                            .state_mut(dialog_id)
                            .external_pending = true;
                        println!("[routing] sending ASR to Home Assistant: {text:?}");
                        self.route_home_assistant(text);
                    }
                }
            }
            return Ok(());
        }

        if namespace == "Dialog" && name == "Finish" {
            let external_pending = self
                .dialogs
                .lock()
                .expect("dialog cache poisoned")
                .states
                .get(dialog_id)
                .is_some_and(|state| state.external_pending);
            if external_pending {
                println!("[routing] deferring wake completion for external route");
                return Ok(());
            }
            self.media.set_wake_active(false);
            self.music.dialog_finished();
            return Ok(());
        }

        let claimed = self
            .dialogs
            .lock()
            .expect("dialog cache poisoned")
            .states
            .get(dialog_id)
            .is_some_and(|state| state.claimed_music);
        if claimed
            && namespace == "AudioPlayer"
            && name == "Play"
            && message["payload"]["audio_type"].as_str() == Some("MUSIC")
        {
            println!("[routing] stopping late native AudioPlayer.Play for claimed dialog");
            self.stop_native_music();
            return Ok(());
        }

        if claimed && namespace == "SpeechSynthesizer" && matches!(name, "Speak" | "SpeakStream") {
            println!("[routing] stopping late native speech for claimed music dialog");
            self.stop_native_music();
            return Ok(());
        }

        if claimed && namespace == "PlaybackController" {
            println!("[routing] suppressing claimed native playback directive: {name}");
            self.stop_native_music();
            return Ok(());
        }
        if self.music.is_active() && namespace == "PlaybackController" {
            let command = playback_command(name);
            if let Some(command) = command {
                {
                    let mut cache = self.dialogs.lock().expect("dialog cache poisoned");
                    cache.state_mut(dialog_id).claimed_music = true;
                }
                println!("[routing] mapped native playback directive: {name}");
                self.stop_native_music();
                self.music.execute(command);
            }
        }
        Ok(())
    }

    fn stop_native_music(&self) {
        let script = self.native_stop_command.trim().to_string();
        if script.is_empty() {
            return;
        }
        tokio::spawn(async move {
            match tokio::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
            {
                Ok(status) if status.success() => {}
                Ok(status) => eprintln!("[routing] native stop command exited with {status}"),
                Err(err) => eprintln!("[routing] native stop command failed: {err}"),
            }
        });
    }

    fn route_home_assistant(&self, text: String) {
        let Some(home_assistant) = self.home_assistant.clone() else {
            return;
        };
        let media = self.media.clone();
        let music = self.music.clone();
        tokio::spawn(async move {
            let awaiting_followup_dialog = match home_assistant.route(&text).await {
                RouteOutcome::Handled { speech } => {
                    println!("[ha] handled: {text:?}");
                    if home_assistant.speak_response() {
                        if let Some(speech) = speech {
                            start_native_tts(&speech).await
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                RouteOutcome::Fallback { reason } => {
                    eprintln!("[ha] falling back to XiaoAI: {reason}");
                    if home_assistant.fallback_to_xiaoai() {
                        start_xiaoai_fallback(&text).await
                    } else {
                        false
                    }
                }
                RouteOutcome::Failed { reason } => {
                    eprintln!("[ha] request not retried through XiaoAI: {reason}");
                    let speech = home_assistant.failure_speech();
                    if speech.is_empty() {
                        false
                    } else {
                        start_native_tts(speech).await
                    }
                }
            };
            if !awaiting_followup_dialog {
                media.set_wake_active(false);
                music.dialog_finished();
            }
        });
    }
}

async fn start_xiaoai_fallback(text: &str) -> bool {
    match ask_xiaoai(text).await {
        Ok(true) => true,
        Ok(false) => {
            eprintln!("[ha] XiaoAI fallback was rejected");
            false
        }
        Err(err) => {
            eprintln!("[ha] XiaoAI fallback failed: {err:#}");
            false
        }
    }
}

async fn start_native_tts(text: &str) -> bool {
    match speak_text(text).await {
        Ok(true) => true,
        Ok(false) => {
            eprintln!("[ha] native TTS was rejected");
            false
        }
        Err(err) => {
            eprintln!("[ha] native TTS failed: {err:#}");
            false
        }
    }
}

fn playback_command(name: &str) -> Option<MusicCommand> {
    match name {
        "Next" => Some(MusicCommand::Next),
        "Previous" => Some(MusicCommand::Previous),
        "Pause" => Some(MusicCommand::Pause),
        "Continue" | "Resume" | "Play" => Some(MusicCommand::Resume),
        "Stop" => Some(MusicCommand::Stop),
        _ => None,
    }
}

impl DialogCache {
    fn state_mut(&mut self, dialog_id: &str) -> &mut DialogState {
        if !self.states.contains_key(dialog_id) {
            self.order.push_back(dialog_id.to_string());
            self.states
                .insert(dialog_id.to_string(), DialogState::default());
            while self.order.len() > 64 {
                if let Some(old) = self.order.pop_front() {
                    self.states.remove(&old);
                }
            }
        }
        self.states.get_mut(dialog_id).expect("state inserted")
    }

    fn clear(&mut self) {
        self.states.clear();
        self.order.clear();
    }
}

fn final_asr_text(message: &Value) -> Option<String> {
    let payload = &message["payload"];
    let results = payload["results"].as_array()?;
    let exact = results.iter().find(|result| {
        result["is_nlp_request"].as_bool() == Some(true)
            && result["is_stop"].as_bool() == Some(true)
    });
    let fallback = (payload["is_final"].as_bool() == Some(true))
        .then(|| results.first())
        .flatten();
    exact.or(fallback)?["text"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{final_asr_text, playback_command};
    use crate::services::music::MusicCommand;
    use serde_json::json;

    #[test]
    fn selects_stop_nlp_result_and_final_fallback() {
        let message = json!({"payload":{"is_final":false,"results":[
            {"text":"播放晴天","is_nlp_request":true,"is_stop":true}
        ]}});
        assert_eq!(final_asr_text(&message).as_deref(), Some("播放晴天"));
        let fallback = json!({"payload":{"is_final":true,"results":[{"text":"下一首"}]}});
        assert_eq!(final_asr_text(&fallback).as_deref(), Some("下一首"));
    }

    #[test]
    fn maps_native_playback_controls() {
        assert_eq!(playback_command("Next"), Some(MusicCommand::Next));
        assert_eq!(playback_command("Previous"), Some(MusicCommand::Previous));
        assert_eq!(playback_command("Pause"), Some(MusicCommand::Pause));
        assert_eq!(playback_command("Continue"), Some(MusicCommand::Resume));
        assert_eq!(playback_command("Unknown"), None);
    }
}

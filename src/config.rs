use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_CONFIG_PATH: &str = "/data/open-xiaoai/client.json";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub server_url: String,
    pub airplay: AirPlayConfig,
    /// Reserved for the later deterministic music implementation.
    /// The current AirPlay build only warns when this flag is enabled.
    pub music: MusicConfig,
    pub audio_policy: AudioPolicyConfig,
    pub led: LedConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://127.0.0.1:4399".to_string(),
            airplay: AirPlayConfig::default(),
            music: MusicConfig::default(),
            audio_policy: AudioPolicyConfig::default(),
            led: LedConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("failed to parse JSON config: {}", path.display()))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AirPlayConfig {
    pub enabled: bool,
    pub name: String,
    pub port: u16,
    pub password: String,
    /// Stable, locally administered MAC-like identifier. Example: 02:4f:48:32:50:01.
    /// Leaving it empty makes shairplay generate a new identifier on every start.
    pub hwaddr: String,
    pub max_clients: usize,
    pub output: AirPlayOutputConfig,
    pub interruption: AirPlayInterruptionConfig,
}

impl Default for AirPlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "XiaoAI AirPlay".to_string(),
            port: 5000,
            password: String::new(),
            hwaddr: String::new(),
            max_clients: 1,
            output: AirPlayOutputConfig::default(),
            interruption: AirPlayInterruptionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AirPlayOutputConfig {
    pub backend: String,
    pub aplay_path: String,
    pub device: String,
    pub format: String,
    pub extra_args: Vec<String>,
}

impl Default for AirPlayOutputConfig {
    fn default() -> Self {
        Self {
            backend: "aplay".to_string(),
            aplay_path: "/usr/bin/aplay".to_string(),
            device: "default".to_string(),
            format: "S16_LE".to_string(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AirPlayInterruptionConfig {
    pub mode: String,
    pub duck_gain: f32,
}

impl Default for AirPlayInterruptionConfig {
    fn default() -> Self {
        Self {
            mode: "duck".into(),
            duck_gain: 0.25,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicConfig {
    pub enabled: bool,
    pub route_mode: String,
    pub commands: MusicCommandConfig,
    pub search: MusicSearchConfig,
    pub play_url: MusicPlayUrlConfig,
    pub player: MusicPlayerConfig,
    pub queue: MusicQueueConfig,
    pub interruption: MusicInterruptionConfig,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            route_mode: "fixed_rules".to_string(),
            commands: MusicCommandConfig::default(),
            search: MusicSearchConfig::default(),
            play_url: MusicPlayUrlConfig::default(),
            player: MusicPlayerConfig::default(),
            queue: MusicQueueConfig::default(),
            interruption: MusicInterruptionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicCommandConfig {
    pub required_prefixes: Vec<String>,
    pub explicit_prefixes: Vec<String>,
    pub play_words: Vec<String>,
    pub singer_words: Vec<String>,
    pub playlist_words: Vec<String>,
    pub pause_words: Vec<String>,
    pub resume_words: Vec<String>,
    pub next_words: Vec<String>,
    pub previous_words: Vec<String>,
    pub stop_words: Vec<String>,
    pub shuffle_words: Vec<String>,
    pub repeat_one_words: Vec<String>,
    pub repeat_all_words: Vec<String>,
}

impl Default for MusicCommandConfig {
    fn default() -> Self {
        Self {
            required_prefixes: Vec::new(),
            explicit_prefixes: vec!["本地音乐".into(), "播放歌曲".into(), "音乐".into()],
            play_words: vec!["来一首".into(), "我想听".into(), "播放".into(), "放".into()],
            singer_words: vec!["歌手".into(), "的歌".into()],
            playlist_words: vec!["歌单".into()],
            pause_words: vec!["暂停播放".into(), "暂停".into()],
            resume_words: vec!["继续播放".into(), "恢复播放".into()],
            next_words: vec!["下一首".into(), "换一首".into()],
            previous_words: vec!["上一首".into()],
            stop_words: vec!["停止播放".into(), "关闭音乐".into()],
            shuffle_words: vec!["随机播放".into(), "打乱播放".into(), "随便放点歌".into()],
            repeat_one_words: vec!["单曲循环".into()],
            repeat_all_words: vec!["列表循环".into()],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicSearchConfig {
    pub provider: String,
    pub api_url: String,
    pub page_size: usize,
    pub timeout_ms: u64,
    pub max_retries: usize,
    pub retry_delay_ms: u64,
    pub random_toplist_id: u64,
}

impl Default for MusicSearchConfig {
    fn default() -> Self {
        Self {
            provider: "qq_music".into(),
            api_url: "https://u.y.qq.com/cgi-bin/musicu.fcg".into(),
            page_size: 20,
            timeout_ms: 5_000,
            max_retries: 3,
            retry_delay_ms: 500,
            random_toplist_id: 4,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicPlayUrlConfig {
    pub primary_url_template: String,
    pub primary_json_path: String,
    pub primary_min_interval_ms: u64,
    pub primary_failure_cooldown_ms: u64,
    pub primary_cooldown_retries: usize,
    pub backup_enabled: bool,
    pub backup_url_template: String,
    pub backup_json_path: String,
    pub backup_headers: std::collections::HashMap<String, String>,
    pub cache_ttl_s: u64,
    pub cache_max_entries: usize,
}

impl Default for MusicPlayUrlConfig {
    fn default() -> Self {
        let backup_headers = std::collections::HashMap::new();
        Self {
            primary_url_template: "https://music.haitangw.cc/music/qq_song_kw.php?id={mid}".into(),
            primary_json_path: "data.url".into(),
            primary_min_interval_ms: 3_000,
            primary_failure_cooldown_ms: 60_000,
            primary_cooldown_retries: 2,
            backup_enabled: true,
            backup_url_template:
                "http://175.27.166.236/kgqq1/qq.php?id={mid}&type=json&level=exhigh".into(),
            backup_json_path: "data.url".into(),
            backup_headers,
            cache_ttl_s: 1_800,
            cache_max_entries: 128,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicPlayerConfig {
    pub path: String,
    pub log_path: String,
    pub start_timeout_ms: u64,
    pub stop_timeout_ms: u64,
    pub unexpected_exit_retries: usize,
    pub max_consecutive_failures: usize,
    pub native_stop_command: String,
}

impl Default for MusicPlayerConfig {
    fn default() -> Self {
        Self {
            path: "/usr/bin/miplayer".into(),
            log_path: "/tmp/open-xiaoai-music-player.log".into(),
            start_timeout_ms: 1_500, stop_timeout_ms: 1_000,
            unexpected_exit_retries: 1, max_consecutive_failures: 3,
            native_stop_command: "killall tts_play.sh 2>/dev/null || true; mphelper pause 2>/dev/null || true; ubus call mediaplayer media_control '{\"player\":\"mediaplayer\",\"action\":\"pause\",\"volume\":0}' >/dev/null 2>&1 || true; ubus call mediaplayer player_play_operation '{\"media\":\"app_ios\",\"action\":\"stop\"}' >/dev/null 2>&1 || true".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicQueueConfig {
    pub autoplay_count: usize,
    pub after_single_song: String,
    pub default_order: String,
    pub repeat_mode: String,
    pub deduplicate_by: String,
    pub on_url_failure: String,
}

impl Default for MusicQueueConfig {
    fn default() -> Self {
        Self {
            autoplay_count: 10,
            after_single_song: "same_artist".into(),
            default_order: "sequential".into(),
            repeat_mode: "off".into(),
            deduplicate_by: "mid".into(),
            on_url_failure: "skip_to_next".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MusicInterruptionConfig {
    pub on_wake: String,
    pub resume_on_dialog_finish: bool,
}

impl Default for MusicInterruptionConfig {
    fn default() -> Self {
        Self {
            on_wake: "pause".into(),
            resume_on_dialog_finish: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AudioPolicyConfig {
    pub on_airplay_start: String,
    pub on_airplay_end: String,
    pub music_while_airplay: String,
}

impl Default for AudioPolicyConfig {
    fn default() -> Self {
        Self {
            on_airplay_start: "pause_music".into(),
            on_airplay_end: "resume_music".into(),
            music_while_airplay: "reject".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LedConfig {
    pub enabled: bool,
    pub update_hz: u32,
    pub min_db: f32,
    pub max_db: f32,
    pub attack: f32,
    pub release: f32,
    pub level_commands: Vec<String>,
    pub off_command: String,
    pub music_start_command: String,
    pub music_pause_command: String,
    pub music_stop_command: String,
    pub suspend_during_wakeup: bool,
}

impl Default for LedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            update_hz: 10,
            min_db: -48.0,
            max_db: -8.0,
            attack: 0.45,
            release: 0.15,
            level_commands: Vec::new(),
            off_command: String::new(),
            music_start_command: String::new(),
            music_pause_command: String::new(),
            music_stop_command: String::new(),
            suspend_during_wakeup: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn missing_fields_use_safe_defaults() {
        let config: AppConfig = serde_json::from_str(r#"{"airplay":{"enabled":true}}"#).unwrap();
        assert!(config.airplay.enabled);
        assert_eq!(config.airplay.port, 5000);
        assert_eq!(config.airplay.output.device, "default");
        assert!(!config.music.enabled);
        assert_eq!(config.music.search.max_retries, 3);
        assert!(config.music.play_url.backup_enabled);
        assert_eq!(config.music.play_url.backup_json_path, "data.url");
        assert_eq!(config.music.play_url.primary_failure_cooldown_ms, 60_000);
        assert_eq!(config.music.play_url.primary_cooldown_retries, 2);
        assert!(!config.led.enabled);
    }
}

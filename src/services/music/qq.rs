use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::config::{MusicPlayUrlConfig, MusicSearchConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Song {
    pub mid: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct QqMusic {
    client: Client,
    search: MusicSearchConfig,
    play_url: MusicPlayUrlConfig,
    play_url_state: Arc<Mutex<PlayUrlState>>,
}

#[derive(Debug, Default)]
struct PlayUrlState {
    last_primary_attempt: Option<Instant>,
    primary_blocked_until: Option<Instant>,
    cache: HashMap<String, CachedUrl>,
}

#[derive(Debug, Clone)]
struct CachedUrl {
    url: String,
    expires_at: Instant,
}

impl QqMusic {
    pub fn new(search: MusicSearchConfig, play_url: MusicPlayUrlConfig) -> Result<Self> {
        anyhow::ensure!(
            search.provider == "qq_music" || search.provider == "qq_music_musicu",
            "unsupported music search provider: {}",
            search.provider
        );
        let client = Client::builder()
            .user_agent("Mozilla/5.0 open-xiaoai-client")
            .timeout(Duration::from_millis(search.timeout_ms.max(100)))
            .build()?;
        Ok(Self {
            client,
            search,
            play_url,
            play_url_state: Arc::new(Mutex::new(PlayUrlState::default())),
        })
    }

    pub async fn search_songs(&self, query: &str, page: usize, limit: usize) -> Result<Vec<Song>> {
        let started = Instant::now();
        let body = search_body(query, 0, page, limit);
        let data = self.post_musicu(body).await?;
        let items = data
            .pointer("/music.search.SearchCgiService/data/body/song/list")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let songs = items
            .iter()
            .map(parse_song)
            .filter(|song| !song.mid.is_empty())
            .collect::<Vec<_>>();
        println!(
            "[music-api] QQ song search: query={query:?}, page={page}, results={}, elapsed_ms={}",
            songs.len(),
            started.elapsed().as_millis()
        );
        Ok(songs)
    }

    pub async fn search_playlist(&self, query: &str) -> Result<Option<(String, String)>> {
        let data = self.post_musicu(search_body(query, 3, 1, 10)).await?;
        let item = data
            .pointer("/music.search.SearchCgiService/data/body/songlist/list")
            .and_then(Value::as_array)
            .and_then(|items| items.first());
        Ok(item.map(|item| {
            (
                value_string(&item["dissid"]),
                value_string(&item["dissname"]),
            )
        }))
    }

    pub async fn playlist_songs(&self, dissid: &str, limit: usize) -> Result<Vec<Song>> {
        let id = dissid
            .parse::<u64>()
            .context("QQ playlist id is not numeric")?;
        let body = json!({
            "req_1": {
                "module": "music.srfDissInfo.aiDissInfo",
                "method": "uniform_get_Dissinfo",
                "param": {"disstid": id, "onlysonglist": 1, "song_begin": 0, "song_num": limit.clamp(1, 100)}
            }
        });
        let data = self.post_musicu(body).await?;
        let items = data
            .pointer("/req_1/data/songlist")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .map(parse_song)
            .filter(|song| !song.mid.is_empty())
            .collect())
    }

    pub async fn random_songs(&self, limit: usize) -> Result<Vec<Song>> {
        let count = limit.saturating_mul(3).clamp(20, 100);
        let body = json!({
            "req_1": {
                "module": "musicToplist.ToplistInfoServer", "method": "GetDetail",
                "param": {"topid": self.search.random_toplist_id, "offset": 0, "num": count, "period": ""}
            }
        });
        let data = self.post_musicu(body).await?;
        let items = data
            .pointer("/req_1/data/songInfoList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut songs: Vec<_> = items
            .iter()
            .map(parse_song)
            .filter(|song| !song.mid.is_empty())
            .collect();
        songs.shuffle(&mut rand::rng());
        songs.truncate(limit);
        Ok(songs)
    }

    pub async fn resolve_url(&self, mid: &str) -> Result<String> {
        if let Some(url) = self.cached_url(mid) {
            println!("[music-api] play URL cache hit: mid={mid}");
            return Ok(url);
        }

        let empty_headers = HashMap::new();
        match self.primary_gate().await {
            PrimaryGate::Attempt => {
                let started = Instant::now();
                match self
                    .resolve_from(
                        &self.play_url.primary_url_template,
                        &self.play_url.primary_json_path,
                        &empty_headers,
                        mid,
                    )
                    .await
                {
                    Ok(url) => {
                        self.primary_succeeded();
                        self.cache_url(mid, &url);
                        println!(
                            "[music-api] primary URL resolved: mid={mid}, elapsed_ms={}",
                            started.elapsed().as_millis()
                        );
                        Ok(url)
                    }
                    Err(primary) => {
                        self.primary_failed();
                        eprintln!(
                            "[music-api] primary URL failed; cooldown_ms={}: {primary:#}",
                            self.play_url.primary_failure_cooldown_ms
                        );
                        self.resolve_backup(
                            mid,
                            Some(primary),
                            Duration::from_millis(self.play_url.primary_failure_cooldown_ms),
                        )
                        .await
                    }
                }
            }
            PrimaryGate::CoolingDown(remaining) => {
                println!(
                    "[music-api] primary URL skipped during cooldown: mid={mid}, remaining_ms={}",
                    remaining.as_millis()
                );
                self.resolve_backup(mid, None, remaining).await
            }
        }
    }

    async fn resolve_backup(
        &self,
        mid: &str,
        primary_error: Option<anyhow::Error>,
        retry_after: Duration,
    ) -> Result<String> {
        if !self.play_url.backup_enabled {
            let detail = primary_error.map_or_else(
                || "primary play URL is cooling down".to_string(),
                |err| format!("primary play URL failed: {err:#}"),
            );
            return Err(PlayUrlCooldown {
                retry_after,
                detail,
            }
            .into());
        }

        let started = Instant::now();
        let result = self
            .resolve_from(
                &self.play_url.backup_url_template,
                &self.play_url.backup_json_path,
                &self.play_url.backup_headers,
                mid,
            )
            .await;
        match result {
            Ok(url) => {
                self.cache_url(mid, &url);
                println!(
                    "[music-api] backup URL resolved: mid={mid}, elapsed_ms={}",
                    started.elapsed().as_millis()
                );
                Ok(url)
            }
            Err(backup) => {
                let detail = primary_error.map_or_else(
                    || format!("primary play URL is cooling down; backup failed: {backup:#}"),
                    |primary| {
                        format!("primary play URL failed: {primary:#}; backup failed: {backup:#}")
                    },
                );
                Err(PlayUrlCooldown {
                    retry_after,
                    detail,
                }
                .into())
            }
        }
    }

    async fn primary_gate(&self) -> PrimaryGate {
        let wait = {
            let mut state = self.play_url_state.lock().expect("play URL state poisoned");
            let now = Instant::now();
            if let Some(until) = state.primary_blocked_until {
                if until > now {
                    return PrimaryGate::CoolingDown(until.duration_since(now));
                }
                state.primary_blocked_until = None;
            }
            state.last_primary_attempt.and_then(|last| {
                let next = last + Duration::from_millis(self.play_url.primary_min_interval_ms);
                (next > now).then(|| next.duration_since(now))
            })
        };

        if let Some(wait) = wait {
            println!(
                "[music-api] rate limiting primary URL request: wait_ms={}",
                wait.as_millis()
            );
            sleep(wait).await;
        }
        self.play_url_state
            .lock()
            .expect("play URL state poisoned")
            .last_primary_attempt = Some(Instant::now());
        PrimaryGate::Attempt
    }

    fn primary_succeeded(&self) {
        self.play_url_state
            .lock()
            .expect("play URL state poisoned")
            .primary_blocked_until = None;
    }

    fn primary_failed(&self) {
        self.play_url_state
            .lock()
            .expect("play URL state poisoned")
            .primary_blocked_until =
            Some(Instant::now() + Duration::from_millis(self.play_url.primary_failure_cooldown_ms));
    }

    fn cached_url(&self, mid: &str) -> Option<String> {
        let mut state = self.play_url_state.lock().expect("play URL state poisoned");
        let now = Instant::now();
        match state.cache.get(mid) {
            Some(cached) if cached.expires_at > now => Some(cached.url.clone()),
            Some(_) => {
                state.cache.remove(mid);
                None
            }
            None => None,
        }
    }

    fn cache_url(&self, mid: &str, url: &str) {
        if self.play_url.cache_ttl_s == 0 || self.play_url.cache_max_entries == 0 {
            return;
        }
        let mut state = self.play_url_state.lock().expect("play URL state poisoned");
        let now = Instant::now();
        state.cache.retain(|_, value| value.expires_at > now);
        if state.cache.len() >= self.play_url.cache_max_entries {
            if let Some(key) = state.cache.keys().next().cloned() {
                state.cache.remove(&key);
            }
        }
        state.cache.insert(
            mid.to_string(),
            CachedUrl {
                url: url.to_string(),
                expires_at: now + Duration::from_secs(self.play_url.cache_ttl_s),
            },
        );
    }

    pub fn invalidate_url(&self, mid: &str) {
        if self
            .play_url_state
            .lock()
            .expect("play URL state poisoned")
            .cache
            .remove(mid)
            .is_some()
        {
            println!("[music-api] invalidated cached play URL: mid={mid}");
        }
    }

    pub fn retry_after(error: &anyhow::Error) -> Option<Duration> {
        error
            .downcast_ref::<PlayUrlCooldown>()
            .map(|error| error.retry_after)
    }

    async fn resolve_from(
        &self,
        template: &str,
        json_path: &str,
        headers: &std::collections::HashMap<String, String>,
        mid: &str,
    ) -> Result<String> {
        let endpoint = template.replace("{mid}", mid);
        let mut request = self.client.get(&endpoint);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let data = request
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let value = json_dot_path(&data, json_path)
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
            .context("play interface returned no URL")?;
        let url = Url::parse(value).context("play interface returned an invalid URL")?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https"),
            "unsupported play URL scheme"
        );
        Ok(url.to_string())
    }

    async fn post_musicu(&self, body: Value) -> Result<Value> {
        let attempts = self.search.max_retries.max(1);
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 1..=attempts {
            match self
                .client
                .post(&self.search.api_url)
                .json(&body)
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => match response.json::<Value>().await {
                        Ok(data) => return Ok(data),
                        Err(err) => last_error = Some(err.into()),
                    },
                    Err(err) => last_error = Some(err.into()),
                },
                Err(err) => last_error = Some(err.into()),
            }
            if attempt < attempts {
                sleep(Duration::from_millis(self.search.retry_delay_ms)).await;
            }
        }
        Err(last_error.context("QQ Music request failed without an error")?)
    }
}

enum PrimaryGate {
    Attempt,
    CoolingDown(Duration),
}

#[derive(Debug)]
struct PlayUrlCooldown {
    retry_after: Duration,
    detail: String,
}

impl std::fmt::Display for PlayUrlCooldown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; retry_after_ms={}",
            self.detail,
            self.retry_after.as_millis()
        )
    }
}

impl std::error::Error for PlayUrlCooldown {}

fn search_body(query: &str, search_type: u8, page: usize, limit: usize) -> Value {
    json!({"music.search.SearchCgiService": {
        "method": "DoSearchForQQMusicDesktop", "module": "music.search.SearchCgiService",
        "param": {"num_per_page": limit.clamp(1, 20), "page_num": page.max(1), "query": query, "search_type": search_type}
    }})
}

fn parse_song(item: &Value) -> Song {
    Song {
        mid: value_string(&item["mid"]),
        title: value_string(&item["title"]),
        artists: item
            .get("singer")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|artist| artist.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        album: item
            .pointer("/album/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        duration_ms: item
            .get("interval")
            .and_then(Value::as_u64)
            .map(|seconds| seconds * 1_000),
    }
}

fn value_string(value: &Value) -> String {
    value.as_str().map(str::to_string).unwrap_or_else(|| {
        if value.is_null() {
            String::new()
        } else {
            value.to_string().trim_matches('"').to_string()
        }
    })
}

fn json_dot_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|part| !part.is_empty())
        .try_fold(value, |current, part| current.get(part))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_qq_song_and_dot_path() {
        let song = parse_song(
            &json!({"mid":"abc","title":"晴天","singer":[{"name":"周杰伦"}],"album":{"name":"叶惠美"},"interval":269}),
        );
        assert_eq!(song.mid, "abc");
        assert_eq!(song.duration_ms, Some(269_000));
        assert_eq!(
            json_dot_path(&json!({"data":{"url":"x"}}), "data.url").and_then(Value::as_str),
            Some("x")
        );
    }

    #[test]
    fn caches_and_invalidates_play_urls() {
        let music = QqMusic::new(MusicSearchConfig::default(), MusicPlayUrlConfig::default())
            .expect("client");
        music.cache_url("mid-1", "https://example.com/song.mp3");
        assert_eq!(
            music.cached_url("mid-1").as_deref(),
            Some("https://example.com/song.mp3")
        );
        music.invalidate_url("mid-1");
        assert_eq!(music.cached_url("mid-1"), None);
    }
}

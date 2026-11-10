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
    random_state: Arc<Mutex<RandomState>>,
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

#[derive(Debug, Default)]
struct RandomState {
    kuwo_page: usize,
    kuwo_cycle: u64,
    kuwo_blocked_until: Option<Instant>,
    qq_offset: usize,
    qq_cycle: u64,
}

#[derive(Debug, Clone)]
pub struct RandomSongBatch {
    pub cycle_key: String,
    pub songs: Vec<Song>,
}

impl QqMusic {
    pub fn new(search: MusicSearchConfig, play_url: MusicPlayUrlConfig) -> Result<Self> {
        anyhow::ensure!(
            matches!(
                search.provider.as_str(),
                "kuwo" | "qq_music" | "qq_music_musicu"
            ),
            "unsupported music search provider: {}",
            search.provider
        );
        anyhow::ensure!(
            matches!(
                search.fallback_provider.as_str(),
                "" | "qq_music" | "qq_music_musicu"
            ),
            "unsupported music fallback provider: {}",
            search.fallback_provider
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
            random_state: Arc::new(Mutex::new(RandomState::default())),
        })
    }

    pub async fn search_songs(&self, query: &str, page: usize, limit: usize) -> Result<Vec<Song>> {
        if self.search.provider == "kuwo" {
            match self.search_kuwo_songs(query, page, limit).await {
                Ok(songs) if !songs.is_empty() => return Ok(songs),
                Ok(_) => eprintln!(
                    "[music-api] Kuwo song search returned no usable songs; query={query:?}; falling back to QQ"
                ),
                Err(err) => eprintln!(
                    "[music-api] Kuwo song search failed; query={query:?}; falling back to QQ: {err:#}"
                ),
            }
            anyhow::ensure!(
                matches!(
                    self.search.fallback_provider.as_str(),
                    "qq_music" | "qq_music_musicu"
                ),
                "Kuwo search returned no usable results and no QQ fallback provider is configured"
            );
        }
        self.search_qq_songs(query, page, limit).await
    }

    /// Search the configured QQ fallback directly when filtering the preferred
    /// source leaves no usable candidates (for example, singer-name matching).
    pub async fn search_qq_fallback_songs(
        &self,
        query: &str,
        page: usize,
        limit: usize,
    ) -> Result<Vec<Song>> {
        anyhow::ensure!(
            self.search.provider == "kuwo"
                && matches!(
                    self.search.fallback_provider.as_str(),
                    "qq_music" | "qq_music_musicu"
                ),
            "QQ fallback search is not configured"
        );
        self.search_qq_songs(query, page, limit).await
    }

    pub fn reset_random_cursor(&self) {
        *self.random_state.lock().expect("random state poisoned") = RandomState::default();
    }

    async fn search_kuwo_songs(&self, query: &str, page: usize, limit: usize) -> Result<Vec<Song>> {
        let started = Instant::now();
        let page = page.saturating_sub(1);
        let data = self
            .client
            .get(&self.search.kuwo_api_url)
            .query(&[
                ("client", "kt"),
                ("all", query),
                ("pn", &page.to_string()),
                ("rn", &limit.clamp(1, 50).to_string()),
                ("ft", "music"),
                ("uid", &self.search.kuwo_uid.to_string()),
                ("ver", &self.search.kuwo_version),
                ("vipver", "1"),
                ("cluster", "0"),
                ("strategy", "2012"),
                ("encoding", "utf8"),
                ("rformat", "json"),
                ("vermerge", "1"),
                ("mobi", "1"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let songs: Vec<Song> = data
            .get("abslist")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(parse_kuwo_song)
                    .filter(|song| !song.mid.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "[music-api] Kuwo song search: query={query:?}, page={}, results={}, elapsed_ms={}",
            page + 1,
            songs.len(),
            started.elapsed().as_millis()
        );
        Ok(songs)
    }

    async fn search_qq_songs(&self, query: &str, page: usize, limit: usize) -> Result<Vec<Song>> {
        let started = Instant::now();
        let body = search_body(query, 0, page, limit);
        let empty_retries = if page <= 1 {
            self.search.empty_result_retries
        } else {
            0
        };

        for semantic_attempt in 0..=empty_retries {
            let data = self.post_musicu(body.clone()).await?;
            validate_musicu_codes(&data, "music.search.SearchCgiService")?;
            let items = search_song_items(&data);
            let songs = items
                .into_iter()
                .flatten()
                .map(parse_qq_song)
                .filter(|song| !song.mid.is_empty())
                .collect::<Vec<_>>();

            if !songs.is_empty() || semantic_attempt == empty_retries {
                println!(
                    "[music-api] QQ song search: query={query:?}, page={page}, results={}, elapsed_ms={}",
                    songs.len(),
                    started.elapsed().as_millis()
                );
                return Ok(songs);
            }

            let list_present = search_song_items(&data).is_some();
            let suggestion = search_suggestion(&data).unwrap_or("-");
            eprintln!(
                "[music-api] QQ song search returned no usable songs; query={query:?}, page={page}, attempt={}/{}, top_code={}, service_code={}, list_present={list_present}, suggestion={suggestion:?}; retrying",
                semantic_attempt + 1,
                empty_retries + 1,
                response_code(&data).unwrap_or(-1),
                service_code(&data, "music.search.SearchCgiService").unwrap_or(-1),
            );
            sleep(Duration::from_millis(self.search.retry_delay_ms)).await;
        }

        unreachable!("QQ search semantic retry loop always returns")
    }

    async fn search_qq_playlist(&self, query: &str) -> Result<Option<(String, String)>> {
        let data = self.post_musicu(search_body(query, 3, 1, 10)).await?;
        validate_musicu_codes(&data, "music.search.SearchCgiService")?;
        let item = data
            .pointer("/music.search.SearchCgiService/data/body/songlist/list")
            .and_then(Value::as_array)
            .and_then(|items| items.first());
        Ok(item.map(|item| {
            (
                format!("qqpl:{}", value_string(&item["dissid"])),
                value_string(&item["dissname"]),
            )
        }))
    }

    async fn search_kuwo_playlist(&self, query: &str) -> Result<Option<(String, String)>> {
        let data = self
            .client
            .get(&self.search.kuwo_api_url)
            .query(&[
                ("client", "kt"),
                ("all", query),
                ("pn", "0"),
                ("rn", "10"),
                ("ft", "playlist"),
                ("uid", &self.search.kuwo_uid.to_string()),
                ("ver", &self.search.kuwo_version),
                ("vipver", "1"),
                ("cluster", "0"),
                ("strategy", "2012"),
                ("encoding", "utf8"),
                ("rformat", "json"),
                ("vermerge", "1"),
                ("mobi", "1"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let item = data
            .get("abslist")
            .and_then(Value::as_array)
            .and_then(|items| {
                items.iter().find(|item| {
                    !value_string(&item["playlistid"]).is_empty()
                        || !value_string(&item["DC_TARGETID"]).is_empty()
                })
            });
        Ok(item.map(|item| {
            let id = first_string(item, &["playlistid", "DC_TARGETID"]);
            let name = first_string(item, &["name", "dissname"]);
            (format!("kwpl:{id}"), name)
        }))
    }

    pub async fn playlist_songs_for_query(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<(String, Vec<Song>)> {
        if self.search.provider == "kuwo" {
            match self.search_kuwo_playlist(query).await {
                Ok(Some((id, name))) => match self.playlist_songs(&id, limit).await {
                    Ok(songs) if !songs.is_empty() => return Ok((name, songs)),
                    Ok(_) => eprintln!(
                        "[music-api] Kuwo playlist was empty; query={query:?}; falling back to QQ"
                    ),
                    Err(err) => eprintln!(
                        "[music-api] Kuwo playlist read failed; query={query:?}; falling back to QQ: {err:#}"
                    ),
                },
                Ok(None) => eprintln!(
                    "[music-api] Kuwo playlist search returned no list; query={query:?}; falling back to QQ"
                ),
                Err(err) => eprintln!(
                    "[music-api] Kuwo playlist search failed; query={query:?}; falling back to QQ: {err:#}"
                ),
            }
            anyhow::ensure!(
                matches!(
                    self.search.fallback_provider.as_str(),
                    "qq_music" | "qq_music_musicu"
                ),
                "Kuwo playlist lookup failed and no QQ fallback provider is configured"
            );
        }

        let (id, name) = self
            .search_qq_playlist(query)
            .await?
            .context("no matching QQ playlist")?;
        let songs = self.playlist_songs(&id, limit).await?;
        Ok((name, songs))
    }

    pub async fn playlist_songs(&self, playlist_id: &str, limit: usize) -> Result<Vec<Song>> {
        if let Some(id) = playlist_id.strip_prefix("kwpl:") {
            return self.kuwo_playlist_songs(id, limit).await;
        }
        let id = playlist_id
            .strip_prefix("qqpl:")
            .unwrap_or(playlist_id)
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
        validate_musicu_codes(&data, "req_1")?;
        let items = data
            .pointer("/req_1/data/songlist")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .map(parse_qq_song)
            .filter(|song| !song.mid.is_empty())
            .collect())
    }

    async fn kuwo_playlist_songs(&self, playlist_id: &str, limit: usize) -> Result<Vec<Song>> {
        anyhow::ensure!(
            !playlist_id.is_empty() && playlist_id.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid Kuwo playlist ID"
        );
        let page_size = limit.clamp(1, 50);
        let mut songs = Vec::new();
        for page in 0..limit.div_ceil(page_size).max(1) {
            let data = self
                .client
                .get(&self.search.kuwo_playlist_info_api_url)
                .query(&[
                    ("pid", playlist_id),
                    ("pn", &(page + 1).to_string()),
                    ("rn", &page_size.to_string()),
                ])
                .header("User-Agent", "Mozilla/5.0 open-xiaoai-client")
                .header("Accept", "application/json,text/plain,*/*")
                .send()
                .await?
                .error_for_status()?
                .json::<Value>()
                .await?;
            anyhow::ensure!(
                data.get("code").and_then(Value::as_i64).unwrap_or(200) == 200,
                "Kuwo playlist API returned an error code"
            );
            let items = data
                .pointer("/data/musicList")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            songs.extend(
                items
                    .iter()
                    .map(parse_kuwo_song)
                    .filter(|song| !song.mid.is_empty()),
            );
            if songs.len() >= limit
                || data
                    .pointer("/data/musicList")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            {
                break;
            }
        }
        songs.truncate(limit);
        Ok(songs)
    }

    pub async fn random_songs(&self, limit: usize) -> Result<RandomSongBatch> {
        if self.search.provider == "kuwo" {
            let cooldown = self
                .random_state
                .lock()
                .expect("random state poisoned")
                .kuwo_blocked_until
                .is_some_and(|until| until > Instant::now());
            if !cooldown {
                match self.kuwo_random_songs(limit).await {
                    Ok(batch) if !batch.songs.is_empty() => return Ok(batch),
                    Ok(_) => eprintln!("[music-api] Kuwo random chart returned no usable songs; falling back to QQ"),
                    Err(err) => {
                        self.random_state.lock().expect("random state poisoned").kuwo_blocked_until =
                            Some(Instant::now() + Duration::from_secs(60));
                        eprintln!("[music-api] Kuwo random chart failed; falling back to QQ: {err:#}");
                    }
                }
            } else {
                println!("[music-api] Kuwo random chart skipped during 60-second failure cooldown");
            }
            anyhow::ensure!(
                matches!(
                    self.search.fallback_provider.as_str(),
                    "qq_music" | "qq_music_musicu"
                ),
                "Kuwo random chart failed and no QQ fallback provider is configured"
            );
        }
        self.qq_random_songs(limit).await
    }

    async fn kuwo_random_songs(&self, limit: usize) -> Result<RandomSongBatch> {
        let page_size = limit.clamp(1, 80);
        let (page, cycle) = {
            let state = self.random_state.lock().expect("random state poisoned");
            (state.kuwo_page, state.kuwo_cycle)
        };
        let data = self
            .client
            .get(&self.search.kuwo_toplist_detail_api_url)
            .query(&[
                ("from", "pc"),
                ("fmt", "json"),
                ("pn", &page.to_string()),
                ("rn", &page_size.to_string()),
                ("type", "bang"),
                ("data", "content"),
                ("id", &self.search.kuwo_random_toplist_id.to_string()),
                ("show_copyright_off", "0"),
                ("pcmp4", "1"),
                ("isbang", "1"),
                ("userid", "0"),
                ("httpStatus", "1"),
            ])
            .header("User-Agent", "Mozilla/5.0 open-xiaoai-client")
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let items = data
            .get("musiclist")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut songs = items
            .iter()
            .map(parse_kuwo_song)
            .filter(|song| !song.mid.is_empty())
            .collect::<Vec<_>>();
        anyhow::ensure!(!songs.is_empty(), "Kuwo toplist page has no usable songs");
        songs.shuffle(&mut rand::rng());
        let total = numeric_value(&data["num"]).unwrap_or(songs.len());
        let page_count = total.div_ceil(page_size).max(1);
        {
            let mut state = self.random_state.lock().expect("random state poisoned");
            state.kuwo_blocked_until = None;
            if page + 1 >= page_count {
                state.kuwo_page = 0;
                state.kuwo_cycle = state.kuwo_cycle.wrapping_add(1);
            } else {
                state.kuwo_page = page + 1;
            }
        }
        println!(
            "[music-api] Kuwo random chart: id={}, page={}/{}, results={}",
            self.search.kuwo_random_toplist_id,
            page + 1,
            page_count,
            songs.len()
        );
        Ok(RandomSongBatch {
            cycle_key: format!("kuwo:{}:{cycle}", self.search.kuwo_random_toplist_id),
            songs,
        })
    }

    async fn qq_random_songs(&self, limit: usize) -> Result<RandomSongBatch> {
        let count = limit.clamp(1, 100);
        let (offset, cycle) = {
            let state = self.random_state.lock().expect("random state poisoned");
            (state.qq_offset, state.qq_cycle)
        };
        let mut batch = self.fetch_qq_random_songs(offset, count).await?;
        let mut batch_cycle = cycle;
        if batch.is_empty() && offset > 0 {
            batch_cycle = cycle.wrapping_add(1);
            batch = self.fetch_qq_random_songs(0, count).await?;
            let mut state = self.random_state.lock().expect("random state poisoned");
            state.qq_offset = batch.len();
            state.qq_cycle = batch_cycle;
        } else {
            let mut state = self.random_state.lock().expect("random state poisoned");
            if batch.len() < count {
                state.qq_offset = 0;
                state.qq_cycle = state.qq_cycle.wrapping_add(1);
            } else {
                state.qq_offset = offset.saturating_add(batch.len());
            }
        }
        anyhow::ensure!(
            !batch.is_empty(),
            "QQ random chart returned no usable songs"
        );
        batch.shuffle(&mut rand::rng());
        println!(
            "[music-api] QQ random chart: offset={offset}, results={}",
            batch.len()
        );
        Ok(RandomSongBatch {
            cycle_key: format!("qq:{}:{batch_cycle}", self.search.random_toplist_id),
            songs: batch,
        })
    }

    async fn fetch_qq_random_songs(&self, offset: usize, count: usize) -> Result<Vec<Song>> {
        let body = json!({
            "req_1": {
                "module": "musicToplist.ToplistInfoServer", "method": "GetDetail",
                "param": {"topid": self.search.random_toplist_id, "offset": offset, "num": count, "period": ""}
            }
        });
        let data = self.post_musicu(body).await?;
        validate_musicu_codes(&data, "req_1")?;
        let items = data
            .pointer("/req_1/data/songInfoList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .map(parse_qq_song)
            .filter(|song| !song.mid.is_empty())
            .collect())
    }

    pub async fn resolve_url(&self, mid: &str) -> Result<String> {
        if let Some(kuwo_id) = mid.strip_prefix("kw:") {
            return self.resolve_kuwo_url(kuwo_id).await;
        }
        let mid = mid.strip_prefix("qq:").unwrap_or(mid);
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

    pub async fn resolve_song(&self, song: &Song) -> Result<(Song, String)> {
        if song.mid.starts_with("kw:") {
            match self.resolve_url(&song.mid).await {
                Ok(url) => return Ok((song.clone(), url)),
                Err(kuwo_error) => {
                    eprintln!(
                        "[music-api] Kuwo stream resolution failed for {:?}; searching QQ fallback: {kuwo_error:#}",
                        song.title
                    );
                    let query = format!("{} {}", song.title, song.artists.join(" "));
                    let candidates = self
                        .search_qq_songs(&query, 1, self.search.page_size)
                        .await
                        .context("QQ fallback search failed after Kuwo stream failure")?;
                    let qq_song = best_qq_match(song, candidates)
                        .context("QQ fallback did not return the same song")?;
                    let url = self
                        .resolve_url(&qq_song.mid)
                        .await
                        .context("QQ fallback song URL resolution failed")?;
                    println!(
                        "[music-api] Kuwo stream fallback resolved through QQ: {:?} -> {}",
                        song.title, qq_song.mid
                    );
                    return Ok((qq_song, url));
                }
            }
        }
        let url = self.resolve_url(&song.mid).await?;
        Ok((song.clone(), url))
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
        let cache_key = mid.strip_prefix("qq:").unwrap_or(mid);
        if self
            .play_url_state
            .lock()
            .expect("play URL state poisoned")
            .cache
            .remove(cache_key)
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

    async fn resolve_kuwo_url(&self, id: &str) -> Result<String> {
        anyhow::ensure!(
            !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid Kuwo RID"
        );
        let cache_key = format!("kw:{id}");
        if let Some(url) = self.cached_url(&cache_key) {
            println!("[music-api] Kuwo play URL cache hit: rid={id}");
            return Ok(url);
        }
        let mut errors = Vec::new();
        if self.play_url.kuwo_primary_enabled {
            match self
                .resolve_kuwo_stream(&self.play_url.kuwo_primary_url_template, id)
                .await
            {
                Ok(url) => {
                    self.cache_url(&cache_key, &url);
                    println!("[music-api] Kuwo primary stream ready: rid={id}");
                    return Ok(url);
                }
                Err(err) => {
                    eprintln!("[music-api] Kuwo primary stream failed: rid={id}: {err:#}");
                    errors.push(format!("primary: {err:#}"));
                }
            }
        }
        if self.play_url.kuwo_backup_enabled {
            match self
                .resolve_kuwo_stream(&self.play_url.kuwo_backup_url_template, id)
                .await
            {
                Ok(url) => {
                    self.cache_url(&cache_key, &url);
                    println!("[music-api] Kuwo backup stream ready: rid={id}");
                    return Ok(url);
                }
                Err(err) => {
                    eprintln!("[music-api] Kuwo backup stream failed: rid={id}: {err:#}");
                    errors.push(format!("backup: {err:#}"));
                }
            }
        }
        anyhow::bail!(
            "all Kuwo stream sources failed for RID {id}: {}",
            errors.join("; ")
        )
    }

    async fn resolve_kuwo_stream(&self, template: &str, id: &str) -> Result<String> {
        anyhow::ensure!(
            !template.trim().is_empty(),
            "Kuwo stream URL template is empty"
        );
        let endpoint = template
            .replace("{id}", id)
            .replace("{mid}", id)
            .replace("{level}", &self.play_url.kuwo_level);
        let response = self
            .client
            .get(&endpoint)
            .header("Range", "bytes=0-1")
            .send()
            .await?
            .error_for_status()?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        anyhow::ensure!(
            content_type.starts_with("audio/")
                || content_type.starts_with("application/octet-stream"),
            "Kuwo stream source returned unexpected content type: {content_type:?}"
        );
        Ok(endpoint)
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

fn response_code(data: &Value) -> Option<i64> {
    integer_value(data.get("code")?)
}

fn service_code(data: &Value, service: &str) -> Option<i64> {
    integer_value(data.get(service)?.get("code")?)
}

fn integer_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|code| i64::try_from(code).ok()))
}

fn validate_musicu_codes(data: &Value, service: &str) -> Result<()> {
    let top_code = response_code(data).context("QQ Music response is missing top-level code")?;
    anyhow::ensure!(
        top_code == 0,
        "QQ Music top-level business code: {top_code}"
    );
    let service_code = service_code(data, service)
        .with_context(|| format!("QQ Music response is missing {service} code"))?;
    anyhow::ensure!(
        service_code == 0,
        "QQ Music {service} business code: {service_code}"
    );
    Ok(())
}

fn search_song_items(data: &Value) -> Option<&Vec<Value>> {
    data.pointer("/music.search.SearchCgiService/data/body/song/list")
        .and_then(Value::as_array)
}

fn search_suggestion(data: &Value) -> Option<&str> {
    data.pointer("/music.search.SearchCgiService/data/body/qc/0/word")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn parse_qq_song(item: &Value) -> Song {
    Song {
        mid: format!("qq:{}", value_string(&item["mid"])),
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

fn parse_kuwo_song(item: &Value) -> Song {
    let raw_id = first_string(item, &["MUSICRID", "musicrid", "rid", "id"]);
    let id = raw_id.strip_prefix("MUSIC_").unwrap_or(&raw_id).trim();
    let artists = first_string(item, &["ARTIST", "artist"])
        .split(['&', '、', '/', ','])
        .map(str::trim)
        .filter(|artist| !artist.is_empty())
        .map(str::to_string)
        .collect();
    Song {
        mid: (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| format!("kw:{id}"))
            .unwrap_or_default(),
        title: {
            let title = first_string(item, &["SONGNAME", "songname", "name", "NAME"]);
            if title.is_empty() {
                value_string(&item["NAME"])
            } else {
                title
            }
        },
        artists,
        album: first_string(item, &["ALBUM", "album"]),
        duration_ms: item
            .get("DURATION")
            .or_else(|| item.get("duration"))
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .map(|seconds| seconds * 1_000),
    }
}

fn first_string(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| value_string(&value[*key]))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn numeric_value(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .map(|value| value as usize)
        .or_else(|| value.as_str()?.parse().ok())
}

fn best_qq_match(original: &Song, candidates: Vec<Song>) -> Option<Song> {
    let wanted_title = normalize_song_text(&original.title);
    if wanted_title.is_empty() {
        return None;
    }
    let wanted_artists = original
        .artists
        .iter()
        .map(|artist| normalize_song_text(artist))
        .filter(|artist| !artist.is_empty())
        .collect::<Vec<_>>();

    candidates
        .into_iter()
        .filter(|candidate| normalize_song_text(&candidate.title) == wanted_title)
        .max_by_key(|candidate| {
            candidate
                .artists
                .iter()
                .map(|artist| normalize_song_text(artist))
                .any(|artist| wanted_artists.iter().any(|wanted| artist == *wanted))
        })
}

fn normalize_song_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
        let song = parse_qq_song(
            &json!({"mid":"abc","title":"晴天","singer":[{"name":"周杰伦"}],"album":{"name":"叶惠美"},"interval":269}),
        );
        assert_eq!(song.mid, "qq:abc");
        assert_eq!(song.duration_ms, Some(269_000));
        assert_eq!(
            json_dot_path(&json!({"data":{"url":"x"}}), "data.url").and_then(Value::as_str),
            Some("x")
        );
    }

    #[test]
    fn parses_musicu_search_response() {
        let data = json!({
            "code": 0,
            "music.search.SearchCgiService": {
                "code": 0,
                "data": {"body": {"song": {"list": [
                    {"mid":"0039MnYb0qxYhV","title":"晴天","singer":[{"name":"周杰伦"}]}
                ]}}}
            }
        });
        validate_musicu_codes(&data, "music.search.SearchCgiService").unwrap();
        let songs = search_song_items(&data)
            .unwrap()
            .iter()
            .map(parse_qq_song)
            .collect::<Vec<_>>();
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "晴天");
    }

    #[test]
    fn distinguishes_missing_and_empty_search_lists() {
        let missing =
            json!({"code":0,"music.search.SearchCgiService":{"code":0,"data":{"body":{}}}});
        let empty = json!({"code":0,"music.search.SearchCgiService":{"code":0,"data":{"body":{"song":{"list":[]}}}}});
        assert!(search_song_items(&missing).is_none());
        assert!(search_song_items(&empty).unwrap().is_empty());
    }

    #[test]
    fn rejects_nonzero_musicu_business_codes() {
        let top = json!({"code":1001,"req_1":{"code":0}});
        let service = json!({"code":0,"req_1":{"code":2001}});
        assert!(validate_musicu_codes(&top, "req_1").is_err());
        assert!(validate_musicu_codes(&service, "req_1").is_err());
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

    #[test]
    fn parses_kuwo_song_and_removes_musicrid_prefix() {
        let song = parse_kuwo_song(&json!({
            "MUSICRID": "MUSIC_51685512", "SONGNAME": "晴天", "ARTIST": "周杰伦&嘉宾",
            "ALBUM": "叶惠美", "DURATION": "269"
        }));
        assert_eq!(song.mid, "kw:51685512");
        assert_eq!(song.title, "晴天");
        assert_eq!(song.artists, vec!["周杰伦", "嘉宾"]);
        assert_eq!(song.duration_ms, Some(269_000));
    }

    #[test]
    fn parses_kuwo_playlist_and_chart_song_fields() {
        let song = parse_kuwo_song(&json!({
            "musicrid": "MUSIC_442266", "name": "睡公主", "artist": "G.E.M.邓紫棋",
            "album": "G.E.M.", "duration": 282
        }));
        assert_eq!(song.mid, "kw:442266");
        assert_eq!(song.title, "睡公主");
        assert_eq!(song.artists, vec!["G.E.M.邓紫棋"]);
        assert_eq!(song.duration_ms, Some(282_000));
    }

    #[test]
    fn qq_cross_source_match_requires_same_title_and_prefers_same_artist() {
        let original = Song {
            mid: "kw:1".into(),
            title: "睡公主".into(),
            artists: vec!["G.E.M. 邓紫棋".into()],
            album: String::new(),
            duration_ms: None,
        };
        let match_song = Song {
            mid: "qq:best".into(),
            title: "睡公主".into(),
            artists: vec!["G.E.M.邓紫棋".into()],
            album: String::new(),
            duration_ms: None,
        };
        let other_artist = Song {
            mid: "qq:cover".into(),
            title: "睡公主".into(),
            artists: vec!["翻唱歌手".into()],
            album: String::new(),
            duration_ms: None,
        };
        let selected = best_qq_match(&original, vec![other_artist, match_song]).unwrap();
        assert_eq!(selected.mid, "qq:best");
        assert!(best_qq_match(
            &original,
            vec![Song {
                mid: "qq:wrong".into(),
                title: "别的歌".into(),
                artists: vec![],
                album: String::new(),
                duration_ms: None
            }]
        )
        .is_none());
    }

    #[test]
    fn rejects_non_numeric_kuwo_musicrid() {
        assert!(parse_kuwo_song(&json!({"MUSICRID": "MUSIC_bad"}))
            .mid
            .is_empty());
    }
}

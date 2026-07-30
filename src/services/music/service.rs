use std::collections::{HashSet, VecDeque};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use tokio::sync::mpsc;

use crate::config::{AudioPolicyConfig, MusicConfig};
use crate::services::media::{MediaBus, MediaEvent, MusicLedState};

use super::command::MusicCommand;
use super::player::{LocalPlayer, SIGNAL_CONT, SIGNAL_KILL, SIGNAL_STOP, SIGNAL_TERM};
use super::qq::{QqMusic, Song};

#[derive(Clone)]
pub struct MusicService {
    tx: mpsc::UnboundedSender<ServiceMessage>,
    active: Arc<AtomicBool>,
}

impl MusicService {
    pub fn start(
        config: MusicConfig,
        audio_policy: AudioPolicyConfig,
        media: MediaBus,
        media_events: mpsc::UnboundedReceiver<MediaEvent>,
    ) -> Result<Self> {
        let provider = QqMusic::new(config.search.clone(), config.play_url.clone())?;
        let player = LocalPlayer::new(config.player.clone());
        let (tx, rx) = mpsc::unbounded_channel();
        let active = Arc::new(AtomicBool::new(false));
        let actor = Actor {
            config,
            audio_policy,
            provider,
            player,
            media,
            tx: tx.clone(),
            rx,
            media_events,
            active: active.clone(),
            queue: VecDeque::new(),
            history: Vec::new(),
            played_mids: HashSet::new(),
            current: None,
            generation: 0,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            consecutive_failures: 0,
            wake_pause_active: false,
            airplay_pause_active: false,
            queue_epoch: 0,
            url_retry_scheduled: false,
            url_cooldown_retry_count: 0,
        };
        tokio::spawn(actor.run());
        Ok(Self { tx, active })
    }

    pub fn execute(&self, command: MusicCommand) {
        let _ = self.tx.send(ServiceMessage::Execute(command));
    }

    pub fn wake_started(&self) {
        let _ = self.tx.send(ServiceMessage::WakeStarted);
    }

    pub fn dialog_finished(&self) {
        let _ = self.tx.send(ServiceMessage::DialogFinished);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

enum ServiceMessage {
    Execute(MusicCommand),
    WakeStarted,
    DialogFinished,
    PlayerExited {
        generation: u64,
        status: std::io::Result<ExitStatus>,
    },
    ForceKill {
        generation: u64,
        pid: u32,
    },
    AppendQueue {
        epoch: u64,
        songs: Vec<Song>,
    },
    RetryQueue {
        epoch: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndReason {
    Replace,
    Next,
    Previous,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepeatMode {
    Off,
    One,
    All,
}

struct CurrentTrack {
    song: Song,
    pid: u32,
    generation: u64,
    ending: Option<EndReason>,
    paused_by_user: bool,
    paused_by_wake: bool,
    paused_by_airplay: bool,
    retry_count: usize,
    started_at: Instant,
}

impl CurrentTrack {
    fn paused(&self) -> bool {
        self.paused_by_user || self.paused_by_wake || self.paused_by_airplay
    }
}

struct Actor {
    config: MusicConfig,
    audio_policy: AudioPolicyConfig,
    provider: QqMusic,
    player: LocalPlayer,
    media: MediaBus,
    tx: mpsc::UnboundedSender<ServiceMessage>,
    rx: mpsc::UnboundedReceiver<ServiceMessage>,
    media_events: mpsc::UnboundedReceiver<MediaEvent>,
    active: Arc<AtomicBool>,
    queue: VecDeque<Song>,
    history: Vec<Song>,
    played_mids: HashSet<String>,
    current: Option<CurrentTrack>,
    generation: u64,
    shuffle: bool,
    repeat_mode: RepeatMode,
    consecutive_failures: usize,
    wake_pause_active: bool,
    airplay_pause_active: bool,
    queue_epoch: u64,
    url_retry_scheduled: bool,
    url_cooldown_retry_count: usize,
}

impl Actor {
    async fn run(mut self) {
        self.repeat_mode = match self.config.queue.repeat_mode.as_str() {
            "one" => RepeatMode::One,
            "all" => RepeatMode::All,
            _ => RepeatMode::Off,
        };
        self.shuffle = self.config.queue.default_order == "random";

        loop {
            tokio::select! {
                Some(message) = self.rx.recv() => self.handle_message(message).await,
                Some(event) = self.media_events.recv() => self.handle_media_event(event),
                else => break,
            }
        }
        self.stop_current(EndReason::Stop);
    }

    async fn handle_message(&mut self, message: ServiceMessage) {
        match message {
            ServiceMessage::Execute(command) => self.execute(command).await,
            ServiceMessage::WakeStarted => {
                if self.config.interruption.on_wake == "pause" {
                    self.set_pause(PauseSource::Wake, true);
                }
            }
            ServiceMessage::DialogFinished => {
                if self.config.interruption.resume_on_dialog_finish {
                    self.set_pause(PauseSource::Wake, false);
                }
            }
            ServiceMessage::PlayerExited { generation, status } => {
                self.player_exited(generation, status).await;
            }
            ServiceMessage::ForceKill { generation, pid } => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|track| track.generation == generation)
                {
                    let _ = LocalPlayer::signal(pid, SIGNAL_KILL);
                }
            }
            ServiceMessage::AppendQueue { epoch, songs } => {
                self.append_queue(epoch, songs).await;
            }
            ServiceMessage::RetryQueue { epoch } => {
                if epoch == self.queue_epoch {
                    self.url_retry_scheduled = false;
                    if self.current.is_none() && !self.queue.is_empty() {
                        self.start_next().await;
                    }
                }
            }
        }
    }

    fn handle_media_event(&mut self, event: MediaEvent) {
        match event {
            MediaEvent::AirPlayStarted if self.audio_policy.on_airplay_start == "pause_music" => {
                self.set_pause(PauseSource::AirPlay, true);
            }
            MediaEvent::AirPlayEnded if self.audio_policy.on_airplay_end == "resume_music" => {
                self.set_pause(PauseSource::AirPlay, false);
            }
            _ => {}
        }
    }

    async fn execute(&mut self, command: MusicCommand) {
        match command {
            MusicCommand::Pause => self.set_pause(PauseSource::User, true),
            MusicCommand::Resume => self.set_pause(PauseSource::User, false),
            MusicCommand::Next => self.stop_current(EndReason::Next),
            MusicCommand::Previous => self.previous(),
            MusicCommand::Stop => {
                self.queue_epoch = self.queue_epoch.wrapping_add(1);
                self.url_retry_scheduled = false;
                self.url_cooldown_retry_count = 0;
                self.queue.clear();
                self.history.clear();
                self.played_mids.clear();
                self.stop_current(EndReason::Stop);
                self.update_active();
            }
            MusicCommand::Shuffle => {
                self.shuffle = true;
                self.queue.make_contiguous().shuffle(&mut rand::rng());
                println!("[music] queue order: random");
            }
            MusicCommand::RepeatOne => {
                self.repeat_mode = RepeatMode::One;
                println!("[music] repeat mode: one");
            }
            MusicCommand::RepeatAll => {
                self.repeat_mode = RepeatMode::All;
                println!("[music] repeat mode: all");
            }
            MusicCommand::Play(query) => {
                if self.reject_for_airplay() {
                    return;
                }
                let started = Instant::now();
                match self.search_first_song(&query).await {
                    Ok(selected) => {
                        let artist = selected.artists.first().cloned();
                        let selected_mid = selected.mid.clone();
                        let epoch = self.replace_queue(vec![selected]).await;
                        println!(
                            "[music] first track scheduled: query={query:?}, elapsed_ms={}",
                            started.elapsed().as_millis()
                        );
                        if self.config.queue.after_single_song == "same_artist" {
                            if let Some(artist) = artist {
                                self.prefetch_same_artist(epoch, artist, selected_mid);
                            }
                        }
                    }
                    Err(err) => eprintln!("[music] search failed for {query:?}: {err:#}"),
                }
            }
            MusicCommand::Singer(singer) => {
                if self.reject_for_airplay() {
                    return;
                }
                match self.build_singer_queue(&singer).await {
                    Ok(songs) => {
                        self.replace_queue(songs).await;
                    }
                    Err(err) => eprintln!("[music] singer search failed for {singer:?}: {err:#}"),
                }
            }
            MusicCommand::Playlist(name) => {
                if self.reject_for_airplay() {
                    return;
                }
                match self.build_playlist_queue(&name).await {
                    Ok(songs) => {
                        self.replace_queue(songs).await;
                    }
                    Err(err) => eprintln!("[music] playlist search failed for {name:?}: {err:#}"),
                }
            }
            MusicCommand::Random(count) => {
                if self.reject_for_airplay() {
                    return;
                }
                match self.provider.random_songs(count.max(1)).await {
                    Ok(songs) => {
                        self.replace_queue(songs).await;
                    }
                    Err(err) => eprintln!("[music] random queue failed: {err:#}"),
                }
            }
        }
    }

    fn reject_for_airplay(&self) -> bool {
        if self.media.airplay_active() && self.audio_policy.music_while_airplay == "reject" {
            eprintln!("[music] request ignored while AirPlay is active");
            true
        } else {
            false
        }
    }

    async fn search_first_song(&self, query: &str) -> Result<Song> {
        let mut songs = self
            .provider
            .search_songs(query, 1, self.config.search.page_size)
            .await?;
        anyhow::ensure!(!songs.is_empty(), "no matching song");
        Ok(songs.remove(0))
    }

    fn prefetch_same_artist(&self, epoch: u64, artist: String, selected_mid: String) {
        let provider = self.provider.clone();
        let tx = self.tx.clone();
        let count = self.config.queue.autoplay_count.max(1);
        tokio::spawn(async move {
            match provider.search_songs(&artist, 1, count).await {
                Ok(candidates) => {
                    let songs = candidates
                        .into_iter()
                        .filter(|song| {
                            song.mid != selected_mid
                                && song.artists.iter().any(|name| name == &artist)
                        })
                        .collect::<Vec<_>>();
                    println!(
                        "[music] background queue ready: artist={artist:?}, tracks={}",
                        songs.len()
                    );
                    let _ = tx.send(ServiceMessage::AppendQueue { epoch, songs });
                }
                Err(err) => {
                    eprintln!("[music] background artist queue failed for {artist:?}: {err:#}")
                }
            }
        });
    }

    async fn build_singer_queue(&self, singer: &str) -> Result<Vec<Song>> {
        let limit = self.config.queue.autoplay_count.max(1);
        let mut result = Vec::new();
        for page in 1..=3 {
            let songs = self
                .provider
                .search_songs(singer, page, self.config.search.page_size)
                .await?;
            if songs.is_empty() {
                break;
            }
            result.extend(
                songs
                    .into_iter()
                    .filter(|song| song.artists.iter().any(|name| name.contains(singer))),
            );
            if result.len() >= limit {
                break;
            }
        }
        result.truncate(limit);
        anyhow::ensure!(!result.is_empty(), "no songs found for singer");
        Ok(result)
    }

    async fn build_playlist_queue(&self, query: &str) -> Result<Vec<Song>> {
        let (id, name) = self
            .provider
            .search_playlist(query)
            .await?
            .context("no matching playlist")?;
        let songs = self
            .provider
            .playlist_songs(&id, self.config.queue.autoplay_count.max(1))
            .await?;
        anyhow::ensure!(!songs.is_empty(), "playlist is empty");
        println!("[music] playlist selected: {name} ({id})");
        Ok(songs)
    }

    async fn replace_queue(&mut self, songs: Vec<Song>) -> u64 {
        let mut seen = HashSet::new();
        let mut songs: Vec<_> = songs
            .into_iter()
            .filter(|song| !song.mid.is_empty() && seen.insert(song.mid.clone()))
            .collect();
        if self.shuffle {
            songs.shuffle(&mut rand::rng());
        }
        if songs.is_empty() {
            eprintln!("[music] no playable search results");
            return self.queue_epoch;
        }
        self.queue_epoch = self.queue_epoch.wrapping_add(1);
        let epoch = self.queue_epoch;
        self.url_retry_scheduled = false;
        self.url_cooldown_retry_count = 0;
        self.queue = songs.into();
        self.history.clear();
        self.played_mids.clear();
        self.consecutive_failures = 0;
        if self.current.is_some() {
            self.stop_current(EndReason::Replace);
        } else {
            self.start_next().await;
        }
        self.update_active();
        epoch
    }

    async fn append_queue(&mut self, epoch: u64, mut songs: Vec<Song>) {
        if epoch != self.queue_epoch {
            println!("[music] discarded stale background queue: epoch={epoch}");
            return;
        }
        let current_mid = self.current.as_ref().map(|track| track.song.mid.as_str());
        let queued = self
            .queue
            .iter()
            .map(|song| song.mid.clone())
            .collect::<HashSet<_>>();
        songs.retain(|song| {
            !song.mid.is_empty()
                && current_mid != Some(song.mid.as_str())
                && !self.played_mids.contains(&song.mid)
                && !queued.contains(&song.mid)
        });
        if self.shuffle {
            songs.shuffle(&mut rand::rng());
        }
        let appended = songs.len();
        self.queue.extend(songs);
        println!("[music] background queue appended: tracks={appended}");
        if self.current.is_none() && !self.queue.is_empty() && !self.url_retry_scheduled {
            self.start_next().await;
        }
        self.update_active();
    }

    fn previous(&mut self) {
        let Some(previous) = self.history.pop() else {
            println!("[music] no previous track");
            return;
        };
        self.played_mids.remove(&previous.mid);
        if let Some(current) = self.current.as_ref() {
            self.played_mids.remove(&current.song.mid);
            self.queue.push_front(current.song.clone());
        }
        self.queue.push_front(previous);
        self.stop_current(EndReason::Previous);
    }

    fn stop_current(&mut self, reason: EndReason) {
        let Some(current) = self.current.as_mut() else {
            if reason != EndReason::Stop {
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(ServiceMessage::PlayerExited {
                        generation: 0,
                        status: Err(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            "no player",
                        )),
                    });
                });
            }
            return;
        };
        if current.ending.is_some() {
            return;
        }
        current.ending = Some(reason);
        let generation = current.generation;
        let pid = current.pid;
        let _ = LocalPlayer::signal(pid, SIGNAL_CONT);
        let _ = LocalPlayer::signal(pid, SIGNAL_TERM);
        let tx = self.tx.clone();
        let delay = Duration::from_millis(self.config.player.stop_timeout_ms.max(100));
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(ServiceMessage::ForceKill { generation, pid });
        });
    }

    fn set_pause(&mut self, source: PauseSource, value: bool) {
        match source {
            PauseSource::Wake => self.wake_pause_active = value,
            PauseSource::AirPlay => self.airplay_pause_active = value,
            PauseSource::User => {}
        }
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let was_paused = current.paused();
        match source {
            PauseSource::User => current.paused_by_user = value,
            PauseSource::Wake => current.paused_by_wake = value,
            PauseSource::AirPlay => current.paused_by_airplay = value,
        }
        let is_paused = current.paused();
        if was_paused == is_paused {
            return;
        }
        let signal = if is_paused { SIGNAL_STOP } else { SIGNAL_CONT };
        match LocalPlayer::signal(current.pid, signal) {
            Ok(()) => self.media.set_music_state(if is_paused {
                MusicLedState::Paused
            } else {
                MusicLedState::Playing
            }),
            Err(err) => eprintln!("[music] failed to change pause state: {err:#}"),
        }
    }

    async fn player_exited(&mut self, generation: u64, status: std::io::Result<ExitStatus>) {
        if generation == 0 && self.current.is_none() {
            self.start_next().await;
            return;
        }
        if !self
            .current
            .as_ref()
            .is_some_and(|track| track.generation == generation)
        {
            return;
        }
        let current = self.current.take().expect("generation checked");
        let play_elapsed_ms = current.started_at.elapsed().as_millis();
        self.media.set_music_state(MusicLedState::Idle);
        let successful = status.as_ref().is_ok_and(|status| status.success());
        if let Err(err) = &status {
            eprintln!("[music] player wait failed: {err}");
        }
        println!(
            "[music] player exited: title={:?}, status={:?}, elapsed_ms={play_elapsed_ms}, ending={:?}",
            current.song.title,
            status.as_ref().ok().and_then(ExitStatus::code),
            current.ending
        );

        match current.ending {
            Some(EndReason::Stop) => {}
            Some(EndReason::Replace | EndReason::Previous) => self.start_next().await,
            Some(EndReason::Next) => {
                self.history.push(current.song);
                self.start_next().await;
            }
            None if !successful
                && current.retry_count < self.config.player.unexpected_exit_retries =>
            {
                self.provider.invalidate_url(&current.song.mid);
                let retry = current.song;
                println!(
                    "[music] retrying after unexpected player exit: {}",
                    retry.title
                );
                self.queue.push_front(retry);
                self.start_next_with_retry(current.retry_count + 1).await;
            }
            None => {
                if !successful {
                    self.provider.invalidate_url(&current.song.mid);
                    self.consecutive_failures += 1;
                    eprintln!(
                        "[music] player exited unsuccessfully for {}",
                        current.song.title
                    );
                } else {
                    self.consecutive_failures = 0;
                }
                if self.repeat_mode == RepeatMode::One {
                    self.played_mids.remove(&current.song.mid);
                    self.queue.push_front(current.song.clone());
                } else {
                    self.history.push(current.song);
                }
                if self.consecutive_failures >= self.config.player.max_consecutive_failures.max(1) {
                    eprintln!("[music] stopped after repeated playback failures");
                    self.queue.clear();
                } else {
                    self.refill_repeat_all();
                    self.start_next().await;
                }
            }
        }
        self.update_active();
    }

    fn refill_repeat_all(&mut self) {
        if self.queue.is_empty() && self.repeat_mode == RepeatMode::All && !self.history.is_empty()
        {
            let mut songs = std::mem::take(&mut self.history);
            if self.shuffle {
                songs.shuffle(&mut rand::rng());
            }
            self.queue = songs.into();
            self.played_mids.clear();
        }
    }

    async fn start_next(&mut self) {
        self.start_next_with_retry(0).await;
    }

    async fn start_next_with_retry(&mut self, retry_count: usize) {
        while let Some(song) = self.queue.pop_front() {
            if retry_count == 0 && self.played_mids.contains(&song.mid) {
                continue;
            }
            match self.provider.resolve_url(&song.mid).await {
                Ok(url) => match self.player.spawn(&url) {
                    Ok(mut child) => {
                        let Some(pid) = child.id() else {
                            eprintln!("[music] player returned no PID for {}", song.title);
                            continue;
                        };
                        self.generation = self.generation.wrapping_add(1).max(1);
                        let generation = self.generation;
                        let tx = self.tx.clone();
                        tokio::spawn(async move {
                            let status = child.wait().await;
                            let _ = tx.send(ServiceMessage::PlayerExited { generation, status });
                        });
                        self.played_mids.insert(song.mid.clone());
                        self.url_cooldown_retry_count = 0;
                        println!(
                            "[music] playing: {} - {}",
                            song.title,
                            song.artists.join("/")
                        );
                        self.current = Some(CurrentTrack {
                            song,
                            pid,
                            generation,
                            ending: None,
                            paused_by_user: false,
                            paused_by_wake: self.wake_pause_active,
                            paused_by_airplay: self.airplay_pause_active,
                            retry_count,
                            started_at: Instant::now(),
                        });
                        let starts_paused = self.current.as_ref().is_some_and(CurrentTrack::paused);
                        if starts_paused {
                            if let Err(err) = LocalPlayer::signal(pid, SIGNAL_STOP) {
                                eprintln!("[music] failed to inherit pause state: {err:#}");
                            }
                        }
                        self.media.set_music_state(if starts_paused {
                            MusicLedState::Paused
                        } else {
                            MusicLedState::Playing
                        });
                        self.update_active();
                        return;
                    }
                    Err(err) => {
                        eprintln!("[music] failed to start player for {}: {err:#}", song.title)
                    }
                },
                Err(err) => {
                    if let Some(retry_after) = QqMusic::retry_after(&err) {
                        let title = song.title.clone();
                        if self.url_cooldown_retry_count
                            >= self.config.play_url.primary_cooldown_retries
                        {
                            eprintln!(
                                "[music] stopped after play URL cooldown retries exhausted: title={title:?}, retries={}",
                                self.url_cooldown_retry_count
                            );
                            self.url_retry_scheduled = false;
                            self.url_cooldown_retry_count = 0;
                            self.queue.clear();
                            self.current = None;
                            self.media.set_music_state(MusicLedState::Idle);
                            self.update_active();
                            return;
                        }
                        self.queue.push_front(song);
                        self.url_cooldown_retry_count += 1;
                        self.schedule_url_retry(retry_after);
                        println!(
                            "[music] play URL cooling down; track retained: title={title:?}, retry={}/{}, retry_after_ms={}",
                            self.url_cooldown_retry_count,
                            self.config.play_url.primary_cooldown_retries,
                            retry_after.as_millis(),
                        );
                        self.current = None;
                        self.media.set_music_state(MusicLedState::Idle);
                        self.update_active();
                        return;
                    }
                    eprintln!("[music] failed to resolve URL for {}: {err:#}", song.title);
                }
            }
            self.consecutive_failures += 1;
            if self.config.queue.on_url_failure != "skip_to_next"
                || self.consecutive_failures >= self.config.player.max_consecutive_failures.max(1)
            {
                self.queue.clear();
                break;
            }
        }
        self.current = None;
        self.media.set_music_state(MusicLedState::Idle);
        self.update_active();
    }

    fn update_active(&self) {
        self.active.store(
            self.current.is_some() || !self.queue.is_empty(),
            Ordering::Release,
        );
    }

    fn schedule_url_retry(&mut self, delay: Duration) {
        if self.url_retry_scheduled {
            return;
        }
        self.url_retry_scheduled = true;
        let epoch = self.queue_epoch;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay.max(Duration::from_millis(100))).await;
            let _ = tx.send(ServiceMessage::RetryQueue { epoch });
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum PauseSource {
    User,
    Wake,
    AirPlay,
}

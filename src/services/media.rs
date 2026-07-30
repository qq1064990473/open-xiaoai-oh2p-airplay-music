use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaEvent {
    AirPlayStarted,
    AirPlayEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MusicLedState {
    Idle = 0,
    Playing = 1,
    Paused = 2,
}

#[derive(Clone)]
pub struct MediaBus {
    inner: Arc<MediaBusInner>,
}

struct MediaBusInner {
    tx: mpsc::UnboundedSender<MediaEvent>,
    airplay_active: AtomicBool,
    wake_active: AtomicBool,
    airplay_level_bits: AtomicU32,
    music_state: AtomicU8,
}

impl MediaBus {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<MediaEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let bus = Self {
            inner: Arc::new(MediaBusInner {
                tx,
                airplay_active: AtomicBool::new(false),
                wake_active: AtomicBool::new(false),
                airplay_level_bits: AtomicU32::new(f32::NEG_INFINITY.to_bits()),
                music_state: AtomicU8::new(MusicLedState::Idle as u8),
            }),
        };
        (bus, rx)
    }

    pub fn set_airplay_active(&self, active: bool) {
        if self.inner.airplay_active.swap(active, Ordering::AcqRel) != active {
            let event = if active {
                MediaEvent::AirPlayStarted
            } else {
                MediaEvent::AirPlayEnded
            };
            let _ = self.inner.tx.send(event);
        }
        if !active {
            self.set_airplay_level_db(f32::NEG_INFINITY);
        }
    }

    pub fn airplay_active(&self) -> bool {
        self.inner.airplay_active.load(Ordering::Acquire)
    }

    pub fn set_wake_active(&self, active: bool) {
        self.inner.wake_active.store(active, Ordering::Release);
    }

    pub fn wake_active(&self) -> bool {
        self.inner.wake_active.load(Ordering::Acquire)
    }

    pub fn set_airplay_level_db(&self, db: f32) {
        self.inner
            .airplay_level_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn airplay_level_db(&self) -> f32 {
        f32::from_bits(self.inner.airplay_level_bits.load(Ordering::Relaxed))
    }

    pub fn set_music_state(&self, state: MusicLedState) {
        self.inner.music_state.store(state as u8, Ordering::Release);
    }

    pub fn music_state(&self) -> MusicLedState {
        match self.inner.music_state.load(Ordering::Acquire) {
            1 => MusicLedState::Playing,
            2 => MusicLedState::Paused,
            _ => MusicLedState::Idle,
        }
    }
}

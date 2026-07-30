use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use shairplay::{AudioFormat, AudioHandler, AudioSession, RaopServer};
use tokio::task::JoinHandle;

use crate::config::AirPlayConfig;
use crate::services::media::MediaBus;

pub struct AirPlayService {
    _server_task: JoinHandle<()>,
}

impl AirPlayService {
    /// Starts an AirPlay 1/AP1 receiver when enabled in the external config.
    /// The returned handle must be kept alive for the process lifetime.
    pub async fn start(config: AirPlayConfig, media: MediaBus) -> Result<Option<Self>> {
        if !config.enabled {
            println!("[airplay] disabled by config");
            return Ok(None);
        }

        validate_config(&config)?;

        let volume_gain_bits = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let handler = Arc::new(AplayHandler {
            config: Arc::new(config.clone()),
            volume_gain_bits,
            active_sessions: Arc::new(AtomicUsize::new(0)),
            media,
        });

        let mut builder = RaopServer::builder()
            .name(&config.name)
            .port(config.port)
            .max_clients(config.max_clients);
        if !config.password.trim().is_empty() {
            builder = builder.password(config.password.clone());
        }
        if !config.hwaddr.trim().is_empty() {
            builder = builder.hwaddr(parse_hwaddr(&config.hwaddr)?);
        }

        let mut server = builder
            .build(handler)
            .context("failed to build AirPlay server")?;
        server
            .start()
            .await
            .context("failed to bind AirPlay port or register mDNS")?;

        let actual_port = server.service_info().port;
        let server_task = tokio::spawn(async move {
            let _server = server;
            std::future::pending::<()>().await;
        });

        println!(
            "[airplay] AP1 receiver started: name={:?}, port={}, output={}",
            config.name, actual_port, config.output.device
        );
        Ok(Some(Self {
            _server_task: server_task,
        }))
    }
}

struct AplayHandler {
    config: Arc<AirPlayConfig>,
    volume_gain_bits: Arc<AtomicU32>,
    active_sessions: Arc<AtomicUsize>,
    media: MediaBus,
}

impl AudioHandler for AplayHandler {
    fn audio_init(&self, format: AudioFormat) -> Box<dyn AudioSession> {
        println!(
            "[airplay] audio session: codec={:?}, channels={}, bits={}, sample_rate={}",
            format.codec, format.channels, format.bits, format.sample_rate
        );

        let (child, stdin) = spawn_aplay(&self.config, format);
        if self.active_sessions.fetch_add(1, Ordering::AcqRel) == 0 {
            self.media.set_airplay_active(true);
        }
        Box::new(AplaySession {
            child,
            stdin,
            channels: usize::from(format.channels.max(1)),
            scratch: Vec::with_capacity(8192),
            volume_gain_bits: self.volume_gain_bits.clone(),
            active_sessions: self.active_sessions.clone(),
            media: self.media.clone(),
            interruption_mode: self.config.interruption.mode.clone(),
            duck_gain: self.config.interruption.duck_gain.clamp(0.0, 1.0),
        })
    }

    fn on_volume(&self, volume: f32) {
        self.volume_gain_bits
            .store(airplay_db_to_gain(volume).to_bits(), Ordering::Relaxed);
        println!("[airplay] sender volume: {volume:.2} dB");
    }

    fn on_client_connected(&self, addr: &str) {
        println!("[airplay] client connected: {addr}");
    }

    fn on_client_disconnected(&self, addr: &str) {
        println!("[airplay] client disconnected: {addr}");
    }
}

struct AplaySession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    channels: usize,
    scratch: Vec<u8>,
    volume_gain_bits: Arc<AtomicU32>,
    active_sessions: Arc<AtomicUsize>,
    media: MediaBus,
    interruption_mode: String,
    duck_gain: f32,
}

impl AudioSession for AplaySession {
    fn audio_process(&mut self, samples: &[f32]) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };

        if !samples.is_empty() {
            let mean_square =
                samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
            let db = 20.0 * mean_square.sqrt().max(1.0e-8).log10();
            self.media.set_airplay_level_db(db);
        }
        let interruption_gain = if self.media.wake_active() {
            match self.interruption_mode.as_str() {
                "duck" => self.duck_gain,
                "mute" => 0.0,
                _ => 1.0,
            }
        } else {
            1.0
        };
        let gain =
            f32::from_bits(self.volume_gain_bits.load(Ordering::Relaxed)) * interruption_gain;
        self.scratch.clear();
        self.scratch.reserve(samples.len() * 2);
        for &sample in samples {
            let scaled = (sample * gain).clamp(-1.0, 1.0);
            let pcm = (scaled * i16::MAX as f32) as i16;
            self.scratch.extend_from_slice(&pcm.to_le_bytes());
        }

        if let Err(err) = stdin.write_all(&self.scratch) {
            eprintln!("[airplay] failed writing PCM to aplay: {err}");
            self.stdin.take();
        }
    }

    fn audio_flush(&mut self) {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.flush();
        }
    }
}

impl Drop for AplaySession {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.active_sessions.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.media.set_airplay_active(false);
        }
        println!(
            "[airplay] audio session ended ({} channel(s))",
            self.channels
        );
    }
}

fn spawn_aplay(config: &AirPlayConfig, format: AudioFormat) -> (Option<Child>, Option<ChildStdin>) {
    let mut command = Command::new(&config.output.aplay_path);
    command
        .arg("-q")
        .arg("-D")
        .arg(&config.output.device)
        .arg("-t")
        .arg("raw")
        .arg("-f")
        .arg(&config.output.format)
        .arg("-c")
        .arg(format.channels.max(1).to_string())
        .arg("-r")
        .arg(format.sample_rate.to_string())
        .args(&config.output.extra_args)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    match command.spawn() {
        Ok(mut child) => {
            let stdin = child.stdin.take();
            println!(
                "[airplay] aplay started: path={}, device={}, rate={}, channels={}",
                config.output.aplay_path,
                config.output.device,
                format.sample_rate,
                format.channels.max(1)
            );
            (Some(child), stdin)
        }
        Err(err) => {
            eprintln!("[airplay] failed to start aplay: {err}");
            (None, None)
        }
    }
}

fn validate_config(config: &AirPlayConfig) -> Result<()> {
    if config.name.trim().is_empty() {
        bail!("airplay.name must not be empty");
    }
    if config.max_clients == 0 {
        bail!("airplay.max_clients must be at least 1");
    }
    if config.output.backend != "aplay" {
        bail!(
            "unsupported airplay.output.backend: {}",
            config.output.backend
        );
    }
    if config.output.format != "S16_LE" {
        bail!(
            "unsupported airplay.output.format: {} (only S16_LE is implemented)",
            config.output.format
        );
    }
    if config.output.aplay_path.trim().is_empty() {
        bail!("airplay.output.aplay_path must not be empty");
    }
    if config.output.device.trim().is_empty() {
        bail!("airplay.output.device must not be empty");
    }
    Ok(())
}

fn parse_hwaddr(raw: &str) -> Result<Vec<u8>> {
    let hex = raw
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .collect::<String>();
    if hex.len() != 12 {
        bail!("airplay.hwaddr must contain exactly 6 hexadecimal bytes");
    }

    let mut out = Vec::with_capacity(6);
    for index in (0..12).step_by(2) {
        let byte = u8::from_str_radix(&hex[index..index + 2], 16)
            .with_context(|| format!("invalid airplay.hwaddr byte: {}", &hex[index..index + 2]))?;
        out.push(byte);
    }
    Ok(out)
}

fn airplay_db_to_gain(volume: f32) -> f32 {
    if volume <= -144.0 {
        0.0
    } else {
        10.0f32.powf(volume.min(0.0) / 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{airplay_db_to_gain, parse_hwaddr};

    #[test]
    fn parses_common_hwaddr_formats() {
        assert_eq!(
            parse_hwaddr("02:4f:48:32:50:01").unwrap(),
            vec![0x02, 0x4f, 0x48, 0x32, 0x50, 0x01]
        );
        assert!(parse_hwaddr("02:4f:48").is_err());
    }

    #[test]
    fn converts_airplay_db_volume() {
        assert_eq!(airplay_db_to_gain(-144.0), 0.0);
        assert!((airplay_db_to_gain(0.0) - 1.0).abs() < f32::EPSILON);
        assert!((airplay_db_to_gain(-20.0) - 0.1).abs() < 0.0001);
    }
}

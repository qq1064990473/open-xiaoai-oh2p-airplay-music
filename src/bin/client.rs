use open_xiaoai::services::audio::config::AudioConfig;
use open_xiaoai::services::monitor::kws::KwsMonitor;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;

use open_xiaoai::base::AppError;
use open_xiaoai::base::VERSION;
use open_xiaoai::config::{AppConfig, DEFAULT_CONFIG_PATH};
use open_xiaoai::services::airplay::AirPlayService;
use open_xiaoai::services::audio::play::AudioPlayer;
use open_xiaoai::services::audio::record::AudioRecorder;
use open_xiaoai::services::connect::data::{Event, Request, Response, Stream};
use open_xiaoai::services::connect::handler::MessageHandler;
use open_xiaoai::services::connect::message::{MessageManager, WsStream};
use open_xiaoai::services::connect::rpc::RPC;
use open_xiaoai::services::led::LedController;
use open_xiaoai::services::media::MediaBus;
use open_xiaoai::services::monitor::instruction::InstructionMonitor;
use open_xiaoai::services::monitor::playing::PlayingMonitor;
use open_xiaoai::services::music::{MusicCommandParser, MusicService};
use open_xiaoai::services::native_events::NativeEventService;
use open_xiaoai::services::routing::RoutingService;

struct AppClient {
    kws_monitor: KwsMonitor,
    playing_monitor: PlayingMonitor,
}

impl AppClient {
    pub fn new() -> Self {
        Self {
            kws_monitor: KwsMonitor::new(),
            playing_monitor: PlayingMonitor::new(),
        }
    }

    pub async fn connect(&self, url: &str) -> Result<WsStream, AppError> {
        let (ws_stream, _) = connect_async(url).await?;
        Ok(WsStream::Client(ws_stream))
    }

    pub async fn run(&mut self, url: &str) {
        println!("✅ 已启动: version={VERSION}");
        loop {
            let Ok(ws_stream) = self.connect(url).await else {
                sleep(Duration::from_secs(1)).await;
                continue;
            };
            println!("✅ 已连接: {url:?}");
            self.init(ws_stream).await;
            if let Err(e) = MessageManager::instance().process_messages().await {
                eprintln!("❌ 消息处理异常: {}", e);
            }
            self.dispose().await;
            eprintln!("❌ 已断开连接");
        }
    }

    async fn init(&mut self, ws_stream: WsStream) {
        MessageManager::instance().init(ws_stream).await;
        MessageHandler::<Event>::instance()
            .set_handler(on_event)
            .await;
        MessageHandler::<Stream>::instance()
            .set_handler(on_stream)
            .await;

        let rpc = RPC::instance();
        rpc.add_command("get_version", get_version).await;
        rpc.add_command("run_shell", run_shell).await;
        rpc.add_command("start_play", start_play).await;
        rpc.add_command("stop_play", stop_play).await;
        rpc.add_command("start_recording", start_recording).await;
        rpc.add_command("stop_recording", stop_recording).await;

        self.playing_monitor
            .start(|event| async move {
                MessageManager::instance()
                    .send_event("playing", Some(json!(event)))
                    .await
            })
            .await;

        self.kws_monitor
            .start(|event| async move {
                MessageManager::instance()
                    .send_event("kws", Some(json!(event)))
                    .await
            })
            .await;
    }

    async fn dispose(&mut self) {
        MessageManager::instance().dispose().await;
        let _ = AudioPlayer::instance().stop().await;
        let _ = AudioRecorder::instance().stop_recording().await;
        self.playing_monitor.stop().await;
        self.kws_monitor.stop().await;
    }
}

async fn get_version(_: Request) -> Result<Response, AppError> {
    let data = json!(VERSION.to_string());
    Ok(Response::from_data(data))
}

async fn start_play(request: Request) -> Result<Response, AppError> {
    let config = request
        .payload
        .and_then(|payload| serde_json::from_value::<AudioConfig>(payload).ok());
    AudioPlayer::instance().start(config).await?;
    Ok(Response::success())
}

async fn stop_play(_: Request) -> Result<Response, AppError> {
    AudioPlayer::instance().stop().await?;
    Ok(Response::success())
}

async fn start_recording(request: Request) -> Result<Response, AppError> {
    let config = request
        .payload
        .and_then(|payload| serde_json::from_value::<AudioConfig>(payload).ok());
    AudioRecorder::instance()
        .start_recording(
            |bytes| async {
                MessageManager::instance()
                    .send_stream("record", bytes, None)
                    .await
            },
            config,
        )
        .await?;
    Ok(Response::success())
}

async fn stop_recording(_: Request) -> Result<Response, AppError> {
    AudioRecorder::instance().stop_recording().await?;
    Ok(Response::success())
}

async fn run_shell(request: Request) -> Result<Response, AppError> {
    let script = match request.payload {
        Some(payload) => serde_json::from_value::<String>(payload)?,
        _ => return Err("empty command".into()),
    };
    let res = open_xiaoai::utils::shell::run_shell(script.as_str()).await?;
    Ok(Response::from_data(json!(res)))
}

async fn on_event(event: Event) -> Result<(), AppError> {
    println!("🔥 收到事件: {:?}", event);
    Ok(())
}

async fn on_stream(stream: Stream) -> Result<(), AppError> {
    let Stream { tag, bytes, .. } = stream;
    if tag.as_str() == "play" {
        // 播放接收到的音频流
        let _ = AudioPlayer::instance().play(bytes).await;
    }
    Ok(())
}

struct CliOptions {
    server_url: Option<String>,
    config_path: PathBuf,
    config_explicit: bool,
}

fn parse_cli() -> anyhow::Result<CliOptions> {
    let env_config = std::env::var_os("OPEN_XIAOAI_CONFIG");
    let mut config_explicit = env_config.is_some();
    let mut config_path = env_config
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));
    let mut server_url = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("{arg} requires a file path"))?;
                config_path = PathBuf::from(path);
                config_explicit = true;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            _ if arg.starts_with("--config=") => {
                config_path = PathBuf::from(arg.trim_start_matches("--config="));
                config_explicit = true;
            }
            _ if arg.starts_with('-') => anyhow::bail!("unknown option: {arg}"),
            _ => {
                if server_url.replace(arg).is_some() {
                    anyhow::bail!("only one WebSocket server URL may be supplied");
                }
            }
        }
    }

    Ok(CliOptions {
        server_url,
        config_path,
        config_explicit,
    })
}

fn print_usage() {
    println!(
        "Usage: client [ws://server:4399] [-c /data/open-xiaoai/client.json]\n\
         Environment: OPEN_XIAOAI_CONFIG=/path/to/client.json"
    );
}

fn load_config(path: &Path, explicit: bool) -> anyhow::Result<AppConfig> {
    if path.exists() {
        println!("[config] loading {}", path.display());
        return AppConfig::load(path);
    }
    if explicit {
        anyhow::bail!("config file does not exist: {}", path.display());
    }

    eprintln!(
        "[config] {} not found; using compatibility defaults (AirPlay disabled)",
        path.display()
    );
    Ok(AppConfig::default())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = parse_cli()?;
    let config = load_config(&options.config_path, options.config_explicit)?;
    let server_url = options
        .server_url
        .unwrap_or_else(|| config.server_url.clone());
    if server_url.trim().is_empty() {
        anyhow::bail!("server_url must not be empty");
    }

    let (media, media_events) = MediaBus::new();
    let music = MusicService::start(
        config.music.clone(),
        config.audio_policy.clone(),
        media.clone(),
        media_events,
    )?;
    if config.music.enabled {
        println!("[music] deterministic local music routing enabled");
    } else {
        println!("[music] disabled by config");
    }

    let routing = RoutingService::new(
        music.clone(),
        MusicCommandParser::new(config.music.clone()),
        media.clone(),
        config.music.player.native_stop_command.clone(),
    );
    let mut instruction_monitor = InstructionMonitor::new();
    instruction_monitor
        .start(move |event| {
            let routing = routing.clone();
            async move {
                routing.process(&event).await?;
                let _ = MessageManager::instance()
                    .send_event("instruction", Some(json!(event)))
                    .await;
                Ok(())
            }
        })
        .await;

    let _led_controller = LedController::start(config.led.clone(), media.clone());
    let _native_event_service = NativeEventService::start(
        config.native_events.clone(),
        config.led.clone(),
        config.music.player.native_stop_command.clone(),
        music,
        media.clone(),
    );

    // Local media services deliberately live outside the WebSocket reconnect loop.
    let _airplay_service = match AirPlayService::start(config.airplay.clone(), media).await {
        Ok(service) => service,
        Err(err) => {
            eprintln!("[airplay] startup failed; WebSocket client will continue: {err:#}");
            None
        }
    };

    AppClient::new().run(&server_url).await;
    Ok(())
}

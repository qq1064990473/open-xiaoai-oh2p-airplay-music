use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio::process::Command;

use crate::config::HomeAssistantConfig;

#[derive(Clone)]
pub struct HomeAssistantService {
    config: HomeAssistantConfig,
    client: Client,
    token: String,
    conversation_url: String,
    api_url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    Handled { speech: Option<String> },
    Fallback { reason: String },
    Failed { reason: String },
}

impl HomeAssistantService {
    pub async fn connect(config: HomeAssistantConfig) -> Result<Option<Self>> {
        if !config.enabled {
            return Ok(None);
        }
        validate_base_url(&config)?;
        let token = tokio::fs::read_to_string(&config.token_file)
            .await
            .with_context(|| format!("failed to read HA token file: {}", config.token_file))?
            .trim()
            .to_string();
        anyhow::ensure!(!token.is_empty(), "HA token file is empty");

        let base_url = config.base_url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(config.connect_timeout_ms.max(100)))
            .timeout(Duration::from_millis(config.request_timeout_ms.max(300)))
            .user_agent(concat!("open-xiaoai/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build HA HTTP client")?;
        let service = Self {
            config,
            client,
            token,
            conversation_url: format!("{base_url}/api/conversation/process"),
            api_url: format!("{base_url}/api/"),
        };
        service.wait_until_ready().await?;
        Ok(Some(service))
    }

    async fn wait_until_ready(&self) -> Result<()> {
        let attempts = self.config.startup_retry_attempts.max(1);
        let retry_delay = Duration::from_millis(self.config.startup_retry_interval_ms.max(100));

        for attempt in 1..=attempts {
            match self.health_check().await {
                Ok(status) if status.is_success() => return Ok(()),
                Ok(status) if status.is_server_error() && attempt < attempts => {
                    eprintln!(
                        "[ha] startup health check {attempt}/{attempts} returned {status}; retrying"
                    );
                }
                Ok(status) => anyhow::bail!("HA health check returned {status}"),
                Err(err)
                    if (err.is_connect() || err.is_timeout()) && attempt < attempts =>
                {
                    eprintln!(
                        "[ha] startup health check {attempt}/{attempts} failed: {err}; retrying"
                    );
                }
                Err(err) => return Err(err).context("HA health check failed"),
            }
            tokio::time::sleep(retry_delay).await;
        }
        unreachable!("startup health check loop always returns")
    }

    async fn health_check(&self) -> reqwest::Result<StatusCode> {
        self
            .client
            .get(&self.api_url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map(|response| response.status())
    }

    pub async fn route(&self, text: &str) -> RouteOutcome {
        let mut body = json!({
            "text": text,
            "language": self.config.language,
        });
        if !self.config.agent_id.trim().is_empty() {
            body["agent_id"] = Value::String(self.config.agent_id.clone());
        }

        let response = match self
            .client
            .post(&self.conversation_url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) if err.is_timeout() => {
                let reason = format!("HA request timed out: {err}");
                return if self.config.fallback_on_timeout {
                    RouteOutcome::Fallback { reason }
                } else {
                    RouteOutcome::Failed { reason }
                };
            }
            Err(err) => {
                return RouteOutcome::Fallback {
                    reason: format!("HA request failed: {err}"),
                };
            }
        };

        if response.status() == StatusCode::UNAUTHORIZED {
            return RouteOutcome::Fallback {
                reason: "HA rejected the access token".into(),
            };
        }
        if !response.status().is_success() {
            return RouteOutcome::Fallback {
                reason: format!("HA returned HTTP {}", response.status()),
            };
        }
        let value = match response.json::<Value>().await {
            Ok(value) => value,
            Err(err) => {
                return RouteOutcome::Fallback {
                    reason: format!("invalid HA response: {err}"),
                };
            }
        };
        parse_conversation_response(&value)
    }

    pub fn fallback_to_xiaoai(&self) -> bool {
        self.config.fallback_to_xiaoai
    }

    pub fn speak_response(&self) -> bool {
        self.config.speak_response
    }

    pub fn failure_speech(&self) -> &str {
        self.config.failure_speech.trim()
    }

    pub fn ready_file(&self) -> &Path {
        Path::new(&self.config.ready_file)
    }

    pub fn routing_enabled(&self) -> bool {
        Path::new(&self.config.lab_file).is_file()
    }
}

pub async fn ask_xiaoai(text: &str) -> Result<bool> {
    call_ai_service(json!({
        "tts": 1,
        "nlp": 1,
        "nlp_text": text,
    }))
    .await
}

pub async fn speak_text(text: &str) -> Result<bool> {
    call_ai_service(json!({
        "tts": 1,
        "nlp": 0,
        "tts_text": text,
        "tts_play": 1,
    }))
    .await
}

async fn call_ai_service(payload: Value) -> Result<bool> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("/bin/ubus")
            .args(["-t", "5", "call", "mibrain", "ai_service"])
            .arg(payload.to_string())
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .context("mibrain ai_service timed out")?
    .context("failed to run mibrain ai_service")?;
    anyhow::ensure!(
        output.status.success(),
        "mibrain ai_service exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let value: Value = serde_json::from_slice(&output.stdout)
        .context("mibrain ai_service returned invalid JSON")?;
    Ok(value["code"].as_i64() == Some(0))
}

fn validate_base_url(config: &HomeAssistantConfig) -> Result<()> {
    let url = config.base_url.trim();
    anyhow::ensure!(
        url.starts_with("http://") || url.starts_with("https://"),
        "HA base_url must start with http:// or https://"
    );
    anyhow::ensure!(
        !url.starts_with("http://") || config.allow_insecure_http,
        "HA base_url uses insecure HTTP; set allow_insecure_http only on a trusted network"
    );
    Ok(())
}

fn parse_conversation_response(value: &Value) -> RouteOutcome {
    let response = &value["response"];
    let response_type = response["response_type"].as_str().unwrap_or("error");
    if response_type == "error" {
        let code = response["data"]["code"].as_str().unwrap_or("unknown_error");
        return RouteOutcome::Fallback {
            reason: format!("HA conversation error: {code}"),
        };
    }
    let speech = response["speech"]["plain"]["speech"]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    RouteOutcome::Handled { speech }
}

#[cfg(test)]
mod tests {
    use super::{parse_conversation_response, validate_base_url, RouteOutcome};
    use crate::config::HomeAssistantConfig;
    use serde_json::json;

    #[test]
    fn parses_handled_and_no_match_responses() {
        let handled = json!({"response":{
            "response_type":"action_done",
            "speech":{"plain":{"speech":"已打开台灯"}}
        }});
        assert_eq!(
            parse_conversation_response(&handled),
            RouteOutcome::Handled {
                speech: Some("已打开台灯".into())
            }
        );
        let unmatched = json!({"response":{
            "response_type":"error",
            "data":{"code":"no_intent_match"}
        }});
        assert!(matches!(
            parse_conversation_response(&unmatched),
            RouteOutcome::Fallback { reason } if reason.contains("no_intent_match")
        ));
    }

    #[test]
    fn rejects_insecure_http_without_explicit_opt_in() {
        let mut config = HomeAssistantConfig::default();
        config.base_url = "http://ha.example:8123".into();
        assert!(validate_base_url(&config).is_err());
        config.allow_insecure_http = true;
        assert!(validate_base_url(&config).is_ok());
    }
}

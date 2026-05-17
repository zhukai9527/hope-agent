//! Volcengine (火山引擎 / 字节豆包) streaming ASR WebSocket — BigModel
//! generation (`/api/v3/sauc/bigmodel`).
//!
//! Auth headers on the WS upgrade:
//! - `X-Api-App-Key: <app_key>` — `extra.app_key`
//! - `X-Api-Access-Key: <access_key>` — `provider.api_key`
//! - `X-Api-Resource-Id: <resource>` — defaults to
//!   `volc.bigasr.sauc.duration`, override via `extra.resource_id`
//! - `X-Api-Request-Id: <UUID>` — fresh per session
//!
//! Binary framing (BigModel protocol, all frames):
//! ```text
//! byte 0: (version<<4) | header_size_in_4byte_units   // 0x11
//! byte 1: (msg_type<<4) | flags                       // see below
//! byte 2: (serialization<<4) | compression            // 0x11 = JSON/gzip,
//!                                                     // 0xf1 = raw/gzip
//! byte 3: reserved (0)
//! bytes 4..8:  payload size, u32 big-endian
//! bytes 8..  : gzip(payload)
//! ```
//! Message types: 0x1 client config, 0x2 client audio, 0x4 server response,
//! 0x9 server error. Audio flags: 0x1 = positive seq (continue), 0x2 =
//! negative seq (last).
//!
//! Server JSON shape after gunzip:
//! `{"result":{"text":"...","utterances":[{"text":"...","definite":bool,
//!   "start_time":ms,"end_time":ms}]}}`
//! `definite=true` ⇒ final utterance; partial deltas come as multiple
//! `definite=false` frames sharing prefix text.

use std::io::Read;

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use uuid::Uuid;

use crate::provider::AuthProfile;
use crate::security::ssrf::{check_url, SsrfPolicy};
use crate::stt::errors::{SttError, SttResult};
use crate::stt::types::{SttModelConfig, SttProviderConfig, TranscriptDelta, TranscriptOptions};

const MAX_WS_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_WS_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_RESOURCE_ID: &str = "volc.bigasr.sauc.duration";

const MSG_CLIENT_CONFIG: u8 = 0x1;
const MSG_CLIENT_AUDIO: u8 = 0x2;
const MSG_SERVER_RESPONSE: u8 = 0x4;
const MSG_SERVER_ERROR: u8 = 0x9;

const FLAG_POSITIVE: u8 = 0x1;
const FLAG_NEGATIVE: u8 = 0x2;

const SER_JSON: u8 = 0x1;
const SER_RAW: u8 = 0xf;
const COMPRESS_GZIP: u8 = 0x1;

pub async fn open_stream(
    provider: &SttProviderConfig,
    model: &SttModelConfig,
    profile: &AuthProfile,
    options: &TranscriptOptions,
) -> SttResult<super::SttStream> {
    let app_key = provider
        .extra
        .get("app_key")
        .ok_or_else(|| SttError::Other("Volcengine provider requires `extra.app_key`".into()))?
        .clone();
    let resource_id = provider
        .extra
        .get("resource_id")
        .map(String::as_str)
        .unwrap_or(DEFAULT_RESOURCE_ID)
        .to_string();

    let base = provider.resolve_base_url(profile).trim_end_matches('/');
    let url = format!("{}/api/v3/sauc/bigmodel", base);

    let https_twin = ws_to_https_twin(&url)?;
    let cfg = crate::config::cached_config();
    let policy = if provider.allow_private_network {
        SsrfPolicy::AllowPrivate
    } else {
        cfg.ssrf.default_policy
    };
    check_url(&https_twin, policy, &cfg.ssrf.trusted_hosts)
        .await
        .map_err(|e| SttError::SsrfBlocked(e.to_string()))?;

    let request_id = Uuid::new_v4().simple().to_string();

    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| SttError::Other(format!("Invalid Volcengine URL: {e}")))?;
    let headers = request.headers_mut();
    headers.insert(
        "X-Api-App-Key",
        app_key
            .parse()
            .map_err(|e| SttError::Other(format!("Bad X-Api-App-Key value: {e}")))?,
    );
    headers.insert(
        "X-Api-Access-Key",
        profile
            .api_key
            .parse()
            .map_err(|e| SttError::Other(format!("Bad X-Api-Access-Key value: {e}")))?,
    );
    headers.insert(
        "X-Api-Resource-Id",
        resource_id
            .parse()
            .map_err(|e| SttError::Other(format!("Bad X-Api-Resource-Id value: {e}")))?,
    );
    headers.insert(
        "X-Api-Request-Id",
        request_id
            .parse()
            .map_err(|e| SttError::Other(format!("Bad X-Api-Request-Id value: {e}")))?,
    );

    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_WS_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_WS_FRAME_BYTES));
    let (ws, _resp) = tokio_tungstenite::connect_async_with_config(request, Some(ws_config), false)
        .await
        .map_err(|e| SttError::Network(format!("Volcengine WS connect failed: {e}")))?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    let language = options
        .language
        .as_deref()
        .filter(|l| !l.is_empty())
        .unwrap_or("zh-CN")
        .to_string();
    let sample_rate = options.sample_rate_hz.unwrap_or(16_000);
    let cfg_body = json!({
        "user": { "uid": request_id },
        "audio": {
            "format": "pcm",
            "sample_rate": sample_rate,
            "bits": 16,
            "channel": 1,
        },
        "request": {
            "model_name": model.id,
            "language": language,
            "enable_itn": true,
            "enable_punc": true,
        }
    });
    let config_frame = build_frame(
        MSG_CLIENT_CONFIG,
        0,
        SER_JSON,
        &cfg_body.to_string().into_bytes(),
    )?;
    ws_sink
        .send(Message::Binary(config_frame.into()))
        .await
        .map_err(|e| SttError::Network(format!("Volcengine config send: {e}")))?;

    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<u8>>(64);
    let (delta_tx, delta_rx) = mpsc::channel::<Result<TranscriptDelta, SttError>>(64);

    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            match build_frame(MSG_CLIENT_AUDIO, FLAG_POSITIVE, SER_RAW, &chunk) {
                Ok(frame) => {
                    if ws_sink.send(Message::Binary(frame.into())).await.is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        // EOS: empty audio frame with negative-sequence flag.
        if let Ok(frame) = build_frame(MSG_CLIENT_AUDIO, FLAG_NEGATIVE, SER_RAW, &[]) {
            let _ = ws_sink.send(Message::Binary(frame.into())).await;
        }
        let _ = ws_sink.send(Message::Close(None)).await;
    });

    let session_id = String::new();
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(bytes)) => match parse_frame(&bytes) {
                    Ok(payload) => {
                        if let Some(delta) = parse_response(&session_id, &payload) {
                            if delta_tx.send(Ok(delta)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = delta_tx.send(Err(e)).await;
                        break;
                    }
                },
                Ok(Message::Text(_))
                | Ok(Message::Frame(_))
                | Ok(Message::Ping(_))
                | Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    let _ = delta_tx
                        .send(Err(SttError::Network(format!("Volcengine WS recv: {e}"))))
                        .await;
                    break;
                }
            }
        }
    });

    Ok(super::SttStream { audio_tx, delta_rx })
}

fn build_frame(msg_type: u8, flags: u8, serialization: u8, payload: &[u8]) -> SttResult<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, payload)
        .map_err(|e| SttError::Other(format!("Volcengine gzip encode: {e}")))?;
    let compressed = encoder
        .finish()
        .map_err(|e| SttError::Other(format!("Volcengine gzip finalize: {e}")))?;

    let mut frame = Vec::with_capacity(8 + compressed.len());
    // protocol_version=1, header_size=1 (4 bytes total header)
    frame.push((1 << 4) | 1);
    frame.push((msg_type << 4) | (flags & 0x0f));
    frame.push((serialization << 4) | COMPRESS_GZIP);
    frame.push(0); // reserved
    frame.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    frame.extend_from_slice(&compressed);
    Ok(frame)
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedFrame {
    msg_type: u8,
    flags: u8,
    serialization: u8,
    body: Vec<u8>,
}

fn parse_frame(bytes: &[u8]) -> SttResult<ParsedFrame> {
    if bytes.len() < 8 {
        return Err(SttError::Other("Volcengine frame too short".into()));
    }
    let header_words = bytes[0] & 0x0f;
    let header_len = header_words as usize * 4;
    if header_len == 0 || bytes.len() < header_len + 4 {
        return Err(SttError::Other(
            "Volcengine frame missing payload prefix".into(),
        ));
    }
    let msg_type = (bytes[1] >> 4) & 0x0f;
    let flags = bytes[1] & 0x0f;
    let ser_compress = bytes[2];
    let serialization = (ser_compress >> 4) & 0x0f;
    let compression = ser_compress & 0x0f;

    let size_offset = header_len;
    let body_size = u32::from_be_bytes([
        bytes[size_offset],
        bytes[size_offset + 1],
        bytes[size_offset + 2],
        bytes[size_offset + 3],
    ]) as usize;
    let body_start = size_offset + 4;
    if bytes.len() < body_start + body_size {
        return Err(SttError::Other(
            "Volcengine frame body shorter than declared size".into(),
        ));
    }
    let raw_body = &bytes[body_start..body_start + body_size];

    let body = match compression {
        COMPRESS_GZIP => {
            let mut decoder = GzDecoder::new(raw_body);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| SttError::Other(format!("Volcengine gzip decode: {e}")))?;
            out
        }
        0 => raw_body.to_vec(),
        other => {
            return Err(SttError::Other(format!(
                "Volcengine unsupported compression {other}"
            )))
        }
    };

    if msg_type == MSG_SERVER_ERROR {
        let text = String::from_utf8_lossy(&body).to_string();
        return Err(SttError::ProviderUnavailable(format!(
            "Volcengine server error: {text}"
        )));
    }

    Ok(ParsedFrame {
        msg_type,
        flags,
        serialization,
        body,
    })
}

fn parse_response(session_id: &str, frame: &ParsedFrame) -> Option<TranscriptDelta> {
    if frame.msg_type != MSG_SERVER_RESPONSE || frame.serialization != SER_JSON {
        return None;
    }
    let value: Value = serde_json::from_slice(&frame.body).ok()?;
    let result = value.get("result")?;
    // Prefer the most recent utterance for partial classification; fall
    // back to the rolling `text` field for cumulative final.
    if let Some(utts) = result.get("utterances").and_then(|v| v.as_array()) {
        if let Some(latest) = utts.last() {
            let text = latest.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if text.is_empty() {
                return None;
            }
            let definite = latest
                .get("definite")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let start_ms = latest.get("start_time").and_then(|v| v.as_u64());
            let end_ms = latest.get("end_time").and_then(|v| v.as_u64());
            return Some(TranscriptDelta {
                session_id: session_id.to_string(),
                text: text.to_string(),
                is_final: definite,
                start_ms,
                end_ms,
                confidence: None,
                language: None,
                accumulated: None,
            });
        }
    }
    let text = result.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return None;
    }
    Some(TranscriptDelta {
        session_id: session_id.to_string(),
        text: text.to_string(),
        is_final: false,
        start_ms: None,
        end_ms: None,
        confidence: None,
        language: None,
        accumulated: None,
    })
}

fn ws_to_https_twin(url: &str) -> SttResult<String> {
    let mut parsed = url::Url::parse(url)
        .map_err(|e| SttError::Other(format!("Invalid Volcengine URL: {e}")))?;
    let new_scheme = match parsed.scheme() {
        "wss" => Some("https"),
        "ws" => Some("http"),
        _ => None,
    };
    if let Some(scheme) = new_scheme {
        parsed
            .set_scheme(scheme)
            .map_err(|_| SttError::Other("Failed to derive SSRF twin URL".into()))?;
    }
    Ok(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_frame_roundtrip_json() {
        let body = br#"{"hello":"world"}"#;
        let bytes = build_frame(MSG_SERVER_RESPONSE, 0, SER_JSON, body).unwrap();
        let parsed = parse_frame(&bytes).unwrap();
        assert_eq!(parsed.msg_type, MSG_SERVER_RESPONSE);
        assert_eq!(parsed.serialization, SER_JSON);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn build_and_parse_frame_roundtrip_binary_pcm() {
        let body: Vec<u8> = (0..4096).map(|i| (i & 0xff) as u8).collect();
        let bytes = build_frame(MSG_CLIENT_AUDIO, FLAG_POSITIVE, SER_RAW, &body).unwrap();
        let parsed = parse_frame(&bytes).unwrap();
        assert_eq!(parsed.msg_type, MSG_CLIENT_AUDIO);
        assert_eq!(parsed.flags, FLAG_POSITIVE);
        assert_eq!(parsed.serialization, SER_RAW);
        assert_eq!(parsed.body, body);
    }

    #[test]
    fn frame_header_first_byte_is_one_one() {
        let bytes = build_frame(MSG_CLIENT_CONFIG, 0, SER_JSON, b"{}").unwrap();
        assert_eq!(bytes[0], 0x11);
    }

    #[test]
    fn parse_error_msg_type_surfaces_provider_unavailable() {
        // Build a server-error frame with a plaintext error body so the
        // parser surfaces the string via SttError::ProviderUnavailable.
        let body = b"app key invalid";
        let bytes = build_frame(MSG_SERVER_ERROR, 0, SER_JSON, body).unwrap();
        let err = parse_frame(&bytes).unwrap_err();
        match err {
            SttError::ProviderUnavailable(msg) => assert!(msg.contains("app key invalid")),
            other => panic!("expected ProviderUnavailable, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_returns_partial_from_utterance() {
        let frame = ParsedFrame {
            msg_type: MSG_SERVER_RESPONSE,
            flags: 0,
            serialization: SER_JSON,
            body: r#"{"result":{"text":"你好世界","utterances":[
                {"text":"你好","definite":false,"start_time":0,"end_time":500}
            ]}}"#
                .as_bytes()
                .to_vec(),
        };
        let d = parse_response("s", &frame).unwrap();
        assert_eq!(d.text, "你好");
        assert!(!d.is_final);
        assert_eq!(d.start_ms, Some(0));
        assert_eq!(d.end_ms, Some(500));
    }

    #[test]
    fn parse_response_marks_final_when_definite() {
        let frame = ParsedFrame {
            msg_type: MSG_SERVER_RESPONSE,
            flags: 0,
            serialization: SER_JSON,
            body: r#"{"result":{"utterances":[{"text":"完成","definite":true}]}}"#
                .as_bytes()
                .to_vec(),
        };
        let d = parse_response("s", &frame).unwrap();
        assert!(d.is_final);
        assert_eq!(d.text, "完成");
    }

    #[test]
    fn parse_response_falls_back_to_rolling_text() {
        let frame = ParsedFrame {
            msg_type: MSG_SERVER_RESPONSE,
            flags: 0,
            serialization: SER_JSON,
            body: br#"{"result":{"text":"hello"}}"#.to_vec(),
        };
        let d = parse_response("s", &frame).unwrap();
        assert_eq!(d.text, "hello");
        assert!(!d.is_final);
    }
}

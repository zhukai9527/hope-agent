//! Deepgram realtime WebSocket STT.
//!
//! `wss://api.deepgram.com/v1/listen?model=...&interim_results=true&punctuate=true&...`
//! Auth: `Authorization: Token <api_key>`.
//!
//! Wire shape:
//! - Upstream: binary frames carrying raw audio bytes (PCM16 / Opus /
//!   WebM-Opus all accepted — Deepgram auto-detects from the first chunk).
//! - Downstream: JSON text frames `{ "channel": { "alternatives": [{ "transcript": "...", "confidence": ... }] }, "is_final": bool, ... }`.
//!   `speech_final` / `is_final` distinguishes a stable utterance edge from
//!   an interim partial.

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::provider::AuthProfile;
use crate::security::ssrf::{check_url, SsrfPolicy};
use crate::stt::errors::{SttError, SttResult};
use crate::stt::types::{
    ActiveSttModel, SttModelConfig, SttProviderConfig, Transcript, TranscriptDelta,
    TranscriptOptions,
};

/// Open a Deepgram streaming session. Returns `(audio_tx, delta_rx)`:
/// callers push raw audio bytes into `audio_tx` and drain transcript
/// deltas off `delta_rx`. Closing `audio_tx` (drop) signals end-of-audio;
/// the engine drains the upstream WS until its final delta then closes
/// `delta_rx`.
///
/// The two background tasks (audio → WS upstream / WS downstream → delta_rx)
/// share a `tokio_util::sync::CancellationToken`-equivalent flag via the
/// channels themselves — drop either end and the task notices.
pub async fn open_stream(
    provider: &SttProviderConfig,
    model: &SttModelConfig,
    profile: &AuthProfile,
    options: &TranscriptOptions,
) -> SttResult<DeepgramStream> {
    let base = provider.resolve_base_url(profile).trim_end_matches('/');
    let mut url = format!("{}/v1/listen?model={}&interim_results=true", base, model.id);
    if let Some(lang) = &options.language {
        if !lang.is_empty() {
            url.push_str(&format!("&language={}", urlencoding_encode(lang)));
        }
    }
    if options.punctuation.unwrap_or(true) {
        url.push_str("&punctuate=true");
    }
    if options.diarization.unwrap_or(false) {
        url.push_str("&diarize=true");
    }
    if let Some(sr) = options.sample_rate_hz {
        // Deepgram needs encoding+sample_rate when the audio is raw PCM. For
        // container-wrapped audio (WebM/Opus, MP3) it autodetects; we forward
        // sample_rate as a hint either way.
        url.push_str(&format!("&sample_rate={}", sr));
    }

    // SSRF check against the http(s) form of the URL — `check_url` rejects
    // ws/wss schemes today. We re-derive an https:// twin for policy only;
    // the actual WS connect still uses the original wss:// URL.
    let https_twin = url
        .replacen("wss://", "https://", 1)
        .replacen("ws://", "http://", 1);
    let cfg = crate::config::cached_config();
    let policy = if provider.allow_private_network {
        SsrfPolicy::AllowPrivate
    } else {
        cfg.ssrf.default_policy
    };
    check_url(&https_twin, policy, &cfg.ssrf.trusted_hosts)
        .await
        .map_err(|e| SttError::SsrfBlocked(e.to_string()))?;

    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| SttError::Other(format!("Invalid Deepgram URL: {e}")))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Token {}", profile.api_key)
            .parse()
            .map_err(|e| SttError::Other(format!("Bad auth header: {e}")))?,
    );

    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| SttError::Network(format!("Deepgram WS connect failed: {e}")))?;
    let (mut ws_sink, mut ws_stream) = ws.split();

    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<u8>>(64);
    let (delta_tx, delta_rx) = mpsc::channel::<Result<TranscriptDelta, SttError>>(64);

    // Audio uplink — forward raw bytes from caller into WS binary frames.
    // When the caller drops `audio_tx`, audio_rx.recv() returns None →
    // we send Deepgram's "close stream" sentinel `{"type":"CloseStream"}`
    // and let the downstream task drain remaining final transcripts.
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            if ws_sink.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
        // Best-effort EOS hint then graceful close.
        let _ = ws_sink
            .send(Message::Text(r#"{"type":"CloseStream"}"#.into()))
            .await;
        let _ = ws_sink.send(Message::Close(None)).await;
    });

    // Session_id is filled in by the SttSessionManager after open_stream
    // returns — we don't need it on the engine side. Use empty placeholder.
    let session_id = String::new();
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Some(delta) = parse_message(&session_id, &text) {
                        if delta_tx.send(Ok(delta)).await.is_err() {
                            break;
                        }
                    }
                }
                Ok(Message::Binary(_))
                | Ok(Message::Frame(_))
                | Ok(Message::Ping(_))
                | Ok(Message::Pong(_)) => {
                    // ignore
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    let _ = delta_tx
                        .send(Err(SttError::Network(format!("Deepgram WS recv: {e}"))))
                        .await;
                    break;
                }
            }
        }
    });

    Ok(DeepgramStream { audio_tx, delta_rx })
}

pub struct DeepgramStream {
    pub audio_tx: mpsc::Sender<Vec<u8>>,
    pub delta_rx: mpsc::Receiver<Result<TranscriptDelta, SttError>>,
}

fn parse_message(session_id: &str, raw: &str) -> Option<TranscriptDelta> {
    let value: Value = serde_json::from_str(raw).ok()?;
    let alt = value.pointer("/channel/alternatives/0")?;
    let text = alt.get("transcript").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return None;
    }
    let is_final = value
        .get("is_final")
        .and_then(|v| v.as_bool())
        .or_else(|| value.get("speech_final").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    let confidence = alt
        .get("confidence")
        .and_then(|v| v.as_f64())
        .map(|c| c as f32);
    let start = value.get("start").and_then(|v| v.as_f64());
    let duration = value.get("duration").and_then(|v| v.as_f64());
    let start_ms = start.map(|s| (s * 1000.0).max(0.0) as u64);
    let end_ms = match (start, duration) {
        (Some(s), Some(d)) => Some(((s + d) * 1000.0).max(0.0) as u64),
        _ => None,
    };
    Some(TranscriptDelta {
        session_id: session_id.to_string(),
        text: text.to_string(),
        is_final,
        start_ms,
        end_ms,
        confidence,
        language: None,
        accumulated: None,
    })
}

fn urlencoding_encode(s: &str) -> String {
    // tiny inline encoder — `url::form_urlencoded` would pull a Serializer
    // for one field, this is enough for the BCP-47 language hints we pass.
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// Batch path for Deepgram — collect the full payload, post once, take
/// the final transcript. Phase 2 stubs this out so the failover chain
/// stays uniform; full HTTP /v1/listen batch will land alongside the
/// Phase 6 cleanup.
#[allow(dead_code)]
pub async fn transcribe_batch(
    _provider: &SttProviderConfig,
    _model: &SttModelConfig,
    _profile: &AuthProfile,
    _audio: crate::stt::AudioPayload,
    _options: &TranscriptOptions,
) -> SttResult<Transcript> {
    Err(SttError::Other(
        "Deepgram batch transcription is not implemented yet (Phase 6)".to_string(),
    ))
}

#[allow(dead_code)]
pub(crate) fn classify_active(_active: &ActiveSttModel) {
    // placeholder to keep ActiveSttModel imported when only batch stub is used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_partial_transcript() {
        let raw = r#"{
            "channel":{"alternatives":[{"transcript":"hello","confidence":0.97}]},
            "is_final":false,
            "start":1.2,
            "duration":0.4
        }"#;
        let d = parse_message("sess-1", raw).expect("partial should parse");
        assert_eq!(d.text, "hello");
        assert!(!d.is_final);
        assert_eq!(d.start_ms, Some(1200));
        assert_eq!(d.end_ms, Some(1600));
        assert!(d.confidence.unwrap() > 0.9);
        assert_eq!(d.session_id, "sess-1");
    }

    #[test]
    fn parse_marks_final_via_is_final() {
        let raw = r#"{"channel":{"alternatives":[{"transcript":"hi"}]},"is_final":true}"#;
        let d = parse_message("s", raw).unwrap();
        assert!(d.is_final);
    }

    #[test]
    fn parse_falls_back_to_speech_final() {
        let raw = r#"{"channel":{"alternatives":[{"transcript":"bye"}]},"speech_final":true}"#;
        let d = parse_message("s", raw).unwrap();
        assert!(d.is_final);
    }

    #[test]
    fn parse_skips_empty_transcripts() {
        let raw = r#"{"channel":{"alternatives":[{"transcript":""}]},"is_final":false}"#;
        assert!(parse_message("s", raw).is_none());
    }

    #[test]
    fn urlencode_passes_through_alphanumeric() {
        assert_eq!(urlencoding_encode("en-US"), "en-US");
        assert_eq!(urlencoding_encode("zh-Hans"), "zh-Hans");
        assert_eq!(urlencoding_encode("a b"), "a%20b");
    }
}

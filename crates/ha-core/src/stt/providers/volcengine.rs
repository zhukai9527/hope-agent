//! Volcengine (火山引擎 / 豆包) streaming ASR WebSocket.
//!
//! `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel` (big-model) or
//! `wss://openspeech.bytedance.com/api/v2/asr` (standard).
//! Auth headers: `X-Api-Resource-Id`, `X-Api-Access-Key`, `X-Api-App-Key`,
//! `X-Api-Request-Id` (a fresh UUID per session).
//!
//! Wire shape:
//! - Upstream: custom binary framing — each frame begins with a 4-byte
//!   header `(version<<4 | header_size, message_type<<4 | flags,
//!   serialization<<4 | compression, reserved)` followed by payload-size
//!   prefix and gzip-compressed JSON / raw PCM body.
//! - Downstream: same framing returning `payload_msg` with
//!   `result.utterances[*].{text, definite, start_time, end_time}`.
//!
//! `app_id` and `cluster` belong in `provider.extra` (e.g.
//! `extra.app_id="123"`, `extra.cluster="volcano_tts"`).

use crate::provider::AuthProfile;
use crate::stt::errors::{SttError, SttResult};
use crate::stt::types::{SttModelConfig, SttProviderConfig, TranscriptOptions};

#[allow(dead_code)]
pub async fn open_stream(
    _provider: &SttProviderConfig,
    _model: &SttModelConfig,
    _profile: &AuthProfile,
    _options: &TranscriptOptions,
) -> SttResult<super::SttStream> {
    Err(SttError::Other(
        "Volcengine (Doubao) streaming is not implemented yet".into(),
    ))
}

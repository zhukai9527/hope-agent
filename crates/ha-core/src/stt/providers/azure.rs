//! Azure Speech Service realtime WebSocket transcription.
//!
//! `wss://{region}.stt.speech.microsoft.com/speech/recognition/conversation/cognitiveservices/v1`
//! Auth header: `Ocp-Apim-Subscription-Key: <subscription_key>` (alternative:
//! `Authorization: Bearer <jwt>` after exchanging the key for a token, but
//! we use the subscription key directly to avoid a second round-trip).
//!
//! Wire shape:
//! - Upstream: chunked PCM audio (16-bit, 16 kHz, mono) wrapped in a
//!   custom binary protocol where each frame is preceded by HTTP-style
//!   headers describing `X-RequestId`, `Content-Type`, etc. Each chunk
//!   is bounded by an `X-Timestamp` per Azure's WS contract.
//! - Downstream: JSON text frames with similar headers prefix:
//!   - `speech.hypothesis` (partial)
//!   - `speech.phrase` (final, with `RecognitionStatus: Success`)
//!   - `turn.start` / `turn.end` lifecycle events

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
        "Azure Speech streaming is not implemented yet".into(),
    ))
}

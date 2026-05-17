//! iFlytek (讯飞) IAT realtime WebSocket transcription.
//!
//! `wss://iat-api.xfyun.cn/v2/iat` — URL must include `host`, `date`, and
//! HMAC-SHA256 `authorization` query params per the iFlytek auth contract
//! (Hawk-flavoured signature derived from `api_key` + `api_secret`).
//!
//! Wire shape:
//! - Upstream: JSON text frames `{"common":{"app_id":...},"business":{...},"data":{"status":N,"format":"audio/L16;rate=16000","encoding":"raw","audio":"<base64 PCM16>"}}`
//!   where `status=0` is "first frame", `status=1` continuation, `status=2`
//!   last frame (signals EOS to the server).
//! - Downstream: JSON text frames with `data.result.ws[*].cw[*].w` words,
//!   `data.result.pgs` (partial vs replace), `data.status=2` for final.

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
        "iFlytek IAT streaming is not implemented yet".into(),
    ))
}

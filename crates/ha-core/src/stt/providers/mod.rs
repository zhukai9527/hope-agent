//! STT provider implementations.
//!
//! Batch path: `openai` (covers OpenAI Whisper, gpt-4o-transcribe batch,
//! Groq, DashScope/Qwen3-ASR via compatible mode, StepFun, SiliconFlow,
//! whisper.cpp server, faster-whisper-server, FunASR + OpenAI wrapper,
//! sherpa-onnx server).
//!
//! Streaming WS path: Deepgram / AssemblyAI / Azure Speech / iFlytek IAT /
//! Volcengine. Each module exposes `open_stream` returning a `SttStream`
//! handle so [`crate::stt::session::SttSessionManager`] can route by kind
//! without leaking per-provider types.

use tokio::sync::mpsc;

use super::errors::SttError;
use super::types::TranscriptDelta;

pub mod assemblyai;
pub mod azure;
pub mod deepgram;
pub mod openai;
pub mod volcengine;
pub mod xunfei;

/// Common streaming handle returned by every WS provider's `open_stream`:
/// callers push raw audio bytes into `audio_tx` and drain partial / final
/// transcript deltas from `delta_rx`. Dropping `audio_tx` signals EOS so
/// the upstream task can close the WS gracefully.
pub struct SttStream {
    pub audio_tx: mpsc::Sender<Vec<u8>>,
    pub delta_rx: mpsc::Receiver<Result<TranscriptDelta, SttError>>,
}

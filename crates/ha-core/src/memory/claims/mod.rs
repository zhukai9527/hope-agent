//! Structured claim layer (next-gen Dreaming).
//!
//! Claims are the dual-track successor to flat `memories`: each is a
//! scoped, typed assertion (`subject predicate object`) with confidence,
//! salience, freshness policy, per-claim evidence, and links back to the
//! legacy `memories` rows it manages (design §2 / §3). They live in the same
//! `memory.db` (tables created in
//! [`crate::memory::sqlite::SqliteMemoryBackend::open`]).
//!
//! This module currently exposes the **read** surface only — the schema +
//! `claim_list` / `claim_get`. Claim extraction, legacy dual-write,
//! canonicalize / merge, and the prompt-injection path land in later PRs.

mod store;
mod types;
mod write;

pub use store::{
    get_claim, init_claim_store, link_claim_memory, list_claims, parse_claim_scope,
    write_claim_candidate, ClaimListFilter, ClaimWriteOutcome,
};
pub use types::{
    ClaimCandidate, ClaimDetail, ClaimLink, ClaimRecord, ClaimScopeHint, ClaimTemporal,
    EvidenceRecord,
};
pub use write::{
    confidence_baseline, effective_status, is_injectable_status, normalize_evidence_class,
    normalize_object, EVIDENCE_CLASSES,
};

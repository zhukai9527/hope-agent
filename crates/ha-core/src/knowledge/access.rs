//! KB access scope (design D10): default-deny + explicit attach, source-aware
//! with a lineage cap and an incognito short-circuit.
//!
//! Why source-aware: the same session can be shared between GUI and IM, and an
//! IM turn can spawn a subagent — so `session_id` alone can't tell whether the
//! current turn (or its origin) is an IM turn. The cap therefore takes the
//! **strictest value over the whole call chain** (`source` ∪ `origin_source`).

use std::collections::HashMap;

use super::types::KbAccess;

/// Call-chain source for a KB access check. Mapped from the chat-engine
/// `ChatSource` at the call site (kept decoupled to avoid a module cycle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KbAccessSource {
    /// Desktop GUI / HTTP UI operating as the owner.
    Gui,
    Http,
    /// IM channel turn. **Phase 1: zero KB access** (D10).
    Im,
    /// Spawned subagent — inherits its origin's cap.
    Subagent,
    Cron,
    Other,
}

impl KbAccessSource {
    fn is_im(self) -> bool {
        matches!(self, KbAccessSource::Im)
    }
}

/// Inputs to [`effective_kb_access`]. Built from the tool-exec context at the
/// call site; `is_incognito` is sourced from `sessions.incognito` (single truth).
pub struct KnowledgeAccessContext {
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    /// Source of the current hop.
    pub source: KbAccessSource,
    /// Source of the spawn-chain root (an IM-origin subagent stays IM-capped).
    pub origin_source: KbAccessSource,
    pub is_incognito: bool,
}

impl KnowledgeAccessContext {
    /// Build a context, resolving incognito from the session (single truth).
    pub fn resolve(
        session_id: Option<String>,
        project_id: Option<String>,
        source: KbAccessSource,
        origin_source: KbAccessSource,
    ) -> Self {
        let is_incognito = crate::session::is_session_incognito(session_id.as_deref());
        Self {
            session_id,
            project_id,
            source,
            origin_source,
            is_incognito,
        }
    }
}

/// Compute the effective `kb_id → access` map for a context.
///
/// Rules (D10):
/// 1. **incognito → {}** (short-circuit, "close = burn").
/// 2. **any IM hop in the lineage → {}** (Phase 1 IM is disabled; a subagent
///    can't launder权限 back via `source=Subagent`).
/// 3. `granted = max(session_attach, project_attach)` (write > read).
/// 4. archived KBs are excluded (attach rows kept; un-archive restores).
/// 5. external (bound) roots are capped to `read` (Phase 1, D11).
pub fn effective_kb_access(ctx: &KnowledgeAccessContext) -> HashMap<String, KbAccess> {
    let empty = HashMap::new();

    // (1) incognito short-circuit.
    if ctx.is_incognito {
        return empty;
    }
    // (2) lineage IM cap → zero.
    if ctx.source.is_im() || ctx.origin_source.is_im() {
        return empty;
    }

    let Some(registry) = crate::get_knowledge_db() else {
        return empty;
    };

    // (3) max(session, project).
    let mut granted: HashMap<String, KbAccess> = HashMap::new();
    if let Some(pid) = ctx.project_id.as_deref() {
        if let Ok(rows) = registry.list_project_attachments(pid) {
            for (kb_id, access) in rows {
                merge_max(&mut granted, kb_id, access);
            }
        }
    }
    if let Some(sid) = ctx.session_id.as_deref() {
        if let Ok(rows) = registry.list_session_attachments(sid) {
            for (kb_id, access) in rows {
                merge_max(&mut granted, kb_id, access);
            }
        }
    }
    if granted.is_empty() {
        return empty;
    }

    // (4) + (5): drop archived, cap external to read.
    let mut out = HashMap::new();
    for (kb_id, access) in granted {
        let Ok(Some(kb)) = registry.get(&kb_id) else {
            continue;
        };
        if kb.archived {
            continue;
        }
        let capped = if kb.is_external() {
            KbAccess::Read
        } else {
            access
        };
        out.insert(kb_id, capped);
    }
    out
}

fn merge_max(map: &mut HashMap<String, KbAccess>, kb_id: String, access: KbAccess) {
    map.entry(kb_id)
        .and_modify(|cur| {
            if access > *cur {
                *cur = access;
            }
        })
        .or_insert(access);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a context directly (bypassing `resolve`'s session lookup) so these
    // tests are deterministic and parallel-safe — the deny rules (1) and (2)
    // short-circuit before any registry hit, so no global `KNOWLEDGE_DB` is
    // touched. The registry-backed rules (max / archived / external cap) are
    // exercised by `registry.rs`'s attach/list tests.
    fn ctx(
        source: KbAccessSource,
        origin: KbAccessSource,
        is_incognito: bool,
    ) -> KnowledgeAccessContext {
        KnowledgeAccessContext {
            session_id: Some("s".into()),
            project_id: None,
            source,
            origin_source: origin,
            is_incognito,
        }
    }

    #[test]
    fn incognito_denies_even_owner() {
        // "Close = burn": an incognito session gets zero KB access even for a GUI
        // owner turn, regardless of any attach.
        let c = ctx(KbAccessSource::Gui, KbAccessSource::Gui, true);
        assert!(effective_kb_access(&c).is_empty());
    }

    #[test]
    fn im_turn_denied() {
        // Phase 1: a direct IM turn is zeroed before any registry lookup.
        let c = ctx(KbAccessSource::Im, KbAccessSource::Im, false);
        assert!(effective_kb_access(&c).is_empty());
    }

    #[test]
    fn im_origin_subagent_denied() {
        // The lineage cap (G1): a subagent carries a *neutral* `Subagent` source,
        // but its IM origin must still zero KB access — otherwise an IM turn could
        // launder access by spawning a subagent. `origin_source = Im` denies.
        let c = ctx(KbAccessSource::Subagent, KbAccessSource::Im, false);
        assert!(effective_kb_access(&c).is_empty());
    }

    #[test]
    fn default_deny_without_attach() {
        // Default-deny baseline: a non-incognito, non-IM owner turn with no
        // attachment resolves to empty (no ambient access).
        let c = ctx(KbAccessSource::Gui, KbAccessSource::Gui, false);
        assert!(effective_kb_access(&c).is_empty());
    }
}

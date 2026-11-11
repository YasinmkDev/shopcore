//! Audit writing + hash-chain verification (spec 001b-2 req. 8,
//! `07-security-compliance`, `04-data-and-sync.md` line 31).
//!
//! Every sensitive action writes an audit row. The rows form a
//! TAMPER-EVIDENT HASH CHAIN, per terminal, from day one:
//! `row_hash = SHA-256(prev_hash || canonical field encoding in FIXED
//! order)`. The chain is verified on read by [`verify_audit_chain`];
//! a broken chain is REPORTED (with the row id), never silently
//! repaired. No audit row is ever written without its chain link.
//!
//! Canonical encoding: the fields are concatenated in a FIXED,
//! documented order, each length-prefixed (`len:field`) so the
//! encoding is unambiguous even when a field contains the separator.
//! It is NEVER a JSON map (map order is not stable). The order is:
//! `prev_hash`, `tenant_id`, `terminal_id`, `user_id`, `action`,
//! `entity`, `entity_id`, `before_json`, `after_json`, `reason`,
//! `at_ms` (decimal). `None` fields encode as the empty string. The
//! first row of a terminal's chain has `prev_hash = ""` (genesis).

use rusqlite::{OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use shopcore_db::Db;

use crate::CoreError;

/// Audit actions written by this spec (001b-2, amendment 7). ONLY events
/// with a producer in this spec are written here; shift open/close
/// events land with their use cases (a later spec), not here.
pub const AUDIT_SALE_COMPLETED: &str = "sale.completed";
/// A line discount above the cashier's threshold, approved by a manager.
pub const AUDIT_DISCOUNT_APPROVED: &str = "discount.approved";
/// A line discount above the cashier's threshold, rejected (no manager
/// PIN, wrong PIN, or self-approval).
pub const AUDIT_DISCOUNT_REJECTED: &str = "discount.rejected";
/// A line discount within the cashier's threshold, applied directly.
pub const AUDIT_DISCOUNT_APPLIED: &str = "discount.applied";
/// A cart with lines was cancelled (draft discarded).
pub const AUDIT_CART_CANCELLED: &str = "cart.cancelled";
/// A receipt reprint was requested (queued or printed).
pub const AUDIT_RECEIPT_REPRINT: &str = "receipt.reprint";
/// A PIN lockout was triggered (the event comes from 001b-1; this
/// module persists it — the seam is explicit).
pub const AUDIT_PIN_LOCKOUT: &str = "pin.lockout";

/// One audit event to write. All ids are strings (`UUIDv7`); `at_ms` is
/// epoch milliseconds (the pending timestamp-format decision — see the
/// 001b-2 session summary — keeps epoch ms for now, consistent with
/// 001b-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// The tenant the event belongs to.
    pub tenant_id: String,
    /// The terminal the event happened on (the chain is per terminal).
    pub terminal_id: String,
    /// The user who acted (`None` for system events).
    pub user_id: Option<String>,
    /// What happened (one of the `AUDIT_*` constants).
    pub action: String,
    /// The entity type (e.g. `"sale"`, `"draft"`).
    pub entity: Option<String>,
    /// The entity id.
    pub entity_id: Option<String>,
    /// The state before the action, as JSON (`None` when not applicable).
    pub before_json: Option<String>,
    /// The state after the action, as JSON (`None` when not applicable).
    pub after_json: Option<String>,
    /// Why (required for rejections and cancellations).
    pub reason: Option<String>,
    /// When, epoch ms.
    pub at_ms: i64,
}

/// Lowercase hex encoding (avoids depending on the `hex` crate).
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if let (Some(&hi), Some(&lo)) = (HEX.get((b >> 4) as usize), HEX.get((b & 0x0f) as usize)) {
            out.push(hi as char);
            out.push(lo as char);
        }
    }
    out
}

/// The canonical field encoding: each field length-prefixed
/// (`len:field`), concatenated in the fixed order documented at the top
/// of this module. Length-prefixing makes the encoding unambiguous:
/// `["a|b", "c"]` and `["a", "b|c"]` encode differently, so no field
/// boundary can be confused.
fn canonical_encode(fields: &[&str]) -> String {
    let mut out = String::new();
    for field in fields {
        out.push_str(&field.len().to_string());
        out.push(':');
        out.push_str(field);
    }
    out
}

/// Compute `row_hash` for an event chained after `prev_hash`: the
/// canonical fields (fixed order, `None` fields as the empty string)
/// are length-prefixed, concatenated, and SHA-256 hashed (hex).
fn compute_row_hash(prev_hash: &str, event: &AuditEvent) -> String {
    let at = event.at_ms.to_string();
    let fields = [
        prev_hash,
        event.tenant_id.as_str(),
        event.terminal_id.as_str(),
        event.user_id.as_deref().unwrap_or(""),
        event.action.as_str(),
        event.entity.as_deref().unwrap_or(""),
        event.entity_id.as_deref().unwrap_or(""),
        event.before_json.as_deref().unwrap_or(""),
        event.after_json.as_deref().unwrap_or(""),
        event.reason.as_deref().unwrap_or(""),
        at.as_str(),
    ];
    let encoded = canonical_encode(&fields);
    let mut hasher = Sha256::new();
    hasher.update(encoded.as_bytes());
    to_hex(&hasher.finalize())
}

/// The chain head for a terminal: the `row_hash` of its latest row
/// (insertion order = `rowid` order; deletes are forbidden by trigger,
/// so `rowid`s are monotonic). `""` when the terminal has no rows yet
/// (genesis).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
fn chain_head(
    tx: &Transaction<'_>,
    tenant_id: &str,
    terminal_id: &str,
) -> Result<String, shopcore_db::DbError> {
    let head: Option<String> = tx
        .query_row(
            "SELECT row_hash FROM audit_log WHERE tenant_id = ?1 AND terminal_id = ?2 ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![tenant_id, terminal_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(head.unwrap_or_default())
}

/// Write one audit row inside `tx`, chained after the terminal's current
/// head. Returns the new row id (`UUIDv7`). The caller owns the
/// transaction: inside the sale transaction the row commits with the
/// sale; standalone callers (discount, cancel, reprint, lockout) use
/// [`write_audit_event`].
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the head read or the insert fails.
pub fn write_audit(tx: &Transaction<'_>, event: &AuditEvent) -> Result<String, CoreError> {
    write_audit_db(tx, event).map_err(CoreError::Db)
}

/// The `DbError` core of [`write_audit`]: the same chained insert,
/// without the `CoreError` wrapper, for callers whose transaction
/// closure already returns `DbError` (e.g. [`write_audit_event`]).
fn write_audit_db(
    tx: &Transaction<'_>,
    event: &AuditEvent,
) -> Result<String, shopcore_db::DbError> {
    let prev_hash = chain_head(tx, &event.tenant_id, &event.terminal_id)?;
    let row_hash = compute_row_hash(&prev_hash, event);
    let id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO audit_log (id, tenant_id, terminal_id, user_id, action, entity, entity_id, before_json, after_json, reason, at, prev_hash, row_hash, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?11, ?11, 1)",
        rusqlite::params![
            id,
            event.tenant_id,
            event.terminal_id,
            event.user_id,
            event.action,
            event.entity,
            event.entity_id,
            event.before_json,
            event.after_json,
            event.reason,
            event.at_ms,
            prev_hash,
            row_hash,
        ],
    )?;
    Ok(id)
}

/// Write one audit row in its own transaction (for events that happen
/// outside a sale: discount approval/rejection, cart cancel, reprint,
/// PIN lockout). Returns the new row id.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the write fails.
pub fn write_audit_event(db: &Db, event: &AuditEvent) -> Result<String, CoreError> {
    let event = event.clone();
    let id_cell = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let id_in = std::sync::Arc::clone(&id_cell);
    db.write(move |conn| {
        let tx = conn.transaction()?;
        let id = write_audit_db(&tx, &event)?;
        tx.commit()?;
        if let Ok(mut slot) = id_in.lock() {
            *slot = Some(id);
        }
        Ok(())
    })?;
    let id = id_cell
        .lock()
        .map_err(|_| CoreError::InvalidInput("audit id cell poisoned".to_string()))?
        .take();
    id.ok_or_else(|| CoreError::InvalidInput("audit write did not return an id".to_string()))
}

/// Verify a terminal's audit chain: walk every row in insertion order,
/// recompute each `row_hash` from the stored fields, and check each
/// row's `prev_hash` equals the previous row's `row_hash`. Returns the
/// number of rows checked when the chain is intact. On the FIRST broken
/// link returns [`CoreError::AuditChain`] with the row id — the break is
/// reported, never silently repaired.
///
/// This is the VERIFY use case: it runs in the background at startup
/// (it never blocks the sale path) and after any suspected tampering.
///
/// # Errors
///
/// Returns [`CoreError::AuditChain`] at the first broken link, and
/// [`CoreError::Db`] if the read fails.
pub fn verify_audit_chain(db: &Db, tenant_id: &str, terminal_id: &str) -> Result<usize, CoreError> {
    let tenant_id = tenant_id.to_string();
    let terminal_id = terminal_id.to_string();
    let rows: Vec<(AuditEvent, Option<String>, Option<String>, String)> = db.read(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT tenant_id, terminal_id, user_id, action, entity, entity_id, before_json, after_json, reason, at, prev_hash, row_hash, id FROM audit_log WHERE tenant_id = ?1 AND terminal_id = ?2 ORDER BY rowid ASC",
        )?;
        let mapped = stmt.query_map(rusqlite::params![tenant_id, terminal_id], |row| {
            Ok((
                AuditEvent {
                    tenant_id: row.get(0)?,
                    terminal_id: row.get(1)?,
                    user_id: row.get(2)?,
                    action: row.get(3)?,
                    entity: row.get(4)?,
                    entity_id: row.get(5)?,
                    before_json: row.get(6)?,
                    after_json: row.get(7)?,
                    reason: row.get(8)?,
                    at_ms: row.get(9)?,
                },
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, String>(12)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in mapped {
            out.push(row?);
        }
        Ok(out)
    })?;
    let mut expected_prev = String::new();
    let mut checked = 0usize;
    for (event, prev_hash, row_hash, row_id) in &rows {
        let stored_prev = prev_hash.clone().unwrap_or_default();
        if stored_prev != expected_prev {
            return Err(CoreError::AuditChain {
                row_id: row_id.clone(),
                detail: "prev_hash does not match the previous row's row_hash (chain link broken before this row)"
                    .to_string(),
            });
        }
        let recomputed = compute_row_hash(&stored_prev, event);
        let stored_hash = row_hash.clone().unwrap_or_default();
        if recomputed != stored_hash {
            return Err(CoreError::AuditChain {
                row_id: row_id.clone(),
                detail: "row_hash does not match the recomputed hash of the stored fields (a field was changed after writing)".to_string(),
            });
        }
        expected_prev = stored_hash;
        checked += 1;
    }
    Ok(checked)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn event(action: &str) -> AuditEvent {
        AuditEvent {
            tenant_id: "t1".to_string(),
            terminal_id: "term1".to_string(),
            user_id: Some("u1".to_string()),
            action: action.to_string(),
            entity: Some("sale".to_string()),
            entity_id: Some("s1".to_string()),
            before_json: Some("{\"a\":1}".to_string()),
            after_json: Some("{\"b\":2}".to_string()),
            reason: None,
            at_ms: 1_790_000_000_000,
        }
    }

    #[test]
    fn canonical_encoding_is_unambiguous() {
        // A separator inside a field must not move the boundary.
        let a = canonical_encode(&["a|b", "c"]);
        let b = canonical_encode(&["a", "b|c"]);
        assert_ne!(a, b, "length-prefixing must separate the fields");
        assert_eq!(canonical_encode(&["ab", "c"]), "2:ab1:c");
        assert_eq!(canonical_encode(&["", "x"]), "0:1:x");
    }

    #[test]
    fn row_hash_is_stable_and_chained() {
        let e = event("sale.completed");
        let h1 = compute_row_hash("", &e);
        let h2 = compute_row_hash("", &e);
        assert_eq!(h1, h2, "same event + same prev_hash must hash the same");
        assert_eq!(h1.len(), 64, "SHA-256 hex is 64 chars");
        let h3 = compute_row_hash(&h1, &e);
        assert_ne!(h3, h1, "chaining after a row must change the hash");
        let mut changed = e.clone();
        changed.after_json = Some("{\"b\":3}".to_string());
        assert_ne!(
            compute_row_hash("", &changed),
            h1,
            "changing one field must change the hash"
        );
    }
}

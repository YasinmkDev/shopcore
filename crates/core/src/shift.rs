//! Shift reads for the sell screen (spec 001c-1, req. 7).
//!
//! The status bar shows whether a shift is open on this terminal.
//! The shift itself is opened/closed by the shift use cases (a later
//! spec); this module only reads the open shift, if one exists.

use rusqlite::OptionalExtension;
use shopcore_db::Db;

use crate::CoreError;

/// The open shift id for a terminal, if a shift is open (`status =
/// 'open'` and not soft-deleted). `None` means no shift is open —
/// the status bar shows "Shift closed" and the sale path refuses to
/// commit (`SHIFT_NOT_OPEN`).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn open_shift_id(
    db: &Db,
    tenant_id: &str,
    terminal_id: &str,
) -> Result<Option<String>, CoreError> {
    let tenant_id = tenant_id.to_string();
    let terminal_id = terminal_id.to_string();
    let shift_id: Option<String> = db.read(move |conn| {
        Ok(conn
            .query_row(
                "SELECT id FROM shift WHERE tenant_id = ?1 AND terminal_id = ?2 AND status = 'open' AND deleted_at IS NULL ORDER BY opened_at DESC, rowid DESC LIMIT 1",
                rusqlite::params![tenant_id, terminal_id],
                |row| row.get(0),
            )
            .optional()?)
    })?;
    Ok(shift_id)
}

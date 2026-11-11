//! SKU search + barcode lookup (spec 001b-1 req. 4-5,
//! `05-performance-budgets`).
//!
//! Barcode exact match is a single indexed lookup. Name search uses an
//! in-memory prefix index for hot SKUs, with `FTS5` for the rest. The
//! prefix index is invalidated by the catalog version counter (req. 4,
//! amendment 9): a version mismatch rebuilds it before the query runs, so
//! a stale index is never served.

use std::collections::HashMap;

use rusqlite::OptionalExtension;
use shopcore_db::Db;

use crate::CoreError;
use crate::settings::{KEY_ACTIVE_PRICE_LIST, SCOPE_TENANT, get_catalog_version, get_setting};

/// How many results a search returns at most (the UI caps its list at 20
/// in 001c-1; the core cap bounds the work, not the display).
pub const SEARCH_LIMIT: usize = 50;

/// A SKU found by search or barcode lookup, with its active price (if a
/// price list is configured and the SKU has a current entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkuHit {
    /// The `SKU` id.
    pub sku_id: String,
    /// The parent `item` id.
    pub item_id: String,
    /// The `SKU` code.
    pub code: String,
    /// The `SKU` name.
    pub name: String,
    /// The tax group id (for the 001a tax lookup).
    pub tax_group_id: Option<String>,
    /// The active price in `e4` (price x 10^4), if one is configured.
    pub price_e4: Option<i64>,
}

/// Search/lookup errors. The stable error code for an unknown barcode is
/// `BARCODE_NOT_FOUND` (mapped to a localized message at the boundary).
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// No `SKU` has this barcode.
    #[error("barcode not found: {0}")]
    BarcodeNotFound(String),
    /// A database error.
    #[error("database error: {0}")]
    Db(#[from] shopcore_db::DbError),
}

impl From<CoreError> for SearchError {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::Db(db) => Self::Db(db),
            other => Self::Db(shopcore_db::DbError::InvalidInput(other.to_string())),
        }
    }
}

impl SearchError {
    /// The stable error code for this error.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BarcodeNotFound(_) => "BARCODE_NOT_FOUND",
            Self::Db(_) => "DB_ERROR",
        }
    }
}

/// One `SKU` in the prefix index (no price — prices are attached after
/// the match, in one query).
#[derive(Debug, Clone)]
struct IndexedSku {
    sku_id: String,
    item_id: String,
    code: String,
    name: String,
    tax_group_id: Option<String>,
}

/// The in-memory prefix index: word-prefix (first 3 chars, lowercased)
/// → SKUs whose name or code has a word starting with that prefix. Built
/// from the catalog on first use and rebuilt whenever the catalog version
/// counter moves.
#[derive(Debug, Default)]
pub struct PrefixIndex {
    version: u64,
    map: HashMap<String, Vec<IndexedSku>>,
}

impl PrefixIndex {
    /// An empty index (version 0 — the first search rebuilds it).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The catalog version this index was built from.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Rebuild the index from the catalog (all active SKUs) and record
    /// the current catalog version.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the catalog cannot be read.
    pub fn rebuild(&mut self, db: &Db, tenant_id: &str) -> Result<(), CoreError> {
        let tenant_id_owned = tenant_id.to_string();
        let rows: Vec<IndexedSku> = db.read(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, item_id, code, name, tax_group_id FROM sku WHERE tenant_id = ?1 AND active = 1 AND deleted_at IS NULL",
            )?;
            let rows = stmt.query_map(rusqlite::params![tenant_id_owned], |row| {
                Ok(IndexedSku {
                    sku_id: row.get(0)?,
                    item_id: row.get(1)?,
                    code: row.get(2)?,
                    name: row.get(3)?,
                    tax_group_id: row.get(4)?,
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })?;
        let mut map: HashMap<String, Vec<IndexedSku>> = HashMap::new();
        for sku in rows {
            for key in prefix_keys(&sku.name, &sku.code) {
                map.entry(key).or_default().push(sku.clone());
            }
        }
        self.map = map;
        self.version = get_catalog_version(db, tenant_id)?;
        Ok(())
    }
}

/// The prefix keys for one `SKU`: the first 3 chars (lowercased) of each
/// word in the name, plus the first 3 chars of the code. Words shorter
/// than 3 chars get no key (short queries use `FTS5`).
fn prefix_keys(name: &str, code: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for word in name.split_whitespace().chain(std::iter::once(code)) {
        let lower = word.to_lowercase();
        if lower.chars().count() >= 3 {
            let key: String = lower.chars().take(3).collect();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// Look up a barcode: known barcode → the `SKU` + its active price;
/// unknown barcode → [`SearchError::BarcodeNotFound`].
///
/// # Errors
///
/// Returns [`SearchError::BarcodeNotFound`] for an unknown barcode and
/// [`SearchError::Db`] if the read fails.
pub fn lookup_barcode(
    db: &Db,
    tenant_id: &str,
    code: &str,
    now_ms: i64,
) -> Result<SkuHit, SearchError> {
    let tenant_id_owned = tenant_id.to_string();
    let code_owned = code.to_string();
    let hit: Option<SkuHit> = db.read(move |conn| {
        let row = conn
            .query_row(
                "SELECT s.id, s.item_id, s.code, s.name, s.tax_group_id FROM barcode b JOIN sku s ON s.id = b.sku_id WHERE b.tenant_id = ?1 AND b.code = ?2 AND b.deleted_at IS NULL AND s.deleted_at IS NULL AND s.active = 1",
                rusqlite::params![tenant_id_owned, code_owned],
                |row| {
                    Ok(SkuHit {
                        sku_id: row.get(0)?,
                        item_id: row.get(1)?,
                        code: row.get(2)?,
                        name: row.get(3)?,
                        tax_group_id: row.get(4)?,
                        price_e4: None,
                    })
                },
            )
            .optional()?;
        Ok(row)
    })?;
    let Some(mut hit) = hit else {
        return Err(SearchError::BarcodeNotFound(code.to_string()));
    };
    attach_prices(db, tenant_id, std::slice::from_mut(&mut hit), now_ms)?;
    Ok(hit)
}

/// Search SKUs by name/code/barcode. The prefix index serves queries of 3+
/// characters when it has the prefix; anything else goes to `FTS5`.
/// Results are capped at [`SEARCH_LIMIT`].
///
/// # Errors
///
/// Returns [`CoreError::Db`] if a read fails.
pub fn search_skus(
    db: &Db,
    tenant_id: &str,
    query: &str,
    index: &mut PrefixIndex,
    now_ms: i64,
) -> Result<Vec<SkuHit>, CoreError> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Invalidate the index on a catalog-version mismatch (amendment 9).
    if index.version() != get_catalog_version(db, tenant_id)? {
        index.rebuild(db, tenant_id)?;
    }
    let mut hits: Vec<SkuHit> = Vec::new();
    if query.chars().count() >= 3 {
        let key: String = query.chars().take(3).collect();
        if let Some(candidates) = index.map.get(&key) {
            for candidate in candidates {
                let matches = candidate
                    .name
                    .split_whitespace()
                    .any(|word| word.to_lowercase().starts_with(&query))
                    || candidate.code.to_lowercase().starts_with(&query);
                if matches && !hits.iter().any(|h: &SkuHit| h.sku_id == candidate.sku_id) {
                    hits.push(SkuHit {
                        sku_id: candidate.sku_id.clone(),
                        item_id: candidate.item_id.clone(),
                        code: candidate.code.clone(),
                        name: candidate.name.clone(),
                        tax_group_id: candidate.tax_group_id.clone(),
                        price_e4: None,
                    });
                }
            }
            if !hits.is_empty() {
                attach_prices(db, tenant_id, &mut hits, now_ms)?;
                hits.truncate(SEARCH_LIMIT);
                return Ok(hits);
            }
            // The index is complete (rebuilt on version mismatch), so a
            // key with no full-query match means no match exists at all.
            return Ok(Vec::new());
        }
        // Prefix key absent: fall through to FTS5 (the SKU may exist under
        // a different first-3-chars spelling, e.g. a short word).
    }
    hits = search_fts(db, tenant_id, &query)?;
    attach_prices(db, tenant_id, &mut hits, now_ms)?;
    hits.truncate(SEARCH_LIMIT);
    Ok(hits)
}

/// The `FTS5` leg of [`search_skus`]: match the sanitized query against
/// name/code/barcodes, restricted to active SKUs of this tenant.
fn search_fts(db: &Db, tenant_id: &str, query: &str) -> Result<Vec<SkuHit>, CoreError> {
    let match_query = fts_query(query);
    if match_query.is_empty() {
        return Ok(Vec::new());
    }
    let tenant_id = tenant_id.to_string();
    let hits = db.read(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.item_id, s.code, s.name, s.tax_group_id FROM sku_fts f JOIN sku s ON s.id = f.sku_id WHERE sku_fts MATCH ?1 AND s.tenant_id = ?2 AND s.active = 1 AND s.deleted_at IS NULL ORDER BY s.name LIMIT 50",
        )?;
        let rows = stmt.query_map(rusqlite::params![match_query, tenant_id], |row| {
            Ok(SkuHit {
                sku_id: row.get(0)?,
                item_id: row.get(1)?,
                code: row.get(2)?,
                name: row.get(3)?,
                tax_group_id: row.get(4)?,
                price_e4: None,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })?;
    Ok(hits)
}

/// Sanitize a user query into an `FTS5` `MATCH` expression: keep only
/// alphanumeric characters and spaces, split into terms, and require each
/// term as a prefix (`term*`). Punctuation is a separator, not a joiner
/// (`1=1` is two terms, not `11`). Returns an empty string when nothing
/// searchable remains (the caller then returns no results).
fn fts_query(query: &str) -> String {
    let cleaned: String = query
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let terms: Vec<String> = cleaned
        .split_whitespace()
        .map(|term| format!("{term}*"))
        .collect();
    terms.join(" AND ")
}

/// Attach the active price to each hit, in one query. No price list
/// configured (or no current entry) leaves `price_e4` as `None`.
fn attach_prices(
    db: &Db,
    tenant_id: &str,
    hits: &mut [SkuHit],
    now_ms: i64,
) -> Result<(), CoreError> {
    if hits.is_empty() {
        return Ok(());
    }
    let Some(price_list_id) = get_setting(db, tenant_id, SCOPE_TENANT, "", KEY_ACTIVE_PRICE_LIST)?
    else {
        return Ok(());
    };
    let tenant_id = tenant_id.to_string();
    // The `IN` placeholders are numbered explicitly from `?4` — a bare
    // `?` would collide with the `?3` used for the validity window (the
    // statement's parameter count is the highest index, so a bare `?`
    // after `?1`/`?2` would silently bind to index 3).
    let placeholders: Vec<String> = (0..hits.len()).map(|i| format!("?{}", i + 4)).collect();
    let sql = format!(
        "SELECT sku_id, price_e4 FROM price_list_entry WHERE tenant_id = ?1 AND price_list_id = ?2 AND sku_id IN ({}) AND deleted_at IS NULL AND (valid_from IS NULL OR valid_from <= ?3) AND (valid_to IS NULL OR valid_to >= ?3)",
        placeholders.join(", ")
    );
    let ids: Vec<String> = hits.iter().map(|hit| hit.sku_id.clone()).collect();
    let prices: Vec<(String, i64)> = db.read(move |conn| {
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<rusqlite::types::Value> =
            vec![tenant_id.into(), price_list_id.into(), now_ms.into()];
        for id in &ids {
            params.push(id.clone().into());
        }
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })?;
    for (sku_id, price_e4) in prices {
        if let Some(hit) = hits.iter_mut().find(|hit| hit.sku_id == sku_id) {
            hit.price_e4 = Some(price_e4);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_sanitizes() {
        assert_eq!(fts_query("milk"), "milk*");
        assert_eq!(fts_query("Milk 1L"), "Milk* AND 1L*");
        assert_eq!(fts_query("a\" OR 1=1 --"), "a* AND OR* AND 1* AND 1*");
        assert_eq!(fts_query("   "), "");
        assert_eq!(fts_query("\"';"), "");
    }

    #[test]
    fn prefix_keys_cover_name_words_and_code() {
        assert_eq!(
            prefix_keys("Milk 1L Full Cream", "MLK1"),
            vec!["mil", "ful", "cre", "mlk"]
        );
        assert_eq!(prefix_keys("Tea", "TEA"), vec!["tea"]);
        assert_eq!(prefix_keys("A B", "AB"), Vec::<String>::new());
    }
}

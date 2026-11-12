#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! `ShopCore` desktop entry point (Tauri 2 shell, Windows first).
//!
//! Thin by design (see `context/09-engineering-standards.md`): commands
//! deserialize, validate at the boundary, then call a use case in
//! `shopcore-core`. No business logic lives here. The Tauri main thread
//! runs the window and IPC only and never blocks.
//!
//! The app holds ONE [`Core`] ([`shopcore_core::Core`], owning the `Db`
//! and the `ScratchDb`) in Tauri managed state, plus the [`Session`]
//! (the actor the sell screen works as), the search prefix index, and
//! the mock printer. On exit the Tauri exit event calls
//! [`Core::shutdown`] (spec 001c-1, amendment 8, ADR-036) so the actor
//! and checkpoint-timer threads are joined before the process exits.
//!
//! Spec 001c-1 commands: `search_skus`, `lookup_barcode`,
//! `draft_add_line`, `draft_set_qty`, `draft_remove_line`,
//! `draft_cancel`, `draft_restore_line` — plus the read commands the
//! sell screen needs on mount (`get_session`, `get_status`, `get_cart`).
//! `draft_apply_discount` and `complete_sale_cash` land in 001c-2 (the
//! ADR-013 drift guard requires a UI caller for every command, and
//! those two have no UI caller until then).

use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use shopcore_core::draft::{
    draft_add_line as core_draft_add_line, draft_cancel as core_draft_cancel,
    draft_remove_line as core_draft_remove_line, draft_restore_line as core_draft_restore_line,
    draft_set_qty as core_draft_set_qty, load_draft, open_draft,
};
use shopcore_core::search::{
    PrefixIndex, lookup_barcode as core_lookup_barcode, search_skus as core_search_skus,
};
#[cfg(feature = "dev-session")]
use shopcore_core::settings::{
    KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, get_setting, get_setting_i64,
};
use shopcore_core::shift::open_shift_id;
#[cfg(feature = "dev-session")]
use shopcore_core::users::get_user_by_username;
use shopcore_core::{Core, CoreError, price_draft_cart};
use shopcore_db::Db;
use shopcore_hardware::MockPrinter;
use shopcore_protocol::{
    CartLineDto, CartTotalsDto, CartViewDto, Pong, SessionDto, SkuHitDto, StatusDto,
};
use tauri::{AppHandle, Manager, State};

#[cfg(feature = "dev-session")]
use rusqlite::OptionalExtension;
#[cfg(feature = "dev-session")]
use shopcore_core::seed::seed_dev_till;

/// Process start, set in `main`. Read by the bench debug route (spec 002,
/// scenario 7: process start → window interactive).
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// The largest integer that survives a JSON round-trip to JavaScript
/// exactly (2^53-1). IPC money convention (spec 001c-1, amendment 3):
/// DTO integers travel as JSON numbers, and Rust rejects anything
/// larger — the same bound applies to `qty_milli`.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// Milliseconds since the Unix epoch (the edge clock; the domain stays
/// pure and takes `now_ms` as a parameter).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}

/// A stable error returned to the UI: the code maps to a localized
/// message there; the message is the diagnostic detail.
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// Stable error code (e.g. `BARCODE_NOT_FOUND`).
    pub code: String,
    /// Human-readable detail (never a secret, never customer data).
    pub message: String,
}

impl AppError {
    /// An error with a stable code and a message.
    fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_string(),
            message,
        }
    }
}

/// The stable error code for a core error (mapped to a localized
/// message at the UI boundary).
fn core_error_code(err: &CoreError) -> &'static str {
    match err {
        CoreError::Db(_) => "DB_ERROR",
        CoreError::Domain(_) => "DOMAIN_ERROR",
        CoreError::Pin(_) => "PIN_ERROR",
        CoreError::Search(search) => search.code(),
        CoreError::NotFound(_) => "NOT_FOUND",
        CoreError::TaxGroupNotFound(_) => "TAX_GROUP_NOT_FOUND",
        CoreError::InvalidInput(_) => "INVALID_INPUT",
        CoreError::ShutdownTimeout => "SHUTDOWN_TIMEOUT",
        CoreError::AuditChain { .. } => "AUDIT_CHAIN_BROKEN",
    }
}

impl From<CoreError> for AppError {
    fn from(err: CoreError) -> Self {
        Self::new(core_error_code(&err), err.to_string())
    }
}

impl From<shopcore_core::search::SearchError> for AppError {
    fn from(err: shopcore_core::search::SearchError) -> Self {
        Self::new(err.code(), err.to_string())
    }
}

/// Reject an integer DTO outside the JSON safe-integer range
/// (amendment 3). Applies to every integer the UI sends (`qty_milli`,
/// `unit_price_e4`) and every integer the core returns in a cart view.
fn check_safe_integer(value: i64) -> Result<(), AppError> {
    if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value) {
        return Err(AppError::new(
            "INVALID_INTEGER",
            format!("integer {value} exceeds the JSON safe-integer limit (2^53-1)"),
        ));
    }
    Ok(())
}

/// The actor the sell screen works as (spec 001c-1, amendment 4):
/// tenant, branch, terminal, cashier, and the open shift. In dev a
/// feature-gated auto-session fills it; real sign-in/lock is spec 001d.
#[derive(Debug, Clone)]
pub struct Session {
    /// The tenant.
    pub tenant_id: String,
    /// The branch id.
    pub branch_id: String,
    /// The branch code (e.g. `MAIN`).
    pub branch_code: String,
    /// The terminal id.
    pub terminal_id: String,
    /// The terminal code (e.g. `T1`).
    pub terminal_code: String,
    /// The cashier user id, if one is signed in.
    pub user_id: Option<String>,
    /// The cashier username, if one is signed in.
    pub username: Option<String>,
    /// The open shift id, if a shift is open.
    pub shift_id: Option<String>,
    /// The tenant currency code (e.g. `PKR`) — ADR-017.
    pub currency_code: String,
    /// The tenant currency's minor-unit exponent.
    pub currency_exponent: u32,
}

impl From<&Session> for SessionDto {
    fn from(session: &Session) -> Self {
        Self {
            tenant_id: session.tenant_id.clone(),
            branch_id: session.branch_id.clone(),
            branch_code: session.branch_code.clone(),
            terminal_id: session.terminal_id.clone(),
            terminal_code: session.terminal_code.clone(),
            username: session.username.clone(),
            shift_id: session.shift_id.clone(),
            currency_code: session.currency_code.clone(),
            currency_exponent: session.currency_exponent,
        }
    }
}

/// The dev auto-session (amendment 4): on a fresh database it seeds
/// the dev till (`MAIN` / `T1` / open shift / cashier `1234`), then
/// loads the actor by code. Compiled ONLY with the `dev-session`
/// feature — never in a release build.
#[cfg(feature = "dev-session")]
fn dev_auto_session(db: &Db, now: i64) -> Result<Session, CoreError> {
    const DEV_TENANT: &str = "dev-tenant";
    const DEV_BRANCH_CODE: &str = "MAIN";
    const DEV_TERMINAL_CODE: &str = "T1";
    const DEV_CASHIER: &str = "cashier";
    // Seed on a fresh DB only: the seed fails on a seeded DB (unique
    // constraints), so a branch row must be absent first.
    let branch_count: i64 = db.read(|conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM branch WHERE tenant_id = ?1 AND code = ?2 AND deleted_at IS NULL",
            rusqlite::params![DEV_TENANT, DEV_BRANCH_CODE],
            |row| row.get::<_, i64>(0),
        )?)
    })?;
    if branch_count == 0 {
        seed_dev_till(db, DEV_TENANT, now)?;
    }
    let (branch_id, terminal_id): (String, String) = db.read(|conn| {
        let branch_id: String = conn
            .query_row(
                "SELECT id FROM branch WHERE tenant_id = ?1 AND code = ?2 AND deleted_at IS NULL",
                rusqlite::params![DEV_TENANT, DEV_BRANCH_CODE],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                shopcore_db::DbError::InvalidInput("dev branch MAIN is missing".to_string())
            })?;
        let terminal_id: String = conn
            .query_row(
                "SELECT id FROM terminal WHERE tenant_id = ?1 AND branch_id = ?2 AND code = ?3 AND deleted_at IS NULL",
                rusqlite::params![DEV_TENANT, branch_id, DEV_TERMINAL_CODE],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                shopcore_db::DbError::InvalidInput("dev terminal T1 is missing".to_string())
            })?;
        Ok((branch_id, terminal_id))
    })?;
    let shift_id = open_shift_id(db, DEV_TENANT, &terminal_id)?;
    let cashier = get_user_by_username(db, DEV_TENANT, DEV_CASHIER)?;
    let (user_id, username) = match cashier {
        Some(user) => (Some(user.id), Some(user.username)),
        None => (None, None),
    };
    let currency_code = get_setting(db, DEV_TENANT, SCOPE_TENANT, "", KEY_TENANT_CURRENCY)?
        .unwrap_or_else(|| "PKR".to_string());
    let exponent = get_setting_i64(
        db,
        DEV_TENANT,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
    )?
    .unwrap_or(2);
    Ok(Session {
        tenant_id: DEV_TENANT.to_string(),
        branch_id,
        branch_code: DEV_BRANCH_CODE.to_string(),
        terminal_id,
        terminal_code: DEV_TERMINAL_CODE.to_string(),
        user_id,
        username,
        shift_id,
        currency_code,
        currency_exponent: u32::try_from(exponent).unwrap_or(2),
    })
}

/// The app's managed state: one [`Core`], the [`Session`] (if any),
/// the search prefix index, and the mock printer.
pub struct AppState {
    core: Core,
    session: Option<Session>,
    search_index: Mutex<PrefixIndex>,
    printer: Mutex<MockPrinter>,
}

impl AppState {
    /// Build the managed state from an already-opened [`Core`] and an
    /// optional [`Session`] (`None` until sign-in lands in 001d, except
    /// in dev where the auto-session fills it).
    #[must_use]
    pub fn new(core: Core, session: Option<Session>) -> Self {
        Self {
            core,
            session,
            search_index: Mutex::new(PrefixIndex::new()),
            printer: Mutex::new(MockPrinter::new()),
        }
    }

    /// The session the sell screen works as, if one is available.
    #[must_use]
    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Gracefully shut the core down (amendment 8): join the `DB`
    /// actor and scratch checkpoint-timer threads and close the
    /// connections. Idempotent. Called from the Tauri exit event and
    /// from [`Drop`].
    ///
    /// # Errors
    ///
    /// Returns the first shutdown error; the caller must not delete
    /// the database files while a thread is stuck.
    pub fn shutdown(&self) -> Result<(), CoreError> {
        self.core.shutdown()
    }

    /// The session, or an error when no session is available (no
    /// sign-in yet — 001d — and no dev auto-session in this build).
    fn require_session(&self) -> Result<&Session, AppError> {
        self.session.as_ref().ok_or_else(|| {
            AppError::new(
                "SESSION_REQUIRED",
                "no session — sign-in lands in spec 001d".to_string(),
            )
        })
    }

    /// Build the [`CartViewDto`] for a draft: price it through the
    /// 001a domain functions and range-check every integer DTO.
    fn cart_view(&self, draft_id: &str) -> Result<CartViewDto, AppError> {
        let draft = load_draft(self.core.scratch(), draft_id)?
            .ok_or_else(|| AppError::new("NOT_FOUND", format!("draft {draft_id} not found")))?;
        let priced = price_draft_cart(self.core.db(), &draft)?;
        let mut lines: Vec<CartLineDto> = Vec::with_capacity(priced.lines.len());
        for line in priced.lines {
            check_safe_integer(line.line.qty_milli)?;
            check_safe_integer(line.line.unit_price_e4)?;
            check_safe_integer(line.line.discount_minor)?;
            check_safe_integer(line.line_total_minor)?;
            lines.push(CartLineDto {
                id: line.line.id,
                sku_id: line.line.sku_id,
                name: line.name,
                qty_milli: line.line.qty_milli,
                unit_price_e4: line.line.unit_price_e4,
                discount_minor: line.line.discount_minor,
                line_total_minor: line.line_total_minor,
            });
        }
        for total in [
            priced.subtotal_minor,
            priced.discount_minor,
            priced.net_subtotal_minor,
            priced.tax_minor,
            priced.total_minor,
        ] {
            check_safe_integer(total)?;
        }
        Ok(CartViewDto {
            draft_id: draft.id,
            lines,
            totals: CartTotalsDto {
                subtotal_minor: priced.subtotal_minor,
                discount_minor: priced.discount_minor,
                net_subtotal_minor: priced.net_subtotal_minor,
                tax_minor: priced.tax_minor,
                total_minor: priced.total_minor,
            },
            currency_exponent: priced.currency_exponent,
        })
    }

    /// The current cart: the terminal's open draft (restored after a
    /// crash/restart, created when none exists) priced as a view.
    fn current_cart_view(&self) -> Result<CartViewDto, AppError> {
        let session = self.require_session()?;
        let draft = open_draft(
            self.core.scratch(),
            &session.tenant_id,
            &session.branch_id,
            &session.terminal_id,
            session.shift_id.as_deref(),
            session.user_id.as_deref(),
            now_ms(),
        )?;
        self.cart_view(&draft.id)
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        // Best-effort graceful shutdown (same path as `shutdown`).
        let _ = self.core.shutdown();
    }
}

/// Proves the UI → core round-trip. Not business logic.
#[tauri::command]
fn ping() -> Pong {
    shopcore_core::ping()
}

/// Debug-only: milliseconds since process start. Used by the bench debug
/// route (spec 002, scenario 7). Not a product feature; only the debug
/// route calls it. Present in release builds because the harness measures
/// release builds only.
#[tauri::command]
fn bench_startup_ms() -> f64 {
    PROCESS_START
        .get()
        .map_or(0.0, |start| start.elapsed().as_secs_f64() * 1_000.0)
}

/// Debug-only: write the UI bench results JSON to the OS temp dir so the
/// `just bench` runner can merge it (`--ui-results`). Returns the path.
/// Only the debug route calls it.
#[tauri::command]
fn bench_ui_results(payload: String) -> Result<String, String> {
    let path = std::env::temp_dir().join("shopcore-ui-results.json");
    std::fs::write(&path, payload).map_err(|err| err.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// The session the sell screen works as (`None` until sign-in lands in
/// 001d, except in dev where the auto-session fills it).
fn get_session_impl(state: &AppState) -> Option<SessionDto> {
    state.session().map(SessionDto::from)
}

/// The till status the status bar renders: whether a shift is open
/// (re-queried, so it stays live), the printer state from the mock
/// driver, and whether sync is configured (`false` until sync is
/// actually configured — never a fake synced state).
fn get_status_impl(state: &AppState) -> Result<StatusDto, AppError> {
    let shift_open = match state.session() {
        Some(session) => {
            open_shift_id(state.core.db(), &session.tenant_id, &session.terminal_id)?.is_some()
        }
        None => false,
    };
    let printer_status = state
        .printer
        .lock()
        .map_or("unknown".to_string(), |printer| {
            if printer.is_offline() {
                "offline".to_string()
            } else {
                "ready".to_string()
            }
        });
    Ok(StatusDto {
        shift_open,
        printer_status,
        sync_configured: false,
    })
}

/// The current cart: the terminal's open draft (restored after a
/// crash/restart, created when none exists) with priced lines + totals.
fn get_cart_impl(state: &AppState) -> Result<CartViewDto, AppError> {
    state.current_cart_view()
}

/// Search SKUs by name/code/barcode. Returns at most 20 hits for the
/// sell screen's simple list (the core caps at 50).
fn search_skus_impl(state: &AppState, query: &str) -> Result<Vec<SkuHitDto>, AppError> {
    let session = state.require_session()?;
    let mut index = state
        .search_index
        .lock()
        .map_err(|_| AppError::new("DB_ERROR", "search index lock poisoned".to_string()))?;
    let hits = core_search_skus(
        state.core.db(),
        &session.tenant_id,
        query,
        &mut index,
        now_ms(),
    )?;
    Ok(hits
        .into_iter()
        .take(20)
        .map(|hit| SkuHitDto {
            sku_id: hit.sku_id,
            item_id: hit.item_id,
            code: hit.code,
            name: hit.name,
            tax_group_id: hit.tax_group_id,
            price_e4: hit.price_e4,
        })
        .collect())
}

/// Look a barcode up: the `SKU` + its active price, or
/// `BARCODE_NOT_FOUND` (the UI shows an inline message with the
/// scanned code and keeps the cart).
fn lookup_barcode_impl(state: &AppState, code: &str) -> Result<SkuHitDto, AppError> {
    let session = state.require_session()?;
    let hit = core_lookup_barcode(state.core.db(), &session.tenant_id, code, now_ms())?;
    Ok(SkuHitDto {
        sku_id: hit.sku_id,
        item_id: hit.item_id,
        code: hit.code,
        name: hit.name,
        tax_group_id: hit.tax_group_id,
        price_e4: hit.price_e4,
    })
}

/// Add a line to the draft (same `SKU` + price merges, a different
/// price opens a separate line) and return the priced cart.
fn draft_add_line_impl(
    state: &AppState,
    draft_id: &str,
    sku_id: &str,
    qty_milli: i64,
    unit_price_e4: i64,
) -> Result<CartViewDto, AppError> {
    check_safe_integer(qty_milli)?;
    check_safe_integer(unit_price_e4)?;
    core_draft_add_line(
        state.core.scratch(),
        draft_id,
        sku_id,
        qty_milli,
        unit_price_e4,
        now_ms(),
    )?;
    state.cart_view(draft_id)
}

/// Set a line's quantity (0 removes the line, stashing it for undo)
/// and return the priced cart.
fn draft_set_qty_impl(
    state: &AppState,
    draft_id: &str,
    line_id: &str,
    qty_milli: i64,
) -> Result<CartViewDto, AppError> {
    check_safe_integer(qty_milli)?;
    core_draft_set_qty(state.core.scratch(), draft_id, line_id, qty_milli, now_ms())?;
    state.cart_view(draft_id)
}

/// Remove a line (stashing it for undo) and return the priced cart.
fn draft_remove_line_impl(
    state: &AppState,
    draft_id: &str,
    line_id: &str,
) -> Result<CartViewDto, AppError> {
    core_draft_remove_line(state.core.scratch(), draft_id, line_id, now_ms())?;
    state.cart_view(draft_id)
}

/// Cancel the sale (with a confirm step in the UI; writes an audit row
/// when the cart had lines) and return a fresh empty cart.
fn draft_cancel_impl(state: &AppState, draft_id: &str) -> Result<CartViewDto, AppError> {
    let session = state.require_session()?;
    core_draft_cancel(
        state.core.db(),
        state.core.scratch(),
        draft_id,
        session.user_id.as_deref().unwrap_or(""),
        now_ms(),
    )?;
    state.current_cart_view()
}

/// Restore the most recently removed line (single-level undo) and
/// return the priced cart.
fn draft_restore_line_impl(state: &AppState, draft_id: &str) -> Result<CartViewDto, AppError> {
    core_draft_restore_line(state.core.scratch(), draft_id, now_ms())?;
    state.cart_view(draft_id)
}

/// The session the sell screen works as (`None` until sign-in lands in
/// 001d, except in dev where the auto-session fills it).
#[tauri::command]
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
fn get_session(state: State<'_, AppState>) -> Result<Option<SessionDto>, AppError> {
    Ok(get_session_impl(&state))
}

/// The till status the status bar renders.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_status(state: State<'_, AppState>) -> Result<StatusDto, AppError> {
    get_status_impl(&state)
}

/// The current cart: the terminal's open draft (restored after a
/// crash/restart, created when none exists) with priced lines + totals.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_cart(state: State<'_, AppState>) -> Result<CartViewDto, AppError> {
    get_cart_impl(&state)
}

/// Search SKUs by name/code/barcode.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn search_skus(state: State<'_, AppState>, query: String) -> Result<Vec<SkuHitDto>, AppError> {
    search_skus_impl(&state, &query)
}

/// Look a barcode up.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn lookup_barcode(state: State<'_, AppState>, code: String) -> Result<SkuHitDto, AppError> {
    lookup_barcode_impl(&state, &code)
}

/// Add a line to the draft and return the priced cart.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn draft_add_line(
    state: State<'_, AppState>,
    draft_id: String,
    sku_id: String,
    qty_milli: i64,
    unit_price_e4: i64,
) -> Result<CartViewDto, AppError> {
    draft_add_line_impl(&state, &draft_id, &sku_id, qty_milli, unit_price_e4)
}

/// Set a line's quantity and return the priced cart.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn draft_set_qty(
    state: State<'_, AppState>,
    draft_id: String,
    line_id: String,
    qty_milli: i64,
) -> Result<CartViewDto, AppError> {
    draft_set_qty_impl(&state, &draft_id, &line_id, qty_milli)
}

/// Remove a line and return the priced cart.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn draft_remove_line(
    state: State<'_, AppState>,
    draft_id: String,
    line_id: String,
) -> Result<CartViewDto, AppError> {
    draft_remove_line_impl(&state, &draft_id, &line_id)
}

/// Cancel the sale and return a fresh empty cart.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn draft_cancel(state: State<'_, AppState>, draft_id: String) -> Result<CartViewDto, AppError> {
    draft_cancel_impl(&state, &draft_id)
}

/// Restore the most recently removed line and return the priced cart.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn draft_restore_line(
    state: State<'_, AppState>,
    draft_id: String,
) -> Result<CartViewDto, AppError> {
    draft_restore_line_impl(&state, &draft_id)
}

/// Open the databases and build the managed state: the `DB` + scratch
/// `DB` under the app data dir (`%AppData%/ShopCore` on Windows),
/// one [`Core`], and the session (dev auto-session with the
/// `dev-session` feature, otherwise `None` until 001d).
fn build_app_state(app: &AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    // Dev uses no key (in-memory/unencrypted is the documented dev
    // state — see 07 key management); production DPAPI sealing is a
    // pre-pilot spec.
    let db = Db::open(data_dir.join("shopcore.db"), None)?;
    let scratch = shopcore_core::ScratchDb::open(data_dir.join("scratch.db"))?;
    let core = Core::new(db, scratch);
    #[cfg(feature = "dev-session")]
    let session = Some(dev_auto_session(core.db(), now_ms())?);
    #[cfg(not(feature = "dev-session"))]
    let session = None;
    Ok(AppState::new(core, session))
}

/// Shut the managed core down (amendment 8): the exit path. Best
/// effort — a stuck thread is logged, never panicked over, at exit.
fn shutdown_on_exit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>()
        && let Err(err) = state.shutdown()
    {
        eprintln!("ShopCore shutdown failed: {err}");
    }
}

// `generate_context!()` expands code generated from tauri.conf.json,
// which trips pedantic lints (manual_string_new, default_trait_access,
// single_match_else) and the workspace unwrap/expect denies. The allow
// is scoped to this function only; hand-written code elsewhere keeps
// the full deny set.
#[allow(
    clippy::manual_string_new,
    clippy::default_trait_access,
    clippy::single_match_else,
    clippy::unwrap_used,
    clippy::expect_used
)]
fn main() {
    let _ = PROCESS_START.set(Instant::now());
    let app = tauri::Builder::default()
        .setup(|app| {
            let state = build_app_state(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            bench_startup_ms,
            bench_ui_results,
            get_session,
            get_status,
            get_cart,
            search_skus,
            lookup_barcode,
            draft_add_line,
            draft_set_qty,
            draft_remove_line,
            draft_cancel,
            draft_restore_line
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("failed to build ShopCore: {err}");
            std::process::exit(1);
        });
    // Amendment 8 (ADR-036): the exit event MUST call
    // `Core::shutdown()` before the process exits — the actor and
    // timer threads are joined (5 s timeout) and the connections
    // closed, so the process cannot segfault during teardown.
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            shutdown_on_exit(app);
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use shopcore_core::settings::{
        KEY_ACTIVE_PRICE_LIST, KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT,
        set_setting,
    };
    use shopcore_core::{Core, ScratchDb};
    use shopcore_db::Db;

    use super::*;

    /// `#[tauri::command]` fn names in this file (`main.rs`).
    fn rust_commands() -> BTreeSet<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
        let text = std::fs::read_to_string(&path).expect("read main.rs");
        let mut out = BTreeSet::new();
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            if line.trim() == "#[tauri::command]" {
                for next in lines.by_ref() {
                    let next = next.trim();
                    // Skip doc comments, blank lines, and attributes
                    // (e.g. the clippy allow on command fns, 001c-1).
                    if next.is_empty()
                        || next.starts_with("///")
                        || next.starts_with("//")
                        || next.starts_with("#[")
                    {
                        continue;
                    }
                    if let Some(rest) = next.strip_prefix("fn ") {
                        let name: String = rest.chars().take_while(|c| *c != '(').collect();
                        out.insert(name.trim().to_string());
                    }
                    break;
                }
            }
        }
        out
    }

    /// `invoke("name")` / `invoke<T>("name")` string literals in every
    /// `.ts` / `.tsx` file under `apps/ui/src`. No regex crate: a small
    /// byte scanner that skips the optional `<...>` generic after
    /// `invoke`. Byte-based on purpose: the sources contain multi-byte
    /// UTF-8 (e.g. `…` in comments), so char-index slicing would panic.
    /// Indexing-free (`.get()` only): the workspace denies
    /// `clippy::indexing_slicing`.
    fn ts_invokes() -> BTreeSet<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../apps/ui/src");
        let mut files = Vec::new();
        collect_ts_files(&root, &mut files);
        let mut out = BTreeSet::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read TS file");
            let bytes = text.as_bytes();
            let mut i = 0;
            while i + 6 <= bytes.len() {
                if bytes.get(i..i + 6).is_some_and(|s| s == b"invoke") {
                    let mut j = i + 6;
                    // Skip whitespace and an optional <...> generic.
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'<') {
                        while bytes.get(j).is_some_and(|b| *b != b'>') {
                            j += 1;
                        }
                        j += 1;
                    }
                    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                        j += 1;
                    }
                    if bytes.get(j) == Some(&b'(') {
                        j += 1;
                        while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                            j += 1;
                        }
                        if bytes.get(j).is_some_and(|b| *b == b'"' || *b == b'\'') {
                            let quote = bytes.get(j).copied().unwrap_or(b'"');
                            j += 1;
                            let start = j;
                            while bytes.get(j).is_some_and(|b| *b != quote) {
                                j += 1;
                            }
                            if let Some(name) = bytes.get(start..j) {
                                out.insert(String::from_utf8_lossy(name).into_owned());
                            }
                        }
                    }
                    i = j.max(i + 1);
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    fn collect_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_ts_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
                out.push(path);
            }
        }
    }

    /// ADR-013 drift guard: every Rust `#[tauri::command]` must have a
    /// matching `invoke("name")` in the UI, and every `invoke` in the
    /// UI must have a matching Rust command. A rename on one side
    /// without the other breaks the call at runtime with no compile
    /// error, so this test is the only thing that catches it.
    /// (The bench commands are invoked directly from `BenchDebug.tsx`,
    /// not through `TauriClient`, so the scan covers all of
    /// `apps/ui/src`, not just the client.)
    #[test]
    fn tauri_commands_match_ui_invokes() {
        let rust = rust_commands();
        let ts = ts_invokes();
        assert!(
            !rust.is_empty(),
            "no #[tauri::command] found in main.rs — the scan is broken"
        );
        assert!(
            !ts.is_empty(),
            "no invoke() found under apps/ui/src — the scan is broken"
        );
        let missing_in_ui: Vec<_> = rust.difference(&ts).collect();
        let missing_in_rust: Vec<_> = ts.difference(&rust).collect();
        assert!(
            missing_in_ui.is_empty() && missing_in_rust.is_empty(),
            "Tauri command / UI invoke drift: commands with no UI invoke: {missing_in_ui:?}; \
             UI invokes with no Rust command: {missing_in_rust:?}"
        );
    }

    /// Amendment 3: 2^53-1 is accepted, 2^53 and -(2^53) are rejected —
    /// for both signs, because a negative out-of-range integer is
    /// equally unrepresentable in JavaScript.
    #[test]
    fn safe_integer_boundary() {
        assert!(check_safe_integer(MAX_SAFE_INTEGER).is_ok());
        assert!(check_safe_integer(-MAX_SAFE_INTEGER).is_ok());
        assert!(check_safe_integer(0).is_ok());
        assert_eq!(
            check_safe_integer(MAX_SAFE_INTEGER + 1).unwrap_err().code,
            "INVALID_INTEGER"
        );
        assert_eq!(
            check_safe_integer(-MAX_SAFE_INTEGER - 1).unwrap_err().code,
            "INVALID_INTEGER"
        );
    }

    /// A fresh temp dir for the scratch `DB` (removed on drop). The
    /// name carries a `UUIDv7`, so concurrent test processes never
    /// share one (same lesson as ADR-035).
    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("shopcore-desktop-test-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&path).expect("create temp dir");
            Self(path)
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A test `AppState`: in-memory `DB` + scratch `DB` in a fresh
    /// temp dir, a hand-built session, and a minimal catalog (one
    /// 18%-exclusive group, one SKU with a barcode + price).
    struct TestApp {
        state: AppState,
        _dir: TempDirGuard,
        sku_id: String,
    }

    /// Seed a minimal catalog (one 18%-exclusive group, one SKU with
    /// a barcode + price) and return `(price_list_id, sku_id)`.
    fn seed_test_catalog(db: &Db) -> (String, String) {
        let tenant = "test-tenant".to_string();
        let tenant_c = tenant.clone();
        let price_list_id = uuid::Uuid::now_v7().to_string();
        let price_list_id_c = price_list_id.clone();
        let tax_group_id = uuid::Uuid::now_v7().to_string();
        let tax_group_id_c = tax_group_id.clone();
        let sku_id = uuid::Uuid::now_v7().to_string();
        let sku_id_c = sku_id.clone();
        let item_id = uuid::Uuid::now_v7().to_string();
        let item_id_c = item_id.clone();
        db.write(move |conn| {
                let tx = conn.transaction().expect("tx");
                tx.execute(
                    "INSERT INTO branch (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'MAIN', 'Main Branch', 1, 1, 1)",
                    rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_c],
                )
                .expect("branch");
                tx.execute(
                    "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard 18%', 1, 1, 1)",
                    rusqlite::params![tax_group_id_c, tenant_c],
                )
                .expect("tax group");
                tx.execute(
                    "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, 1, 1, 1)",
                    rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_c, tax_group_id_c],
                )
                .expect("tax rate");
                tx.execute(
                    "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard', 1, 1, 1)",
                    rusqlite::params![price_list_id_c, tenant_c],
                )
                .expect("price list");
                tx.execute(
                    "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, 'TST100', 'Test 100', '', ?3, 1, 1, 1)",
                    rusqlite::params![item_id_c, tenant_c, tax_group_id_c],
                )
                .expect("item");
                tx.execute(
                    "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, 'TST100', 'Test 100', '', ?4, 1, 1, 1)",
                    rusqlite::params![sku_id_c, tenant_c, item_id_c, tax_group_id_c],
                )
                .expect("sku");
                tx.execute(
                    "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES (?1, ?2, ?3, '1234567890100', 1, 1, 1, 1)",
                    rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_c, sku_id_c],
                )
                .expect("barcode");
                tx.execute(
                    "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1000000, 1, 1, 1)",
                    rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_c, price_list_id_c, sku_id_c],
                )
                .expect("price entry");
                tx.commit().expect("commit");
                Ok(())
            })
            .expect("seed catalog");
        (price_list_id, sku_id)
    }

    impl TestApp {
        fn new() -> Self {
            let dir = TempDirGuard::new();
            let db = Db::open_in_memory().expect("in-memory db");
            let scratch = ScratchDb::open(dir.0.join("scratch.db")).expect("scratch db");
            let (price_list_id, sku_id) = seed_test_catalog(&db);
            set_setting(
                &db,
                "test-tenant",
                SCOPE_TENANT,
                "",
                KEY_TENANT_CURRENCY,
                "PKR",
                1,
            )
            .expect("currency");
            set_setting(
                &db,
                "test-tenant",
                SCOPE_TENANT,
                "",
                KEY_TENANT_CURRENCY_EXPONENT,
                "2",
                1,
            )
            .expect("exponent");
            set_setting(
                &db,
                "test-tenant",
                SCOPE_TENANT,
                "",
                KEY_ACTIVE_PRICE_LIST,
                &price_list_id,
                1,
            )
            .expect("price list setting");
            let core = Core::new(db, scratch);
            let session = Session {
                tenant_id: "test-tenant".to_string(),
                branch_id: "branch-1".to_string(),
                branch_code: "MAIN".to_string(),
                terminal_id: "terminal-1".to_string(),
                terminal_code: "T1".to_string(),
                user_id: Some("user-1".to_string()),
                username: Some("cashier".to_string()),
                shift_id: None,
                currency_code: "PKR".to_string(),
                currency_exponent: 2,
            };
            Self {
                state: AppState::new(core, Some(session)),
                _dir: dir,
                sku_id,
            }
        }

        /// The current cart view through the command impl path.
        fn get_cart(&self) -> CartViewDto {
            self.state.current_cart_view().expect("get cart")
        }
    }

    /// The full scan flow through the command impls: lookup → add →
    /// priced cart, with the 001a golden totals (100.00 @18% → 118.00).
    #[test]
    fn scan_flow_lookup_add_prices_the_cart() {
        let app = TestApp::new();
        let hit = lookup_barcode_impl(&app.state, "1234567890100").expect("lookup");
        assert_eq!(hit.name, "Test 100");
        assert_eq!(hit.price_e4, Some(1_000_000));
        let cart = app.get_cart();
        let view = draft_add_line_impl(
            &app.state,
            &cart.draft_id,
            &hit.sku_id,
            1_000,
            hit.price_e4.expect("price"),
        )
        .expect("add line");
        assert_eq!(view.lines.len(), 1);
        let line = view.lines.first().expect("line");
        assert_eq!(line.name, "Test 100");
        assert_eq!(view.totals.subtotal_minor, 10_000);
        assert_eq!(view.totals.net_subtotal_minor, 10_000);
        assert_eq!(view.totals.tax_minor, 1_800);
        assert_eq!(view.totals.total_minor, 11_800);
        // A second scan of the same SKU merges into qty 2.
        let view = draft_add_line_impl(&app.state, &cart.draft_id, &hit.sku_id, 1_000, 1_000_000)
            .expect("add again");
        assert_eq!(view.lines.len(), 1);
        let line = view.lines.first().expect("line");
        assert_eq!(line.qty_milli, 2_000);
        assert_eq!(view.totals.total_minor, 23_600);
    }

    /// An unknown barcode fails with the stable code (the UI shows an
    /// inline message with the scanned code and keeps the cart).
    #[test]
    fn unknown_barcode_returns_barcode_not_found() {
        let app = TestApp::new();
        let err = lookup_barcode_impl(&app.state, "1234567890123").unwrap_err();
        assert_eq!(err.code, "BARCODE_NOT_FOUND");
        assert!(err.message.contains("1234567890123"));
    }

    /// A quantity past the JSON safe-integer limit is rejected before
    /// it reaches the use case.
    #[test]
    fn out_of_range_quantity_is_rejected() {
        let app = TestApp::new();
        let cart = app.get_cart();
        let err = draft_add_line_impl(
            &app.state,
            &cart.draft_id,
            &app.sku_id,
            MAX_SAFE_INTEGER + 1,
            1_000_000,
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_INTEGER");
    }

    /// Remove + Ctrl+Z restore round-trips through the command impls.
    #[test]
    fn remove_and_restore_line_round_trips() {
        let app = TestApp::new();
        let cart = app.get_cart();
        let view = draft_add_line_impl(&app.state, &cart.draft_id, &app.sku_id, 1_000, 1_000_000)
            .expect("add");
        assert_eq!(view.lines.len(), 1);
        let line_id = view.lines.first().expect("line").id.clone();
        let view = draft_remove_line_impl(&app.state, &cart.draft_id, &line_id).expect("remove");
        assert_eq!(view.lines, []);
        let view = draft_restore_line_impl(&app.state, &cart.draft_id).expect("restore");
        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.totals.total_minor, 11_800);
    }

    /// Cancel discards the draft and returns a fresh empty cart.
    #[test]
    fn cancel_returns_a_fresh_empty_cart() {
        let app = TestApp::new();
        let cart = app.get_cart();
        draft_add_line_impl(&app.state, &cart.draft_id, &app.sku_id, 1_000, 1_000_000)
            .expect("add");
        let view = draft_cancel_impl(&app.state, &cart.draft_id).expect("cancel");
        assert_eq!(view.lines, []);
        assert_eq!(view.totals.total_minor, 0);
        assert_ne!(view.draft_id, cart.draft_id, "a fresh draft is opened");
    }

    /// `get_status` reports the real states: no shift open in the
    /// fixture, the mock printer ready, sync not configured.
    #[test]
    fn status_reports_shift_printer_and_sync() {
        let app = TestApp::new();
        let status = get_status_impl(&app.state).expect("status");
        assert!(!status.shift_open);
        assert_eq!(status.printer_status, "ready");
        assert!(!status.sync_configured);
    }

    /// Amendment 8: the exit path calls `shutdown` — after it, the
    /// actor thread is gone (writes fail) and the scratch files can
    /// be deleted (the timer thread is joined).
    #[test]
    fn shutdown_joins_threads_and_closes_connections() {
        let app = TestApp::new();
        let cart = app.get_cart();
        assert!(!cart.draft_id.is_empty(), "the cart loads before shutdown");
        app.state.shutdown().expect("shutdown");
        // A second shutdown is a no-op (idempotent).
        app.state.shutdown().expect("second shutdown");
        // The actor thread is gone: writes fail with ActorStopped.
        let db = app.state.core.db();
        let write_result = db.write(|_| Ok(()));
        assert!(
            matches!(write_result, Err(shopcore_db::DbError::ActorStopped)),
            "expected ActorStopped after shutdown, got {write_result:?}"
        );
    }
}

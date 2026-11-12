//! The e2e shim (spec 001c-2, amendment 1, option A): an HTTP server
//! the Playwright tests drive the desktop through. It exists ONLY
//! under the `e2e-shim` cargo feature — the release build has no
//! axum, no shim module, and no token handling (CI greps the release
//! binary for `SHOPCORE_E2E_SHIM_V1`, and `cargo tree` proves axum
//! is absent from the release graph).
//!
//! The five option-A conditions:
//! (a) cargo feature gate (`e2e-shim`), never `debug_assertions`;
//! (b) binds 127.0.0.1 only, on an ephemeral port;
//! (c) a per-run token (from the environment, else generated —
//!     nanosecond time + pid + counter, hex; its job is to prove the
//!     caller is the harness that spawned this process, not to be a
//!     cryptographic secret), requests without it are rejected 401;
//! (d) ONE dispatch table: `dispatch` below matches on the command
//!     name and calls the SAME `*_impl` functions the Tauri commands
//!     call — the shim adds no logic of its own. Note the Tauri
//!     `complete_sale_cash` wrapper ALSO emits `sale.completed`; the
//!     shim does not emit it (the e2e flow drives state from command
//!     returns, and there is no push channel over HTTP in 001c-2);
//! (e) release-absence test (CI grep + `cargo tree`).
//!
//! The bench commands (`bench_startup_ms`, `bench_ui_results`) are
//! NOT exposed: they are debug-only (one writes a file into the OS
//! temp dir) and the e2e flow never needs them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::{
    AppError, AppState, complete_sale_cash_impl, draft_add_line_impl, draft_apply_discount_impl,
    draft_cancel_impl, draft_remove_line_impl, draft_restore_line_impl, draft_set_qty_impl,
    get_cart_impl, get_session_impl, get_status_impl, list_sample_barcodes_impl,
    lookup_barcode_impl, reprint_receipt_impl, search_skus_impl,
};

/// Marker for the release-absence test: CI greps the release binary
/// for this string and fails if it is present. It is printed at shim
/// startup (so the literal survives in the e2e binary's rodata).
pub const SHOPCORE_E2E_SHIM_V1: &str = "SHOPCORE_E2E_SHIM_V1";

/// The request header carrying the per-run token.
const TOKEN_HEADER: &str = "x-shopcore-e2e-token";

#[derive(Clone)]
struct E2eState {
    app: AppHandle,
    token: String,
}

/// The per-run token: `SHOPCORE_E2E_TOKEN` when the harness sets it,
/// else a generated value (nanosecond time + pid + counter, hex).
fn resolve_token() -> String {
    match std::env::var("SHOPCORE_E2E_TOKEN") {
        Ok(token) if !token.is_empty() => token,
        _ => generate_token(),
    }
}

fn generate_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{:x}-{n:x}", std::process::id())
}

/// Whether the request carries this run's token. Pure (no axum
/// service needed) so the unit test covers it directly.
pub(crate) fn check_token(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == token)
}

/// Parse the command's JSON args, or fail `INVALID_INPUT`.
fn args<T>(body: Value, command: &str) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(body)
        .map_err(|err| AppError::new("INVALID_INPUT", format!("bad {command} args: {err}")))
}

/// Serialize a command result to JSON, or fail `INTERNAL`.
fn to_json<T>(value: &T) -> Result<Value, AppError>
where
    T: serde::Serialize,
{
    serde_json::to_value(value)
        .map_err(|err| AppError::new("INTERNAL", format!("cannot serialize result: {err}")))
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
}

#[derive(Deserialize)]
struct BarcodeArgs {
    code: String,
}

#[derive(Deserialize)]
struct AddLineArgs {
    draft_id: String,
    sku_id: String,
    qty_milli: i64,
    unit_price_e4: i64,
}

#[derive(Deserialize)]
struct SetQtyArgs {
    draft_id: String,
    line_id: String,
    qty_milli: i64,
}

#[derive(Deserialize)]
struct LineArgs {
    draft_id: String,
    line_id: String,
}

#[derive(Deserialize)]
struct DraftArgs {
    draft_id: String,
}

#[derive(Deserialize)]
struct DiscountArgs {
    draft_id: String,
    line_id: String,
    kind: String,
    value: i64,
    #[serde(default)]
    manager_username: Option<String>,
    #[serde(default)]
    manager_pin: Option<String>,
}

#[derive(Deserialize)]
struct CompleteArgs {
    draft_id: String,
    tender_minor: i64,
}

#[derive(Deserialize)]
struct ReprintArgs {
    sale_id: String,
}

/// THE dispatch table (condition d): the command name routes to the
/// same `*_impl` the Tauri command calls. Nothing here re-implements
/// a use case; an unknown name is `NOT_FOUND`, bad args are
/// `INVALID_INPUT`.
pub(crate) async fn dispatch(
    state: &AppState,
    command: &str,
    body: Value,
) -> Result<Value, AppError> {
    match command {
        "ping" => to_json(&shopcore_core::ping()),
        "get_session" => to_json(&get_session_impl(state)),
        "get_status" => to_json(&get_status_impl(state)?),
        "list_sample_barcodes" => to_json(&list_sample_barcodes_impl(state)?),
        "get_cart" => to_json(&get_cart_impl(state)?),
        "search_skus" => {
            let parsed: SearchArgs = args(body, command)?;
            to_json(&search_skus_impl(state, &parsed.query)?)
        }
        "lookup_barcode" => {
            let parsed: BarcodeArgs = args(body, command)?;
            to_json(&lookup_barcode_impl(state, &parsed.code)?)
        }
        "draft_add_line" => {
            let parsed: AddLineArgs = args(body, command)?;
            to_json(&draft_add_line_impl(
                state,
                &parsed.draft_id,
                &parsed.sku_id,
                parsed.qty_milli,
                parsed.unit_price_e4,
            )?)
        }
        "draft_set_qty" => {
            let parsed: SetQtyArgs = args(body, command)?;
            to_json(&draft_set_qty_impl(
                state,
                &parsed.draft_id,
                &parsed.line_id,
                parsed.qty_milli,
            )?)
        }
        "draft_remove_line" => {
            let parsed: LineArgs = args(body, command)?;
            to_json(&draft_remove_line_impl(
                state,
                &parsed.draft_id,
                &parsed.line_id,
            )?)
        }
        "draft_cancel" => {
            let parsed: DraftArgs = args(body, command)?;
            to_json(&draft_cancel_impl(state, &parsed.draft_id)?)
        }
        "draft_restore_line" => {
            let parsed: DraftArgs = args(body, command)?;
            to_json(&draft_restore_line_impl(state, &parsed.draft_id)?)
        }
        "draft_apply_discount" => {
            let parsed: DiscountArgs = args(body, command)?;
            to_json(&draft_apply_discount_impl(
                state,
                &parsed.draft_id,
                &parsed.line_id,
                &parsed.kind,
                parsed.value,
                parsed.manager_username.as_deref(),
                parsed.manager_pin.as_deref(),
            )?)
        }
        "complete_sale_cash" => {
            let parsed: CompleteArgs = args(body, command)?;
            to_json(&complete_sale_cash_impl(state, &parsed.draft_id, parsed.tender_minor).await?)
        }
        "reprint_receipt" => {
            let parsed: ReprintArgs = args(body, command)?;
            to_json(&reprint_receipt_impl(state, &parsed.sale_id).await?)
        }
        _ => Err(AppError::new(
            "NOT_FOUND",
            format!("unknown e2e command {command}"),
        )),
    }
}

/// The CORS headers the browser needs: the e2e UI (served from
/// `vite preview` on :4173) calls the shim on an ephemeral port,
/// so every call is cross-origin, and the custom token header +
/// JSON body force a preflight. The shim is dev-only, bound to
/// 127.0.0.1, and token-gated — `*` is fine here; there are no
/// credentials and no other origin can reach it.
const CORS_ALLOW_ORIGIN: &str = "*";
const CORS_ALLOW_HEADERS: &str = "content-type, x-shopcore-e2e-token";
const CORS_ALLOW_METHODS: &str = "POST, OPTIONS";

/// Answer a CORS preflight for any shim command.
async fn handle_options() -> Response {
    let mut response = (StatusCode::NO_CONTENT, ()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        "access-control-allow-origin",
        HeaderValue::from_static(CORS_ALLOW_ORIGIN),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static(CORS_ALLOW_HEADERS),
    );
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static(CORS_ALLOW_METHODS),
    );
    response
}

async fn handle_command(
    State(state): State<E2eState>,
    headers: HeaderMap,
    Path(command): Path<String>,
    body: String,
) -> Response {
    if !check_token(&headers, &state.token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": "UNAUTHORIZED", "message": "missing or invalid e2e token"})),
        )
            .into_response();
    }
    let value: Value = if body.trim().is_empty() {
        Value::Null
    } else {
        match serde_json::from_str(&body) {
            Ok(parsed) => parsed,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"code": "INVALID_INPUT", "message": format!("body is not JSON: {err}")})),
                )
                    .into_response();
            }
        }
    };
    let app_state = state.app.state::<AppState>();
    // Every response carries the CORS origin header: without it the
    // browser refuses to expose even a successful cross-origin call.
    let mut response = match dispatch(&app_state, &command, value).await {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) if err.code == "NOT_FOUND" => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": err.code, "message": err.message})),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"code": err.code, "message": err.message})),
        )
            .into_response(),
    };
    response.headers_mut().insert(
        "access-control-allow-origin",
        HeaderValue::from_static(CORS_ALLOW_ORIGIN),
    );
    response
}

/// Bind the shim on 127.0.0.1 (ephemeral port), print the marker +
/// port + token for the harness to read from stdout, and serve until
/// the process exits. Spawned from `.setup()` — never awaited.
pub async fn serve_shim(app: AppHandle) -> Result<(), std::io::Error> {
    let token = resolve_token();
    let state = E2eState {
        app,
        token: token.clone(),
    };
    // axum 0.8 uses `{capture}`, not the old `:capture` — the `:`
    // form panics at router construction ("Path segments must not
    // start with `:`").
    let router = Router::new()
        .route(
            "/__e2e/{command}",
            post(handle_command).options(handle_options),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    println!("{SHOPCORE_E2E_SHIM_V1}");
    println!("SHOPCORE_E2E_PORT={port}");
    println!("SHOPCORE_E2E_TOKEN={token}");
    axum::serve(listener, router).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn token_check_accepts_only_this_runs_token() {
        let mut headers = HeaderMap::new();
        assert!(!check_token(&headers, "abc"));
        headers.insert(TOKEN_HEADER, "wrong".parse().expect("header value"));
        assert!(!check_token(&headers, "abc"));
        headers.insert(TOKEN_HEADER, "abc".parse().expect("header value"));
        assert!(check_token(&headers, "abc"));
    }

    #[test]
    fn generated_tokens_differ_per_call() {
        assert_ne!(generate_token(), generate_token());
    }
}

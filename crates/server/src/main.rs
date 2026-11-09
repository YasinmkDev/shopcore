//! `ShopCore` cloud server: Axum + `PostgreSQL` API for head office,
//! multi-branch reporting, remote admin, and backups (Phase 5). Tenants
//! are isolated by row-level security; large tables are partitioned by
//! month.
//!
//! Spec 000: stub binary. The `API` lands with the Phase 5 server spec.

fn main() {
    eprintln!("shopcore-server is not implemented yet (Phase 5).");
    std::process::exit(2);
}

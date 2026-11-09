//! `ShopCore` hub: headless `LAN` service for multi-till stores (Phase 5).
//! One PC in a store runs the hub; it owns the store `DB`, applies sales in
//! a single writer, and syncs with cloud. Client tills connect over `LAN`.
//!
//! Spec 000: stub binary. The service lands with the Phase 5 hub spec.

fn main() {
    eprintln!("shopcore-hub is not implemented yet (Phase 5).");
    std::process::exit(2);
}

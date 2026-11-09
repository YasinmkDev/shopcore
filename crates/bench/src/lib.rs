//! `ShopCore` benchmarks: seeds data (`just seed-large`: 500k `SKUs`, `5M` sale
//! lines) and runs the budgets in `context/05-performance-budgets.md`
//! using criterion + scripted scenarios. `just bench` is the budget gate
//! in CI.
//!
//! Spec 000: this crate is empty. Benchmarks and the seeder land with the
//! Phase 0 benchmark spec.

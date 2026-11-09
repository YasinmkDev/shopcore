//! `ShopCore` db crate: migrations, queries, `FTS`, and aggregates over the
//! local `SQLite` database (`WAL`, `synchronous=FULL`, `SQLCipher` — see
//! `context/04-data-and-sync.md`).
//!
//! Access rules (AGENTS.md): one write connection owned by the `DB` actor;
//! a pool of read-only connections serves queries. Migrations are
//! forward-only numbered `SQL` files, never edited after shipping.
//!
//! Spec 000: this crate is empty. Its `rusqlite` dependency exists here
//! only as a compile check — it proves the riskiest native dependency
//! (`SQLCipher`, built from vendored `OpenSSL`) compiles on the Windows CI
//! runner before any schema work depends on it. Migrations, the `DB` actor,
//! and queries land with the `DB` spec.

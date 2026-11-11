//! `ShopCore` db crate: migrations, the `DB` actor, and queries over the
//! local `SQLite` database (`WAL`, `synchronous=FULL`, `SQLCipher` — see
//! `context/04-data-and-sync.md`).
//!
//! Access rules (AGENTS.md): one write connection owned by the `DB` actor
//! (a dedicated thread); a pool of read-only connections serves queries.
//! All access goes through the actor — there is no public way to get a
//! `Connection` out of this crate. Migrations are forward-only numbered
//! `SQL` files, never edited after shipping (ADR-031).

pub mod actor;
pub mod error;
pub mod migrations;

pub use actor::Db;
pub use error::DbError;

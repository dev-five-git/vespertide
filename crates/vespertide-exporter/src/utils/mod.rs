//! Language-grouped utility helpers shared across ORM exporters.
//!
//! Each submodule groups helpers used by exporters targeting a single host
//! language. This layout scales cleanly as new languages are added (e.g.
//! `rust` for Diesel, `typescript` for Drizzle, `java` for full Hibernate),
//! without polluting the crate root with a flat list of `*_common.rs` files.
//!
//! Helpers that are not tied to one host language live in
//! `crate::utils::common`; keep a new helper in its language submodule until a
//! second language needs it.

pub(crate) mod common;
pub(crate) mod python;
pub(crate) mod typescript;
// Add future language helpers as siblings, e.g.
// pub(crate) mod rust;
// pub(crate) mod java;

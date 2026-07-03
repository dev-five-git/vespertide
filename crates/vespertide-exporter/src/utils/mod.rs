//! Language-grouped utility helpers shared across ORM exporters.
//!
//! Each submodule groups helpers used by exporters targeting a single host
//! language. This layout scales cleanly as new languages are added (e.g.
//! `rust` for Diesel, `typescript` for Drizzle, `java` for full Hibernate),
//! without polluting the crate root with a flat list of `*_common.rs` files.
//!
//! Cross-language helpers (case conversion, naming sanitization that is
//! language-agnostic) belong in `crate::utils::common` once they exist;
//! for now keep them in their language submodule until a second user appears.

pub(crate) mod python;
// Add future language helpers as siblings, e.g.
// pub(crate) mod rust;
// pub(crate) mod typescript;
// pub(crate) mod java;
// pub(crate) mod common;

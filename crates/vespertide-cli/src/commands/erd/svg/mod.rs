//! SVG ERD renderer.
//!
//! Layout-rs / Graphviz produced unattractive output (huge whitespace,
//! flat record shapes, no visual hierarchy). This module replaces it with
//! a fully custom layout + SVG emitter:
//!
//! * Tables are laid out in topological ranks (parents → children).
//! * Each table card has a dark header, rounded corners, and per-column
//!   PK/FK badges.
//! * Foreign-key edges are drawn as cubic Bézier curves between the
//!   nearest sides of the connected tables.
//!
//! Internal structure:
//!
//! * [`style`]  — palette + sizing constants shared across the module.
//! * [`model`]  — [`model::TableBox`] / [`model::EdgeSpec`] data model and
//!   builders that translate [`TableDef`]s into laid-out boxes.
//! * [`layout`] — rank assignment and grid placement.
//! * [`edges`]  — Bézier routing, anchor selection, and cardinality labels.
//! * [`render`] — SVG document scaffold and table-card emission.
//! * [`util`]   — `escape_xml` and the empty-diagram fallback.

mod edges;
mod layout;
mod model;
mod render;
mod style;
mod util;

use vespertide_core::TableDef;

use super::collect_foreign_key_relations;
use layout::{compute_ranks, layout_grid, view_size};
use model::{build_boxes, build_edges};
use render::render_doc;
use util::render_empty;

#[expect(
    clippy::unnecessary_wraps,
    reason = "render_svg keeps a Result API for future graph validation without changing ERD callers"
)]
pub fn render_svg(tables: &[TableDef]) -> Result<String, String> {
    if tables.is_empty() {
        return Ok(render_empty());
    }

    let mut boxes = build_boxes(tables);
    let relations = collect_foreign_key_relations(tables);
    let edges = build_edges(tables, &boxes, &relations);

    let ranks = compute_ranks(&boxes, &edges);
    layout_grid(&mut boxes, &ranks);

    let (vw, vh) = view_size(&boxes);
    Ok(render_doc(&boxes, &edges, vw, vh))
}

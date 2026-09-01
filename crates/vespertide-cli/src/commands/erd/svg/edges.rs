//! Edge routing + cardinality label placement.
//!
//! Foreign-key edges are drawn as cubic Bézier curves between the nearest
//! sides of the connected tables. Parallel edges are fanned out so labels
//! and curves never collapse onto a single arc.

// Edge geometry mixes the wire u32 parallel index with floating-point
// offsets and uses conventional short Bézier coordinate names that mirror
// the math formulas.
#![expect(
    clippy::cast_precision_loss,
    reason = "SVG layout converts bounded table/row counts into pixel coordinates"
)]
#![expect(
    clippy::uninlined_format_args,
    reason = "long SVG template strings keep repeated named arguments explicit for readability"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "geometry helpers pass Bézier anchors and side metadata directly; renderer context extraction is deferred"
)]
#![expect(
    clippy::similar_names,
    reason = "ERD geometry uses conventional short coordinate names that mirror Bézier formulas"
)]

use std::fmt::Write as _;

use super::model::{EdgeSpec, TableBox};
use super::style::{CARD_BORDER, EDGE_END, EDGE_STROKE, HEADER_H, ROW_H};
use super::util::escape_xml;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// Pre-computed routing data for a single edge.
///
/// Computed once per edge in [`super::render::render_doc`] and reused by both
/// [`render_edge_path`] and [`render_edge_label`] so the two render passes do
/// not recompute the anchors / curvature for every edge twice.
#[derive(Copy, Clone, Debug)]
pub(super) struct EdgeGeometry {
    pub sx: f64,
    pub sy: f64,
    pub ex: f64,
    pub ey: f64,
    pub sdir: Side,
    pub edir: Side,
    pub curvature: f64,
}

pub(super) fn edge_geometry(child: &TableBox, parent: &TableBox, edge: &EdgeSpec) -> EdgeGeometry {
    let child_y = child.y + HEADER_H + edge.child_row as f64 * ROW_H + ROW_H / 2.0;
    let parent_y = parent.y + HEADER_H + edge.parent_row as f64 * ROW_H + ROW_H / 2.0;
    let (sx, sy, ex, ey, sdir, edir) = pick_anchors(child, parent, child_y, parent_y);
    let curvature = parallel_curvature_offset(edge.parallel_index, edge.parallel_count);
    EdgeGeometry {
        sx,
        sy,
        ex,
        ey,
        sdir,
        edir,
        curvature,
    }
}

pub(super) fn render_edge_path(
    out: &mut String,
    child: &TableBox,
    parent: &TableBox,
    edge: &EdgeSpec,
    geom: EdgeGeometry,
) {
    let path = bezier_path(
        geom.sx,
        geom.sy,
        geom.ex,
        geom.ey,
        geom.sdir,
        geom.edir,
        geom.curvature,
    );

    // Two-layer stroke: subtle wide halo + crisp narrow stroke for a soft look.
    let _ = writeln!(
        out,
        "    <path d=\"{path}\" stroke=\"#ffffff\" stroke-width=\"4\" opacity=\"0.7\"/>"
    );
    let _ = writeln!(
        out,
        "    <path d=\"{path}\" stroke=\"{stroke}\" stroke-width=\"1.6\" \
         marker-start=\"url(#vespCircle)\" marker-end=\"url(#vespArrow)\">\
         <title>{title}</title></path>",
        stroke = EDGE_STROKE,
        title = escape_xml(&format!("{} {} → {}", child.name, edge.label, parent.name)),
    );
}

pub(super) fn render_edge_label(out: &mut String, edge: &EdgeSpec, geom: EdgeGeometry) {
    // Label position is spread along the curve for parallel edges so the
    // cardinality badges no longer stack on top of one another.
    let label_t = label_t_for_parallel(edge.parallel_index, edge.parallel_count);
    let (label_x, label_y) = bezier_at(
        geom.sx,
        geom.sy,
        geom.ex,
        geom.ey,
        geom.sdir,
        geom.edir,
        geom.curvature,
        label_t,
    );

    // Pill-shaped white background guarantees the label stays readable when
    // curves or other labels cross it.
    let char_count = edge.cardinality_label.chars().count() as f64;
    let pill_w = (char_count * 5.6 + 12.0).max(22.0);
    let pill_h = 15.0;
    let _ = writeln!(
        out,
        "    <rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" \
         rx=\"7\" ry=\"7\" fill=\"#ffffff\" stroke=\"{border}\" stroke-width=\"1\"/>",
        x = label_x - pill_w / 2.0,
        y = label_y - pill_h / 2.0,
        w = pill_w,
        h = pill_h,
        border = CARD_BORDER,
    );

    let _ = writeln!(
        out,
        "    <text class=\"edge-cardinality\" x=\"{x:.1}\" y=\"{y:.1}\" \
         fill=\"{fg}\" font-size=\"9\" font-weight=\"700\" text-anchor=\"middle\" \
         dominant-baseline=\"central\">{label}</text>",
        x = label_x,
        y = label_y,
        fg = EDGE_END,
        label = escape_xml(edge.cardinality_label),
    );
}

/// Sideways offset applied to a curve's control points so parallel edges fan
/// out instead of collapsing onto the same arc.
fn parallel_curvature_offset(index: u32, count: u32) -> f64 {
    if count <= 1 {
        return 0.0;
    }
    let center = (f64::from(count) - 1.0) / 2.0;
    (f64::from(index) - center) * 28.0
}

/// Parameter `t ∈ [0, 1]` along the curve where the cardinality label sits.
/// For single edges we keep the visual centre (`0.5`); for `N`-way bundles we
/// spread labels evenly between `0.30` and `0.70`.
fn label_t_for_parallel(index: u32, count: u32) -> f64 {
    if count <= 1 {
        return 0.5;
    }
    let span = 0.40;
    let start = 0.30;
    start + (f64::from(index) / f64::from(count - 1)) * span
}

fn pick_anchors(
    child: &TableBox,
    parent: &TableBox,
    child_y: f64,
    parent_y: f64,
) -> (f64, f64, f64, f64, Side, Side) {
    let child_left = child.x;
    let child_right = child.x + child.width;
    let parent_left = parent.x;
    let parent_right = parent.x + parent.width;

    // Prefer horizontal connections — they read cleaner for ERDs.
    let horizontal_separation = parent_left > child_right || child_left > parent_right;
    if horizontal_separation {
        if parent_left >= child_right {
            // Parent is to the right of the child.
            return (
                child_right,
                child_y,
                parent_left,
                parent_y,
                Side::Right,
                Side::Left,
            );
        }
        // Parent is to the left of the child.
        return (
            child_left,
            child_y,
            parent_right,
            parent_y,
            Side::Left,
            Side::Right,
        );
    }

    // Otherwise route top/bottom.
    if parent.y + parent.height <= child.y {
        let sx = child.x + child.width / 2.0;
        let ex = parent.x + parent.width / 2.0;
        return (
            sx,
            child.y,
            ex,
            parent.y + parent.height,
            Side::Top,
            Side::Bottom,
        );
    }
    let sx = child.x + child.width / 2.0;
    let ex = parent.x + parent.width / 2.0;
    (
        sx,
        child.y + child.height,
        ex,
        parent.y,
        Side::Bottom,
        Side::Top,
    )
}

fn bezier_path(
    sx: f64,
    sy: f64,
    ex: f64,
    ey: f64,
    s_side: Side,
    e_side: Side,
    lateral_offset: f64,
) -> String {
    let dx = (ex - sx).abs();
    let dy = (ey - sy).abs();
    let pull = dx.max(dy).max(40.0) * 0.5;

    let (cs_x, cs_y) = control_point(sx, sy, s_side, pull, lateral_offset);
    let (ce_x, ce_y) = control_point(ex, ey, e_side, pull, lateral_offset);

    format!(
        "M {sx:.1} {sy:.1} C {csx:.1} {csy:.1} {cex:.1} {cey:.1} {ex:.1} {ey:.1}",
        sx = sx,
        sy = sy,
        csx = cs_x,
        csy = cs_y,
        cex = ce_x,
        cey = ce_y,
        ex = ex,
        ey = ey
    )
}

/// Evaluate a cubic Bezier at parameter `t ∈ [0, 1]`.
/// Used to place cardinality labels at varying positions along an edge.
fn bezier_at(
    sx: f64,
    sy: f64,
    ex: f64,
    ey: f64,
    s_side: Side,
    e_side: Side,
    lateral_offset: f64,
    t: f64,
) -> (f64, f64) {
    let dx = (ex - sx).abs();
    let dy = (ey - sy).abs();
    let pull = dx.max(dy).max(40.0) * 0.5;
    let (cs_x, cs_y) = control_point(sx, sy, s_side, pull, lateral_offset);
    let (ce_x, ce_y) = control_point(ex, ey, e_side, pull, lateral_offset);

    let one_minus_t = 1.0 - t;
    let b0 = one_minus_t * one_minus_t * one_minus_t;
    let b1 = 3.0 * one_minus_t * one_minus_t * t;
    let b2 = 3.0 * one_minus_t * t * t;
    let b3 = t * t * t;
    (
        b0 * sx + b1 * cs_x + b2 * ce_x + b3 * ex,
        b0 * sy + b1 * cs_y + b2 * ce_y + b3 * ey,
    )
}

/// Compute a cubic-Bezier control point relative to an anchor side.
/// `lateral_offset` perpendicular to the pull direction lets parallel edges
/// fan out so multi-edge bundles don't collapse onto a single arc.
fn control_point(x: f64, y: f64, side: Side, pull: f64, lateral_offset: f64) -> (f64, f64) {
    match side {
        Side::Left => (x - pull, y + lateral_offset),
        Side::Right => (x + pull, y + lateral_offset),
        Side::Top => (x + lateral_offset, y - pull),
        Side::Bottom => (x + lateral_offset, y + pull),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn box_at(name: &str, x: f64, y: f64, width: f64, height: f64) -> TableBox {
        TableBox {
            name: name.into(),
            rows: Vec::new(),
            width,
            height,
            x,
            y,
            row_index: BTreeMap::new(),
            pk_row: None,
        }
    }

    #[test]
    fn label_t_parallel_single_and_spread_cases() {
        assert_eq!(label_t_for_parallel(0, 1), 0.5);
        assert!((label_t_for_parallel(0, 3) - 0.30).abs() < f64::EPSILON);
        assert!((label_t_for_parallel(2, 3) - 0.70).abs() < f64::EPSILON);
    }

    #[test]
    fn pick_anchors_covers_right_left_top_and_bottom_routes() {
        let child = box_at("child", 0.0, 100.0, 80.0, 40.0);
        let parent_right = box_at("parent", 120.0, 100.0, 80.0, 40.0);
        let (_, _, _, _, child_side, parent_side) =
            pick_anchors(&child, &parent_right, 120.0, 120.0);
        assert_eq!((child_side, parent_side), (Side::Right, Side::Left));

        let parent_left = box_at("parent", -120.0, 100.0, 80.0, 40.0);
        let (_, _, _, _, child_side, parent_side) =
            pick_anchors(&child, &parent_left, 120.0, 120.0);
        assert_eq!((child_side, parent_side), (Side::Left, Side::Right));

        let parent_above = box_at("parent", 0.0, 0.0, 80.0, 40.0);
        let (_, sy, _, ey, child_side, parent_side) =
            pick_anchors(&child, &parent_above, 120.0, 20.0);
        assert_eq!(
            (sy, ey, child_side, parent_side),
            (100.0, 40.0, Side::Top, Side::Bottom)
        );

        let parent_below = box_at("parent", 0.0, 220.0, 80.0, 40.0);
        let (_, sy, _, ey, child_side, parent_side) =
            pick_anchors(&child, &parent_below, 120.0, 240.0);
        assert_eq!(
            (sy, ey, child_side, parent_side),
            (140.0, 220.0, Side::Bottom, Side::Top)
        );
    }

    #[test]
    fn control_point_covers_all_sides() {
        assert_eq!(control_point(10.0, 20.0, Side::Left, 5.0, 2.0), (5.0, 22.0));
        assert_eq!(
            control_point(10.0, 20.0, Side::Right, 5.0, 2.0),
            (15.0, 22.0)
        );
        assert_eq!(control_point(10.0, 20.0, Side::Top, 5.0, 2.0), (12.0, 15.0));
        assert_eq!(
            control_point(10.0, 20.0, Side::Bottom, 5.0, 2.0),
            (12.0, 25.0)
        );
    }
}

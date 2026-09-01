//! Top-level SVG emission: document scaffold, defs, table cards, and rows.
//!
//! Edge paths and cardinality labels are delegated to [`super::edges`] so the
//! two passes (edges below tables, labels above) stay easy to reorder.

// Row indices and column counts are integer values converted to pixel
// coordinates; the long writeln! templates rely on named arguments for
// readability.
#![expect(
    clippy::cast_precision_loss,
    reason = "SVG layout converts bounded table/row counts into pixel coordinates"
)]
#![expect(
    clippy::uninlined_format_args,
    reason = "long SVG template strings keep repeated named arguments explicit for readability"
)]

use std::fmt::Write as _;

use super::edges::{EdgeGeometry, edge_geometry, render_edge_label, render_edge_path};
use super::model::{EdgeSpec, RowSpec, TableBox};
use super::style::{
    BADGE_FS, BADGE_GAP, BADGE_H, BADGE_W, BG, CARD_BG, CARD_BORDER, FK_BG, FK_FG, FONT_FAMILY,
    HEADER_FG, HEADER_FILL, HEADER_H, HEADER_SUB, MONO_FAMILY, NAME_FS, PK_BG, PK_FG, ROW_ALT_BG,
    ROW_DIVIDER, ROW_FG, ROW_FG_MUTED, ROW_H, TABLE_PAD_X, TABLE_RADIUS, TITLE_FS, TYPE_FS,
};
use super::util::escape_xml;

pub(super) fn render_doc(boxes: &[TableBox], edges: &[EdgeSpec], vw: f64, vh: f64) -> String {
    let mut out = String::with_capacity(4096);

    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w:.0} {h:.0}\" \
         width=\"{w:.0}\" height=\"{h:.0}\" font-family=\"{ff}\" \
         style=\"letter-spacing:-0.25px\">",
        w = vw,
        h = vh,
        ff = FONT_FAMILY,
    );

    render_defs(&mut out);

    let _ = writeln!(
        out,
        "  <rect x=\"0\" y=\"0\" width=\"{w:.0}\" height=\"{h:.0}\" fill=\"{bg}\"/>",
        w = vw,
        h = vh,
        bg = BG
    );

    // Pre-compute every edge's routing geometry once. Previously each edge
    // ran `edge_geometry` twice — once during the path pass and again during
    // the label pass — so for an ERD with N edges we were doing 2N anchor +
    // curvature computations. Caching the per-edge result before the two
    // passes halves that work without changing a single pixel of the output.
    let geometries: Vec<EdgeGeometry> = edges
        .iter()
        .map(|e| edge_geometry(&boxes[e.child_idx], &boxes[e.parent_idx], e))
        .collect();

    // Pass 1: draw every edge path. Doing all paths before any labels
    // guarantees label pills are never overdrawn by another edge in a
    // dense bundle (junction tables, self-references, etc.).
    out.push_str("  <g class=\"edges\" fill=\"none\">\n");
    for (edge, geom) in edges.iter().zip(&geometries) {
        render_edge_path(
            &mut out,
            &boxes[edge.child_idx],
            &boxes[edge.parent_idx],
            edge,
            *geom,
        );
    }
    out.push_str("  </g>\n");

    // Tables — rendered above edge paths but below labels so column rows are
    // legible and FK badges line up with their anchor points.
    out.push_str("  <g class=\"tables\">\n");
    for bx in boxes {
        render_table(&mut out, bx);
    }
    out.push_str("  </g>\n");

    // Pass 2: cardinality labels (pill + text). Always on top so they stay
    // readable regardless of how many curves cross their location.
    out.push_str("  <g class=\"edge-labels\">\n");
    for (edge, geom) in edges.iter().zip(&geometries) {
        render_edge_label(&mut out, edge, *geom);
    }
    out.push_str("  </g>\n");

    out.push_str("</svg>\n");
    out
}

fn render_defs(out: &mut String) {
    out.push_str("  <defs>\n");
    out.push_str(
        "    <linearGradient id=\"vespHeader\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\n\
             \x20     <stop offset=\"0\" stop-color=\"#5b34f7\"/>\n\
             \x20     <stop offset=\"1\" stop-color=\"#7e5cff\"/>\n\
             \x20   </linearGradient>\n",
    );
    out.push_str(
        "    <filter id=\"vespShadow\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\">\n\
             \x20     <feDropShadow dx=\"0\" dy=\"2\" stdDeviation=\"3\" \
             flood-color=\"#5b34f7\" flood-opacity=\"0.10\"/>\n\
             \x20   </filter>\n",
    );
    out.push_str(
        "    <marker id=\"vespArrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" \
         markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\n\
             \x20     <path d=\"M0 0 L10 5 L0 10 z\" fill=\"#5b34f7\"/>\n\
             \x20   </marker>\n",
    );
    out.push_str(
        "    <marker id=\"vespCircle\" viewBox=\"0 0 10 10\" refX=\"5\" refY=\"5\" \
         markerWidth=\"6\" markerHeight=\"6\" orient=\"auto\">\n\
             \x20     <circle cx=\"5\" cy=\"5\" r=\"3\" fill=\"#ffffff\" \
             stroke=\"#5b34f7\" stroke-width=\"1.6\"/>\n\
             \x20   </marker>\n",
    );
    out.push_str("  </defs>\n");
}

fn render_table(out: &mut String, bx: &TableBox) {
    let _ = writeln!(
        out,
        "    <g class=\"table\" transform=\"translate({x:.1} {y:.1})\">",
        x = bx.x,
        y = bx.y
    );

    // Card background with shadow.
    let _ = writeln!(
        out,
        "      <rect class=\"card\" x=\"0\" y=\"0\" width=\"{w:.0}\" height=\"{h:.0}\" \
         rx=\"{r}\" ry=\"{r}\" fill=\"{cbg}\" stroke=\"{cb}\" stroke-width=\"1\" \
         filter=\"url(#vespShadow)\"/>",
        w = bx.width,
        h = bx.height,
        r = TABLE_RADIUS,
        cbg = CARD_BG,
        cb = CARD_BORDER,
    );

    // Header band — use a path so only the top corners are rounded.
    let header_path = rounded_top_path(bx.width, HEADER_H, TABLE_RADIUS);
    let _ = writeln!(
        out,
        "      <path d=\"{path}\" fill=\"{fill}\"/>",
        path = header_path,
        fill = HEADER_FILL
    );

    // Title.
    let _ = writeln!(
        out,
        "      <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"600\" letter-spacing=\"0.2\">{name}</text>",
        tx = TABLE_PAD_X,
        ty = HEADER_H / 2.0 + TITLE_FS / 2.0 - 2.0,
        fg = HEADER_FG,
        fs = TITLE_FS,
        name = escape_xml(&bx.name),
    );

    // Column count hint, right-aligned in header.
    let count_str = format!("{} cols", bx.rows.len());
    let _ = writeln!(
        out,
        "      <text x=\"{cx:.1}\" y=\"{cy:.1}\" fill=\"{sub}\" font-size=\"10\" \
         font-weight=\"500\" text-anchor=\"end\">{count}</text>",
        cx = bx.width - TABLE_PAD_X,
        cy = HEADER_H / 2.0 + 4.0,
        sub = HEADER_SUB,
        count = escape_xml(&count_str),
    );

    // Rows.
    for (idx, row) in bx.rows.iter().enumerate() {
        render_row(out, bx, idx, row);
    }

    out.push_str("    </g>\n");
}

fn render_row(out: &mut String, bx: &TableBox, idx: usize, row: &RowSpec) {
    let y = HEADER_H + idx as f64 * ROW_H;
    let is_last = idx == bx.rows.len() - 1;

    // Alt background for zebra striping. Skip the very last row's stripe to keep
    // the rounded bottom corners clean (the card border handles the visual).
    if idx % 2 == 1 {
        if is_last {
            let path = rounded_bottom_path(bx.width, y, ROW_H, TABLE_RADIUS);
            let _ = writeln!(out, "      <path d=\"{path}\" fill=\"{ROW_ALT_BG}\"/>");
        } else {
            let _ = writeln!(
                out,
                "      <rect x=\"0\" y=\"{y:.1}\" width=\"{w:.0}\" height=\"{h:.1}\" \
                 fill=\"{bg}\"/>",
                y = y,
                w = bx.width,
                h = ROW_H,
                bg = ROW_ALT_BG,
            );
        }
    }

    // Top divider (skip on the first row — header bottom acts as divider).
    if idx > 0 {
        let _ = writeln!(
            out,
            "      <line x1=\"{x1:.0}\" y1=\"{y:.1}\" x2=\"{x2:.0}\" y2=\"{y:.1}\" \
             stroke=\"{c}\" stroke-width=\"1\"/>",
            x1 = 1.0,
            x2 = bx.width - 1.0,
            y = y,
            c = ROW_DIVIDER,
        );
    }

    // Badges.
    let mut badge_x = TABLE_PAD_X;
    if row.is_pk {
        render_badge(
            out,
            badge_x,
            y + (ROW_H - BADGE_H) / 2.0,
            "PK",
            PK_BG,
            PK_FG,
        );
        badge_x += BADGE_W + 4.0;
    }
    if row.is_fk {
        render_badge(
            out,
            badge_x,
            y + (ROW_H - BADGE_H) / 2.0,
            "FK",
            FK_BG,
            FK_FG,
        );
        badge_x += BADGE_W + 4.0;
    }

    let name_x = if row.is_pk || row.is_fk {
        badge_x + BADGE_GAP - 4.0
    } else {
        TABLE_PAD_X
    };

    // Column name.
    let name_weight = if row.is_pk { "600" } else { "500" };
    let _ = writeln!(
        out,
        "      <text x=\"{nx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"{w}\">{name}</text>",
        nx = name_x,
        ty = y + ROW_H / 2.0 + NAME_FS / 2.0 - 2.0,
        fg = ROW_FG,
        fs = NAME_FS,
        w = name_weight,
        name = escape_xml(&row.name),
    );

    // Type, right-aligned in monospace. The nullable branch needs an owned
    // `String` (it appends `?`, then escapes it); the non-nullable branch
    // escapes the borrowed `type_str` directly via `Cow`, so no clone is paid
    // on the common path.
    let type_display: std::borrow::Cow<'_, str> = if row.nullable {
        std::borrow::Cow::Owned(escape_xml(&format!("{}?", row.type_str)).into_owned())
    } else {
        escape_xml(&row.type_str)
    };
    let _ = writeln!(
        out,
        "      <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-family=\"{ff}\" font-style=\"italic\" text-anchor=\"end\">{t}</text>",
        tx = bx.width - TABLE_PAD_X,
        ty = y + ROW_H / 2.0 + TYPE_FS / 2.0 - 2.0,
        fg = ROW_FG_MUTED,
        fs = TYPE_FS,
        ff = MONO_FAMILY,
        t = type_display,
    );
}

fn render_badge(out: &mut String, x: f64, y: f64, label: &str, bg: &str, fg: &str) {
    let _ = writeln!(
        out,
        "      <g class=\"badge\"><rect x=\"{x:.1}\" y=\"{y:.1}\" \
         width=\"{w}\" height=\"{h}\" rx=\"3\" ry=\"3\" fill=\"{bg}\"/>\
         <text x=\"{tx:.1}\" y=\"{ty:.1}\" fill=\"{fg}\" font-size=\"{fs}\" \
         font-weight=\"700\" text-anchor=\"middle\" letter-spacing=\"0.4\">{label}</text></g>",
        x = x,
        y = y,
        w = BADGE_W,
        h = BADGE_H,
        bg = bg,
        tx = x + BADGE_W / 2.0,
        ty = y + BADGE_H / 2.0 + BADGE_FS / 2.0 - 1.5,
        fg = fg,
        fs = BADGE_FS,
        label = label,
    );
}

fn rounded_top_path(w: f64, h: f64, r: f64) -> String {
    format!(
        "M 0 {h:.1} L 0 {r:.1} Q 0 0 {r:.1} 0 L {wr:.1} 0 Q {w:.1} 0 {w:.1} {r:.1} \
         L {w:.1} {h:.1} Z",
        w = w,
        h = h,
        r = r,
        wr = w - r,
    )
}

fn rounded_bottom_path(w: f64, top_y: f64, h: f64, r: f64) -> String {
    let bot = top_y + h;
    format!(
        "M 0 {top:.1} L {w:.1} {top:.1} L {w:.1} {br:.1} Q {w:.1} {bot:.1} {wr:.1} {bot:.1} \
         L {r:.1} {bot:.1} Q 0 {bot:.1} 0 {br:.1} Z",
        top = top_y,
        w = w,
        bot = bot,
        br = bot - r,
        wr = w - r,
        r = r,
    )
}

//! SVG / junction mutation-coverage tests, split out of `tests/mod.rs` to
//! keep that file under the 1200-line budget. `use super::*;` reaches the
//! shared fixtures (`table`, `primary_key`, `integer`, `foreign_key`,
//! `normalize`, `unique_foreign_key`, `nullable_foreign_key`, `render_svg`,
//! `is_junction_table`, `ForeignKeySyntax`, …) defined in `tests/mod.rs`.
use super::*;

// 3-PK / 3-FK junction: kills `len() < 2 -> len() > 2` mutants on
// mod.rs:384 (primary_key_columns.len() < 2) and mod.rs:389
// (foreign_key_groups.len() < 2). NOT normalized → inline FKs stay inline.
#[test]
fn is_junction_table_three_way_junction_returns_true() {
    let junction = table(
        "user_tag_role",
        vec![
            primary_key("user_id", integer())
                .foreign_key(ForeignKeySyntax::String("user.id".into())),
            primary_key("tag_id", integer()).foreign_key(ForeignKeySyntax::String("tag.id".into())),
            primary_key("role_id", integer())
                .foreign_key(ForeignKeySyntax::String("role.id".into())),
        ],
    );
    assert!(
        is_junction_table(&junction),
        "3-PK/3-FK is a junction (kills len()<2 -> >2 mutants on erd/mod.rs:384,389)"
    );
}

// The two length guards above short-circuit before `is_junction_table`'s final
// `primary_key_columns.iter().all(..)` check, and every other junction fixture
// makes that check succeed. This table clears both guards (2 PK columns, 2 FK
// groups) but one PK column (`seq`) is NOT a foreign key, so `all(..)` returns
// false — the only path that reaches the end of the function and still rejects.
#[test]
fn is_junction_table_pk_column_outside_fk_set_returns_false() {
    let not_a_junction = table(
        "audit_entry",
        vec![
            primary_key("user_id", integer())
                .foreign_key(ForeignKeySyntax::String("user.id".into())),
            primary_key("seq", integer()),
            foreign_key("tag_id", "tag.id"),
        ],
    );
    assert!(
        !is_junction_table(&not_a_junction),
        "a PK column that is not part of any FK group disqualifies the table"
    );
}

// `detect_cardinality` classifies through `is_junction_table` first. Driving the
// false case above through the public relation collector proves the junction
// guard is evaluated (and rejected) rather than short-circuited away.
#[test]
fn detect_cardinality_skips_many_to_many_when_pk_is_not_all_fk() {
    let user = normalize(&table("user", vec![primary_key("id", integer())]));
    let tag = normalize(&table("tag", vec![primary_key("id", integer())]));
    let audit = normalize(&table(
        "audit_entry",
        vec![
            primary_key("user_id", integer())
                .foreign_key(ForeignKeySyntax::String("user.id".into())),
            primary_key("seq", integer()),
            foreign_key("tag_id", "tag.id"),
        ],
    ));

    let relations = collect_foreign_key_relations(&[user, tag, audit]);
    assert!(
        relations
            .iter()
            .filter(|r| r.child_table == "audit_entry")
            .all(|r| r.cardinality != Cardinality::ManyToMany),
        "non-junction table must not classify as M:N, got: {relations:?}"
    );
}

// Deterministic SVG fixture — pins bezier path coords (1 decimal) and
// cardinality labels to kill ~400 coord-arithmetic mutants in svg/edges.rs.
// Fixture includes: parallel edges (2 FKs child→parent), curved edges,
// every cardinality (1:1, 1:N, 0..1:N, M:N).
fn deterministic_svg_fixture() -> Vec<TableDef> {
    vec![
        normalize(&table("user", vec![primary_key("id", integer())])),
        normalize(&table("tag", vec![primary_key("id", integer())])),
        normalize(&table(
            "profile",
            vec![
                primary_key("id", integer()),
                unique_foreign_key("user_id", "user.id"),
            ],
        )),
        normalize(&table(
            "photo",
            vec![
                primary_key("id", integer()),
                nullable_foreign_key("owner_id", "user.id"),
            ],
        )),
        // Parallel edges: two FK columns from `audit` to the same `user`.
        normalize(&table(
            "audit",
            vec![
                primary_key("id", integer()),
                foreign_key("author_id", "user.id"),
                foreign_key("reviewer_id", "user.id"),
            ],
        )),
        // Junction: M:N
        normalize(&table(
            "user_tag",
            vec![
                primary_key("user_id", integer())
                    .foreign_key(ForeignKeySyntax::String("user.id".into())),
                primary_key("tag_id", integer())
                    .foreign_key(ForeignKeySyntax::String("tag.id".into())),
            ],
        )),
    ]
}

#[test]
fn render_svg_full_deterministic_fixture_snapshot() {
    let svg = render_svg(&deterministic_svg_fixture()).unwrap();
    insta::assert_snapshot!(svg);
}

const SVG_HEADER_H: f64 = 34.0;
const SVG_ROW_H: f64 = 24.0;
const SVG_COORD_EPSILON: f64 = 0.11;

#[derive(Debug, Copy, Clone)]
struct RenderedTableBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Copy, Clone)]
struct RenderedPath {
    start_x: f64,
    start_y: f64,
    control_start_x: f64,
    control_start_y: f64,
    control_end_x: f64,
    control_end_y: f64,
    end_x: f64,
    end_y: f64,
}

#[derive(Debug, Copy, Clone)]
struct RenderedLabel {
    x: f64,
    y: f64,
}

fn extra_integer_columns(prefix: &str, count: usize) -> Vec<ColumnDef> {
    (0..count)
        .map(|index| column(&format!("{prefix}_{index}"), integer()))
        .collect()
}

fn bottom_route_cycle_schema() -> Vec<TableDef> {
    let mut cycle_c_columns = vec![
        primary_key("id", integer()),
        foreign_key("a_id", "cycle_a.id"),
    ];
    cycle_c_columns.extend(extra_integer_columns("payload", 8));

    vec![
        normalize(&table(
            "cycle_a",
            vec![
                primary_key("id", integer()),
                foreign_key("b_id", "cycle_b.id"),
            ],
        )),
        normalize(&table(
            "cycle_b",
            vec![
                primary_key("id", integer()),
                foreign_key("c_id", "cycle_c.id"),
            ],
        )),
        normalize(&table("cycle_c", cycle_c_columns)),
    ]
}

fn top_route_cycle_schema() -> Vec<TableDef> {
    // Input order A, C, B makes `cycle_b` and `cycle_c` land in the same rank;
    // name sorting then places B above C, so C→B must route through Top/Bottom.
    vec![
        normalize(&table(
            "cycle_a",
            vec![
                primary_key("id", integer()),
                foreign_key("c_id", "cycle_c.id"),
            ],
        )),
        normalize(&table(
            "cycle_c",
            vec![
                primary_key("id", integer()),
                foreign_key("b_id", "cycle_b.id"),
            ],
        )),
        normalize(&table(
            "cycle_b",
            vec![
                primary_key("id", integer()),
                foreign_key("a_id", "cycle_a.id"),
            ],
        )),
    ]
}

fn non_pk_parent_row_schema() -> Vec<TableDef> {
    vec![
        normalize(&table(
            "lookup_parent",
            vec![primary_key("id", integer()), column("code", integer())],
        )),
        normalize(&table(
            "lookup_child",
            vec![
                primary_key("id", integer()),
                foreign_key("lookup_code", "lookup_parent.code"),
            ],
        )),
    ]
}

fn tall_parallel_offset_schema() -> Vec<TableDef> {
    let mut schema = vec![normalize(&table(
        "user",
        vec![primary_key("id", integer())],
    ))];
    for table_name in ["offset_a", "offset_b"] {
        schema.push(normalize(&table(
            table_name,
            vec![
                primary_key("id", integer()),
                foreign_key("user_id", "user.id"),
            ],
        )));
    }
    schema.push(normalize(&table(
        "offset_z",
        vec![
            primary_key("id", integer()),
            foreign_key("first_user_id", "user.id"),
            foreign_key("second_user_id", "user.id"),
        ],
    )));
    schema
}

fn quoted_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn attr_f64(line: &str, attr: &str) -> f64 {
    quoted_attr(line, attr)
        .unwrap_or_else(|| panic!("missing {attr}=\"...\" in SVG line: {line}"))
        .parse::<f64>()
        .unwrap_or_else(|error| panic!("invalid numeric {attr} in SVG line {line}: {error}"))
}

fn parse_translate(line: &str) -> (f64, f64) {
    let transform = quoted_attr(line, "transform")
        .unwrap_or_else(|| panic!("missing transform=\"...\" in SVG table line: {line}"));
    let coords = transform
        .strip_prefix("translate(")
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or_else(|| panic!("unexpected SVG translate format: {transform}"));
    let mut parts = coords.split_whitespace();
    let x = parts
        .next()
        .unwrap_or_else(|| panic!("missing translate x in {transform}"))
        .parse::<f64>()
        .unwrap_or_else(|error| panic!("invalid translate x in {transform}: {error}"));
    let y = parts
        .next()
        .unwrap_or_else(|| panic!("missing translate y in {transform}"))
        .parse::<f64>()
        .unwrap_or_else(|error| panic!("invalid translate y in {transform}: {error}"));
    (x, y)
}

fn rendered_table_box(svg: &str, name: &str) -> RenderedTableBox {
    let header_text = format!(">{name}</text>");
    let mut current_transform = None;
    let mut current_rect = None;

    for line in svg.lines() {
        if line.contains("<g class=\"table\"") {
            current_transform = Some(line);
            current_rect = None;
            continue;
        }
        if current_transform.is_some() && line.contains("<rect class=\"card\"") {
            current_rect = Some(line);
            continue;
        }
        if line.contains("font-size=\"14\"") && line.contains(&header_text) {
            let transform_line = current_transform
                .unwrap_or_else(|| panic!("missing table transform before header {name}"));
            let rect_line = current_rect
                .unwrap_or_else(|| panic!("missing table card rect before header {name}"));
            let (x, y) = parse_translate(transform_line);
            return RenderedTableBox {
                x,
                y,
                width: attr_f64(rect_line, "width"),
                height: attr_f64(rect_line, "height"),
            };
        }
    }

    panic!("missing rendered table named {name}\nSVG:\n{svg}");
}

fn rendered_path_for_title(svg: &str, title: &str) -> RenderedPath {
    let title_markup = format!("<title>{title}</title>");
    let line = svg
        .lines()
        .find(|line| line.contains("marker-end=") && line.contains(&title_markup))
        .unwrap_or_else(|| panic!("missing edge path titled {title}\nSVG:\n{svg}"));
    let d = quoted_attr(line, "d")
        .unwrap_or_else(|| panic!("missing d=\"...\" for edge titled {title}: {line}"));
    parse_rendered_path(d)
}

fn parse_rendered_path(d: &str) -> RenderedPath {
    let numeric = d.replace(['M', 'C'], "");
    let numbers: Vec<f64> = numeric
        .split_whitespace()
        .map(|part| {
            part.parse::<f64>()
                .unwrap_or_else(|error| panic!("invalid path coordinate {part} in {d}: {error}"))
        })
        .collect();
    let [
        start_x,
        start_y,
        control_start_x,
        control_start_y,
        control_end_x,
        control_end_y,
        end_x,
        end_y,
    ] = numbers.as_slice()
    else {
        panic!("expected 8 coordinates in SVG path d=\"{d}\", got {numbers:?}");
    };
    RenderedPath {
        start_x: *start_x,
        start_y: *start_y,
        control_start_x: *control_start_x,
        control_start_y: *control_start_y,
        control_end_x: *control_end_x,
        control_end_y: *control_end_y,
        end_x: *end_x,
        end_y: *end_y,
    }
}

fn edge_index_for_title(svg: &str, title: &str) -> usize {
    let title_markup = format!("<title>{title}</title>");
    for (edge_index, line) in svg
        .lines()
        .filter(|line| line.contains("marker-end="))
        .enumerate()
    {
        if line.contains(&title_markup) {
            return edge_index;
        }
    }
    panic!("missing edge titled {title}\nSVG:\n{svg}");
}

fn rendered_label_at(svg: &str, target_index: usize) -> RenderedLabel {
    for (label_index, line) in svg
        .lines()
        .filter(|line| line.contains("class=\"edge-cardinality\""))
        .enumerate()
    {
        if label_index == target_index {
            return RenderedLabel {
                x: attr_f64(line, "x"),
                y: attr_f64(line, "y"),
            };
        }
    }
    panic!("missing edge label index {target_index}\nSVG:\n{svg}");
}

fn cubic_point(path: RenderedPath, t: f64) -> (f64, f64) {
    let one_minus_t = 1.0 - t;
    let b0 = one_minus_t * one_minus_t * one_minus_t;
    let b1 = 3.0 * one_minus_t * one_minus_t * t;
    let b2 = 3.0 * one_minus_t * t * t;
    let b3 = t * t * t;
    (
        b0 * path.start_x + b1 * path.control_start_x + b2 * path.control_end_x + b3 * path.end_x,
        b0 * path.start_y + b1 * path.control_start_y + b2 * path.control_end_y + b3 * path.end_y,
    )
}

fn assert_svg_coord(context: &str, actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= SVG_COORD_EPSILON,
        "{context}: expected {expected:.1}, got {actual:.1}"
    );
}

fn assert_label_lies_on_path_at_t(svg: &str, title: &str, t: f64) {
    let path = rendered_path_for_title(svg, title);
    let label = rendered_label_at(svg, edge_index_for_title(svg, title));
    let (expected_x, expected_y) = cubic_point(path, t);
    assert_svg_coord(&format!("{title} label x"), label.x, expected_x);
    assert_svg_coord(&format!("{title} label y"), label.y, expected_y);
}

fn row_midpoint_y(table_box: RenderedTableBox, row_index: usize) -> f64 {
    let row_index = u32::try_from(row_index).expect("SVG test row index fits in u32");
    table_box.y + SVG_HEADER_H + f64::from(row_index) * SVG_ROW_H + SVG_ROW_H / 2.0
}

#[test]
fn parent_right_cycle_edge_uses_child_right_anchor() {
    let svg = render_svg(&top_route_cycle_schema()).unwrap();
    let child = rendered_table_box(&svg, "cycle_a");
    let parent = rendered_table_box(&svg, "cycle_c");
    let path = rendered_path_for_title(&svg, "cycle_a c_id → id → cycle_c");

    assert!(
        parent.x > child.x + child.width,
        "fixture must place cycle_c to the right of cycle_a\nSVG:\n{svg}"
    );
    assert_svg_coord(
        "parent-right edge starts at child right",
        path.start_x,
        child.x + child.width,
    );
    assert_svg_coord(
        "parent-right edge ends at parent left",
        path.end_x,
        parent.x,
    );
}

#[test]
fn bottom_cycle_edge_uses_bottom_to_top_midpoint_anchors() {
    let svg = render_svg(&bottom_route_cycle_schema()).unwrap();
    let child = rendered_table_box(&svg, "cycle_b");
    let parent = rendered_table_box(&svg, "cycle_c");
    let path = rendered_path_for_title(&svg, "cycle_b c_id → id → cycle_c");

    assert!(
        parent.y > child.y + child.height,
        "fixture must place cycle_c below cycle_b\nSVG:\n{svg}"
    );
    assert!(
        parent.y - parent.height <= child.y,
        "fixture must make `parent.y - parent.height <= child.y` true so +→- flips the route\nSVG:\n{svg}"
    );
    assert_svg_coord(
        "bottom route starts at child midpoint x",
        path.start_x,
        child.x + child.width / 2.0,
    );
    assert_svg_coord(
        "bottom route starts at child bottom",
        path.start_y,
        child.y + child.height,
    );
    assert_svg_coord(
        "bottom route ends at parent midpoint x",
        path.end_x,
        parent.x + parent.width / 2.0,
    );
    assert_svg_coord("bottom route ends at parent top", path.end_y, parent.y);
}

#[test]
fn top_cycle_edge_uses_top_to_bottom_midpoint_anchors() {
    let svg = render_svg(&top_route_cycle_schema()).unwrap();
    let child = rendered_table_box(&svg, "cycle_c");
    let parent = rendered_table_box(&svg, "cycle_b");
    let path = rendered_path_for_title(&svg, "cycle_c b_id → id → cycle_b");

    assert!(
        parent.y + parent.height <= child.y,
        "fixture must place cycle_b above cycle_c\nSVG:\n{svg}"
    );
    assert_svg_coord(
        "top route starts at child midpoint x",
        path.start_x,
        child.x + child.width / 2.0,
    );
    assert_svg_coord("top route starts at child top", path.start_y, child.y);
    assert_svg_coord(
        "top route ends at parent midpoint x",
        path.end_x,
        parent.x + parent.width / 2.0,
    );
    assert_svg_coord(
        "top route ends at parent bottom",
        path.end_y,
        parent.y + parent.height,
    );
}

#[test]
fn non_pk_parent_row_anchors_to_referenced_column_midpoint() {
    let svg = render_svg(&non_pk_parent_row_schema()).unwrap();
    let parent = rendered_table_box(&svg, "lookup_parent");
    let path = rendered_path_for_title(&svg, "lookup_child lookup_code → code → lookup_parent");

    assert_svg_coord(
        "edge endpoint uses referenced parent column row midpoint",
        path.end_y,
        row_midpoint_y(parent, 1),
    );
}

#[test]
fn parallel_tall_offset_labels_lie_on_their_rendered_beziers() {
    let svg = render_svg(&tall_parallel_offset_schema()).unwrap();
    let first_path = rendered_path_for_title(&svg, "offset_z first_user_id → id → user");
    let second_path = rendered_path_for_title(&svg, "offset_z second_user_id → id → user");

    assert!(
        (first_path.start_y - first_path.end_y).abs()
            > (first_path.start_x - first_path.end_x).abs(),
        "fixture must make dy dominate dx for the first parallel edge\nSVG:\n{svg}"
    );
    assert!(
        (second_path.start_y - second_path.end_y).abs()
            > (second_path.start_x - second_path.end_x).abs(),
        "fixture must make dy dominate dx for the second parallel edge\nSVG:\n{svg}"
    );

    assert_label_lies_on_path_at_t(&svg, "offset_z first_user_id → id → user", 0.30);
    assert_label_lies_on_path_at_t(&svg, "offset_z second_user_id → id → user", 0.70);
}

// Two parallel FK edges (same child→parent pair) must produce DISTINCT
// bezier path d="..." strings. Kills `parallel_curvature_offset → constant`
// mutants without brittle floating-point comparisons.
#[test]
fn parallel_edges_distinct_bezier_paths() {
    let schema = vec![
        normalize(&table("user", vec![primary_key("id", integer())])),
        normalize(&table(
            "audit",
            vec![
                primary_key("id", integer()),
                foreign_key("author_id", "user.id"),
                foreign_key("reviewer_id", "user.id"),
            ],
        )),
    ];
    let svg = render_svg(&schema).unwrap();

    // Extract every d="..." value belonging to a colored edge path. Each
    // edge emits two <path> elements (a white halo + the colored line); the
    // colored one carries marker-end, the halo does not. Filtering on
    // marker-end isolates one path per edge so duplicate-by-shadow doesn't
    // skew the count. Card outlines and header tabs never carry marker-end.
    let edge_paths: Vec<String> = svg
        .lines()
        .filter(|line| line.contains("<path") && line.contains("marker-end="))
        .filter_map(|line| {
            let start = line.find("d=\"")? + 3;
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect();

    assert!(
        edge_paths.len() >= 2,
        "expected at least 2 edge path d=\"...\" values, got {}: {edge_paths:?}\nSVG:\n{svg}",
        edge_paths.len()
    );

    // Every parallel-edge path must be unique. If parallel_curvature_offset
    // was mutated to a constant, both paths would collapse to the same d="".
    let mut unique = edge_paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        edge_paths.len(),
        "parallel edges produced duplicate bezier paths (parallel_curvature_offset → constant?):\n{edge_paths:?}"
    );
}

// `parse_reference` rejects malformed `table.column` strings. The guard is
// `parts.next().is_some() || table.is_empty() || column.is_empty()`. A `||`→`&&`
// mutation would only reject when ALL three hold, so `"a.b.c"` / `"user."` /
// `".id"` would wrongly parse. Direct assertions on each kill `||`→`&&`.
#[test]
fn parse_reference_rejects_malformed_strings() {
    assert_eq!(
        parse_reference("user.id"),
        Some(("user".to_string(), vec!["id".to_string()]))
    );
    assert_eq!(parse_reference("a.b.c"), None, "3+ parts must be rejected");
    assert_eq!(
        parse_reference("user."),
        None,
        "empty column must be rejected"
    );
    assert_eq!(parse_reference(".id"), None, "empty table must be rejected");
    assert_eq!(
        parse_reference("noseparator"),
        None,
        "missing '.' must be rejected"
    );
}

// `filter_tables` is the eprintln wrapper over `filter_tables_with_warnings`.
// A `-> vec![]` mutation drops every table; this passthrough assertion (no
// include/exclude → identity) catches it.
#[test]
fn filter_tables_passthrough_returns_all_tables() {
    let tables = filter_schema();
    let n = tables.len();
    let filtered = filter_tables(tables, &[], &[], 0);
    assert_eq!(
        filtered.len(),
        n,
        "no filters → all tables pass through (kills -> vec![])"
    );
}

// `normalize_tables` maps each table through `.normalize()`. A `-> Ok(vec![])`
// mutation returns an empty Vec; asserting the normalized output preserves
// table count AND converts an inline FK to a table-level constraint kills it.
#[test]
fn normalize_tables_preserves_tables_and_normalizes() {
    let raw = vec![
        table("user", vec![primary_key("id", integer())]),
        table(
            "post",
            vec![
                primary_key("id", integer()),
                foreign_key("user_id", "user.id"),
            ],
        ),
    ];
    let normalized = normalize_tables(raw).expect("normalize must succeed");
    assert_eq!(
        normalized.len(),
        2,
        "all tables preserved (kills -> Ok(vec![]))"
    );
    // The inline FK on `post.user_id` must become a table-level ForeignKey.
    let post = normalized
        .iter()
        .find(|t| t.name == "post")
        .expect("post table");
    assert!(
        post.constraints
            .iter()
            .any(|c| matches!(c, vespertide_core::TableConstraint::ForeignKey { .. })),
        "inline FK must normalize to table-level constraint: {:?}",
        post.constraints
    );
}

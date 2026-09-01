//! Shared tree-sitter traversal helpers used across every LSP feature.
//!
//! Previously every feature module (hover, definition, references, rename,
//! file_features, check_expr_locate, …) carried its own byte-identical
//! private copy of [`node_at_byte`]. Hoisting the BFS descent into one
//! `pub(crate)` helper kills ~80 lines of pure duplication and prevents
//! the next LSP feature from copy-pasting an eighth variant.
//!
//! NOTE: `completion::context` keeps its own `node_at_byte` because that
//! call site uses [`tree_sitter::Node::descendant_for_byte_range`] for
//! end-of-token cursor semantics, which differs from this BFS descent at
//! byte boundaries. Do not migrate it here.

/// Descend from the root to the deepest node whose byte range contains
/// `byte_offset`.
///
/// Returns `Some(_)` for every well-formed tree — the loop always
/// terminates at a leaf when no child contains the offset — so the
/// `Option` wrapper is preserved purely so existing call sites keep
/// their `?` short-circuits unchanged.
pub(crate) fn node_at_byte(
    tree: &tree_sitter::Tree,
    byte_offset: usize,
) -> Option<tree_sitter::Node<'_>> {
    let root = tree.root_node();
    let mut current = root;
    'outer: loop {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.byte_range().contains(&byte_offset) {
                current = child;
                continue 'outer;
            }
        }
        return Some(current);
    }
}

/// Is `node` a YAML/JSON key-value pair? `pair` is the JSON grammar's name,
/// `block_mapping_pair` is the YAML grammar's. Hoisted here so every LSP
/// feature module can call one canonical helper instead of carrying its own
/// byte-identical copy (was duplicated in 6 sibling modules pre-0.2.0).
pub(crate) fn is_pair(node: tree_sitter::Node<'_>) -> bool {
    matches!(node.kind(), "pair" | "block_mapping_pair")
}

/// Walk the tree depth-first and return the first node whose kind is a
/// top-level mapping (`object`/`block_mapping`/`flow_mapping`). Every LSP
/// feature that wants the document's outermost table object needs this
/// primitive — previously duplicated in 4 sibling modules pre-0.2.0.
pub(crate) fn find_outer_mapping(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if matches!(node.kind(), "object" | "block_mapping" | "flow_mapping") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_outer_mapping(child) {
            return Some(found);
        }
    }
    None
}

/// Peel tree-sitter-yaml's `flow_node` / `block_node` wrappers so downstream
/// `match`es see the real kind (no-op on JSON, whose grammar has no such
/// wrappers). Fused while-let so the empty-wrapper case (no usable
/// `named_child(0)`) and the kind-mismatch case share the same loop exit —
/// no defensive `return` line that depends on a tree-sitter-yaml release
/// producing empty wrappers. Hoisted here so every LSP feature module can
/// call one canonical helper instead of carrying its own byte-identical
/// copy (was duplicated in 4 sibling modules pre-0.2.0, including one
/// `unwrap_flow_node` rename in `completion::context`).
pub(crate) fn unwrap_yaml_node(node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node")
        && let Some(inner) = current
            .named_child(0)
            .filter(|inner| inner.id() != current.id())
    {
        current = inner;
    }
    current
}

/// Strip one byte from each side of `range` if it spans at least 2 bytes.
/// Used to peel quote delimiters off `double_quote_scalar` /
/// `single_quote_scalar` byte ranges (and JSON `string` ranges when the
/// `string_content` named child is absent — the empty-string case).
///
/// Centralises the four byte-identical private copies that lived in
/// `file_features`, `rename`, `symbols`, and `references::search`
/// pre-this-hoist (the last took `Range` by value; call sites now pass a
/// reference).
pub(crate) fn trim_one_byte_each_side(range: &std::ops::Range<usize>) -> std::ops::Range<usize> {
    if range.end.saturating_sub(range.start) >= 2 {
        (range.start + 1)..(range.end - 1)
    } else {
        range.clone()
    }
}

/// Walk the PARENT chain skipping tree-sitter-yaml's `flow_node` /
/// `block_node` wrapper kinds. Counterpart to [`unwrap_yaml_node`], which
/// descends INTO a wrapper; this helper escapes UP through them so the
/// caller can reach the real ancestor (e.g. the surrounding mapping pair
/// or constraint object). Returns `None` when the parent chain runs out
/// before a non-wrapper node is reached.
///
/// Centralises the two byte-equivalent private copies that lived in
/// `definition::foreign_key` and `references::resolver` pre-0.2.0.
pub(crate) fn skip_yaml_wrappers(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node;
    while matches!(current.kind(), "flow_node" | "block_node") {
        current = current.parent()?;
    }
    Some(current)
}

/// Walk from `node` up the parent chain returning the nearest tree-sitter
/// node whose kind is a JSON / YAML string scalar (`string` /
/// `double_quote_scalar` / `single_quote_scalar` / `string_scalar` /
/// `plain_scalar`). When the cursor sits on `string_content` (the inner
/// span without quotes), climb one level so the returned node covers the
/// surrounding quotes. Stops at the first structural-container boundary
/// (`array` / `object` / `pair` / `block_mapping_pair` / `block_mapping` /
/// `block_sequence` / `flow_mapping` / `flow_sequence`) so a nested
/// object value never counts as "in string".
///
/// Centralises the two byte-equivalent private copies that lived in
/// `definition::foreign_key` and `references::resolver` pre-0.2.0.
///
/// NOTE: `completion::context::enclosing_string_range` deliberately stays
/// SEPARATE — it returns a `Range<usize>` (not a `Node`) and uses an
/// `unwrap_or(candidate)` fallback on the `string_content` arm, so its
/// behaviour differs at end-of-token boundaries. Do not migrate it here.
pub(crate) fn enclosing_string(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        match candidate.kind() {
            "string"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "string_scalar"
            | "plain_scalar" => return Some(candidate),
            "string_content" => return candidate.parent(),
            "array" | "object" | "pair" | "block_mapping_pair" | "block_mapping"
            | "block_sequence" | "flow_mapping" | "flow_sequence" => return None,
            _ => {}
        }
        current = candidate.parent();
    }
    None
}

/// Walk the parent chain looking for the nearest [`is_pair`] ancestor whose
/// key (after [`crate::text_util::strip_quotes`]) equals `expected_key`.
///
/// Centralises the two byte-identical private copies that lived in
/// `definition::foreign_key` and `completion::context`. Uses the safe
/// `source.get(..)` form so mid-edit byte ranges that fall outside `source`
/// return `None` instead of panicking — `tree_sitter` ranges and the
/// document text can momentarily disagree while the user types.
pub(crate) fn enclosing_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == expected_key
        {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Parent-only variant of [`enclosing_pair_with_key`]: walk strictly the
/// PARENT chain (skip `node` itself) looking for the nearest [`is_pair`]
/// ancestor whose key (after [`crate::text_util::strip_quotes`]) equals
/// `expected_key`.
///
/// Centralises the three byte-equivalent `is_inside_*` ancestor walks
/// previously open-coded in `completion::context::is_inside_constraints`,
/// `hover::check_expr::is_inside_constraints`, and
/// `hover::column::is_inside_columns`. Use this when "am I inside a pair
/// whose key is `<X>`?" is the question; use [`enclosing_pair_with_key`]
/// when the cursor's own node may itself be the pair.
pub(crate) fn ancestor_pair_with_key<'tree>(
    node: tree_sitter::Node<'tree>,
    source: &str,
    expected_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_pair(candidate)
            && let Some(key) = candidate.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == expected_key
        {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Walk strictly the PARENT chain (skip `node` itself) and return the
/// nearest [`is_pair`] ancestor — no key filter. This is the
/// unfiltered-key counterpart to [`ancestor_pair_with_key`]; use this
/// when "is there *any* pair ancestor?" is the question.
///
/// Centralises the two byte-equivalent private copies that lived in
/// `check_expr_locate` and `references::resolver` pre-this-hoist.
pub(crate) fn ancestor_pair(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_pair(candidate) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

/// Find a direct child pair of `object` whose key (after
/// [`crate::text_util::strip_quotes`]) equals `target_key` and return the
/// value-side byte slice **verbatim** (quotes preserved for quoted
/// scalars).
///
/// Centralises the three byte-identical private copies that lived in
/// `definition::foreign_key`, `completion::context`, and
/// `references::resolver`. Each pre-existing caller already stripped
/// quotes off the returned slice, so this helper deliberately keeps the
/// raw form to preserve the existing call-site contract. Uses the safe
/// `source.get(..)` form on both the key and value spans so a stale
/// byte range mid-edit returns `None` instead of panicking.
pub(crate) fn direct_child_value<'a>(
    object: tree_sitter::Node<'_>,
    source: &'a str,
    target_key: &str,
) -> Option<&'a str> {
    let mut cursor = object.walk();
    for child in object.children(&mut cursor) {
        if is_pair(child)
            && let Some(key) = child.named_child(0)
            && let Some(key_text) = source.get(key.byte_range())
            && crate::text_util::strip_quotes(key_text) == target_key
            && let Some(value) = child.named_child(1)
            && let Some(text) = source.get(value.byte_range())
        {
            return Some(text);
        }
    }
    None
}

/// Direct-child variant of [`enclosing_pair_with_key`] for the
/// **byte-slice** call sites: walk only the immediate children of
/// `mapping` (no parent traversal, no recursion) and return the first
/// [`is_pair`] child whose key — UTF-8 decoded then run through
/// [`crate::text_util::strip_quotes`] — equals `target_key`.
///
/// Centralises the three byte-identical private copies that lived in
/// `file_features`, `inlay_hints`, and `symbols` pre-0.2.0. The
/// `references::search` variant stays put because it does a RECURSIVE
/// walk, and the `completion::context` / `diagnostics::validation::types`
/// variants stay put because they operate on `&str` source. Use
/// [`direct_child_value`] when you only need the value-side text;
/// reach for this helper when you need the pair `Node` itself (e.g. to
/// continue navigating into its named children).
pub(crate) fn find_pair_with_key<'tree>(
    mapping: tree_sitter::Node<'tree>,
    source: &[u8],
    target_key: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut cursor = mapping.walk();
    mapping.children(&mut cursor).find(|&child| {
        is_pair(child)
            && child
                .named_child(0)
                .and_then(|key| std::str::from_utf8(&source[key.byte_range()]).ok())
                .map(crate::text_util::strip_quotes)
                == Some(target_key)
    })
}

/// A pair is "top level" when its direct ancestor mapping is itself the
/// outermost mapping of the document — this is how an LSP feature tells the
/// table's own `name: ...` apart from a column's `name: ...`. The `pair`'s
/// parent must be a mapping kind (`object` / `block_mapping` /
/// `flow_mapping`); walking above that parent must encounter NO further
/// mapping ancestor.
///
/// Centralises the two byte-equivalent copies that lived in
/// `references::search::is_top_level` and
/// `references::resolver::is_top_level_pair` pre-this-hoist.
pub(crate) fn is_top_level_pair(pair: tree_sitter::Node<'_>) -> bool {
    let Some(parent_mapping) = pair.parent() else {
        return false;
    };
    if !matches!(
        parent_mapping.kind(),
        "object" | "block_mapping" | "flow_mapping"
    ) {
        return false;
    }
    // Walk above the parent mapping; if we encounter another mapping with
    // the same kind, we are nested and therefore not top level.
    let mut current = parent_mapping.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            return false;
        }
        current = candidate.parent();
    }
    true
}

/// Walk strictly the PARENT chain of `node` and return the OUTERMOST
/// ancestor whose kind is a mapping (`object` / `block_mapping` /
/// `flow_mapping`) — i.e. the last mapping-kind node encountered before the
/// parent chain runs out. Returns `None` when no ancestor is a mapping.
///
/// This is the UPWARD counterpart to [`find_outer_mapping`], which descends
/// DFS to find the topmost mapping. Centralises the three byte-identical
/// upward walks that lived in `references::search::outer_table_name`,
/// `references::search::is_column_pair`, and
/// `references::resolver::enclosing_table_name` pre-this-hoist.
pub(crate) fn outermost_ancestor_mapping(
    node: tree_sitter::Node<'_>,
) -> Option<tree_sitter::Node<'_>> {
    let mut current = node.parent();
    let mut outer = None;
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "object" | "block_mapping" | "flow_mapping"
        ) {
            outer = Some(candidate);
        }
        current = candidate.parent();
    }
    outer
}

/// True when any ancestor pair of `node` has key `"constraints"`. Shared LSP
/// predicate for "cursor is inside a `constraints` array" — used by both the
/// CHECK-expression hover (`hover::check_expr`) and the CHECK-expression
/// completion context (`completion::context`).
pub(crate) fn is_inside_constraints(node: tree_sitter::Node<'_>, source: &str) -> bool {
    ancestor_pair_with_key(node, source, "constraints").is_some()
}

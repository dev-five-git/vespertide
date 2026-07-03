//! Semantic token legend — the LSP requires the server to publish an
//! ordered list of `tokenTypes` and `tokenModifiers` at `initialize`,
//! after which it emits indices into those lists for every reported
//! token. Both vectors MUST stay stable for the connection's lifetime.

use tower_lsp_server::ls_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};

/// Indices into `TOKEN_TYPE_NAMES`. Kept as a typed enum so the
/// classifier never hand-codes magic numbers.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum TokenIdx {
    /// Table-level identifier (top-level `name` value, `ref_table` value).
    Class = 0,
    /// Column-level identifier (column `name` value, `ref_columns` entry).
    Property = 1,
    /// Simple SQL column type (`"integer"`, `"text"`, …).
    Type = 2,
    /// Enum-like literal (`kind` value, enum `values[]`, `on_delete` etc.).
    EnumMember = 3,
    /// Reserved JSON / SQL keyword (`true`, `false`, `null`).
    Keyword = 4,
    /// Numeric literal.
    Number = 5,
    /// String literal that doesn't fall into a more specific category
    /// (free-form `default` values, `custom_type`, `comment` strings).
    String = 6,
}

/// Indices into `TOKEN_MODIFIER_NAMES`. Used as a bitmask in the LSP
/// wire format.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum ModIdx {
    /// Set on the bytes that *introduce* a symbol (top-level `name`
    /// value, column `name` value). Lets themes give declarations a
    /// stronger weight than references.
    Declaration = 1,
    /// Set on places that *refer to* a declared symbol (the value of
    /// `ref_table`, individual entries of `ref_columns`).
    Definition = 2,
}

/// String identifiers for each token type, in the exact order required
/// for the LSP legend.
pub const TOKEN_TYPE_NAMES: &[SemanticTokenType] = &[
    SemanticTokenType::CLASS,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::TYPE,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
];

/// String identifiers for each modifier, in the order matching their
/// bit position in the modifier bitmask. Bit `n` ⇔ index `n` here.
pub const TOKEN_MODIFIER_NAMES: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
];

/// Build the legend payload published on `initialize`.
#[must_use]
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPE_NAMES.to_vec(),
        token_modifiers: TOKEN_MODIFIER_NAMES.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legend_lengths_match_index_enums() {
        let legend = legend();
        assert_eq!(
            legend.token_types.len(),
            7,
            "TokenIdx variants must be reflected"
        );
        assert_eq!(
            legend.token_modifiers.len(),
            2,
            "ModIdx variants must be reflected"
        );
        assert_eq!(legend.token_types, TOKEN_TYPE_NAMES);
        assert_eq!(legend.token_modifiers, TOKEN_MODIFIER_NAMES);
    }

    #[test]
    fn token_indices_are_stable_against_legend_order() {
        // Guard against accidental reordering — the LSP wire protocol
        // refers to types by their position in `token_types`.
        assert_eq!(TokenIdx::Class as u32, 0);
        assert_eq!(TokenIdx::Property as u32, 1);
        assert_eq!(TokenIdx::Type as u32, 2);
        assert_eq!(TokenIdx::EnumMember as u32, 3);
        assert_eq!(TokenIdx::Keyword as u32, 4);
        assert_eq!(TokenIdx::Number as u32, 5);
        assert_eq!(TokenIdx::String as u32, 6);
    }
}

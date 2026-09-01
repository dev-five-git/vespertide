mod choices_and_apply;
mod drop_recreate_fk_policy;
mod fill_with;
mod narrowing;
mod timezone;

pub(in crate::commands::revision) use choices_and_apply::*;
pub(in crate::commands::revision) use drop_recreate_fk_policy::*;
pub(in crate::commands::revision) use fill_with::*;
pub(in crate::commands::revision) use narrowing::*;
pub(in crate::commands::revision) use timezone::*;

use colored::Colorize;

/// Print the standard 60-glyph horizontal separator used by every
/// revision-prompt banner. Centralises the width/glyph/colour so the
/// look-and-feel cannot drift across files.
#[cfg(not(tarpaulin_include))] // reason: trivial CLI display helper, exercised only by humans
pub(in crate::commands::revision) fn print_section_rule() {
    println!("{}", "\u{2500}".repeat(60).bright_black());
}

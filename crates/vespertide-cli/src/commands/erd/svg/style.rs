//! Aesthetic constants for the SVG ERD renderer.
//!
//! Centralising palette + sizing here keeps the rest of the renderer free
//! of magic numbers and makes theme tweaks a one-file diff.

// ---------------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------------

pub(super) const HEADER_H: f64 = 34.0;
pub(super) const ROW_H: f64 = 24.0;
pub(super) const TABLE_PAD_X: f64 = 14.0;
pub(super) const BADGE_W: f64 = 22.0;
pub(super) const BADGE_H: f64 = 14.0;
pub(super) const BADGE_GAP: f64 = 6.0;
pub(super) const COL_GAP_TYPE: f64 = 18.0;
pub(super) const TABLE_RADIUS: f64 = 14.0;

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

pub(super) const FONT_FAMILY: &str = "Pretendard, 'Noto Sans KR', ui-sans-serif, system-ui, -apple-system, 'Segoe UI', \
    Roboto, 'Helvetica Neue', Arial, sans-serif";
pub(super) const MONO_FAMILY: &str =
    "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Courier New', monospace";

pub(super) const TITLE_FS: f64 = 14.0;
pub(super) const TITLE_CH: f64 = 7.9;
pub(super) const NAME_FS: f64 = 12.0;
pub(super) const NAME_CH: f64 = 6.7;
pub(super) const TYPE_FS: f64 = 11.0;
pub(super) const TYPE_CH: f64 = 5.8;
pub(super) const BADGE_FS: f64 = 9.0;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

pub(super) const RANK_GAP: f64 = 80.0;
pub(super) const NODE_GAP: f64 = 32.0;
pub(super) const VIEW_PAD: f64 = 40.0;

// ---------------------------------------------------------------------------
// Palette — DevFive (devfive.kr) brand: purple #5b34f7, light bg #f7f8fb,
// accent yellow #ffe139.
// ---------------------------------------------------------------------------

pub(super) const BG: &str = "#f7f8fb";
pub(super) const CARD_BG: &str = "#ffffff";
pub(super) const CARD_BORDER: &str = "#eaeaed";
pub(super) const HEADER_FILL: &str = "url(#vespHeader)";
pub(super) const HEADER_FG: &str = "#ffffff";
pub(super) const HEADER_SUB: &str = "#e9defe";
pub(super) const ROW_FG: &str = "#1a1a1a";
pub(super) const ROW_FG_MUTED: &str = "#50505d";
pub(super) const ROW_ALT_BG: &str = "#fafbfd";
pub(super) const ROW_DIVIDER: &str = "#f0f0f4";
pub(super) const PK_BG: &str = "#fff7d4";
pub(super) const PK_FG: &str = "#8a6d04";
pub(super) const FK_BG: &str = "#f0e9ff";
pub(super) const FK_FG: &str = "#5b34f7";
pub(super) const EDGE_STROKE: &str = "#b5a4f6";
pub(super) const EDGE_END: &str = "#5b34f7";

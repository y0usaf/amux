//! Charmtone Pantera theme — Crush's default dark theme.
//!
//! Mirrors `internal/ui/styles/themes.go::CharmtonePantera` from the upstream
//! Crush repo. Role names follow Crush's `quickStyleOpts` vocabulary.

use super::charmtone as ct;
use super::DerivedTheme;

pub fn theme() -> DerivedTheme {
    DerivedTheme {
        // Brand
        primary: ct::CHARPLE,
        secondary: ct::DOLLY,
        accent: ct::BOK,
        keyword: ct::BLUSH,
        on_primary: ct::BUTTER,

        // Foreground tiers
        fg_base: ct::ASH,
        fg_subtle: ct::SMOKE,
        fg_more_subtle: ct::SQUID,
        fg_most_subtle: ct::OYSTER,

        // Background tiers
        bg_base: ct::PEPPER,
        bg_least_visible: ct::BBQ,
        bg_less_visible: ct::CHARCOAL,
        bg_most_visible: ct::IRON,

        separator: ct::CHARCOAL,

        // Status semantics
        destructive: ct::CORAL,
        error: ct::SRIRACHA,
        warning: ct::MUSTARD,
        warning_subtle: ct::ZEST,
        busy: ct::CITRON,
        info: ct::MALIBU,
        info_more_subtle: ct::SARDINE,
        info_most_subtle: ct::DAMSON,
        success: ct::JULEP,
        success_more_subtle: ct::BOK,
        success_most_subtle: ct::GUAC,

        // Legacy aliases (kept for current ScenePalette/overlay call sites)
        text: ct::ASH,
        muted: ct::SQUID,
        surface: ct::PEPPER,
        surface_raised: ct::BBQ,
        sidebar_bg: super::TRANSPARENT,
        status_fg: ct::BUTTER,
        status_bg: ct::CHARPLE,
        border: ct::CHARCOAL,
        running: ct::CITRON,
        success_subtle: ct::GUAC,
        term_fg: ct::ASH,
        term_bg: super::TRANSPARENT,

        accent_2: ct::DOLLY,
        ansi: super::default_ansi_palette(),
    }
}

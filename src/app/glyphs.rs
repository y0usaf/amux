use super::sidebar::{
    SIDEBAR_CROWN_JEWEL, SIDEBAR_CROWN_JEWEL_OPEN, SIDEBAR_NOTIFICATION_GLYPH,
    SIDEBAR_SPINNER_FRAMES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphStyle {
    Unicode,
    Ascii,
}

/// Corner/h-line/v-line set for a drawn border box. This maps to `draw_box`
/// so the same shapes switch between box-drawing lines and plain ASCII
/// depending on the configured glyph style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GlyphBox {
    pub(super) h: &'static str,
    pub(super) v: &'static str,
    pub(super) tl: &'static str,
    pub(super) tr: &'static str,
    pub(super) bl: &'static str,
    pub(super) br: &'static str,
}

/// Every runtime-drawn glyph the harness emits. Unicode entries are the exact
/// characters rendered today; Ascii entries are their plain-ASCII stand-ins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GlyphSet {
    pub(super) box_: GlyphBox,
    pub(super) scrollbar_track: &'static str,
    pub(super) scrollbar_thumb: &'static str,
    pub(super) status_separator: &'static str,
    pub(super) header_rule: &'static str,
    pub(super) spinner: &'static [&'static str],
    pub(super) notification: &'static str,
    pub(super) crown_open: &'static str,
    pub(super) crown_closed: &'static str,
    pub(super) dialog_slash: &'static str,
    pub(super) tree_branch_last: &'static str,
    pub(super) tree_branch: &'static str,
    pub(super) usage_separator: &'static str,
    pub(super) usage_cross: &'static str,
    pub(super) usage_vertical: &'static str,
}

impl GlyphSet {
    pub(super) fn unicode() -> Self {
        Self {
            box_: GlyphBox {
                h: "─",
                v: "│",
                tl: "┌",
                tr: "┐",
                bl: "└",
                br: "┘",
            },
            scrollbar_track: "│",
            scrollbar_thumb: "┃",
            status_separator: "│",
            header_rule: "─",
            spinner: SIDEBAR_SPINNER_FRAMES,
            notification: SIDEBAR_NOTIFICATION_GLYPH,
            crown_open: SIDEBAR_CROWN_JEWEL_OPEN,
            crown_closed: SIDEBAR_CROWN_JEWEL,
            dialog_slash: "╱",
            tree_branch_last: "  └─ ",
            tree_branch: "  ├─ ",
            usage_separator: "─",
            usage_cross: "┼",
            usage_vertical: "│",
        }
    }

    pub(super) fn ascii() -> Self {
        Self {
            box_: GlyphBox {
                h: "-",
                v: "|",
                tl: "+",
                tr: "+",
                bl: "+",
                br: "+",
            },
            scrollbar_track: "|",
            scrollbar_thumb: "#",
            status_separator: "|",
            header_rule: "-",
            spinner: &["-", "\\", "|", "/"],
            notification: "[!]",
            crown_open: "*",
            crown_closed: "*",
            dialog_slash: "/",
            tree_branch_last: "`- ",
            tree_branch: "|- ",
            usage_separator: "-",
            usage_cross: "+",
            usage_vertical: "|",
        }
    }

    pub(super) fn for_style(style: GlyphStyle) -> Self {
        match style {
            GlyphStyle::Unicode => Self::unicode(),
            GlyphStyle::Ascii => Self::ascii(),
        }
    }
}

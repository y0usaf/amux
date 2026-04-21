#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalSelectionPoint {
    pub row: u16,
    pub col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelectionRange {
    pub start: TerminalSelectionPoint,
    pub end: TerminalSelectionPoint,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalSelection {
    anchor: Option<TerminalSelectionPoint>,
    focus: Option<TerminalSelectionPoint>,
}

impl TerminalSelection {
    pub(crate) fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }

    pub(crate) fn set(&mut self, point: TerminalSelectionPoint) {
        self.anchor = Some(point);
        self.focus = Some(point);
    }

    pub(crate) fn update_focus(&mut self, point: TerminalSelectionPoint) {
        self.focus = Some(point);
    }

    pub(crate) fn normalized(&self) -> Option<TerminalSelectionRange> {
        let (anchor, focus) = (self.anchor?, self.focus?);
        if anchor == focus {
            None
        } else if (anchor.row, anchor.col) <= (focus.row, focus.col) {
            Some(TerminalSelectionRange {
                start: anchor,
                end: focus,
            })
        } else {
            Some(TerminalSelectionRange {
                start: focus,
                end: anchor,
            })
        }
    }

    pub(crate) fn anchor(&self) -> Option<TerminalSelectionPoint> {
        self.anchor
    }

    pub(crate) fn focus(&self) -> Option<TerminalSelectionPoint> {
        self.focus
    }
}

pub(crate) fn terminal_selection_span(
    selection: Option<TerminalSelectionRange>,
    row: u16,
    cols: u16,
) -> Option<(u16, u16)> {
    let selection = selection?;
    if row < selection.start.row || row > selection.end.row {
        return None;
    }

    let row_start = if row == selection.start.row {
        selection.start.col
    } else {
        0
    };
    let row_end = if row == selection.end.row {
        selection.end.col
    } else {
        cols
    };
    (row_start < row_end).then_some((row_start, row_end - row_start))
}

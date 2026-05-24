use crate::backend::{
    cache::{CacheStatus, CachedValue},
    document::{CashewDocument, CellValue, cell_key},
};

use super::state::{AppState, CellInteractionMode, GROWTH_BUFFER_COLS, GROWTH_BUFFER_ROWS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionRange {
    pub(crate) start_row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_row: usize,
    pub(crate) end_col: usize,
}

impl SelectionRange {
    pub(crate) fn new(anchor: (usize, usize), end: (usize, usize)) -> Self {
        Self {
            start_row: anchor.0.min(end.0),
            start_col: anchor.1.min(end.1),
            end_row: anchor.0.max(end.0),
            end_col: anchor.1.max(end.1),
        }
    }

    pub(crate) fn contains(&self, row: usize, col: usize) -> bool {
        (self.start_row..=self.end_row).contains(&row)
            && (self.start_col..=self.end_col).contains(&col)
    }

    pub(crate) fn contains_multiple_cells(&self) -> bool {
        self.start_row != self.end_row || self.start_col != self.end_col
    }
}

impl AppState {
    pub(crate) fn begin_selection(&mut self, row: usize, col: usize, extend: bool) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        if self.selected_cell != (row, col) {
            self.editing_cell = None;
        }
        if !extend {
            self.selection_anchor = (row, col);
            self.selected_cell = (row, col);
            self.selected_cell_mode = CellInteractionMode::Display;
            self.refresh_formula_input_from_cell(row, col);
            self.completions_open = false;
            self.completion_index = 0;
        }
        self.selection_end = (row, col);
        self.selecting = true;
    }

    pub(crate) fn begin_cell_interaction(&mut self, row: usize, col: usize, extend: bool) -> bool {
        if super::state::formula_accepts_cell_reference(&self.formula_input)
            && self.selected_cell != (row, col)
        {
            self.insert_cell_reference(row, col);
            self.selecting = false;
            true
        } else {
            let should_advance = self.selected_cell == (row, col) && !extend;
            let previous_mode = self.selected_cell_mode;
            self.begin_selection(row, col, extend);
            if should_advance {
                self.selected_cell_mode = previous_mode;
                self.advance_cell_mode(row, col);
            }
            false
        }
    }

    pub(crate) fn extend_selection(&mut self, row: usize, col: usize) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        self.selection_end = (row, col);
    }

    pub(crate) fn finish_selection(&mut self) {
        self.selecting = false;
    }

    pub(crate) fn selection_range(&self) -> SelectionRange {
        SelectionRange::new(self.selection_anchor, self.selection_end)
    }

    pub(crate) fn extend_selection_with_keyboard(
        &mut self,
        row_delta: isize,
        col_delta: isize,
    ) -> (usize, usize) {
        let (row, col) = self.selection_end;
        let next_row = row.saturating_add_signed(row_delta);
        let next_col = col.saturating_add_signed(col_delta);
        self.ensure_work_area(next_row + GROWTH_BUFFER_ROWS, next_col + GROWTH_BUFFER_COLS);
        self.selection_end = (next_row, next_col);
        self.selected_cell = (next_row, next_col);
        self.selected_cell_mode = CellInteractionMode::Display;
        self.editing_cell = None;
        self.refresh_formula_input_from_cell(next_row, next_col);
        self.completions_open = false;
        self.completion_index = 0;
        self.selected_cell
    }

    pub(crate) fn copy_selection(&mut self) -> String {
        let Some(sheet) = self.document.active_sheet() else {
            return String::new();
        };

        let range = self.selection_range();
        if range.start_row == range.end_row
            && range.start_col == range.end_col
            && self.selected_cell_mode == CellInteractionMode::Value
        {
            let text = cell_value_for_copy(&self.document, range.start_row, range.start_col);
            self.status = format!(
                "Copied value from {}",
                cell_key(range.start_row, range.start_col)
            );
            return text;
        }

        let cells = (range.start_row..=range.end_row)
            .map(|row| {
                (range.start_col..=range.end_col)
                    .map(|col| {
                        sheet
                            .cell(row, col)
                            .map(|cell| cell.input.clone())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let text = cells_to_tsv(&cells);
        self.status = format!(
            "Copied {}",
            range_label(
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col
            )
        );
        text
    }

    pub(crate) fn cut_selection(&mut self) -> String {
        let copied = self.copy_selection();
        self.clear_selection_with_status("Cut");
        copied
    }

    pub(crate) fn clear_selection(&mut self) {
        self.clear_selection_with_status("Cleared");
    }

    fn clear_selection_with_status(&mut self, action: &str) {
        let range = self.selection_range();

        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                self.set_cell_input(row, col, String::new());
            }
        }

        self.selection_anchor = (range.start_row, range.start_col);
        self.selection_end = (range.end_row, range.end_col);
        self.selected_cell = (range.start_row, range.start_col);
        self.selected_cell_mode = CellInteractionMode::Display;
        self.editing_cell = None;
        self.refresh_formula_input_from_cell(range.start_row, range.start_col);
        self.completions_open = false;
        self.completion_index = 0;
        self.status = format!(
            "{} {}",
            action,
            range_label(
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col
            )
        );
    }

    pub(crate) fn paste_selection(&mut self, text: &str) {
        let rows = clipboard_text_to_rows(text);
        let (start_row, start_col) = self.selected_cell;

        let row_count = rows.len();
        let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
        if row_count == 0 || col_count == 0 {
            self.set_cell_input(start_row, start_col, String::new());
            self.selection_anchor = (start_row, start_col);
            self.selection_end = (start_row, start_col);
            self.selected_cell_mode = CellInteractionMode::Display;
            self.editing_cell = None;
            self.refresh_formula_input_from_cell(start_row, start_col);
            self.completions_open = false;
            self.completion_index = 0;
            self.status = format!("Pasted {}", cell_key(start_row, start_col));
            return;
        }

        for (row_offset, row_values) in rows.into_iter().enumerate() {
            for (col_offset, value) in row_values.into_iter().enumerate() {
                self.set_cell_input(start_row + row_offset, start_col + col_offset, value);
            }
        }

        self.selection_anchor = (start_row, start_col);
        self.selection_end = (
            start_row + row_count.saturating_sub(1),
            start_col + col_count.saturating_sub(1),
        );
        self.selected_cell = (start_row, start_col);
        self.selected_cell_mode = CellInteractionMode::Display;
        self.editing_cell = None;
        self.refresh_formula_input_from_cell(start_row, start_col);
        self.completions_open = false;
        self.completion_index = 0;
        self.status = format!(
            "Pasted {}",
            range_label(
                start_row,
                start_col,
                self.selection_end.0,
                self.selection_end.1
            )
        );
    }
}

fn cell_value_for_copy(document: &CashewDocument, row: usize, col: usize) -> String {
    let Some(sheet) = document.active_sheet() else {
        return String::new();
    };
    let Some(cell) = sheet.cell(row, col) else {
        return String::new();
    };

    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Text(value) | CellValue::Cached(value) => {
            if let Some(cache_key) = cell.cache_key.as_ref() {
                if let Some(entry) = document.cache.get(cache_key) {
                    if entry.status == CacheStatus::Ready {
                        if let CachedValue::MediaAsset(asset) = &entry.value {
                            return asset.data_uri.clone().unwrap_or_else(|| asset.uri.clone());
                        }
                    }
                }
            }
            value.clone()
        }
        CellValue::FormulaPending { message } => message.clone(),
        CellValue::Error(error) => format!("#ERROR: {error}"),
    }
}

pub(crate) fn range_label(
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
) -> String {
    let start = cell_key(start_row, start_col);
    let end = cell_key(end_row, end_col);
    if start == end {
        start
    } else {
        format!("{start}:{end}")
    }
}

fn cells_to_tsv(cells: &[Vec<String>]) -> String {
    cells
        .iter()
        .map(|row| row.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn clipboard_text_to_rows(text: &str) -> Vec<Vec<String>> {
    if text.is_empty() {
        return vec![vec![String::new()]];
    }

    text.split_terminator('\n')
        .map(|line| {
            line.trim_end_matches('\r')
                .split('\t')
                .map(|cell| cell.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_clears_selected_cells_after_copying() {
        let mut state = AppState::new();
        state.set_cell_input(0, 0, "first".to_string());
        state.set_cell_input(0, 1, "second".to_string());
        state.begin_selection(0, 0, false);
        state.extend_selection(0, 1);
        state.finish_selection();

        assert_eq!(state.cut_selection(), "first\tsecond");

        let sheet = state.document.active_sheet().unwrap();
        assert_eq!(
            sheet.cell(0, 0).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Empty)
        );
        assert_eq!(
            sheet.cell(0, 1).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Empty)
        );
    }

    #[test]
    fn clear_selection_deletes_selected_cells_without_copying() {
        let mut state = AppState::new();
        state.set_cell_input(0, 0, "first".to_string());
        state.set_cell_input(0, 1, "second".to_string());
        state.begin_selection(0, 0, false);
        state.extend_selection(0, 1);
        state.finish_selection();

        state.clear_selection();

        let sheet = state.document.active_sheet().unwrap();
        assert_eq!(
            sheet.cell(0, 0).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Empty)
        );
        assert_eq!(
            sheet.cell(0, 1).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Empty)
        );
        assert_eq!(state.selection_range(), SelectionRange::new((0, 0), (0, 1)));
    }

    #[test]
    fn paste_selection_fills_multiple_cells_from_tsv() {
        let mut state = AppState::new();
        state.begin_selection(0, 0, false);
        state.paste_selection("first\tsecond\nthird\tfourth");

        let sheet = state.document.active_sheet().unwrap();
        assert_eq!(
            sheet.cell(0, 0).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Text(
                "first".to_string()
            ))
        );
        assert_eq!(
            sheet.cell(0, 1).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Text(
                "second".to_string()
            ))
        );
        assert_eq!(
            sheet.cell(1, 0).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Text(
                "third".to_string()
            ))
        );
        assert_eq!(
            sheet.cell(1, 1).map(|cell| &cell.value),
            Some(&crate::backend::document::CellValue::Text(
                "fourth".to_string()
            ))
        );
    }
}

use std::collections::BTreeMap;

use crate::backend::{
    cache::{CacheStatus, CachedValue},
    document::{CashewDocument, Cell, CellValue, cell_key},
};

use super::state::{AppState, CellInteractionMode, GROWTH_BUFFER_COLS, GROWTH_BUFFER_ROWS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionRange {
    pub(crate) start_row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_row: usize,
    pub(crate) end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CopiedCells {
    text: String,
    row_count: usize,
    col_count: usize,
    cells: BTreeMap<(usize, usize), Cell>,
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
        let sheet = self.document.sheet();

        let range = self.selection_range();
        if range.start_row == range.end_row
            && range.start_col == range.end_col
            && self.selected_cell_mode == CellInteractionMode::Value
        {
            let mut cells = BTreeMap::new();
            if let Some(cell) = sheet
                .cell(range.start_row, range.start_col)
                .cloned()
                .map(strip_spill_metadata)
            {
                cells.insert((0, 0), cell);
            }
            let text = cell_value_for_copy(&self.document, range.start_row, range.start_col);
            self.copied_cells = Some(CopiedCells {
                text: text.clone(),
                row_count: 1,
                col_count: 1,
                cells,
            });
            self.status = format!(
                "Copied value from {}",
                cell_key(range.start_row, range.start_col)
            );
            return text;
        }

        let row_count = range.end_row - range.start_row + 1;
        let col_count = range.end_col - range.start_col + 1;
        let mut copied_rows = Vec::with_capacity(row_count);
        let mut copied_cells = BTreeMap::new();

        for row_offset in 0..row_count {
            let row = range.start_row + row_offset;
            let mut copied_row = Vec::with_capacity(col_count);

            for col_offset in 0..col_count {
                let col = range.start_col + col_offset;
                let cell = sheet.cell(row, col).cloned().map(strip_spill_metadata);
                copied_row.push(
                    cell.as_ref()
                        .map(|cell| cell.input.clone())
                        .unwrap_or_default(),
                );

                if let Some(cell) = cell {
                    copied_cells.insert((row_offset, col_offset), cell);
                }
            }

            copied_rows.push(copied_row);
        }

        let text = cells_to_tsv(&copied_rows);
        self.copied_cells = Some(CopiedCells {
            text: text.clone(),
            row_count,
            col_count,
            cells: copied_cells,
        });
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
        if let Some(copied) = self.copied_cells.as_ref() {
            if copied.text == text {
                self.paste_copied_cells();
                return;
            }
        }

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

    fn paste_copied_cells(&mut self) {
        let Some(copied) = self.copied_cells.clone() else {
            return;
        };
        let (start_row, start_col) = self.selected_cell;

        let row_count = copied.row_count;
        let col_count = copied.col_count;
        if row_count == 0 || col_count == 0 {
            self.paste_selection("");
            return;
        }

        self.ensure_work_area(
            start_row + row_count + GROWTH_BUFFER_ROWS,
            start_col + col_count + GROWTH_BUFFER_COLS,
        );

        let sheet = self.document.sheet_mut();
        for row_offset in 0..row_count {
            for col_offset in 0..col_count {
                let cell = copied
                    .cells
                    .get(&(row_offset, col_offset))
                    .cloned()
                    .map(strip_spill_metadata);
                let row = start_row + row_offset;
                let col = start_col + col_offset;
                sheet.ensure_size(row + 1, col + 1);
                sheet.cells.insert(
                    cell_key(row, col),
                    cell.unwrap_or(Cell {
                        input: String::new(),
                        value: CellValue::Empty,
                        cache_key: None,
                        spill_range: None,
                    }),
                );
            }
        }
        sheet.recalculate_formulas();
        let _ = sheet;
        self.fit_media_rows_in_range(
            start_row,
            start_col,
            start_row + row_count.saturating_sub(1),
            start_col + col_count.saturating_sub(1),
        );

        self.dirty = true;
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
    let sheet = document.sheet();
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

fn strip_spill_metadata(mut cell: Cell) -> Cell {
    cell.spill_range = None;
    cell
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

        let sheet = state.document.sheet();
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

        let sheet = state.document.sheet();
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

        let sheet = state.document.sheet();
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

    #[test]
    fn paste_selection_preserves_internal_copied_cached_cell() {
        let mut state = AppState::new();
        state.document.sheet_mut().set_cell_value_with_cache(
            0,
            0,
            "=LLM(A2)".to_string(),
            CellValue::Cached("cached answer".to_string()),
            Some("cache-key".to_string()),
        );

        state.begin_selection(0, 0, false);
        let copied = state.copy_selection();

        state.begin_selection(1, 1, false);
        state.paste_selection(&copied);

        let sheet = state.document.sheet();
        assert_eq!(
            sheet
                .cell(1, 1)
                .map(|cell| { (cell.input.as_str(), &cell.value, cell.cache_key.as_deref(),) }),
            Some((
                "=LLM(A2)",
                &CellValue::Cached("cached answer".to_string()),
                Some("cache-key")
            ))
        );
    }

    #[test]
    fn copy_selection_stores_only_populated_cells() {
        let mut state = AppState::new();
        state.set_cell_input(0, 0, "first".to_string());
        state.begin_selection(0, 0, false);
        state.extend_selection(1, 1);
        state.finish_selection();

        state.copy_selection();

        let copied = state
            .copied_cells
            .as_ref()
            .expect("selection should be copied");
        assert_eq!(copied.row_count, 2);
        assert_eq!(copied.col_count, 2);
        assert_eq!(copied.cells.len(), 1);
        assert!(copied.cells.contains_key(&(0, 0)));
        assert_eq!(
            copied.cells.get(&(0, 0)).map(|cell| cell.input.as_str()),
            Some("first")
        );
    }

    #[test]
    fn pasting_cached_media_into_empty_row_fits_row_height() {
        let mut state = AppState::new();
        state.new_document();
        state.document.cache.insert(
            "image-key".to_string(),
            crate::backend::cache::CacheEntry {
                key: "image-key".to_string(),
                status: crate::backend::cache::CacheStatus::Ready,
                value: crate::backend::cache::CachedValue::MediaAsset(
                    crate::backend::cache::MediaAsset {
                        provider: "fal.image".to_string(),
                        media_type: crate::backend::cache::MediaType::Image,
                        uri: "https://example.com/tall.png".to_string(),
                        data_uri: None,
                        metadata: serde_json::json!({
                            "response": {
                                "images": [{
                                    "width": 100,
                                    "height": 400
                                }]
                            }
                        }),
                    },
                ),
            },
        );
        state.document.sheet_mut().set_cell_value_with_cache(
            0,
            0,
            "=GENERATEIMAGE(\"prompt\", \"flux/dev\")".to_string(),
            CellValue::Cached("https://example.com/tall.png".to_string()),
            Some("image-key".to_string()),
        );

        state.begin_selection(0, 0, false);
        let copied = state.copy_selection();

        state.begin_selection(2, 0, false);
        state.paste_selection(&copied);

        let sheet = state.document.sheet();
        assert_eq!(sheet.row_height(2), 720);
    }

    #[test]
    fn pasting_openai_cached_image_uses_data_uri_dimensions_when_metadata_is_missing() {
        let mut state = AppState::new();
        state.new_document();
        state.document.cache.insert(
            "image-key".to_string(),
            crate::backend::cache::CacheEntry {
                key: "image-key".to_string(),
                status: crate::backend::cache::CacheStatus::Ready,
                value: crate::backend::cache::CachedValue::MediaAsset(
                    crate::backend::cache::MediaAsset {
                        provider: "fal.image".to_string(),
                        media_type: crate::backend::cache::MediaType::Image,
                        uri: "https://example.com/tall.png".to_string(),
                        data_uri: Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAECAIAAAAmkwkpAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC".to_string()),
                        metadata: serde_json::json!({
                            "response": {
                                "images": [{
                                    "width": null,
                                    "height": null
                                }]
                            }
                        }),
                    },
                ),
            },
        );
        state.document.sheet_mut().set_cell_value_with_cache(
            0,
            0,
            "=GENERATEIMAGE(\"prompt\", \"openai/gpt-image-2\", \"medium\", C29)".to_string(),
            CellValue::Cached("https://example.com/tall.png".to_string()),
            Some("image-key".to_string()),
        );

        state.begin_selection(0, 0, false);
        let copied = state.copy_selection();

        state.begin_selection(3, 0, false);
        state.paste_selection(&copied);

        let sheet = state.document.sheet();
        assert_eq!(sheet.row_height(3), 720);
    }

    #[test]
    fn value_mode_cut_and_paste_preserves_cached_media_cell_internally() {
        let mut state = AppState::new();
        state.new_document();
        state.document.cache.insert(
            "image-key".to_string(),
            crate::backend::cache::CacheEntry {
                key: "image-key".to_string(),
                status: crate::backend::cache::CacheStatus::Ready,
                value: crate::backend::cache::CachedValue::MediaAsset(
                    crate::backend::cache::MediaAsset {
                        provider: "fal.image".to_string(),
                        media_type: crate::backend::cache::MediaType::Image,
                        uri: "https://example.com/tall.png".to_string(),
                        data_uri: Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAAECAIAAAAmkwkpAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC".to_string()),
                        metadata: serde_json::json!({
                            "response": {
                                "images": [{
                                    "width": null,
                                    "height": null
                                }]
                            }
                        }),
                    },
                ),
            },
        );
        state.document.sheet_mut().set_cell_value_with_cache(
            0,
            0,
            "=GENERATEIMAGE(\"prompt\", \"openai/gpt-image-2\", \"medium\", C29)".to_string(),
            CellValue::Cached("https://example.com/tall.png".to_string()),
            Some("image-key".to_string()),
        );
        state.begin_selection(0, 0, false);
        state.selected_cell_mode = CellInteractionMode::Value;

        let copied = state.cut_selection();
        assert!(copied.starts_with("data:image/png;base64,"));

        state.begin_selection(4, 0, false);
        state.paste_selection(&copied);

        let sheet = state.document.sheet();
        assert_eq!(
            sheet
                .cell(4, 0)
                .map(|cell| (cell.input.as_str(), cell.cache_key.as_deref())),
            Some((
                "=GENERATEIMAGE(\"prompt\", \"openai/gpt-image-2\", \"medium\", C29)",
                Some("image-key")
            ))
        );
    }
}

use crate::backend::document::{cell_key, column_name};

use super::state::{
    AppState, GROWTH_BUFFER_COLS, GROWTH_BUFFER_ROWS, cell_input, normalize_editor_text,
};

#[derive(Debug, Clone)]
pub(crate) struct CopiedCells {
    pub(crate) origin: (usize, usize),
    pub(crate) cells: Vec<Vec<String>>,
    pub(crate) text: String,
}

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
}

impl AppState {
    pub(crate) fn begin_selection(&mut self, row: usize, col: usize, extend: bool) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        if !extend {
            self.selection_anchor = (row, col);
            self.selected_cell = (row, col);
            self.formula_input = cell_input(&self.document, row, col);
            self.completions_open = false;
        }
        self.selection_end = (row, col);
        self.selecting = true;
    }

    pub(crate) fn begin_cell_interaction(&mut self, row: usize, col: usize, extend: bool) {
        if super::state::formula_accepts_cell_reference(&self.formula_input)
            && self.selected_cell != (row, col)
        {
            self.insert_cell_reference(row, col);
            self.selecting = false;
        } else {
            self.begin_selection(row, col, extend);
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
        self.formula_input = cell_input(&self.document, next_row, next_col);
        self.completions_open = false;
        self.selected_cell
    }

    pub(crate) fn copy_selection(&mut self) -> String {
        let Some(sheet) = self.document.active_sheet() else {
            return String::new();
        };

        let range = self.selection_range();
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
        self.copied_cells = Some(CopiedCells {
            origin: (range.start_row, range.start_col),
            cells,
            text: text.clone(),
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
        let range = self.selection_range();

        for row in range.start_row..=range.end_row {
            for col in range.start_col..=range.end_col {
                self.set_cell_input(row, col, String::new());
            }
        }

        self.selection_anchor = (range.start_row, range.start_col);
        self.selection_end = (range.end_row, range.end_col);
        self.selected_cell = (range.start_row, range.start_col);
        self.formula_input = cell_input(&self.document, range.start_row, range.start_col);
        self.completions_open = false;
        self.status = format!(
            "Cut {}",
            range_label(
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col
            )
        );
        copied
    }

    pub(crate) fn paste_cells(&mut self, clipboard_text: &str) {
        let (target_row, target_col) = self.selected_cell;
        let pasted = match self
            .copied_cells
            .as_ref()
            .filter(|copied| copied.text == clipboard_text)
        {
            Some(copied) => {
                let row_delta = target_row as isize - copied.origin.0 as isize;
                let col_delta = target_col as isize - copied.origin.1 as isize;
                copied
                    .cells
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|value| shift_formula_references(value, row_delta, col_delta))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            }
            None => tsv_to_cells(clipboard_text),
        };

        if pasted.is_empty() || pasted.iter().all(|row| row.is_empty()) {
            return;
        }

        let row_count = pasted.len();
        let col_count = pasted.iter().map(Vec::len).max().unwrap_or(0);
        self.ensure_work_area(
            target_row + row_count + GROWTH_BUFFER_ROWS,
            target_col + col_count + GROWTH_BUFFER_COLS,
        );

        for (row_offset, row) in pasted.iter().enumerate() {
            for (col_offset, value) in row.iter().enumerate() {
                self.set_cell_input(
                    target_row + row_offset,
                    target_col + col_offset,
                    value.clone(),
                );
            }
        }

        self.selection_anchor = (target_row, target_col);
        self.selection_end = (
            target_row + row_count.saturating_sub(1),
            target_col + col_count.saturating_sub(1),
        );
        self.selected_cell = (target_row, target_col);
        self.formula_input = cell_input(&self.document, target_row, target_col);
        self.completions_open = false;
        self.status = format!(
            "Pasted {} cells at {}",
            row_count * col_count,
            cell_key(target_row, target_col)
        );
    }
}

fn range_label(start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> String {
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

fn tsv_to_cells(text: &str) -> Vec<Vec<String>> {
    text.trim_end_matches(['\r', '\n'])
        .split('\n')
        .map(|row| {
            row.trim_end_matches('\r')
                .split('\t')
                .map(normalize_editor_text)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn shift_formula_references(input: &str, row_delta: isize, col_delta: isize) -> String {
    if !input.trim_start().starts_with('=') {
        return input.to_string();
    }

    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some((start, end, reference)) = find_cell_reference(rest) {
        output.push_str(&rest[..start]);
        output.push_str(&shift_cell_reference(reference, row_delta, col_delta));
        rest = &rest[end..];
    }

    output.push_str(rest);
    output
}

fn find_cell_reference(input: &str) -> Option<(usize, usize, &str)> {
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'"' || bytes[index] == b'\'' {
            index = skip_quoted_formula_text(bytes, index);
            continue;
        }

        let start = index;
        let col_absolute = bytes.get(index) == Some(&b'$');
        if col_absolute {
            index += 1;
        }

        let col_start = index;
        while matches!(bytes.get(index), Some(b'A'..=b'Z') | Some(b'a'..=b'z')) {
            index += 1;
        }

        if index == col_start || index - col_start > 3 {
            index = start + 1;
            continue;
        }

        let row_absolute = bytes.get(index) == Some(&b'$');
        if row_absolute {
            index += 1;
        }

        let row_start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }

        if index == row_start {
            index = start + 1;
            continue;
        }

        if start > 0 && is_reference_name_char(bytes[start - 1]) {
            index = start + 1;
            continue;
        }

        if bytes
            .get(index)
            .copied()
            .is_some_and(is_reference_name_char)
        {
            index = start + 1;
            continue;
        }

        return Some((start, index, &input[start..index]));
    }

    None
}

fn skip_quoted_formula_text(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn is_reference_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

fn shift_cell_reference(reference: &str, row_delta: isize, col_delta: isize) -> String {
    let bytes = reference.as_bytes();
    let mut index = 0;
    let col_absolute = bytes.get(index) == Some(&b'$');
    if col_absolute {
        index += 1;
    }

    let col_start = index;
    while matches!(bytes.get(index), Some(b'A'..=b'Z') | Some(b'a'..=b'z')) {
        index += 1;
    }
    let col_name = &reference[col_start..index];

    let row_absolute = bytes.get(index) == Some(&b'$');
    if row_absolute {
        index += 1;
    }
    let row_number = reference[index..].parse::<usize>().unwrap_or(1);

    let col = column_index(col_name).unwrap_or(0);
    let shifted_col = if col_absolute {
        col
    } else {
        col.saturating_add_signed(col_delta)
    };
    let shifted_row = if row_absolute {
        row_number.saturating_sub(1)
    } else {
        row_number
            .saturating_sub(1)
            .saturating_add_signed(row_delta)
    };

    format!(
        "{}{}{}{}",
        if col_absolute { "$" } else { "" },
        column_name(shifted_col),
        if row_absolute { "$" } else { "" },
        shifted_row + 1
    )
}

fn column_index(name: &str) -> Option<usize> {
    let mut col = 0usize;
    for byte in name.bytes() {
        if !byte.is_ascii_alphabetic() {
            return None;
        }
        col = col * 26 + (byte.to_ascii_uppercase() - b'A' + 1) as usize;
    }
    col.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_formulas_shift_relative_references_only() {
        assert_eq!(
            shift_formula_references("=A1+$B1+C$1+$D$1", 2, 3),
            "=D3+$B3+F$1+$D$1"
        );
        assert_eq!(
            shift_formula_references("=LLM(\"keep A1\", A1)", 1, 1),
            "=LLM(\"keep A1\", B2)"
        );
        assert_eq!(shift_formula_references("plain A1", 2, 3), "plain A1");
    }

    #[test]
    fn paste_uses_internal_copy_origin_to_shift_formulas() {
        let mut state = AppState::new();
        state.set_cell_input(0, 0, "2".to_string());
        state.set_cell_input(0, 1, "=A1".to_string());
        state.begin_selection(0, 1, false);
        state.finish_selection();
        let copied = state.copy_selection();

        state.set_selected_cell(1, 1);
        state.paste_cells(&copied);

        let sheet = state.document.active_sheet().unwrap();
        assert_eq!(
            sheet.cell(1, 1).map(|cell| cell.input.as_str()),
            Some("=A2")
        );
    }

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
}

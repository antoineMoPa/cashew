use std::path::PathBuf;

use dioxus::prelude::WritableExt;

use crate::backend::cache::{CacheEntry, CacheStatus, CachedValue, stable_cache_key};
use crate::backend::document::{CashewDocument, CellValue, cell_key, column_name};
use crate::backend::formula_implementations::llm_request_for_sheet;
use crate::backend::formulas::FormulaFunction;
use crate::backend::providers::openrouter::{OpenRouterClient, OpenRouterRequest};
use crate::backend::settings::{UserSettings, settings_path};

const MIN_COLUMN_WIDTH: i32 = 72;
const MAX_COLUMN_WIDTH: i32 = 520;
const MIN_ROW_HEIGHT: i32 = 24;
const MAX_ROW_HEIGHT: i32 = 260;

pub(crate) const MIN_VISIBLE_ROWS: usize = 100;
pub(crate) const MIN_VISIBLE_COLS: usize = 26;
const GROWTH_BUFFER_ROWS: usize = 25;
const GROWTH_BUFFER_COLS: usize = 10;

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    pub(crate) document: CashewDocument,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) file_menu_open: bool,
    pub(crate) dirty: bool,
    pub(crate) status: String,
    pub(crate) selected_cell: (usize, usize),
    pub(crate) selection_anchor: (usize, usize),
    pub(crate) selection_end: (usize, usize),
    pub(crate) selecting: bool,
    pub(crate) copied_cells: Option<CopiedCells>,
    pub(crate) formula_input: String,
    pub(crate) resizing: Option<ResizeDrag>,
    pub(crate) completions_open: bool,
    pub(crate) settings_open: bool,
    pub(crate) settings_fal_key: String,
    pub(crate) settings_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResizeDrag {
    pub(crate) kind: ResizeKind,
    pub(crate) index: usize,
    pub(crate) start: i32,
    pub(crate) original: i32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResizeKind {
    Column,
    Row,
}

#[derive(Debug, Clone)]
pub(crate) struct CopiedCells {
    pub(crate) origin: (usize, usize),
    pub(crate) cells: Vec<Vec<String>>,
    pub(crate) text: String,
}

impl AppState {
    pub(crate) fn new() -> Self {
        let mut document = CashewDocument::default();
        if let Some(sheet) = document.active_sheet_mut() {
            sheet.ensure_size(MIN_VISIBLE_ROWS, MIN_VISIBLE_COLS);
        }
        let formula_input = cell_input(&document, 0, 0);
        let (settings_fal_key, settings_path, settings_status) = load_settings_for_ui();

        Self {
            document,
            file_path: None,
            file_menu_open: false,
            dirty: false,
            status: settings_status.unwrap_or_else(|| "Ready".to_string()),
            selected_cell: (0, 0),
            selection_anchor: (0, 0),
            selection_end: (0, 0),
            selecting: false,
            copied_cells: None,
            formula_input,
            resizing: None,
            completions_open: false,
            settings_open: false,
            settings_fal_key,
            settings_path,
        }
    }

    pub(crate) fn set_selected_cell(&mut self, row: usize, col: usize) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        self.selected_cell = (row, col);
        self.selection_anchor = (row, col);
        self.selection_end = (row, col);
        self.formula_input = cell_input(&self.document, row, col);
        self.completions_open = should_show_completions(&self.formula_input);
    }

    pub(crate) fn begin_selection(&mut self, row: usize, col: usize, extend: bool) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        if !extend {
            self.selection_anchor = (row, col);
            self.selected_cell = (row, col);
            self.formula_input = cell_input(&self.document, row, col);
            self.completions_open = should_show_completions(&self.formula_input);
        }
        self.selection_end = (row, col);
        self.selecting = true;
    }

    pub(crate) fn begin_cell_interaction(&mut self, row: usize, col: usize, extend: bool) {
        if formula_accepts_cell_reference(&self.formula_input) && self.selected_cell != (row, col) {
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

    pub(crate) fn set_cell_input(&mut self, row: usize, col: usize, value: String) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_input(row, col, value.clone());
            self.dirty = true;
            self.status = format!("Edited {}", cell_key(row, col));
        }

        if self.selected_cell == (row, col) {
            self.completions_open = should_show_completions(&value);
            self.formula_input = value;
        }
    }

    pub(crate) async fn run_llm_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: OpenRouterRequest,
    ) {
        let result = run_openrouter_request(&request).await;

        state.with_mut(|state| {
            state.finish_llm_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) fn prepare_llm_for_cell(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, OpenRouterRequest)> {
        let sheet = self.document.active_sheet()?;
        let cell = sheet.cell(row, col)?;
        if !llm_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = llm_request_for_sheet(&input, sheet).ok().flatten()?;
        let cache_key = llm_cache_key(&input, &request);

        if let Some(cached) = cached_text(&self.document, &cache_key) {
            if let Some(sheet) = self.document.active_sheet_mut() {
                sheet.set_cell_value_with_cache(
                    row,
                    col,
                    input,
                    CellValue::Cached(cached),
                    Some(cache_key),
                );
            }
            self.status = format!("Used cached LLM result for {}", cell_key(row, col));
            return None;
        }

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_value_with_cache(
                row,
                col,
                input.clone(),
                CellValue::FormulaPending {
                    message: "Running LLM...".to_string(),
                },
                Some(cache_key.clone()),
            );
        }
        self.status = format!("Running LLM for {}", cell_key(row, col));

        Some((row, col, input, cache_key, request))
    }

    pub(crate) fn finish_llm_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<String>,
    ) {
        let value = match result {
            Ok(output) => {
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Ready,
                        value: CachedValue::Text(output.clone()),
                    },
                );
                self.status = format!("LLM completed for {}", cell_key(row, col));
                CellValue::Cached(output)
            }
            Err(error) => {
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Failed {
                            message: error.to_string(),
                        },
                        value: CachedValue::Text(String::new()),
                    },
                );
                self.status = format!("LLM failed for {}", cell_key(row, col));
                CellValue::Error(error.to_string())
            }
        };

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
            self.dirty = true;
        }
    }

    pub(crate) fn set_selected_formula(&mut self, value: String) {
        let (row, col) = self.selected_cell;
        self.completions_open = should_show_completions(&value);
        self.formula_input = value.clone();
        self.set_cell_input(row, col, value);
    }

    pub(crate) fn set_formula_buffer(&mut self, value: String) {
        self.completions_open = should_show_completions(&value);
        self.formula_input = value;
    }

    pub(crate) fn commit_formula_buffer(&mut self) {
        let (row, col) = self.selected_cell;
        self.set_cell_input(row, col, self.formula_input.clone());
    }

    pub(crate) fn select_or_insert_cell_reference(&mut self, row: usize, col: usize) {
        if formula_accepts_cell_reference(&self.formula_input) && self.selected_cell != (row, col) {
            self.insert_cell_reference(row, col);
        } else {
            self.set_selected_cell(row, col);
        }
    }

    pub(crate) fn move_selection(&mut self, row_delta: isize, col_delta: isize) -> (usize, usize) {
        let (row, col) = self.selected_cell;
        let next_row = row.saturating_add_signed(row_delta);
        let next_col = col.saturating_add_signed(col_delta);
        self.set_selected_cell(next_row, next_col);
        self.selected_cell
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
        self.completions_open = should_show_completions(&self.formula_input);
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
        self.completions_open = should_show_completions(&self.formula_input);
        self.status = format!(
            "Pasted {} cells at {}",
            row_count * col_count,
            cell_key(target_row, target_col)
        );
    }

    pub(crate) fn insert_cell_reference(&mut self, row: usize, col: usize) {
        let reference = absolute_column_reference(row, col);
        let separator = formula_reference_separator(&self.formula_input);
        let formula = format!("{}{}{}", self.formula_input, separator, reference);
        self.set_selected_formula(formula);
        self.completions_open = false;
    }

    pub(crate) fn insert_formula(&mut self, function: FormulaFunction) {
        let (row, col) = self.selected_cell;
        self.completions_open = false;
        self.set_cell_input(row, col, function.insert_text.to_string());
    }

    pub(crate) fn new_document(&mut self) {
        self.document = CashewDocument::default();
        self.file_path = None;
        self.file_menu_open = false;
        self.dirty = false;
        self.status = "Created a new document".to_string();
        self.copied_cells = None;
        self.set_selected_cell(0, 0);
    }

    pub(crate) fn open_document(&mut self) {
        self.file_menu_open = false;

        let Some(path) = rfd::FileDialog::new()
            .add_filter("Cashew JSON", &["json"])
            .pick_file()
        else {
            self.status = "Open canceled".to_string();
            return;
        };

        match CashewDocument::load_json(&path) {
            Ok(document) => {
                self.document = document;
                self.file_path = Some(path.clone());
                self.dirty = false;
                self.status = format!("Opened {}", path.display());
                self.set_selected_cell(0, 0);
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    pub(crate) fn save_document(&mut self) {
        self.file_menu_open = false;

        let Some(path) = self.file_path.clone() else {
            self.save_document_as();
            return;
        };

        self.write_document(path);
    }

    pub(crate) fn save_document_as(&mut self) {
        self.file_menu_open = false;

        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Cashew JSON", &["json"])
            .set_file_name("cashew.json")
            .save_file()
        else {
            self.status = "Save canceled".to_string();
            return;
        };

        if path.extension().is_none() {
            path.set_extension("json");
        }

        self.write_document(path);
    }

    pub(crate) fn open_settings(&mut self) {
        self.file_menu_open = false;
        self.settings_open = true;

        match UserSettings::load_default() {
            Ok(settings) => {
                self.settings_fal_key = settings.fal_key.unwrap_or_default();
                self.settings_path = settings_path().ok();
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    pub(crate) fn close_settings(&mut self) {
        self.settings_open = false;
    }

    pub(crate) fn set_settings_fal_key(&mut self, value: String) {
        self.settings_fal_key = value;
    }

    pub(crate) fn save_settings(&mut self) {
        let fal_key = match self.settings_fal_key.trim() {
            "" => None,
            key => Some(key.to_string()),
        };

        match (UserSettings { fal_key }).save_default() {
            Ok(path) => {
                self.settings_path = Some(path.clone());
                self.settings_open = false;
                self.status = format!("Saved settings to {}", path.display());
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    pub(crate) fn update_resize(&mut self, coordinate: i32) {
        let Some(resizing) = self.resizing else {
            return;
        };

        let delta = coordinate - resizing.start;
        let size = resizing.original + delta;

        if let Some(sheet) = self.document.active_sheet_mut() {
            match resizing.kind {
                ResizeKind::Column => {
                    let width = size.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH) as u16;
                    sheet.set_column_width(resizing.index, width);
                }
                ResizeKind::Row => {
                    let height = size.clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT) as u16;
                    sheet.set_row_height(resizing.index, height);
                }
            }
            self.dirty = true;
        }
    }

    fn write_document(&mut self, path: PathBuf) {
        match self.document.save_json(&path) {
            Ok(()) => {
                self.file_path = Some(path.clone());
                self.dirty = false;
                self.status = format!("Saved {}", path.display());
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }

    fn ensure_work_area(&mut self, rows: usize, cols: usize) {
        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.ensure_size(rows.max(MIN_VISIBLE_ROWS), cols.max(MIN_VISIBLE_COLS));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionRange {
    pub(crate) start_row: usize,
    pub(crate) start_col: usize,
    pub(crate) end_row: usize,
    pub(crate) end_col: usize,
}

impl SelectionRange {
    fn new(anchor: (usize, usize), end: (usize, usize)) -> Self {
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

fn cell_input(document: &CashewDocument, row: usize, col: usize) -> String {
    document
        .active_sheet()
        .and_then(|sheet| sheet.cell(row, col))
        .map(|cell| cell.input.clone())
        .unwrap_or_default()
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

fn load_settings_for_ui() -> (String, Option<PathBuf>, Option<String>) {
    let path = settings_path().ok();
    match UserSettings::load_default() {
        Ok(settings) => (settings.fal_key.unwrap_or_default(), path, None),
        Err(error) => (
            String::new(),
            path,
            Some(format!("Could not load settings: {error}")),
        ),
    }
}

pub(crate) fn should_show_completions(input: &str) -> bool {
    let trimmed = input.trim_start();
    trimmed.starts_with('=') && !trimmed.contains('(')
}

fn absolute_column_reference(row: usize, col: usize) -> String {
    format!("${}{}", column_name(col), row + 1)
}

fn formula_reference_separator(input: &str) -> &'static str {
    let trimmed = input.trim_end();

    if trimmed.ends_with('=')
        || trimmed.ends_with('(')
        || trimmed.ends_with(',')
        || trimmed.ends_with('+')
        || trimmed.ends_with('-')
        || trimmed.ends_with('*')
        || trimmed.ends_with('/')
        || trimmed.is_empty()
    {
        ""
    } else {
        "+"
    }
}

fn formula_accepts_cell_reference(input: &str) -> bool {
    let trimmed = input.trim_end();

    trimmed.starts_with('=')
        && (trimmed.ends_with('=')
            || trimmed.ends_with('(')
            || trimmed.ends_with(',')
            || trimmed.ends_with('+')
            || trimmed.ends_with('-')
            || trimmed.ends_with('*')
            || trimmed.ends_with('/'))
}

fn normalize_editor_text(value: &str) -> String {
    value.replace(['“', '”'], "\"").replace(['‘', '’'], "'")
}

async fn run_openrouter_request(request: &OpenRouterRequest) -> anyhow::Result<String> {
    let client = OpenRouterClient::from_settings_or_env()?;
    client.run(request).await.map(|response| response.output)
}

fn llm_cache_key(input: &str, request: &OpenRouterRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn cached_text(document: &CashewDocument, cache_key: &str) -> Option<String> {
    let entry = document.cache.get(cache_key)?;
    if entry.status != CacheStatus::Ready {
        return None;
    }

    match &entry.value {
        CachedValue::Text(value) => Some(value.clone()),
        CachedValue::Json(_) | CachedValue::MediaAsset(_) => None,
    }
}

fn llm_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => message.ends_with("request is ready to run"),
        CellValue::Error(_) => true,
        CellValue::Empty | CellValue::Text(_) | CellValue::Cached(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_reference_insertion_only_captures_operand_positions() {
        assert!(formula_accepts_cell_reference("="));
        assert!(formula_accepts_cell_reference("=A1+"));
        assert!(formula_accepts_cell_reference("=SUM("));

        assert!(!formula_accepts_cell_reference("=$A1"));
        assert!(!formula_accepts_cell_reference("=1+1"));
        assert!(!formula_accepts_cell_reference("plain text"));
    }

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
}

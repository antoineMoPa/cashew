use std::path::PathBuf;

use crate::backend::document::{CashewDocument, cell_key, column_name};
use crate::backend::formulas::FormulaFunction;
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
        self.formula_input = cell_input(&self.document, row, col);
        self.completions_open = should_show_completions(&self.formula_input);
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

    pub(crate) fn set_selected_formula(&mut self, value: String) {
        let (row, col) = self.selected_cell;
        self.completions_open = should_show_completions(&value);
        self.formula_input = value.clone();
        self.set_cell_input(row, col, value);
    }

    pub(crate) fn select_or_insert_cell_reference(&mut self, row: usize, col: usize) {
        if formula_accepts_cell_reference(&self.formula_input) && self.selected_cell != (row, col) {
            self.insert_cell_reference(row, col);
        } else {
            self.set_selected_cell(row, col);
        }
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

fn cell_input(document: &CashewDocument, row: usize, col: usize) -> String {
    document
        .active_sheet()
        .and_then(|sheet| sheet.cell(row, col))
        .map(|cell| cell.input.clone())
        .unwrap_or_default()
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
}

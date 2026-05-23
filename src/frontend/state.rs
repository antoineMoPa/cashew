use std::{path::PathBuf, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::WritableExt;

use crate::backend::cache::{
    CacheEntry, CacheStatus, CachedValue, MediaAsset, MediaType, stable_cache_key,
};
use crate::backend::document::{CashewDocument, CellValue, cell_key, column_name};
use crate::backend::formula_implementations::{
    generate_image_request_for_sheet, llm_request_for_sheet,
};
use crate::backend::formulas::FormulaFunction;
use crate::backend::providers::fal_image::{FalImageClient, GenerateImageRequest};
use crate::backend::providers::openrouter::{OpenRouterClient, OpenRouterRequest};
use crate::backend::settings::{UserSettings, settings_path};

const MIN_COLUMN_WIDTH: i32 = 72;
const MIN_ROW_HEIGHT: i32 = 24;
const GENERATED_IMAGE_COLUMN_WIDTH: u16 = 180;
const GENERATED_IMAGE_ROW_HEIGHT: u16 = 160;

pub(crate) const MIN_VISIBLE_ROWS: usize = 100;
pub(crate) const MIN_VISIBLE_COLS: usize = 26;
pub(crate) const GROWTH_BUFFER_ROWS: usize = 25;
pub(crate) const GROWTH_BUFFER_COLS: usize = 10;

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    pub(crate) document: CashewDocument,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) file_menu_open: bool,
    pub(crate) dirty: bool,
    pub(crate) status: String,
    pub(crate) selected_cell: (usize, usize),
    pub(crate) selected_cell_mode: CellInteractionMode,
    pub(crate) editing_cell: Option<(usize, usize)>,
    pub(crate) selection_anchor: (usize, usize),
    pub(crate) selection_end: (usize, usize),
    pub(crate) selecting: bool,
    pub(crate) formula_input: String,
    pub(crate) formula_input_revision: u64,
    pub(crate) resizing: Option<ResizeDrag>,
    pub(crate) completions_open: bool,
    pub(crate) completion_index: usize,
    pub(crate) settings_open: bool,
    pub(crate) settings_fal_key: String,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) bottom_panel_tab: BottomPanelTab,
    pub(crate) network_calls: Vec<NetworkCallRecord>,
    next_network_call_id: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellInteractionMode {
    Display,
    Value,
    FormulaEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomPanelTab {
    FunctionDocs,
    NetworkCalls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetworkCallRecord {
    pub(crate) id: u64,
    pub(crate) cell: String,
    pub(crate) function_name: String,
    pub(crate) provider: String,
    pub(crate) url: String,
    pub(crate) status: NetworkCallStatus,
    pub(crate) request_body: serde_json::Value,
    pub(crate) image_inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkCallStatus {
    Running,
    Completed,
    Failed,
}

impl AppState {
    pub(crate) fn new() -> Self {
        let (mut document, file_path, document_status) = load_default_document_for_ui();
        if let Some(sheet) = document.active_sheet_mut() {
            sheet.ensure_size(MIN_VISIBLE_ROWS, MIN_VISIBLE_COLS);
        }
        let formula_input = cell_input(&document, 0, 0);
        let (settings_fal_key, settings_path, settings_status) = load_settings_for_ui();

        Self {
            document,
            file_path,
            file_menu_open: false,
            dirty: false,
            status: settings_status
                .or(document_status)
                .unwrap_or_else(|| "Ready".to_string()),
            selected_cell: (0, 0),
            selected_cell_mode: CellInteractionMode::Display,
            editing_cell: None,
            selection_anchor: (0, 0),
            selection_end: (0, 0),
            selecting: false,
            formula_input,
            formula_input_revision: 0,
            resizing: None,
            completions_open: false,
            completion_index: 0,
            settings_open: false,
            settings_fal_key,
            settings_path,
            bottom_panel_tab: BottomPanelTab::FunctionDocs,
            network_calls: Vec::new(),
            next_network_call_id: 1,
        }
    }

    pub(crate) fn set_selected_cell(&mut self, row: usize, col: usize) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        self.selected_cell = (row, col);
        self.selected_cell_mode = CellInteractionMode::Display;
        self.editing_cell = None;
        self.selection_anchor = (row, col);
        self.selection_end = (row, col);
        self.refresh_formula_input_from_cell(row, col);
        self.completions_open = false;
        self.completion_index = 0;
    }

    pub(crate) fn set_cell_input(&mut self, row: usize, col: usize, value: String) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_input(row, col, value.clone());
            self.dirty = true;
            self.status = format!("Edited {}", cell_key(row, col));
        }

        if self.selected_cell == (row, col) {
            self.completions_open = false;
            self.completion_index = 0;
            self.formula_input = value;
            self.formula_input_revision = self.formula_input_revision.wrapping_add(1);
        }
    }

    pub(crate) fn set_editing_cell_input(&mut self, row: usize, col: usize, value: String) {
        self.editing_cell = Some((row, col));
        self.selected_cell = (row, col);
        self.selected_cell_mode = CellInteractionMode::FormulaEdit;
        self.set_cell_input(row, col, value);
    }

    pub(crate) fn cell_is_being_edited(&self, row: usize, col: usize) -> bool {
        self.selected_cell == (row, col)
            && self.selected_cell_mode == CellInteractionMode::FormulaEdit
            && self.editing_cell == Some((row, col))
    }

    pub(crate) fn finish_cell_edit(&mut self, row: usize, col: usize) {
        if self.editing_cell == Some((row, col)) {
            self.editing_cell = None;
            if self.selected_cell == (row, col) {
                self.selected_cell_mode = CellInteractionMode::Display;
            }
        }
    }

    pub(crate) fn finish_formula_edit(&mut self) {
        self.editing_cell = None;
        self.selected_cell_mode = CellInteractionMode::Display;
        self.completions_open = false;
        self.completion_index = 0;
    }

    pub(crate) async fn run_llm_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: OpenRouterRequest,
        network_call_id: u64,
    ) {
        let result = run_openrouter_request(&request).await;

        state.with_mut(|state| {
            state.finish_network_call(network_call_id, result.is_ok());
            state.finish_llm_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) async fn run_generate_image_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: GenerateImageRequest,
        network_call_id: u64,
    ) {
        let result = run_generate_image_request(&request).await;

        state.with_mut(|state| {
            state.finish_network_call(network_call_id, result.is_ok());
            state.finish_generate_image_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) fn prepare_llm_for_cell(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, OpenRouterRequest, u64)> {
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
        let network_call_id = self.push_network_call(NetworkCallRecord::for_llm(
            self.next_network_call_id,
            row,
            col,
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
    }

    pub(crate) fn prepare_generate_image_for_cell(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, GenerateImageRequest, u64)> {
        let sheet = self.document.active_sheet()?;
        let cell = sheet.cell(row, col)?;
        if !generate_image_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = match generate_image_request_for_sheet(&input, sheet) {
            Ok(Some(request)) => request,
            Ok(None) => return None,
            Err(error) => {
                if let Some(sheet) = self.document.active_sheet_mut() {
                    sheet.set_cell_value_with_cache(row, col, input, CellValue::Error(error), None);
                }
                self.dirty = true;
                return None;
            }
        };
        let cache_key = generate_image_cache_key(&input, &request);

        if let Some(cached) = cached_media_cell_value(&self.document, &cache_key) {
            if let Some(sheet) = self.document.active_sheet_mut() {
                sheet.set_cell_value_with_cache(
                    row,
                    col,
                    input,
                    CellValue::Cached(cached),
                    Some(cache_key),
                );
            }
            self.status = format!("Used cached image result for {}", cell_key(row, col));
            return None;
        }

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_value_with_cache(
                row,
                col,
                input.clone(),
                CellValue::FormulaPending {
                    message: "Running image generation...".to_string(),
                },
                Some(cache_key.clone()),
            );
        }
        self.status = format!("Running image generation for {}", cell_key(row, col));
        let network_call_id = self.push_network_call(NetworkCallRecord::for_generate_image(
            self.next_network_call_id,
            row,
            col,
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
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
            sheet.recalculate_formulas();
            self.dirty = true;
        }
    }

    pub(crate) fn finish_generate_image_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<MediaAsset>,
    ) {
        let value = match result {
            Ok(asset) => {
                let uri = asset.uri.clone();
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Ready,
                        value: CachedValue::MediaAsset(asset),
                    },
                );
                self.status = format!("Image generation completed for {}", cell_key(row, col));
                CellValue::Cached(uri)
            }
            Err(error) => {
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Failed {
                            message: error.to_string(),
                        },
                        value: CachedValue::Json(serde_json::Value::Null),
                    },
                );
                self.status = format!("Image generation failed for {}", cell_key(row, col));
                CellValue::Error(error.to_string())
            }
        };

        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
            if matches!(
                sheet.cell(row, col).map(|cell| &cell.value),
                Some(CellValue::Cached(_))
            ) {
                let width = sheet.column_width(col).max(GENERATED_IMAGE_COLUMN_WIDTH);
                let height = sheet.row_height(row).max(GENERATED_IMAGE_ROW_HEIGHT);
                sheet.set_column_width(col, width);
                sheet.set_row_height(row, height);
            }
            sheet.recalculate_formulas();
            self.dirty = true;
        }
    }

    pub(crate) fn set_selected_formula(&mut self, value: String) {
        let (row, col) = self.selected_cell;
        self.selected_cell_mode = CellInteractionMode::FormulaEdit;
        self.editing_cell = Some((row, col));
        self.completions_open = should_show_completions(&value);
        self.completion_index = 0;
        self.formula_input = value.clone();
        self.set_cell_input(row, col, value);
    }

    pub(crate) fn set_formula_buffer(&mut self, value: String) {
        self.completions_open = should_show_completions(&value);
        self.completion_index = 0;
        self.formula_input = value;
    }

    pub(crate) fn refresh_formula_input_from_cell(&mut self, row: usize, col: usize) {
        self.formula_input = cell_input(&self.document, row, col);
        self.formula_input_revision = self.formula_input_revision.wrapping_add(1);
    }

    pub(crate) fn move_completion_selection(&mut self, delta: isize, completion_count: usize) {
        if completion_count == 0 {
            self.completion_index = 0;
            return;
        }

        self.completion_index = self
            .completion_index
            .saturating_add_signed(delta)
            .min(completion_count - 1);
    }

    pub(crate) fn commit_formula_buffer(&mut self) {
        let (row, col) = self.selected_cell;
        self.set_cell_input(row, col, self.formula_input.clone());
    }

    fn commit_formula_buffer_if_changed(&mut self) {
        let (row, col) = self.selected_cell;
        if cell_input(&self.document, row, col) != self.formula_input {
            self.set_cell_input(row, col, self.formula_input.clone());
        }
    }

    pub(crate) fn select_or_insert_cell_reference(&mut self, row: usize, col: usize) {
        if formula_accepts_cell_reference(&self.formula_input) && self.selected_cell != (row, col) {
            self.insert_cell_reference(row, col);
        } else if self.selected_cell == (row, col)
            && self.selected_cell_mode == CellInteractionMode::FormulaEdit
        {
            self.editing_cell = Some((row, col));
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

    pub(crate) fn insert_cell_reference(&mut self, row: usize, col: usize) {
        let reference = absolute_column_reference(row, col);
        let formula = if let Some(marker) = formula_reference_marker(&self.formula_input) {
            let mut formula = self.formula_input.clone();
            formula.replace_range(marker..marker + 1, &reference);
            formula
        } else {
            let separator = formula_reference_separator(&self.formula_input);
            format!("{}{}{}", self.formula_input, separator, reference)
        };
        self.set_selected_formula(formula);
        self.completions_open = false;
    }

    pub(crate) fn insert_formula(&mut self, function: FormulaFunction) {
        let (row, col) = self.selected_cell;
        self.completions_open = false;
        self.set_cell_input(row, col, function.insert_text.to_string());
        self.selected_cell_mode = CellInteractionMode::FormulaEdit;
        self.editing_cell = Some((row, col));
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
        self.commit_formula_buffer_if_changed();

        let Some(path) = self.file_path.clone() else {
            self.save_document_as();
            return;
        };

        self.write_document(path);
    }

    pub(crate) fn save_document_as(&mut self) {
        self.file_menu_open = false;
        self.commit_formula_buffer_if_changed();

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

    pub(crate) fn set_bottom_panel_tab(&mut self, tab: BottomPanelTab) {
        self.bottom_panel_tab = tab;
    }

    fn push_network_call(&mut self, record: NetworkCallRecord) -> u64 {
        let id = record.id;
        self.next_network_call_id = self.next_network_call_id.wrapping_add(1).max(1);
        self.network_calls.push(record);
        id
    }

    fn finish_network_call(&mut self, id: u64, ok: bool) {
        if let Some(record) = self.network_calls.iter_mut().find(|record| record.id == id) {
            record.status = if ok {
                NetworkCallStatus::Completed
            } else {
                NetworkCallStatus::Failed
            };
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
                    let width = size.max(MIN_COLUMN_WIDTH).min(u16::MAX as i32) as u16;
                    sheet.set_column_width(resizing.index, width);
                }
                ResizeKind::Row => {
                    let height = size.max(MIN_ROW_HEIGHT).min(u16::MAX as i32) as u16;
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

    pub(crate) fn ensure_work_area(&mut self, rows: usize, cols: usize) {
        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.ensure_size(rows.max(MIN_VISIBLE_ROWS), cols.max(MIN_VISIBLE_COLS));
        }
    }

    pub(crate) fn selected_cell_mode_for(&self, row: usize, col: usize) -> CellInteractionMode {
        if self.selected_cell == (row, col) {
            self.selected_cell_mode
        } else {
            CellInteractionMode::Display
        }
    }

    pub(crate) fn advance_cell_mode(&mut self, row: usize, col: usize) {
        if self.selected_cell != (row, col) {
            self.set_selected_cell(row, col);
            return;
        }

        self.selected_cell_mode = match self.selected_cell_mode {
            CellInteractionMode::Display => CellInteractionMode::Value,
            CellInteractionMode::Value | CellInteractionMode::FormulaEdit => {
                self.editing_cell = Some((row, col));
                CellInteractionMode::FormulaEdit
            }
        };
    }
}

impl NetworkCallRecord {
    fn for_llm(id: u64, row: usize, col: usize, request: &OpenRouterRequest) -> Self {
        Self {
            id,
            cell: cell_key(row, col),
            function_name: "LLM".to_string(),
            provider: "fal.openrouter".to_string(),
            url: crate::backend::providers::openrouter::ENDPOINT.to_string(),
            status: NetworkCallStatus::Running,
            request_body: serde_json::to_value(request).unwrap_or(serde_json::Value::Null),
            image_inputs: Vec::new(),
        }
    }

    fn for_generate_image(id: u64, row: usize, col: usize, request: &GenerateImageRequest) -> Self {
        let image_inputs = image_inputs_from_body(&request.input);

        Self {
            id,
            cell: cell_key(row, col),
            function_name: "GENERATEIMAGE".to_string(),
            provider: "fal.image".to_string(),
            url: format!("https://queue.fal.run/{}", request.endpoint),
            status: NetworkCallStatus::Running,
            request_body: request_body_without_images(&request.input),
            image_inputs,
        }
    }
}

fn image_inputs_from_body(body: &serde_json::Value) -> Vec<String> {
    let mut images = Vec::new();

    if let Some(image_url) = body.get("image_url").and_then(|value| value.as_str()) {
        images.push(image_url.to_string());
    }

    if let Some(image_urls) = body.get("image_urls").and_then(|value| value.as_array()) {
        images.extend(
            image_urls
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::to_string),
        );
    }

    images
}

fn request_body_without_images(body: &serde_json::Value) -> serde_json::Value {
    let mut sanitized = body.clone();
    if let Some(object) = sanitized.as_object_mut() {
        if let Some(value) = object.get_mut("image_url") {
            *value = serde_json::Value::String("<shown in Images>".to_string());
        }
        if let Some(value) = object.get_mut("image_urls") {
            let count = value.as_array().map(Vec::len).unwrap_or(0);
            *value = serde_json::Value::String(format!("<{count} images shown in Images>"));
        }
    }
    sanitized
}

fn load_default_document_for_ui() -> (CashewDocument, Option<PathBuf>, Option<String>) {
    for (index, path) in default_document_candidates().into_iter().enumerate() {
        if let Ok(document) = CashewDocument::load_json(&path) {
            let message = if index == 0 && home_dir_path().is_some() {
                "Opened ~/default_cashew.json".to_string()
            } else {
                "Opened default_cashew.json".to_string()
            };
            return (document, Some(path), Some(message));
        }
    }

    (
        CashewDocument::default(),
        None,
        Some("Could not open ~/default_cashew.json or default_cashew.json".to_string()),
    )
}

fn default_document_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home_dir) = home_dir_path() {
        candidates.push(home_dir.join("default_cashew.json"));
    }

    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("default_cashew.json"));
    candidates
}

fn home_dir_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub(crate) fn cell_input(document: &CashewDocument, row: usize, col: usize) -> String {
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

pub(crate) fn formula_accepts_cell_reference(input: &str) -> bool {
    let trimmed = input.trim_end();

    trimmed.starts_with('=')
        && (formula_reference_marker(input).is_some()
            || trimmed.ends_with('=')
            || trimmed.ends_with('(')
            || trimmed.ends_with(',')
            || trimmed.ends_with('+')
            || trimmed.ends_with('-')
            || trimmed.ends_with('*')
            || trimmed.ends_with('/'))
}

fn formula_reference_marker(input: &str) -> Option<usize> {
    if !input.trim_start().starts_with('=') {
        return None;
    }

    let bytes = input.as_bytes();
    let mut markers = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'"' || bytes[index] == b'\'' {
            index = skip_quoted_text(bytes, index);
            continue;
        }

        if bytes[index] == b'$' && !dollar_starts_cell_reference(&input[index..]) {
            markers.push(index);
        }

        index += 1;
    }

    markers.pop()
}

fn dollar_starts_cell_reference(input: &str) -> bool {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'$') {
        return false;
    }

    let mut index = 1;
    let col_start = index;
    while matches!(bytes.get(index), Some(b'A'..=b'Z') | Some(b'a'..=b'z')) {
        index += 1;
    }

    if index == col_start || index - col_start > 3 {
        return false;
    }

    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }

    let row_start = index;
    while matches!(bytes.get(index), Some(b'0'..=b'9')) {
        index += 1;
    }

    index > row_start
}

fn skip_quoted_text(bytes: &[u8], start: usize) -> usize {
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

async fn run_openrouter_request(request: &OpenRouterRequest) -> anyhow::Result<String> {
    let client = OpenRouterClient::from_settings_or_env()?;
    client.run(request).await.map(|response| response.output)
}

async fn run_generate_image_request(request: &GenerateImageRequest) -> anyhow::Result<MediaAsset> {
    eprintln!(
        "[fal.image] run_generate_image_request start model={} endpoint={} prompt_len={}",
        request.model,
        request.endpoint,
        request.prompt.len()
    );

    let client = FalImageClient::from_settings_or_env()?;
    eprintln!("[fal.image] client ready, awaiting provider response");
    let response = client.run(request).await?;
    eprintln!(
        "[fal.image] provider response received images={} beginning media persistence",
        response.images.len()
    );
    let image = response
        .images
        .first()
        .ok_or_else(|| anyhow::anyhow!("fal image response did not include any images"))?;
    let mut metadata = serde_json::json!({
        "model": request.model,
        "endpoint": request.endpoint,
        "request": request.input,
        "response": response,
    });

    let data_uri = match persist_media_data(&image.url, image.content_type.as_deref()).await {
        Ok(data_uri) => Some(data_uri),
        Err(error) => {
            eprintln!(
                "[fal.image] media persistence failed uri={} error={}",
                image.url, error
            );
            metadata["persistence_warning"] =
                serde_json::Value::String(format!("Could not embed media data: {error}"));
            None
        }
    };

    eprintln!(
        "[fal.image] image generation completed uri={} data_uri_embedded={}",
        image.url,
        data_uri.is_some()
    );

    Ok(MediaAsset {
        provider: "fal.image".to_string(),
        media_type: MediaType::Image,
        uri: image.url.clone(),
        data_uri,
        metadata,
    })
}

async fn persist_media_data(uri: &str, content_type_hint: Option<&str>) -> anyhow::Result<String> {
    eprintln!(
        "[fal.image] persist_media_data start uri={} content_type_hint={:?}",
        uri, content_type_hint
    );

    if uri.starts_with("data:") {
        eprintln!("[fal.image] persist_media_data short-circuit data uri");
        return Ok(uri.to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()?;
    let response = client.get(uri).send().await?.error_for_status()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| content_type_hint.map(str::to_string))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = response.bytes().await?;

    eprintln!(
        "[fal.image] persist_media_data fetched uri={} bytes={} content_type={}",
        uri,
        bytes.len(),
        content_type
    );

    Ok(format!(
        "data:{content_type};base64,{}",
        STANDARD.encode(bytes)
    ))
}

fn llm_cache_key(input: &str, request: &OpenRouterRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn generate_image_cache_key(input: &str, request: &GenerateImageRequest) -> String {
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

fn cached_media_cell_value(document: &CashewDocument, cache_key: &str) -> Option<String> {
    let entry = document.cache.get(cache_key)?;
    if entry.status != CacheStatus::Ready {
        return None;
    }

    match &entry.value {
        CachedValue::MediaAsset(asset) => Some(asset.uri.clone()),
        CachedValue::Text(_) | CachedValue::Json(_) => None,
    }
}

fn llm_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => {
            message == "fal.openrouter request is ready to run"
        }
        CellValue::Error(_) => true,
        CellValue::Empty | CellValue::Text(_) | CellValue::Cached(_) => false,
    }
}

fn generate_image_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => message == "fal.image request is ready to run",
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
        assert!(formula_accepts_cell_reference("=LLM($, \"model\")"));
        assert!(formula_accepts_cell_reference(
            "=LLM($ prompt text, \"model\")"
        ));

        assert!(!formula_accepts_cell_reference("=$A1"));
        assert!(!formula_accepts_cell_reference("=LLM(\"price is $5\")"));
        assert!(!formula_accepts_cell_reference("=1+1"));
        assert!(!formula_accepts_cell_reference("plain text"));
    }

    #[test]
    fn cell_reference_insertion_replaces_dollar_marker_before_trailing_text() {
        let mut state = AppState::new();
        state.set_selected_formula("=LLM($, \"model\")".to_string());

        state.insert_cell_reference(1, 2);

        assert_eq!(state.formula_input, "=LLM($C2, \"model\")");
    }

    #[test]
    fn cached_media_cell_value_prefers_provider_uri_over_embedded_data() {
        let mut document = CashewDocument::new("Cache");
        document.cache.insert(
            "image-key".to_string(),
            CacheEntry {
                key: "image-key".to_string(),
                status: CacheStatus::Ready,
                value: CachedValue::MediaAsset(MediaAsset {
                    provider: "fal.image".to_string(),
                    media_type: MediaType::Image,
                    uri: "https://example.com/ref.png".to_string(),
                    data_uri: Some("data:image/png;base64,abc".to_string()),
                    metadata: serde_json::Value::Null,
                }),
            },
        );

        assert_eq!(
            cached_media_cell_value(&document, "image-key"),
            Some("https://example.com/ref.png".to_string())
        );
    }

    #[test]
    fn finish_formula_edit_leaves_formula_edit_mode() {
        let mut state = AppState::new();
        state.set_selected_formula("=SUM(".to_string());
        state.completions_open = true;
        state.completion_index = 2;

        state.finish_formula_edit();

        assert_eq!(state.selected_cell_mode, CellInteractionMode::Display);
        assert_eq!(state.editing_cell, None);
        assert!(!state.completions_open);
        assert_eq!(state.completion_index, 0);
    }

    #[test]
    fn generate_image_network_record_extracts_and_sanitizes_images() {
        let request = GenerateImageRequest::new(
            "edit it",
            "openai/gpt-image-2",
            None,
            vec![
                "data:image/png;base64,abc".to_string(),
                "https://example.com/ref.png".to_string(),
            ],
        )
        .unwrap();

        let record = NetworkCallRecord::for_generate_image(7, 1, 2, &request);

        assert_eq!(record.cell, "C2");
        assert_eq!(record.url, "https://queue.fal.run/openai/gpt-image-2/edit");
        assert_eq!(record.image_inputs.len(), 2);
        assert_eq!(
            record.request_body["image_urls"],
            "<2 images shown in Images>"
        );
    }

    #[test]
    fn openrouter_network_record_does_not_include_auth_data() {
        let request = OpenRouterRequest::new("hello");

        let record = NetworkCallRecord::for_llm(3, 0, 0, &request);
        let body = serde_json::to_string(&record.request_body).unwrap();

        assert_eq!(record.url, crate::backend::providers::openrouter::ENDPOINT);
        assert!(!body.contains("Authorization"));
        assert!(!body.contains("Key "));
    }
}

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use dioxus::prelude::WritableExt;

use super::components::ProviderWork;
use super::selection::range_label;
use super::selection::{CopiedCells, SelectionRange};
use crate::backend::cache::{
    CacheEntry, CacheStatus, CachedValue, MediaAsset, MediaType, stable_cache_key,
};
use crate::backend::document::{CashewDocument, Cell, CellValue, Sheet, cell_key, column_name};
use crate::backend::fill::FillRange;
use crate::backend::formula_implementations::{
    LlmOutputMode, LlmRequest, concatenate_video_inputs_for_sheet,
    generate_image_request_for_sheet, generate_video_request_for_sheet, llm_request_for_sheet,
    segment_request_for_sheet,
};
use crate::backend::formulas::FormulaFunction;
use crate::backend::providers::fal_image::{FalImageClient, GenerateImageRequest};
use crate::backend::providers::fal_segment::{FalSegmentClient, SegmentImageRequest};
use crate::backend::providers::fal_video::{FalVideoClient, GenerateVideoRequest};
use crate::backend::providers::openrouter::{OpenRouterClient, OpenRouterRequest};
use crate::backend::settings::{UserSettings, settings_path};

const MIN_COLUMN_WIDTH: i32 = 72;
const MIN_ROW_HEIGHT: i32 = 24;
const MIN_BOTTOM_PANEL_HEIGHT: i32 = 120;
const DEFAULT_BOTTOM_PANEL_HEIGHT: i32 = 220;
const GENERATED_IMAGE_COLUMN_WIDTH: u16 = 180;
const GENERATED_IMAGE_ROW_HEIGHT: u16 = 160;
const GENERATED_VIDEO_COLUMN_WIDTH: u16 = 240;
const GENERATED_VIDEO_ROW_HEIGHT: u16 = 180;

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
    pub(crate) fill_dragging: Option<FillDrag>,
    pub(crate) formula_input: String,
    pub(crate) formula_input_revision: u64,
    pub(crate) resizing: Option<ResizeDrag>,
    pub(crate) bottom_panel_height: i32,
    pub(crate) completions_open: bool,
    pub(crate) completion_index: usize,
    pub(crate) settings_open: bool,
    pub(crate) settings_fal_key: String,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) bottom_panel_tab: BottomPanelTab,
    pub(crate) network_calls: Vec<NetworkCallRecord>,
    pub(crate) pending_provider_calls: Vec<QueuedProviderCall>,
    pub(crate) copied_cells: Option<CopiedCells>,
    next_network_call_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResizeDrag {
    pub(crate) kind: ResizeKind,
    pub(crate) index: usize,
    pub(crate) start: i32,
    pub(crate) original: i32,
}

#[derive(Debug, Clone)]
pub(crate) struct FillDrag {
    pub(crate) source: FillRange,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResizeKind {
    Column,
    Row,
    BottomPanel,
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

#[derive(Debug, Clone)]
pub(crate) struct QueuedProviderCall {
    pub(crate) work: ProviderWork,
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
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkCallStatus {
    PendingApproval,
    Running,
    Completed,
    Failed,
}

impl AppState {
    pub(crate) fn new() -> Self {
        let (mut document, file_path, document_status) = load_default_document_for_ui();
        document
            .sheet_mut()
            .ensure_size(MIN_VISIBLE_ROWS, MIN_VISIBLE_COLS);
        let formula_input = cell_input(&document, 0, 0);
        let (settings_fal_key, settings_path, settings_status) = load_settings_for_ui();

        let mut state = Self {
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
            fill_dragging: None,
            formula_input,
            formula_input_revision: 0,
            resizing: None,
            bottom_panel_height: DEFAULT_BOTTOM_PANEL_HEIGHT,
            completions_open: false,
            completion_index: 0,
            settings_open: false,
            settings_fal_key,
            settings_path,
            bottom_panel_tab: BottomPanelTab::FunctionDocs,
            network_calls: Vec::new(),
            pending_provider_calls: Vec::new(),
            copied_cells: None,
            next_network_call_id: 1,
        };
        state.rebuild_pending_provider_calls_from_document();
        state
    }

    pub(crate) fn set_selected_cell(&mut self, row: usize, col: usize) {
        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        self.fill_dragging = None;
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

        self.document.sheet_mut().set_cell_input(row, col, value.clone());
        self.dirty = true;
        self.status = format!("Edited {}", cell_key(row, col));

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

    pub(crate) async fn run_openrouter_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: LlmRequest,
        network_call_id: u64,
    ) {
        let result = run_openrouter_request(&request.request).await;

        state.with_mut(|state| {
            let error_message = result.as_ref().err().map(ToString::to_string);
            state.finish_network_call(network_call_id, result.is_ok(), error_message);
            state.finish_openrouter_for_cell(
                row,
                col,
                input,
                cache_key,
                request,
                result,
            );
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
            let error_message = result.as_ref().err().map(ToString::to_string);
            state.finish_network_call(network_call_id, result.is_ok(), error_message);
            state.finish_generate_image_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) async fn run_generate_video_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: GenerateVideoRequest,
        network_call_id: u64,
    ) {
        let result = run_generate_video_request(&request).await;

        state.with_mut(|state| {
            let error_message = result.as_ref().err().map(ToString::to_string);
            state.finish_network_call(network_call_id, result.is_ok(), error_message);
            state.finish_generate_video_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) async fn run_segment_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: SegmentImageRequest,
        network_call_id: u64,
    ) {
        let result = run_segment_request(&request).await;

        state.with_mut(|state| {
            let error_message = result.as_ref().err().map(ToString::to_string);
            state.finish_network_call(network_call_id, result.is_ok(), error_message);
            state.finish_segment_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) async fn run_concatenate_video_for_cell(
        mut state: dioxus::prelude::Signal<Self>,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        video_inputs: Vec<String>,
    ) {
        let result = run_concatenate_video_request(&video_inputs).await;

        state.with_mut(|state| {
            state.finish_concatenate_video_for_cell(row, col, input, cache_key, result);
        });
    }

    pub(crate) fn queue_pending_provider_call(&mut self, work: ProviderWork) {
        self.pending_provider_calls
            .push(QueuedProviderCall { work });
        self.status = "Queued provider call for approval".to_string();
    }

    pub(crate) fn dispatch_pending_provider_calls(&mut self) -> Vec<ProviderWork> {
        let pending = std::mem::take(&mut self.pending_provider_calls);
        if pending.is_empty() {
            self.status = "No pending provider calls".to_string();
            return Vec::new();
        }

        self.status = format!("Running {} pending provider calls", pending.len());

        pending
            .into_iter()
            .map(|pending_call| {
                self.start_pending_provider_work(&pending_call.work);
                pending_call.work
            })
            .collect()
    }

    fn rebuild_pending_provider_calls_from_document(&mut self) {
        self.pending_provider_calls.clear();
        self.network_calls.clear();
        self.next_network_call_id = 1;

        let mut rebuilt = 0;
        for (row, col) in self.document.pending_provider_cells() {
            let work = self
                .prepare_generate_image_for_cell_internal(row, col, true)
                .map(ProviderWork::GenerateImage)
                .or_else(|| {
                    self.prepare_generate_video_for_cell_internal(row, col, true)
                        .map(ProviderWork::GenerateVideo)
                })
                .or_else(|| {
                    self.prepare_segment_for_cell_internal(row, col, true)
                        .map(ProviderWork::Segment)
                })
                .or_else(|| self.prepare_llm_for_cell_internal(row, col).map(ProviderWork::Llm))
                .or_else(|| {
                    self.prepare_concatenate_video_for_cell_internal(row, col)
                        .map(ProviderWork::ConcatenateVideo)
                });

            if let Some(work) = work {
                self.pending_provider_calls
                    .push(QueuedProviderCall { work });
                rebuilt += 1;
            }
        }

        if rebuilt > 0 {
            self.status = format!("Queued {rebuilt} pending provider calls from document");
        }
    }

    pub(crate) fn prepare_llm_for_cell(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, LlmRequest, u64)> {
        self.prepare_llm_for_cell_internal(row, col)
    }

    fn prepare_llm_for_cell_internal(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, LlmRequest, u64)> {
        let sheet = self.document.sheet();
        let cell = sheet.cell(row, col)?;
        if !llm_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = llm_request_for_sheet(&input, sheet).ok().flatten()?;
        let cache_key = llm_cache_key(&input, &request.request);

        if let Some(cached) = cached_text(&self.document, &cache_key) {
            self.document.finish_openrouter_for_cell(
                row,
                col,
                input,
                cache_key,
                request.clone(),
                Ok(cached),
            );
            self.dirty = true;
            self.status = format!(
                "Used cached {} result for {}",
                request.function_name,
                cell_key(row, col)
            );
            return None;
        }

        self.document.sheet_mut().set_cell_value_with_cache(
            row,
            col,
            input.clone(),
            CellValue::FormulaPending {
                message: openrouter_pending_message(request.output_mode).to_string(),
            },
            Some(cache_key.clone()),
        );
        self.status = format!(
            "Running {} for {}",
            request.function_name,
            cell_key(row, col)
        );
        let network_call_id = self.push_network_call(NetworkCallRecord::for_openrouter(
            self.next_network_call_id,
            row,
            col,
            NetworkCallStatus::Running,
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
    }

    pub(crate) fn prepare_generate_image_for_cell(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, GenerateImageRequest, u64)> {
        self.prepare_generate_image_for_cell_internal(row, col, approval_required)
    }

    fn prepare_generate_image_for_cell_internal(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, GenerateImageRequest, u64)> {
        let sheet = self.document.sheet();
        let cell = sheet.cell(row, col)?;
        if !generate_image_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = match generate_image_request_for_sheet(&input, sheet) {
            Ok(Some(request)) => request,
            Ok(None) => return None,
            Err(error) => {
                self.document
                    .sheet_mut()
                    .set_cell_value_with_cache(row, col, input, CellValue::Error(error), None);
                self.dirty = true;
                return None;
            }
        };
        let cache_key = generate_image_cache_key(&input, &request);

        if let Some(cached) = cached_media_cell_value(&self.document, &cache_key) {
            self.document.sheet_mut().set_cell_value_with_cache(
                row,
                col,
                input,
                CellValue::Cached(cached),
                Some(cache_key),
            );
            self.status = format!("Used cached image result for {}", cell_key(row, col));
            return None;
        }

        let pending_message = if approval_required {
            "fal.image request is pending approval".to_string()
        } else {
            "Running image generation...".to_string()
        };
        self.document.sheet_mut().set_cell_value_with_cache(
            row,
            col,
            input.clone(),
            CellValue::FormulaPending {
                message: pending_message,
            },
            Some(cache_key.clone()),
        );
        self.status = if approval_required {
            format!("Queued image generation for {}", cell_key(row, col))
        } else {
            format!("Running image generation for {}", cell_key(row, col))
        };
        let network_call_id = self.push_network_call(NetworkCallRecord::for_generate_image(
            self.next_network_call_id,
            row,
            col,
            if approval_required {
                NetworkCallStatus::PendingApproval
            } else {
                NetworkCallStatus::Running
            },
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
    }

    pub(crate) fn prepare_generate_video_for_cell(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, GenerateVideoRequest, u64)> {
        self.prepare_generate_video_for_cell_internal(row, col, approval_required)
    }

    fn prepare_generate_video_for_cell_internal(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, GenerateVideoRequest, u64)> {
        let sheet = self.document.sheet();
        let cell = sheet.cell(row, col)?;
        if !generate_video_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = match generate_video_request_for_sheet(&input, sheet) {
            Ok(Some(request)) => request,
            Ok(None) => return None,
            Err(error) => {
                self.document
                    .sheet_mut()
                    .set_cell_value_with_cache(row, col, input, CellValue::Error(error), None);
                self.dirty = true;
                return None;
            }
        };
        let cache_key = generate_video_cache_key(&input, &request);

        if let Some(cached) = cached_media_cell_value(&self.document, &cache_key) {
            self.document.sheet_mut().set_cell_value_with_cache(
                row,
                col,
                input,
                CellValue::Cached(cached),
                Some(cache_key),
            );
            self.status = format!("Used cached video result for {}", cell_key(row, col));
            return None;
        }

        let pending_message = if approval_required {
            "fal.video request is pending approval".to_string()
        } else {
            "Running video generation...".to_string()
        };
        self.document.sheet_mut().set_cell_value_with_cache(
            row,
            col,
            input.clone(),
            CellValue::FormulaPending {
                message: pending_message,
            },
            Some(cache_key.clone()),
        );
        self.status = if approval_required {
            format!("Queued video generation for {}", cell_key(row, col))
        } else {
            format!("Running video generation for {}", cell_key(row, col))
        };
        let network_call_id = self.push_network_call(NetworkCallRecord::for_generate_video(
            self.next_network_call_id,
            row,
            col,
            if approval_required {
                NetworkCallStatus::PendingApproval
            } else {
                NetworkCallStatus::Running
            },
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
    }

    pub(crate) fn prepare_segment_for_cell(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, SegmentImageRequest, u64)> {
        self.prepare_segment_for_cell_internal(row, col, approval_required)
    }

    fn prepare_segment_for_cell_internal(
        &mut self,
        row: usize,
        col: usize,
        approval_required: bool,
    ) -> Option<(usize, usize, String, String, SegmentImageRequest, u64)> {
        let sheet = self.document.sheet();
        let cell = sheet.cell(row, col)?;
        if !segment_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let request = match segment_request_for_sheet(&input, sheet) {
            Ok(Some(request)) => request,
            Ok(None) => return None,
            Err(error) => {
                self.document
                    .sheet_mut()
                    .set_cell_value_with_cache(row, col, input, CellValue::Error(error), None);
                self.dirty = true;
                return None;
            }
        };
        let cache_key = segment_cache_key(&input, &request);

        if let Some(cached) = cached_media_cell_value(&self.document, &cache_key) {
            self.document.sheet_mut().set_cell_value_with_cache(
                row,
                col,
                input,
                CellValue::Cached(cached),
                Some(cache_key),
            );
            self.status = format!("Used cached segmentation result for {}", cell_key(row, col));
            return None;
        }

        let pending_message = if approval_required {
            "fal.segment request is pending approval".to_string()
        } else {
            "Running segmentation...".to_string()
        };
        self.document.sheet_mut().set_cell_value_with_cache(
            row,
            col,
            input.clone(),
            CellValue::FormulaPending {
                message: pending_message,
            },
            Some(cache_key.clone()),
        );
        self.status = if approval_required {
            format!("Queued segmentation for {}", cell_key(row, col))
        } else {
            format!("Running segmentation for {}", cell_key(row, col))
        };
        let network_call_id = self.push_network_call(NetworkCallRecord::for_segment(
            self.next_network_call_id,
            row,
            col,
            if approval_required {
                NetworkCallStatus::PendingApproval
            } else {
                NetworkCallStatus::Running
            },
            &request,
        ));

        Some((row, col, input, cache_key, request, network_call_id))
    }

    pub(crate) fn prepare_concatenate_video_for_cell(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, Vec<String>)> {
        self.prepare_concatenate_video_for_cell_internal(row, col)
    }

    fn prepare_concatenate_video_for_cell_internal(
        &mut self,
        row: usize,
        col: usize,
    ) -> Option<(usize, usize, String, String, Vec<String>)> {
        let sheet = self.document.sheet();
        let cell = sheet.cell(row, col)?;
        if !concatenate_video_cell_is_runnable(&cell.value) {
            return None;
        }

        let input = cell.input.clone();
        let video_inputs = match concatenate_video_inputs_for_sheet(&input, sheet) {
            Ok(Some(video_inputs)) => video_inputs,
            Ok(None) => return None,
            Err(error) => {
                self.document
                    .sheet_mut()
                    .set_cell_value_with_cache(row, col, input, CellValue::Error(error), None);
                self.dirty = true;
                return None;
            }
        };
        let cache_key = concatenate_video_cache_key(&input, &video_inputs);

        if let Some(cached) = cached_media_cell_value(&self.document, &cache_key) {
            self.document.sheet_mut().set_cell_value_with_cache(
                row,
                col,
                input,
                CellValue::Cached(cached),
                Some(cache_key),
            );
            self.status = format!("Used cached concatenated video for {}", cell_key(row, col));
            return None;
        }

        self.document.sheet_mut().set_cell_value_with_cache(
            row,
            col,
            input.clone(),
            CellValue::FormulaPending {
                message: "Concatenating video clips...".to_string(),
            },
            Some(cache_key.clone()),
        );
        self.status = format!("Concatenating video clips for {}", cell_key(row, col));

        Some((row, col, input, cache_key, video_inputs))
    }

    pub(crate) fn finish_openrouter_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        request: LlmRequest,
        result: anyhow::Result<String>,
    ) {
        self.status = if result.is_ok() {
            format!(
                "{} completed for {}",
                request.function_name,
                cell_key(row, col)
            )
        } else {
            format!(
                "{} failed for {}",
                request.function_name,
                cell_key(row, col)
            )
        };
        self.document
            .finish_openrouter_for_cell(row, col, input, cache_key, request, result);
        self.dirty = true;
    }

    pub(crate) fn finish_generate_image_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<MediaAsset>,
    ) {
        let mut media_dimensions = None;
        let value = match result {
            Ok(asset) => {
                media_dimensions = media_dimensions_from_metadata(&asset.metadata);
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

        let sheet = self.document.sheet_mut();
        sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
        if matches!(
            sheet.cell(row, col).map(|cell| &cell.value),
            Some(CellValue::Cached(_))
        ) {
            resize_media_cell(
                sheet,
                row,
                col,
                GENERATED_IMAGE_COLUMN_WIDTH,
                GENERATED_IMAGE_ROW_HEIGHT,
                media_dimensions,
            );
        }
        sheet.recalculate_formulas();
        self.dirty = true;
    }

    pub(crate) fn finish_generate_video_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<MediaAsset>,
    ) {
        let mut media_dimensions = None;
        let value = match result {
            Ok(asset) => {
                media_dimensions = media_dimensions_from_metadata(&asset.metadata);
                let uri = asset.uri.clone();
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Ready,
                        value: CachedValue::MediaAsset(asset),
                    },
                );
                self.status = format!("Video generation completed for {}", cell_key(row, col));
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
                self.status = format!("Video generation failed for {}", cell_key(row, col));
                CellValue::Error(error.to_string())
            }
        };

        let sheet = self.document.sheet_mut();
        sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
        if matches!(
            sheet.cell(row, col).map(|cell| &cell.value),
            Some(CellValue::Cached(_))
        ) {
            resize_media_cell(
                sheet,
                row,
                col,
                GENERATED_VIDEO_COLUMN_WIDTH,
                GENERATED_VIDEO_ROW_HEIGHT,
                media_dimensions,
            );
        }
        sheet.recalculate_formulas();
        self.dirty = true;
    }

    pub(crate) fn finish_segment_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<MediaAsset>,
    ) {
        let mut media_dimensions = None;
        let value = match result {
            Ok(asset) => {
                media_dimensions = media_dimensions_from_metadata(&asset.metadata);
                let uri = asset.uri.clone();
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Ready,
                        value: CachedValue::MediaAsset(asset),
                    },
                );
                self.status = format!("Segmentation completed for {}", cell_key(row, col));
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
                self.status = format!("Segmentation failed for {}", cell_key(row, col));
                CellValue::Error(error.to_string())
            }
        };

        let sheet = self.document.sheet_mut();
        sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
        if matches!(
            sheet.cell(row, col).map(|cell| &cell.value),
            Some(CellValue::Cached(_))
        ) {
            resize_media_cell(
                sheet,
                row,
                col,
                GENERATED_IMAGE_COLUMN_WIDTH,
                GENERATED_IMAGE_ROW_HEIGHT,
                media_dimensions,
            );
        }
        sheet.recalculate_formulas();
        self.dirty = true;
    }

    pub(crate) fn finish_concatenate_video_for_cell(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        cache_key: String,
        result: anyhow::Result<MediaAsset>,
    ) {
        let mut media_dimensions = None;
        let value = match result {
            Ok(asset) => {
                media_dimensions = media_dimensions_from_metadata(&asset.metadata);
                let uri = asset.uri.clone();
                self.document.cache.insert(
                    cache_key.clone(),
                    CacheEntry {
                        key: cache_key.clone(),
                        status: CacheStatus::Ready,
                        value: CachedValue::MediaAsset(asset),
                    },
                );
                self.status = format!("Video concatenation completed for {}", cell_key(row, col));
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
                self.status = format!("Video concatenation failed for {}", cell_key(row, col));
                CellValue::Error(error.to_string())
            }
        };

        let sheet = self.document.sheet_mut();
        sheet.set_cell_value_with_cache(row, col, input, value, Some(cache_key));
        if matches!(
            sheet.cell(row, col).map(|cell| &cell.value),
            Some(CellValue::Cached(_))
        ) {
            resize_media_cell(
                sheet,
                row,
                col,
                GENERATED_VIDEO_COLUMN_WIDTH,
                GENERATED_VIDEO_ROW_HEIGHT,
                media_dimensions,
            );
        }
        sheet.recalculate_formulas();
        self.dirty = true;
    }

    pub(crate) fn fit_media_rows_in_range(
        &mut self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) {
        let mut planned_resizes = Vec::new();

        {
            let sheet = self.document.sheet();
            for row in start_row..=end_row {
                for col in start_col..=end_col {
                    let Some(cell) = sheet.cell(row, col) else {
                        continue;
                    };
                    let Some(cache_key) = cell.cache_key.as_deref() else {
                        continue;
                    };
                    let Some(asset) = cached_media_asset(&self.document, cache_key) else {
                        continue;
                    };
                    let Some((minimum_column_width, minimum_row_height)) =
                        media_size_defaults(&asset.media_type)
                    else {
                        continue;
                    };
                    planned_resizes.push((
                        row,
                        col,
                        minimum_column_width,
                        minimum_row_height,
                        media_dimensions_from_asset(asset),
                    ));
                }
            }
        }

        let sheet = self.document.sheet_mut();
        for (row, col, minimum_column_width, minimum_row_height, media_dimensions) in planned_resizes {
            resize_media_cell(
                sheet,
                row,
                col,
                minimum_column_width,
                minimum_row_height,
                media_dimensions,
            );
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
        let reference = cell_reference(row, col);
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
        self.fill_dragging = None;
        self.pending_provider_calls.clear();
        self.network_calls.clear();
        self.next_network_call_id = 1;
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
                self.fill_dragging = None;
                self.status = format!("Opened {}", path.display());
                self.set_selected_cell(0, 0);
                self.rebuild_pending_provider_calls_from_document();
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

    fn finish_network_call(&mut self, id: u64, ok: bool, error_message: Option<String>) {
        if let Some(record) = self.network_calls.iter_mut().find(|record| record.id == id) {
            record.status = if ok {
                NetworkCallStatus::Completed
            } else {
                NetworkCallStatus::Failed
            };
            record.error_message = error_message;
        }
    }

    pub(crate) fn update_resize(&mut self, coordinate: i32) {
        let Some(resizing) = self.resizing else {
            return;
        };

        let delta = coordinate - resizing.start;
        let size = match resizing.kind {
            ResizeKind::BottomPanel => resizing.original - delta,
            _ => resizing.original + delta,
        };

        match resizing.kind {
            ResizeKind::Column => {
                let width = size.max(MIN_COLUMN_WIDTH).min(u16::MAX as i32) as u16;
                self.document.sheet_mut().set_column_width(resizing.index, width);
                self.dirty = true;
            }
            ResizeKind::Row => {
                let height = size.max(MIN_ROW_HEIGHT).min(u16::MAX as i32) as u16;
                self.document.sheet_mut().set_row_height(resizing.index, height);
                self.dirty = true;
            }
            ResizeKind::BottomPanel => {
                self.bottom_panel_height = size.max(MIN_BOTTOM_PANEL_HEIGHT);
            }
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
        self.document
            .sheet_mut()
            .ensure_size(rows.max(MIN_VISIBLE_ROWS), cols.max(MIN_VISIBLE_COLS));
    }

    pub(crate) fn begin_fill_drag(&mut self, row: usize, col: usize) {
        self.commit_formula_buffer_if_changed();
        let source = self.current_selection_range();

        self.ensure_work_area(
            source.end_row.max(row) + GROWTH_BUFFER_ROWS,
            source.end_col.max(col) + GROWTH_BUFFER_COLS,
        );
        self.fill_dragging = Some(FillDrag { source });
        self.selecting = false;
    }

    pub(crate) fn update_fill_drag(&mut self, row: usize, col: usize) {
        if self.fill_dragging.is_none() {
            return;
        }

        self.ensure_work_area(row + GROWTH_BUFFER_ROWS, col + GROWTH_BUFFER_COLS);
        self.selection_end = (row, col);
    }

    pub(crate) fn finish_fill_drag(&mut self) {
        let Some(fill_drag) = self.fill_dragging.take() else {
            return;
        };

        if fill_drag
            .source
            .contains(self.selection_end.0, self.selection_end.1)
        {
            self.selecting = false;
            return;
        }

        match self.document.sheet_mut().fill_from_source(
            fill_drag.source,
            self.selection_end.0,
            self.selection_end.1,
        ) {
            Ok(filled) => {
                self.dirty = true;
                self.selection_anchor = (filled.start_row, filled.start_col);
                self.selection_end = (filled.end_row, filled.end_col);
                self.selected_cell = (filled.start_row, filled.start_col);
                self.status = format!(
                    "Filled {}",
                    range_label(
                        filled.start_row,
                        filled.start_col,
                        filled.end_row,
                        filled.end_col
                    )
                );
            }
            Err(error) => {
                self.status = error;
                self.selection_anchor = (fill_drag.source.start_row, fill_drag.source.start_col);
                self.selection_end = (fill_drag.source.end_row, fill_drag.source.end_col);
                self.selected_cell = (fill_drag.source.start_row, fill_drag.source.start_col);
            }
        }

        self.selected_cell_mode = CellInteractionMode::Display;
        self.editing_cell = None;
        self.refresh_formula_input_from_cell(self.selected_cell.0, self.selected_cell.1);
        self.completions_open = false;
        self.completion_index = 0;
        self.selecting = false;
    }

    fn current_selection_range(&self) -> FillRange {
        let SelectionRange {
            start_row,
            start_col,
            end_row,
            end_col,
        } = self.selection_range();

        FillRange {
            start_row,
            start_col,
            end_row,
            end_col,
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
    fn for_openrouter(
        id: u64,
        row: usize,
        col: usize,
        status: NetworkCallStatus,
        request: &LlmRequest,
    ) -> Self {
        let request_body =
            serde_json::to_value(&request.request).unwrap_or(serde_json::Value::Null);
        Self {
            id,
            cell: cell_key(row, col),
            function_name: request.function_name.to_string(),
            provider: "fal.openrouter".to_string(),
            url: request.request.endpoint().to_string(),
            status,
            request_body: request_body_without_images(&request_body),
            image_inputs: image_inputs_from_body(&request_body),
            error_message: None,
        }
    }

    fn for_generate_image(
        id: u64,
        row: usize,
        col: usize,
        status: NetworkCallStatus,
        request: &GenerateImageRequest,
    ) -> Self {
        let image_inputs = image_inputs_from_body(&request.input);

        Self {
            id,
            cell: cell_key(row, col),
            function_name: "GENERATEIMAGE".to_string(),
            provider: "fal.image".to_string(),
            url: format!(
                "{}/{}",
                request.queue_api_base.trim_end_matches('/'),
                request.endpoint
            ),
            status,
            request_body: request_body_without_images(&request.input),
            image_inputs,
            error_message: None,
        }
    }

    fn for_generate_video(
        id: u64,
        row: usize,
        col: usize,
        status: NetworkCallStatus,
        request: &GenerateVideoRequest,
    ) -> Self {
        Self {
            id,
            cell: cell_key(row, col),
            function_name: "GENERATEVIDEO".to_string(),
            provider: "fal.video".to_string(),
            url: format!(
                "{}/{}",
                request.queue_api_base.trim_end_matches('/'),
                request.endpoint
            ),
            status,
            request_body: request_body_without_images(&request.input),
            image_inputs: image_inputs_from_body(&request.input),
            error_message: None,
        }
    }

    fn for_segment(
        id: u64,
        row: usize,
        col: usize,
        status: NetworkCallStatus,
        request: &SegmentImageRequest,
    ) -> Self {
        Self {
            id,
            cell: cell_key(row, col),
            function_name: "SEGMENT".to_string(),
            provider: "fal.segment".to_string(),
            url: format!(
                "{}/{}",
                request.queue_api_base.trim_end_matches('/'),
                request.endpoint
            ),
            status,
            request_body: request_body_without_images(&request.input),
            image_inputs: image_inputs_from_body(&request.input),
            error_message: None,
        }
    }
}

impl AppState {
    fn start_pending_provider_work(&mut self, work: &ProviderWork) {
        match work {
            ProviderWork::Llm((
                row,
                col,
                input,
                cache_key,
                request,
                network_call_id,
            )) => {
                self.document.sheet_mut().set_cell_value_with_cache(
                    *row,
                    *col,
                    (*input).clone(),
                    CellValue::FormulaPending {
                        message: openrouter_pending_message(request.output_mode).to_string(),
                    },
                    Some((*cache_key).clone()),
                );
                self.start_network_call(*network_call_id);
            }
            ProviderWork::GenerateImage((
                row,
                col,
                input,
                cache_key,
                _,
                network_call_id,
            )) => {
                self.document.sheet_mut().set_cell_value_with_cache(
                    *row,
                    *col,
                    (*input).clone(),
                    CellValue::FormulaPending {
                        message: "Running image generation...".to_string(),
                    },
                    Some((*cache_key).clone()),
                );
                self.start_network_call(*network_call_id);
            }
            ProviderWork::GenerateVideo((
                row,
                col,
                input,
                cache_key,
                _,
                network_call_id,
            )) => {
                self.document.sheet_mut().set_cell_value_with_cache(
                    *row,
                    *col,
                    (*input).clone(),
                    CellValue::FormulaPending {
                        message: "Running video generation...".to_string(),
                    },
                    Some((*cache_key).clone()),
                );
                self.start_network_call(*network_call_id);
            }
            ProviderWork::Segment((
                row,
                col,
                input,
                cache_key,
                _,
                network_call_id,
            )) => {
                self.document.sheet_mut().set_cell_value_with_cache(
                    *row,
                    *col,
                    (*input).clone(),
                    CellValue::FormulaPending {
                        message: "Running segmentation...".to_string(),
                    },
                    Some((*cache_key).clone()),
                );
                self.start_network_call(*network_call_id);
            }
            ProviderWork::ConcatenateVideo((row, col, input, cache_key, _)) => {
                self.document.sheet_mut().set_cell_value_with_cache(
                    *row,
                    *col,
                    (*input).clone(),
                    CellValue::FormulaPending {
                        message: "Concatenating video clips...".to_string(),
                    },
                    Some((*cache_key).clone()),
                );
                self.status = format!("Concatenating video clips for {}", cell_key(*row, *col));
            }
        }
    }

    fn start_network_call(&mut self, id: u64) {
        if let Some(record) = self.network_calls.iter_mut().find(|record| record.id == id) {
            record.status = NetworkCallStatus::Running;
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
        .sheet()
        .cell(row, col)
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

fn cell_reference(row: usize, col: usize) -> String {
    format!("{}{}", column_name(col), row + 1)
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

async fn run_segment_request(request: &SegmentImageRequest) -> anyhow::Result<MediaAsset> {
    let client = FalSegmentClient::from_settings_or_env()?;
    let response = client.run(request).await?;
    let primary_image = response
        .image
        .clone()
        .filter(|image| !image.url.is_empty())
        .or_else(|| response.masks.first().cloned())
        .ok_or_else(|| anyhow::anyhow!("fal segment response did not include any mask images"))?;
    let mut metadata = serde_json::json!({
        "model": request.endpoint,
        "endpoint": request.endpoint,
        "request": request.input,
        "response": response,
    });

    let data_uri =
        match persist_media_data(&primary_image.url, primary_image.content_type.as_deref()).await {
            Ok(data_uri) => Some(data_uri),
            Err(error) => {
                metadata["persistence_warning"] =
                    serde_json::Value::String(format!("Could not embed media data: {error}"));
                None
            }
        };

    Ok(MediaAsset {
        provider: "fal.segment".to_string(),
        media_type: MediaType::Image,
        uri: primary_image.url.clone(),
        data_uri,
        metadata,
    })
}

async fn run_generate_video_request(request: &GenerateVideoRequest) -> anyhow::Result<MediaAsset> {
    let client = FalVideoClient::from_settings_or_env()?;
    let response = client.run(request).await?;
    let mut metadata = serde_json::json!({
        "model": request.model,
        "endpoint": request.endpoint,
        "request": request.input,
        "response": response,
    });

    let data_uri =
        match persist_media_data(&response.video.url, response.video.content_type.as_deref()).await
        {
            Ok(data_uri) => Some(data_uri),
            Err(error) => {
                metadata["persistence_warning"] =
                    serde_json::Value::String(format!("Could not embed media data: {error}"));
                None
            }
        };

    Ok(MediaAsset {
        provider: "fal.video".to_string(),
        media_type: MediaType::Video,
        uri: response.video.url.clone(),
        data_uri,
        metadata,
    })
}

async fn run_concatenate_video_request(video_inputs: &[String]) -> anyhow::Result<MediaAsset> {
    if !ffmpeg_is_available() {
        anyhow::bail!("ffmpeg is not installed. Install ffmpeg to use CONCATENATEVIDEO.");
    }

    let workspace = concat_workspace_dir();
    fs::create_dir_all(&workspace)?;
    let cache_tag = stable_cache_key("CONCATENATEVIDEO", video_inputs);
    let job_dir = workspace.join(cache_tag);
    fs::create_dir_all(&job_dir)?;

    let mut prepared_inputs = Vec::new();
    for (index, input) in video_inputs.iter().enumerate() {
        prepared_inputs.push(materialize_video_input(&job_dir, index, input).await?);
    }

    let concat_list_path = job_dir.join("inputs.txt");
    let concat_list = prepared_inputs
        .iter()
        .map(|path| format!("file '{}'\n", escape_ffmpeg_concat_path(path)))
        .collect::<String>();
    fs::write(&concat_list_path, concat_list)?;

    let output_path = job_dir.join("concatenated.mp4");
    let output = Command::new("ffmpeg")
        .arg("-y")
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(&concat_list_path)
        .arg("-c")
        .arg("copy")
        .arg(&output_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("ffmpeg concat failed: {stderr}");
    }

    let output_uri = output_path.to_string_lossy().to_string();
    let data_uri = persist_media_data(&output_uri, Some("video/mp4"))
        .await
        .ok();

    Ok(MediaAsset {
        provider: "local.ffmpeg".to_string(),
        media_type: MediaType::Video,
        uri: output_uri,
        data_uri,
        metadata: serde_json::json!({
            "inputs": video_inputs,
            "output_path": output_path,
            "tool": "ffmpeg",
        }),
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

    if Path::new(uri).exists() {
        let bytes = fs::read(uri)?;
        let content_type = content_type_hint.unwrap_or("application/octet-stream");
        return Ok(format!(
            "data:{content_type};base64,{}",
            STANDARD.encode(bytes)
        ));
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

fn ffmpeg_is_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn concat_workspace_dir() -> PathBuf {
    std::env::temp_dir().join("cashew-video-concat")
}

async fn materialize_video_input(
    job_dir: &Path,
    index: usize,
    input: &str,
) -> anyhow::Result<PathBuf> {
    if input.starts_with("data:") {
        return write_data_uri_to_file(job_dir, index, input);
    }

    let path = Path::new(input);
    if path.exists() {
        return Ok(path.to_path_buf());
    }

    let extension = media_extension_from_uri(input).unwrap_or("mp4");
    let destination = job_dir.join(format!("input-{index}.{extension}"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;
    let bytes = client
        .get(input)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    fs::write(&destination, &bytes)?;
    Ok(destination)
}

fn write_data_uri_to_file(job_dir: &Path, index: usize, data_uri: &str) -> anyhow::Result<PathBuf> {
    let (header, payload) = data_uri
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("Invalid data URI video input"))?;
    let extension = if header.contains("video/webm") {
        "webm"
    } else if header.contains("video/quicktime") {
        "mov"
    } else {
        "mp4"
    };
    let data = STANDARD.decode(payload)?;
    let destination = job_dir.join(format!("input-{index}.{extension}"));
    fs::write(&destination, data)?;
    Ok(destination)
}

fn media_extension_from_uri(uri: &str) -> Option<&'static str> {
    let trimmed = uri.split('?').next().unwrap_or(uri);
    if trimmed.ends_with(".webm") {
        Some("webm")
    } else if trimmed.ends_with(".mov") {
        Some("mov")
    } else if trimmed.ends_with(".mkv") {
        Some("mkv")
    } else if trimmed.ends_with(".mp4") {
        Some("mp4")
    } else {
        None
    }
}

fn resize_media_cell(
    sheet: &mut Sheet,
    row: usize,
    col: usize,
    minimum_column_width: u16,
    minimum_row_height: u16,
    media_dimensions: Option<(u32, u32)>,
) {
    let width = sheet.column_width(col).max(minimum_column_width);
    sheet.set_column_width(col, width);

    let height = if row_has_only_media_content(sheet, row, col) {
        fitted_row_height(width, media_dimensions.unwrap_or((0, 0)))
            .map(|height| height.max(minimum_row_height))
            .unwrap_or(minimum_row_height)
    } else {
        minimum_row_height
    };

    sheet.set_row_height(row, sheet.row_height(row).max(height));
}

fn row_has_only_media_content(sheet: &Sheet, row: usize, media_col: usize) -> bool {
    if !sheet
        .cell(row, media_col)
        .is_some_and(cell_contains_media_value)
    {
        return false;
    }

    for col in 0..sheet.cols {
        if col == media_col {
            continue;
        }

        if sheet.cell(row, col).is_some_and(cell_has_visible_content) {
            return false;
        }
    }

    true
}

fn cell_contains_media_value(cell: &Cell) -> bool {
    matches!(cell.value, CellValue::Cached(_))
}

fn cell_has_visible_content(cell: &Cell) -> bool {
    match &cell.value {
        CellValue::Empty => false,
        CellValue::Text(value) | CellValue::Cached(value) => !value.trim().is_empty(),
        CellValue::FormulaPending { .. } | CellValue::Error(_) => true,
    }
}

fn media_dimensions_from_metadata(metadata: &serde_json::Value) -> Option<(u32, u32)> {
    image_dimensions_from_metadata(metadata).or_else(|| video_dimensions_from_metadata(metadata))
}

fn media_dimensions_from_asset(asset: &MediaAsset) -> Option<(u32, u32)> {
    media_dimensions_from_metadata(&asset.metadata).or_else(|| {
        asset
            .data_uri
            .as_deref()
            .and_then(media_dimensions_from_data_uri)
    })
}

fn media_size_defaults(media_type: &MediaType) -> Option<(u16, u16)> {
    match media_type {
        MediaType::Image => Some((GENERATED_IMAGE_COLUMN_WIDTH, GENERATED_IMAGE_ROW_HEIGHT)),
        MediaType::Video => Some((GENERATED_VIDEO_COLUMN_WIDTH, GENERATED_VIDEO_ROW_HEIGHT)),
        MediaType::Audio | MediaType::Other(_) => None,
    }
}

fn cached_media_asset<'a>(document: &'a CashewDocument, cache_key: &str) -> Option<&'a MediaAsset> {
    let entry = document.cache.get(cache_key)?;
    if entry.status != CacheStatus::Ready {
        return None;
    }

    match &entry.value {
        CachedValue::MediaAsset(asset) => Some(asset),
        CachedValue::Text(_) | CachedValue::Json(_) => None,
    }
}

fn image_dimensions_from_metadata(metadata: &serde_json::Value) -> Option<(u32, u32)> {
    let image = metadata
        .get("response")?
        .get("images")?
        .as_array()?
        .first()?;
    let width = image.get("width")?.as_u64()? as u32;
    let height = image.get("height")?.as_u64()? as u32;
    Some((width, height))
}

fn video_dimensions_from_metadata(metadata: &serde_json::Value) -> Option<(u32, u32)> {
    let aspect_ratio = metadata
        .get("request")
        .and_then(|request| request.get("aspect_ratio"))
        .and_then(serde_json::Value::as_str)?;
    aspect_ratio_dimensions(aspect_ratio)
}

fn aspect_ratio_dimensions(aspect_ratio: &str) -> Option<(u32, u32)> {
    let (width, height) = aspect_ratio.split_once(':')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

fn fitted_row_height(width: u16, dimensions: (u32, u32)) -> Option<u16> {
    let (media_width, media_height) = dimensions;
    if media_width == 0 || media_height == 0 {
        return None;
    }

    let scaled_height = (u32::from(width) * media_height) / media_width;
    let scaled_height = scaled_height.max(1).min(u32::from(u16::MAX));
    u16::try_from(scaled_height).ok()
}

fn media_dimensions_from_data_uri(data_uri: &str) -> Option<(u32, u32)> {
    let (header, payload) = data_uri.split_once(',')?;
    if !header.contains(";base64") {
        return None;
    }

    let bytes = STANDARD.decode(payload).ok()?;
    png_dimensions(&bytes).or_else(|| jpeg_dimensions(&bytes))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE {
        return None;
    }

    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }

        while index < bytes.len() && bytes[index] == 0xFF {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }

        let marker = bytes[index];
        index += 1;

        if marker == 0xD9 || marker == 0xDA {
            return None;
        }

        if index + 1 >= bytes.len() {
            return None;
        }
        let segment_length = u16::from_be_bytes(bytes[index..index + 2].try_into().ok()?) as usize;
        if segment_length < 2 || index + segment_length > bytes.len() {
            return None;
        }

        let is_sof = matches!(
            marker,
            0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
        );
        if is_sof {
            if segment_length < 7 {
                return None;
            }
            let height =
                u16::from_be_bytes(bytes[index + 3..index + 5].try_into().ok()?) as u32;
            let width =
                u16::from_be_bytes(bytes[index + 5..index + 7].try_into().ok()?) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }

        index += segment_length;
    }

    None
}

fn escape_ffmpeg_concat_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}

fn llm_cache_key(input: &str, request: &OpenRouterRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn generate_image_cache_key(input: &str, request: &GenerateImageRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn generate_video_cache_key(input: &str, request: &GenerateVideoRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn segment_cache_key(input: &str, request: &SegmentImageRequest) -> String {
    let request_json = serde_json::to_string(request).unwrap_or_else(|_| request.prompt.clone());
    stable_cache_key(input, &[request_json])
}

fn concatenate_video_cache_key(input: &str, video_inputs: &[String]) -> String {
    stable_cache_key(input, video_inputs)
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

fn openrouter_pending_message(mode: LlmOutputMode) -> &'static str {
    match mode {
        LlmOutputMode::Text => "Running LLM...",
        LlmOutputMode::ListDown | LlmOutputMode::ListRight => "Running LLM list...",
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

fn generate_video_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => message == "fal.video request is ready to run",
        CellValue::Error(_) => true,
        CellValue::Empty | CellValue::Text(_) | CellValue::Cached(_) => false,
    }
}

fn segment_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => message == "fal.segment request is ready to run",
        CellValue::Error(_) => true,
        CellValue::Empty | CellValue::Text(_) | CellValue::Cached(_) => false,
    }
}

fn concatenate_video_cell_is_runnable(value: &CellValue) -> bool {
    match value {
        CellValue::FormulaPending { message } => {
            message == "Local video concatenation is ready to run"
        }
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

        assert_eq!(state.formula_input, "=LLM(C2, \"model\")");
    }

    #[test]
    fn inserted_generate_video_formula_uses_cell_reference_marker() {
        let mut state = AppState::new();
        let function = crate::backend::formulas::FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEVIDEO")
            .copied()
            .unwrap();

        state.insert_formula(function);
        state.insert_cell_reference(30, 2);

        assert_eq!(
            state.formula_input,
            "=GENERATEVIDEO(prompt, C31, \"fal-ai/veo3.1/reference-to-video\", 8, \"16:9\")"
        );
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
    fn media_only_row_grows_to_match_image_aspect_ratio() {
        let mut state = AppState::new();

        state.finish_generate_image_for_cell(
            0,
            0,
            "=GENERATEIMAGE(\"prompt\", \"flux/dev\")".to_string(),
            "image-key".to_string(),
            Ok(test_image_asset("https://example.com/tall.png", 100, 400)),
        );

        let sheet = state.document.sheet();
        assert_eq!(sheet.column_width(0), GENERATED_IMAGE_COLUMN_WIDTH);
        assert_eq!(sheet.row_height(0), 720);
    }

    #[test]
    fn media_row_keeps_default_height_when_other_cells_have_content() {
        let mut state = AppState::new();
        state.set_cell_input(0, 1, "notes".to_string());

        state.finish_generate_image_for_cell(
            0,
            0,
            "=GENERATEIMAGE(\"prompt\", \"flux/dev\")".to_string(),
            "image-key".to_string(),
            Ok(test_image_asset("https://example.com/tall.png", 100, 400)),
        );

        let sheet = state.document.sheet();
        assert_eq!(sheet.row_height(0), GENERATED_IMAGE_ROW_HEIGHT);
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
    fn queued_provider_calls_can_be_dispatched_later() {
        let mut state = AppState::new();
        state.set_selected_formula(r#"=GENERATEIMAGE("A cat", "flux/dev")"#.to_string());

        let work = state
            .prepare_generate_image_for_cell(0, 0, true)
            .expect("image formula should prepare");
        state.queue_pending_provider_call(ProviderWork::GenerateImage(work));

        assert_eq!(state.pending_provider_calls.len(), 1);
        assert!(matches!(
            state.network_calls.first().map(|record| record.status),
            Some(NetworkCallStatus::PendingApproval)
        ));

        let works = state.dispatch_pending_provider_calls();

        assert_eq!(works.len(), 1);
        assert!(state.pending_provider_calls.is_empty());
        assert!(matches!(
            state.network_calls.first().map(|record| record.status),
            Some(NetworkCallStatus::Running)
        ));
        assert!(matches!(
            state
                .document
                .sheet()
                .cell(0, 0)
                .map(|cell| &cell.value),
            Some(CellValue::FormulaPending { message }) if message == "Running image generation..."
        ));
    }

    #[test]
    fn saved_pending_provider_calls_are_rebuilt_for_approval() {
        let mut state = AppState::new();
        state.new_document();
        state
            .document
            .sheet_mut()
            .set_cell_input(0, 0, r#"=GENERATEIMAGE("A cat", "flux/dev")"#.to_string());

        state.pending_provider_calls.clear();
        state.network_calls.clear();
        state.next_network_call_id = 1;
        state.rebuild_pending_provider_calls_from_document();

        assert_eq!(state.pending_provider_calls.len(), 1);
        assert!(matches!(
            state.network_calls.first().map(|record| record.status),
            Some(NetworkCallStatus::PendingApproval)
        ));
        assert!(matches!(
            state
                .document
                .sheet()
                .cell(0, 0)
                .map(|cell| &cell.value),
            Some(CellValue::FormulaPending { message })
                if message == "fal.image request is pending approval"
        ));

        let works = state.dispatch_pending_provider_calls();
        assert_eq!(works.len(), 1);
        assert!(matches!(
            state
                .document
                .sheet()
                .cell(0, 0)
                .map(|cell| &cell.value),
            Some(CellValue::FormulaPending { message }) if message == "Running image generation..."
        ));
    }

    #[test]
    fn failed_network_calls_store_error_output_for_the_network_panel() {
        let mut state = AppState::new();
        let request = GenerateImageRequest::new(
            "edit it",
            "openai/gpt-image-2",
            None,
            vec!["https://example.com/ref.png".to_string()],
        )
        .unwrap();
        let record =
            NetworkCallRecord::for_generate_image(9, 1, 2, NetworkCallStatus::Running, &request);
        state.push_network_call(record);

        state.finish_network_call(9, false, Some("provider said no".to_string()));

        let record = state
            .network_calls
            .iter()
            .find(|record| record.id == 9)
            .unwrap();
        assert!(matches!(record.status, NetworkCallStatus::Failed));
        assert_eq!(record.error_message.as_deref(), Some("provider said no"));
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

        let record =
            NetworkCallRecord::for_generate_image(7, 1, 2, NetworkCallStatus::Running, &request);

        assert_eq!(record.cell, "C2");
        assert_eq!(record.url, "https://queue.fal.run/openai/gpt-image-2/edit");
        assert_eq!(record.image_inputs.len(), 2);
        assert_eq!(
            record.request_body["image_urls"],
            "<2 images shown in Images>"
        );
    }

    fn test_image_asset(uri: &str, width: u32, height: u32) -> MediaAsset {
        MediaAsset {
            provider: "fal.image".to_string(),
            media_type: MediaType::Image,
            uri: uri.to_string(),
            data_uri: None,
            metadata: serde_json::json!({
                "response": {
                    "images": [{
                        "width": width,
                        "height": height
                    }]
                }
            }),
        }
    }

    #[test]
    fn segment_network_record_extracts_image_input() {
        let request = SegmentImageRequest::new(
            "https://example.com/image.png",
            "wheel",
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let record = NetworkCallRecord::for_segment(8, 1, 2, NetworkCallStatus::Running, &request);

        assert_eq!(record.function_name, "SEGMENT");
        assert_eq!(record.provider, "fal.segment");
        assert_eq!(record.url, "https://queue.fal.run/fal-ai/sam-3/image");
        assert_eq!(record.image_inputs, vec!["https://example.com/image.png"]);
        assert_eq!(record.request_body["image_url"], "<shown in Images>");
    }

    #[test]
    fn openrouter_network_record_does_not_include_auth_data() {
        let request = OpenRouterRequest::new("hello")
            .with_image_urls(vec!["https://example.com/image.png".to_string()]);

        let llm_request = LlmRequest {
            function_name: "LLM",
            output_mode: LlmOutputMode::Text,
            request,
        };
        let record =
            NetworkCallRecord::for_openrouter(3, 0, 0, NetworkCallStatus::Running, &llm_request);
        let body = serde_json::to_string(&record.request_body).unwrap();

        assert_eq!(
            record.url,
            crate::backend::providers::openrouter::VISION_ENDPOINT
        );
        assert_eq!(record.image_inputs, vec!["https://example.com/image.png"]);
        assert!(!body.contains("Authorization"));
        assert!(!body.contains("Key "));
        assert!(body.contains("shown in Images"));
    }
}

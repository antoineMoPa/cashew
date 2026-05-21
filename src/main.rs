mod backend;

use std::path::PathBuf;

use backend::document::{CashewDocument, CellValue, cell_key, column_name};
use dioxus::prelude::*;

const MIN_COLUMN_WIDTH: i32 = 72;
const MAX_COLUMN_WIDTH: i32 = 520;
const MIN_ROW_HEIGHT: i32 = 24;
const MAX_ROW_HEIGHT: i32 = 260;

fn main() {
    dioxus::launch(App);
}

#[derive(Debug, Clone)]
struct AppState {
    document: CashewDocument,
    file_path: Option<PathBuf>,
    file_menu_open: bool,
    dirty: bool,
    status: String,
    selected_cell: (usize, usize),
    formula_input: String,
    resizing: Option<ResizeDrag>,
}

#[derive(Debug, Clone, Copy)]
struct ResizeDrag {
    kind: ResizeKind,
    index: usize,
    start: i32,
    original: i32,
}

#[derive(Debug, Clone, Copy)]
enum ResizeKind {
    Column,
    Row,
}

impl AppState {
    fn new() -> Self {
        let document = CashewDocument::default();
        let formula_input = cell_input(&document, 0, 0);

        Self {
            document,
            file_path: None,
            file_menu_open: false,
            dirty: false,
            status: "Ready".to_string(),
            selected_cell: (0, 0),
            formula_input,
            resizing: None,
        }
    }

    fn set_selected_cell(&mut self, row: usize, col: usize) {
        self.selected_cell = (row, col);
        self.formula_input = cell_input(&self.document, row, col);
    }

    fn set_cell_input(&mut self, row: usize, col: usize, value: String) {
        if let Some(sheet) = self.document.active_sheet_mut() {
            sheet.set_cell_input(row, col, value.clone());
            self.dirty = true;
            self.status = format!("Edited {}", cell_key(row, col));
        }

        if self.selected_cell == (row, col) {
            self.formula_input = value;
        }
    }

    fn set_selected_formula(&mut self, value: String) {
        let (row, col) = self.selected_cell;
        self.formula_input = value.clone();
        self.set_cell_input(row, col, value);
    }

    fn new_document(&mut self) {
        self.document = CashewDocument::default();
        self.file_path = None;
        self.file_menu_open = false;
        self.dirty = false;
        self.status = "Created a new document".to_string();
        self.set_selected_cell(0, 0);
    }

    fn open_document(&mut self) {
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

    fn save_document(&mut self) {
        self.file_menu_open = false;

        let Some(path) = self.file_path.clone() else {
            self.save_document_as();
            return;
        };

        self.write_document(path);
    }

    fn save_document_as(&mut self) {
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

    fn update_resize(&mut self, coordinate: i32) {
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
}

#[component]
fn App() -> Element {
    let mut state = use_signal(AppState::new);

    let on_mouse_move = move |event: MouseEvent| {
        let coordinates = event.client_coordinates();
        state.with_mut(|state| {
            if let Some(resizing) = state.resizing {
                let coordinate = match resizing.kind {
                    ResizeKind::Column => coordinates.x as i32,
                    ResizeKind::Row => coordinates.y as i32,
                };
                state.update_resize(coordinate);
            }
        });
    };

    let on_mouse_up = move |_| {
        state.with_mut(|state| state.resizing = None);
    };

    rsx! {
        style { "{APP_CSS}" }
        main {
            class: "app",
            onmousemove: on_mouse_move,
            onmouseup: on_mouse_up,
            onmouseleave: on_mouse_up,
            MenuBar { state }
            FormulaBar { state }
            SheetView { state }
            StatusBar { state }
        }
    }
}

#[component]
fn MenuBar(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let title = snapshot.document.title.clone();
    let dirty = snapshot.dirty;
    let title_text = if dirty { format!("{title} *") } else { title };
    let path = snapshot
        .file_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "Unsaved JSON document".to_string());
    let file_menu_open = snapshot.file_menu_open;
    drop(snapshot);

    rsx! {
        header { class: "top-shell",
            div { class: "title-strip",
                div { class: "brand", "Cashew" }
                div { class: "document-title", "{title_text}" }
                div { class: "document-path", "{path}" }
            }
            nav { class: "menu-bar",
                div { class: "menu-root",
                    button {
                        class: "menu-trigger",
                        onmousedown: move |event| event.stop_propagation(),
                        onclick: move |_| state.with_mut(|state| state.file_menu_open = !state.file_menu_open),
                        "File"
                    }
                    if file_menu_open {
                        div { class: "menu-popover",
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::new_document), "New" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::open_document), "Open..." }
                            div { class: "menu-separator" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::save_document), "Save" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::save_document_as), "Save As..." }
                        }
                    }
                }
                button { class: "menu-trigger disabled", "Edit" }
            }
        }
    }
}

#[component]
fn FormulaBar(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let (row, col) = snapshot.selected_cell;
    let address = cell_key(row, col);
    let formula_input = snapshot.formula_input.clone();
    drop(snapshot);

    rsx! {
        section { class: "formula-bar",
            div { class: "name-box", "{address}" }
            div { class: "fx-label", "fx" }
            input {
                class: "formula-input",
                value: "{formula_input}",
                oninput: move |event| {
                    let value = event.value();
                    state.with_mut(|state| state.set_selected_formula(value));
                }
            }
        }
    }
}

#[component]
fn SheetView(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let Some(sheet) = snapshot.document.active_sheet() else {
        return rsx! { div { class: "empty-sheet", "No sheet" } };
    };

    let rows = sheet.rows;
    let cols = sheet.cols;
    let column_widths = (0..cols)
        .map(|col| sheet.column_width(col))
        .collect::<Vec<_>>();
    let grid_columns = std::iter::once("46px".to_string())
        .chain(column_widths.iter().map(|width| format!("{width}px")))
        .collect::<Vec<_>>()
        .join(" ");
    let grid_style = format!("grid-template-columns: {grid_columns};");
    let row_heights = (0..rows)
        .map(|row| sheet.row_height(row))
        .collect::<Vec<_>>();
    let selected_cell = snapshot.selected_cell;
    let document = snapshot.document.clone();
    drop(snapshot);

    rsx! {
        section { class: "sheet-viewport",
            div { class: "sheet-grid", style: "{grid_style}",
                div { class: "corner-cell" }
                for col in 0..cols {
                    ColumnHeader {
                        state,
                        col,
                        width: column_widths[col],
                    }
                }
                for row in 0..rows {
                    RowHeader {
                        state,
                        row,
                        height: row_heights[row],
                    }
                    for col in 0..cols {
                        CellEditor {
                            state,
                            document: document.clone(),
                            row,
                            col,
                            width: column_widths[col],
                            height: row_heights[row],
                            selected: selected_cell == (row, col),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ColumnHeader(mut state: Signal<AppState>, col: usize, width: u16) -> Element {
    let label = column_name(col);
    let style = format!("width: {}px;", width);

    rsx! {
        div { class: "column-header", style,
            span { "{label}" }
            div {
                class: "column-resizer",
                onmousedown: move |event| {
                    event.stop_propagation();
                    state.with_mut(|state| {
                        state.resizing = Some(ResizeDrag {
                            kind: ResizeKind::Column,
                            index: col,
                            start: event.client_coordinates().x as i32,
                            original: width as i32,
                        });
                    });
                }
            }
        }
    }
}

#[component]
fn RowHeader(mut state: Signal<AppState>, row: usize, height: u16) -> Element {
    let style = format!("height: {}px;", height);

    rsx! {
        div { class: "row-header", style,
            span { "{row + 1}" }
            div {
                class: "row-resizer",
                onmousedown: move |event| {
                    event.stop_propagation();
                    state.with_mut(|state| {
                        state.resizing = Some(ResizeDrag {
                            kind: ResizeKind::Row,
                            index: row,
                            start: event.client_coordinates().y as i32,
                            original: height as i32,
                        });
                    });
                }
            }
        }
    }
}

#[component]
fn CellEditor(
    mut state: Signal<AppState>,
    document: CashewDocument,
    row: usize,
    col: usize,
    width: u16,
    height: u16,
    selected: bool,
) -> Element {
    let cell = document
        .active_sheet()
        .and_then(|sheet| sheet.cell(row, col))
        .cloned();

    let value = cell.as_ref().map(|cell| cell.input.as_str()).unwrap_or("");
    let status_class = match cell.as_ref().map(|cell| &cell.value) {
        Some(CellValue::FormulaPending) => " formula",
        Some(CellValue::Cached(_)) => " cached",
        Some(CellValue::Error(_)) => " error",
        _ => "",
    };
    let selected_class = if selected { " selected" } else { "" };
    let class = format!("cell{}{}", status_class, selected_class);
    let style = format!("width: {}px; height: {}px;", width, height);

    rsx! {
        input {
            class,
            style,
            value: "{value}",
            onfocus: move |_| state.with_mut(|state| state.set_selected_cell(row, col)),
            onclick: move |_| state.with_mut(|state| state.set_selected_cell(row, col)),
            oninput: move |event| {
                let value = event.value();
                state.with_mut(|state| state.set_cell_input(row, col, value));
            }
        }
    }
}

#[component]
fn StatusBar(state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let status = snapshot.status.clone();
    let cache_entries = snapshot.document.cache.len();
    drop(snapshot);

    rsx! {
        footer { class: "status-bar",
            span { "{status}" }
            span { "{cache_entries} cached results" }
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

const APP_CSS: &str = r#"
html,
body,
#main {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    font-family: Arial, Helvetica, sans-serif;
    color: #202124;
    background: #fff;
}

* {
    box-sizing: border-box;
}

button,
input {
    font: inherit;
}

.app {
    width: 100vw;
    height: 100vh;
    display: grid;
    grid-template-rows: auto auto 1fr auto;
    background: #fff;
    user-select: none;
}

.top-shell {
    border-bottom: 1px solid #dadce0;
    background: #f8fafd;
}

.title-strip {
    height: 36px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
}

.brand {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    background: #188038;
    color: #fff;
    font-size: 13px;
    font-weight: 700;
}

.document-title {
    min-width: 160px;
    font-size: 15px;
    font-weight: 500;
}

.document-path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #5f6368;
    font-size: 12px;
}

.menu-bar {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 8px;
    position: relative;
}

.menu-root {
    position: relative;
}

.menu-trigger {
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: #202124;
    font-size: 13px;
}

.menu-trigger:hover {
    background: #e8eaed;
}

.menu-trigger.disabled {
    color: #9aa0a6;
}

.menu-popover {
    position: absolute;
    top: 26px;
    left: 0;
    z-index: 20;
    min-width: 196px;
    padding: 6px 0;
    border: 1px solid #dadce0;
    border-radius: 4px;
    background: #fff;
    box-shadow: 0 8px 24px rgba(60, 64, 67, 0.24);
}

.menu-item {
    width: 100%;
    height: 28px;
    padding: 0 28px 0 14px;
    border: 0;
    background: transparent;
    text-align: left;
    color: #202124;
    font-size: 13px;
}

.menu-item:hover {
    background: #e8f0fe;
}

.menu-separator {
    height: 1px;
    margin: 5px 0;
    background: #e0e0e0;
}

.formula-bar {
    height: 34px;
    display: grid;
    grid-template-columns: 76px 34px 1fr;
    align-items: center;
    border-bottom: 1px solid #dadce0;
    background: #fff;
}

.name-box {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    border-right: 1px solid #dadce0;
    color: #3c4043;
    font-size: 12px;
}

.fx-label {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    border-right: 1px solid #dadce0;
    color: #5f6368;
    font-style: italic;
    font-size: 14px;
}

.formula-input {
    width: 100%;
    height: 100%;
    padding: 0 10px;
    border: 0;
    outline: 0;
    color: #202124;
    background: #fff;
    user-select: text;
}

.sheet-viewport {
    overflow: auto;
    background: #fff;
}

.sheet-grid {
    display: grid;
    align-items: stretch;
    width: max-content;
    min-width: 100%;
}

.corner-cell,
.column-header,
.row-header {
    position: sticky;
    z-index: 5;
    display: flex;
    align-items: center;
    justify-content: center;
    border-right: 1px solid #dadce0;
    border-bottom: 1px solid #dadce0;
    background: #f8f9fa;
    color: #5f6368;
    font-size: 12px;
}

.corner-cell {
    left: 0;
    top: 0;
    width: 46px;
    height: 28px;
    z-index: 12;
}

.column-header {
    top: 0;
    height: 28px;
}

.row-header {
    left: 0;
    width: 46px;
}

.column-resizer,
.row-resizer {
    position: absolute;
    z-index: 10;
}

.column-resizer {
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
}

.row-resizer {
    left: 0;
    bottom: -3px;
    width: 100%;
    height: 6px;
    cursor: row-resize;
}

.column-resizer:hover,
.row-resizer:hover {
    background: #1a73e8;
}

.cell {
    margin: 0;
    padding: 0 6px;
    border: 0;
    border-right: 1px solid #e0e0e0;
    border-bottom: 1px solid #e0e0e0;
    border-radius: 0;
    outline: 0;
    background: #fff;
    color: #202124;
    font-size: 13px;
    user-select: text;
}

.cell:focus,
.cell.selected {
    position: relative;
    z-index: 3;
    box-shadow: inset 0 0 0 2px #1a73e8;
}

.cell.formula {
    color: #188038;
}

.cell.cached {
    background: #f1f8e9;
}

.cell.error {
    color: #b3261e;
    background: #fce8e6;
}

.empty-sheet {
    padding: 16px;
    color: #5f6368;
}

.status-bar {
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px;
    border-top: 1px solid #dadce0;
    background: #f8f9fa;
    color: #5f6368;
    font-size: 12px;
}
"#;

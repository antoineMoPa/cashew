use dioxus::prelude::*;

use crate::backend::document::{CashewDocument, CellValue, cell_key, column_name};
use crate::backend::formulas::matching_functions;

use super::state::{
    AppState, MIN_VISIBLE_COLS, MIN_VISIBLE_ROWS, ResizeDrag, ResizeKind, should_show_completions,
};

#[component]
pub(crate) fn MenuBar(mut state: Signal<AppState>) -> Element {
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
                            div { class: "menu-separator" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::open_settings), "Settings..." }
                        }
                    }
                }
                button { class: "menu-trigger disabled", "Edit" }
            }
        }
    }
}

#[component]
pub(crate) fn SettingsDialog(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let open = snapshot.settings_open;
    let fal_key = snapshot.settings_fal_key.clone();
    let path = snapshot
        .settings_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "$HOME/.cashewai/settings.json".to_string());
    drop(snapshot);

    if !open {
        return rsx! {};
    }

    rsx! {
        div { class: "modal-backdrop",
            div { class: "settings-dialog",
                div { class: "settings-title", "Settings" }
                label { class: "settings-field",
                    span { "FAL key" }
                    input {
                        class: "settings-input",
                        r#type: "password",
                        value: "{fal_key}",
                        placeholder: "fal key",
                        oninput: move |event| {
                            let value = event.value();
                            state.with_mut(|state| state.set_settings_fal_key(value));
                        }
                    }
                }
                div { class: "settings-path", "{path}" }
                div { class: "settings-actions",
                    button {
                        class: "dialog-button secondary",
                        onclick: move |_| state.with_mut(AppState::close_settings),
                        "Cancel"
                    }
                    button {
                        class: "dialog-button primary",
                        onclick: move |_| state.with_mut(AppState::save_settings),
                        "Save"
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn FormulaBar(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let (row, col) = snapshot.selected_cell;
    let address = cell_key(row, col);
    let formula_input = snapshot.formula_input.clone();
    drop(snapshot);

    rsx! {
        section { class: "formula-bar",
            div { class: "name-box", "{address}" }
            div { class: "fx-label", "fx" }
            div { class: "formula-input-wrap",
                input {
                    class: "formula-input",
                    value: "{formula_input}",
                    onfocus: move |_| state.with_mut(|state| {
                        state.completions_open = should_show_completions(&state.formula_input);
                    }),
                    oninput: move |event| {
                        let value = event.value();
                        state.with_mut(|state| state.set_selected_formula(value));
                    }
                }
                FormulaCompletions { state }
            }
        }
    }
}

#[component]
fn FormulaCompletions(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let open = snapshot.completions_open;
    let input = snapshot.formula_input.clone();
    drop(snapshot);

    let matches = if open {
        matching_functions(&input)
    } else {
        Vec::new()
    };

    if matches.is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "formula-completions",
            for function in matches {
                button {
                    class: "completion-item",
                    onmousedown: move |event| event.stop_propagation(),
                    onclick: move |_| state.with_mut(|state| state.insert_formula(function)),
                    div { class: "completion-main",
                        span { class: "completion-name", "{function.name}" }
                        span { class: "completion-signature", "{function.signature}" }
                    }
                    div { class: "completion-summary", "{function.summary}" }
                    div { class: "completion-details", "{function.details}" }
                }
            }
        }
    }
}

#[component]
pub(crate) fn SheetView(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let Some(sheet) = snapshot.document.active_sheet() else {
        return rsx! { div { class: "empty-sheet", "No sheet" } };
    };

    let rows = sheet.rows.max(MIN_VISIBLE_ROWS);
    let cols = sheet.cols.max(MIN_VISIBLE_COLS);
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

    let value = cell_display_value(cell.as_ref(), selected);
    let status_class = match cell.as_ref().map(|cell| &cell.value) {
        Some(CellValue::FormulaPending { .. }) => " formula",
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
            onfocus: move |_| {
                state.with_mut(|state| state.select_or_insert_cell_reference(row, col));
            },
            oninput: move |event| {
                let value = event.value();
                state.with_mut(|state| state.set_cell_input(row, col, value));
            }
        }
    }
}

fn cell_display_value(cell: Option<&crate::backend::document::Cell>, selected: bool) -> String {
    let Some(cell) = cell else {
        return String::new();
    };

    if selected {
        return cell.input.clone();
    }

    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Text(value) => value.clone(),
        CellValue::FormulaPending { message } => message.clone(),
        CellValue::Cached(value) => value.clone(),
        CellValue::Error(error) => format!("#ERROR: {error}"),
    }
}

#[component]
pub(crate) fn StatusBar(state: Signal<AppState>) -> Element {
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

use dioxus::prelude::*;

use crate::backend::cache::{CacheStatus, CachedValue, MediaType};
use crate::backend::document::{Cell, CellValue, column_name};
use crate::backend::formulas::matching_functions;

use super::super::state::{
    AppState, CellInteractionMode, MIN_VISIBLE_COLS, MIN_VISIBLE_ROWS, ResizeDrag, ResizeKind,
};

type LlmWork = (
    usize,
    usize,
    String,
    String,
    crate::backend::providers::openrouter::OpenRouterRequest,
);

type GenerateImageWork = (
    usize,
    usize,
    String,
    String,
    crate::backend::providers::fal_image::GenerateImageRequest,
);

pub(crate) enum ProviderWork {
    Llm(LlmWork),
    GenerateImage(GenerateImageWork),
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
                    autocomplete: "off",
                    autocorrect: "off",
                    autocapitalize: "off",
                    spellcheck: "false",
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
pub(crate) fn SheetView(mut state: Signal<AppState>) -> Element {
    use_effect(move || {
        install_clipboard_bridge();
    });

    let snapshot = state.read();
    let Some(sheet) = snapshot.document.active_sheet() else {
        return rsx! { div { class: "empty-sheet", "No sheet" } };
    };

    let rows = sheet.rows.max(MIN_VISIBLE_ROWS);
    let cols = sheet.cols.max(MIN_VISIBLE_COLS);
    let column_widths = (0..cols).map(|col| sheet.column_width(col)).collect::<Vec<_>>();
    let grid_columns = std::iter::once("46px".to_string())
        .chain(column_widths.iter().map(|width| format!("{width}px")))
        .collect::<Vec<_>>()
        .join(" ");
    let grid_style = format!("grid-template-columns: {grid_columns};");
    let row_heights = (0..rows).map(|row| sheet.row_height(row)).collect::<Vec<_>>();
    let selected_cell = snapshot.selected_cell;
    let selection_range = snapshot.selection_range();
    let cell_modes = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| snapshot.selected_cell_mode_for(row, col))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let visible_cells = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let cell = sheet.cell(row, col).cloned();
                    let image_uri = cell
                        .as_ref()
                        .and_then(|cell| cached_image_uri(&snapshot.document, cell));
                    (cell, image_uri)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
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
                            cell: visible_cells[row][col].0.clone(),
                            image_uri: visible_cells[row][col].1.clone(),
                            row,
                            col,
                            width: column_widths[col],
                            height: row_heights[row],
                            selected: selected_cell == (row, col),
                            mode: cell_modes[row][col],
                            in_selection: selection_range.contains(row, col),
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
    cell: Option<Cell>,
    image_uri: Option<String>,
    row: usize,
    col: usize,
    width: u16,
    height: u16,
    selected: bool,
    mode: CellInteractionMode,
    in_selection: bool,
) -> Element {
    let value = cell_display_value(cell.as_ref(), mode);
    let error_message = cell.as_ref().and_then(|cell| match &cell.value {
        CellValue::Error(error) => Some(error.clone()),
        _ => None,
    });
    let status_class = match cell.as_ref().map(|cell| &cell.value) {
        Some(CellValue::FormulaPending { .. }) => " formula",
        Some(CellValue::Cached(_)) => " cached",
        Some(CellValue::Error(_)) => " error",
        _ => "",
    };
    let selected_class = if selected { " selected" } else { "" };
    let selection_class = if in_selection { " in-selection" } else { "" };
    let class = format!("cell{}{}{}", status_class, selected_class, selection_class);
    let style = format!("width: {}px; height: {}px;", width, height);
    let image_uri = image_uri.filter(|_| mode != CellInteractionMode::FormulaEdit);
    let editor_id = format!("cell-{row}-{col}");

    if mode == CellInteractionMode::FormulaEdit {
        use_effect(move || {
            let editor_id = editor_id.clone();
            spawn(async move {
                let script = format!(
                    r#"
                    const editor = document.getElementById({editor_id:?});
                    if (editor) {{
                        editor.focus();
                    }}
                    "#
                );
                let _ = dioxus::document::eval(&script).await;
            });
        });
    }

    if let Some(image_uri) = image_uri {
        return rsx! {
            div {
                id: "cell-{row}-{col}",
                class: "{class} image-cell",
                style,
                tabindex: "0",
                onmousedown: move |event| {
                    let extend = event.modifiers().shift();
                    let inserted_reference = state.with_mut(|state| {
                        state.begin_cell_interaction(row, col, extend)
                    });
                    if inserted_reference {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                },
                onmouseenter: move |_| {
                    state.with_mut(|state| {
                        if state.selecting {
                            state.extend_selection(row, col);
                        }
                    });
                },
                onkeydown: move |event| {
                    handle_cell_keydown(state, event, row, col);
                },
                img {
                    class: "cell-image-preview",
                    src: "{image_uri}",
                    alt: ""
                }
                if selected {
                    if let Some(error) = error_message.clone() {
                        ErrorPopover { message: error }
                    }
                }
            }
        };
    }

    if mode != CellInteractionMode::FormulaEdit {
        return rsx! {
            div {
                id: "cell-{row}-{col}",
                class,
                style,
                tabindex: "0",
                onmousedown: move |event| {
                    let extend = event.modifiers().shift();
                    let inserted_reference = state.with_mut(|state| {
                        state.begin_cell_interaction(row, col, extend)
                    });
                    if inserted_reference {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                },
                onmouseenter: move |_| {
                    state.with_mut(|state| {
                        if state.selecting {
                            state.extend_selection(row, col);
                        }
                    });
                },
                onkeydown: move |event| {
                    handle_cell_keydown(state, event, row, col);
                },
                "{value}"
                if selected {
                    if let Some(error) = error_message.clone() {
                        ErrorPopover { message: error }
                    }
                }
            }
        };
    }

    rsx! {
        input {
            id: "cell-{row}-{col}",
            class,
            style,
            value: "{value}",
            autocomplete: "off",
            autocorrect: "off",
            autocapitalize: "off",
            spellcheck: "false",
            onmousedown: move |event| {
                let extend = event.modifiers().shift();
                let inserted_reference = state.with_mut(|state| {
                    state.begin_cell_interaction(row, col, extend)
                });
                if inserted_reference {
                    event.prevent_default();
                    event.stop_propagation();
                }
            },
            onmouseenter: move |_| {
                state.with_mut(|state| {
                    if state.selecting {
                        state.extend_selection(row, col);
                    }
                });
            },
            onfocus: move |_| {
                state.with_mut(|state| {
                    if !state.selecting {
                        state.select_or_insert_cell_reference(row, col);
                    }
                });
            },
            onblur: move |_| {
                let work = state.with_mut(|state| prepare_provider_work(state, row, col));
                spawn_provider_work(state, work);
            },
            onkeydown: move |event| {
                handle_cell_keydown(state, event, row, col);
            },
            oninput: move |event| {
                let value = map_editor_text(event.value());
                state.with_mut(|state| state.set_editing_cell_input(row, col, value));
            },
            onpaste: move |event| {
                event.prevent_default();
                paste_into_cell_at_cursor(state, row, col);
            }
        }
        if selected {
            if let Some(error) = error_message.clone() {
                ErrorPopover { message: error }
            }
        }
    }
}

#[component]
fn ErrorPopover(message: String) -> Element {
    rsx! {
        div { class: "formula-error-popover",
            div { class: "formula-error-title", "Formula error" }
            div { class: "formula-error-message", "{message}" }
        }
    }
}

fn cached_image_uri(
    document: &crate::backend::document::CashewDocument,
    cell: &Cell,
) -> Option<String> {
    let cache_key = cell.cache_key.as_ref()?;
    let entry = document.cache.get(cache_key)?;
    if entry.status != CacheStatus::Ready {
        return None;
    }

    match &entry.value {
        CachedValue::MediaAsset(asset) if asset.media_type == MediaType::Image => {
            Some(asset.data_uri.clone().unwrap_or_else(|| asset.uri.clone()))
        }
        CachedValue::Text(_) | CachedValue::Json(_) | CachedValue::MediaAsset(_) => None,
    }
}

fn cell_display_value(
    cell: Option<&crate::backend::document::Cell>,
    mode: CellInteractionMode,
) -> String {
    let Some(cell) = cell else {
        return String::new();
    };

    if mode == CellInteractionMode::FormulaEdit {
        return cell.input.clone();
    }

    match &cell.value {
        CellValue::Empty => String::new(),
        CellValue::Text(value) => value.clone(),
        CellValue::FormulaPending { message } => message.clone(),
        CellValue::Cached(value) => value.clone(),
        CellValue::Error(_) => cell.input.clone(),
    }
}

fn handle_cell_keydown(mut state: Signal<AppState>, event: KeyboardEvent, row: usize, col: usize) {
    if shortcut_key(&event, "c") {
        event.prevent_default();
        let copied_text = state.with_mut(AppState::copy_selection);
        write_clipboard(copied_text);
        return;
    }

    if shortcut_key(&event, "x") {
        event.prevent_default();
        let copied_text = state.with_mut(AppState::cut_selection);
        write_clipboard(copied_text);
        return;
    }

    if event.key() == Key::Tab {
        let accepted = state.with_mut(|state| {
            matching_functions(&state.formula_input)
                .into_iter()
                .next()
                .map(|function| {
                    state.insert_formula(function);
                })
                .is_some()
        });
        if accepted {
            event.prevent_default();
        }
        return;
    }

    let editing_cell = state.read().cell_is_being_edited(row, col);
    if !editing_cell {
        match event.key() {
            Key::Character(value)
                if !event.modifiers().ctrl() && !event.modifiers().meta() && !value.is_empty() =>
            {
                event.prevent_default();
                state.with_mut(|state| state.set_editing_cell_input(row, col, value));
                return;
            }
            _ => {}
        }
    }

    let extend = event.modifiers().shift();
    match event.key() {
        Key::ArrowUp if !editing_cell => {
            event.prevent_default();
            state.with_mut(|state| {
                if extend {
                    state.extend_selection_with_keyboard(-1, 0)
                } else {
                    state.move_selection(-1, 0)
                };
            });
        }
        Key::ArrowDown if !editing_cell => {
            event.prevent_default();
            state.with_mut(|state| {
                if extend {
                    state.extend_selection_with_keyboard(1, 0)
                } else {
                    state.move_selection(1, 0)
                };
            });
        }
        Key::ArrowLeft if !editing_cell => {
            event.prevent_default();
            state.with_mut(|state| {
                if extend {
                    state.extend_selection_with_keyboard(0, -1)
                } else {
                    state.move_selection(0, -1)
                };
            });
        }
        Key::ArrowRight if !editing_cell => {
            event.prevent_default();
            state.with_mut(|state| {
                if extend {
                    state.extend_selection_with_keyboard(0, 1)
                } else {
                    state.move_selection(0, 1)
                };
            });
        }
        Key::Enter => {
            event.prevent_default();
            let work = state.with_mut(|state| {
                state.finish_cell_edit(row, col);
                prepare_provider_work(state, row, col)
            });
            if work.is_some() {
                spawn_provider_work(state, work);
            }
            state.with_mut(|state| {
                state.move_selection(1, 0);
            });
        }
        _ => {}
    }
}

fn install_clipboard_bridge() {
    let script = r#"
        if (!window.__cashewPasteBridgeInstalled) {
            window.__cashewPasteBridgeInstalled = true;
            window.__cashewLastPaste = "";
            document.addEventListener("paste", (event) => {
                const active = document.activeElement;
                if (!active || !active.classList || !active.classList.contains("cell")) {
                    return;
                }

                const clipboard = event.clipboardData || window.clipboardData;
                window.__cashewLastPaste = clipboard ? clipboard.getData("text/plain") : "";
                event.preventDefault();
            }, true);
        }
    "#;
    let _ = dioxus::document::eval(script);
}

fn shortcut_key(event: &KeyboardEvent, expected: &str) -> bool {
    let modifiers = event.modifiers();
    if !modifiers.ctrl() && !modifiers.meta() {
        return false;
    }

    match event.key() {
        Key::Character(value) => value.eq_ignore_ascii_case(expected),
        _ => false,
    }
}

fn write_clipboard(text: String) {
    let Ok(text_json) = serde_json::to_string(&text) else {
        return;
    };
    let script = format!(
        r#"
        const text = {text_json};
        if (navigator.clipboard && navigator.clipboard.writeText) {{
            await navigator.clipboard.writeText(text);
        }} else {{
            const textarea = document.createElement("textarea");
            textarea.value = text;
            textarea.style.position = "fixed";
            textarea.style.opacity = "0";
            document.body.appendChild(textarea);
            textarea.focus();
            textarea.select();
            document.execCommand("copy");
            textarea.remove();
        }}
        "#
    );
    let _ = dioxus::document::eval(&script);
}

fn paste_into_cell_at_cursor(mut state: Signal<AppState>, row: usize, col: usize) {
    spawn(async move {
        let script = format!(
            r#"
            const editor = document.getElementById({id:?});
            const paste = window.__cashewLastPaste || "";
            if (!editor) {{
                return paste;
            }}

            const value = editor.value || "";
            const start = editor.selectionStart ?? value.length;
            const end = editor.selectionEnd ?? value.length;
            const next = value.slice(0, start) + paste + value.slice(end);
            editor.value = next;
            const cursor = start + paste.length;
            editor.setSelectionRange(cursor, cursor);
            return next;
            "#,
            id = format!("cell-{row}-{col}")
        );

        if let Ok(text) = dioxus::document::eval(&script).await {
            let text = text.as_str().unwrap_or_default().to_string();
            state.with_mut(|state| state.set_editing_cell_input(row, col, text));
        }
    });
}

pub(crate) fn accept_highlighted_formula_completion(state: &mut AppState) -> bool {
    let matches = matching_functions(&state.formula_input);
    let Some(function) = matches
        .get(state.completion_index.min(matches.len().saturating_sub(1)))
        .copied()
    else {
        return false;
    };

    state.insert_formula(function);
    true
}

pub(crate) fn prepare_provider_work(state: &mut AppState, row: usize, col: usize) -> Option<ProviderWork> {
    state
        .prepare_llm_for_cell(row, col)
        .map(ProviderWork::Llm)
        .or_else(|| {
            state
                .prepare_generate_image_for_cell(row, col)
                .map(ProviderWork::GenerateImage)
        })
}

pub(crate) fn spawn_provider_work(state: Signal<AppState>, work: Option<ProviderWork>) {
    match work {
        Some(ProviderWork::Llm((row, col, input, cache_key, request))) => {
            spawn(async move {
                AppState::run_llm_for_cell(state, row, col, input, cache_key, request).await;
            });
        }
        Some(ProviderWork::GenerateImage((row, col, input, cache_key, request))) => {
            spawn(async move {
                AppState::run_generate_image_for_cell(state, row, col, input, cache_key, request)
                    .await;
            });
        }
        None => {}
    }
}

pub(crate) fn map_editor_text(value: String) -> String {
    value.replace(['“', '”'], "\"").replace(['‘', '’'], "'")
}

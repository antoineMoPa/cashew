use dioxus::prelude::*;

use crate::backend::cache::{CacheStatus, CachedValue, MediaType};
use crate::backend::document::{Cell, CellValue, column_name};
use crate::backend::formulas::{function_for_formula_input, matching_functions};

use super::super::state::{
    AppState, CellInteractionMode, MIN_VISIBLE_COLS, MIN_VISIBLE_ROWS, ResizeDrag, ResizeKind,
};

type LlmWork = (
    usize,
    usize,
    String,
    String,
    crate::backend::providers::openrouter::OpenRouterRequest,
    u64,
);

type GenerateImageWork = (
    usize,
    usize,
    String,
    String,
    crate::backend::providers::fal_image::GenerateImageRequest,
    u64,
);

type GenerateVideoWork = (
    usize,
    usize,
    String,
    String,
    crate::backend::providers::fal_video::GenerateVideoRequest,
    u64,
);

type ConcatenateVideoWork = (usize, usize, String, String, Vec<String>);

#[derive(Debug, Clone)]
pub(crate) enum ProviderWork {
    Llm(LlmWork),
    GenerateImage(GenerateImageWork),
    GenerateVideo(GenerateVideoWork),
    ConcatenateVideo(ConcatenateVideoWork),
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
    let selection_range = snapshot.selection_range();
    let selection_contains_multiple_cells = selection_range.contains_multiple_cells();
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
                    let media_preview = cell
                        .as_ref()
                        .and_then(|cell| cached_media_preview(&snapshot.document, cell));
                    (cell, media_preview)
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
                            media_preview: visible_cells[row][col].1.clone(),
                            row,
                            col,
                            width: column_widths[col],
                            height: row_heights[row],
                            selected: selected_cell == (row, col),
                            mode: cell_modes[row][col],
                            in_selection: selection_range.contains(row, col),
                            in_multi_cell_selection: selection_contains_multiple_cells,
                            selection_top: selection_range.start_row == row,
                            selection_bottom: selection_range.end_row == row,
                            selection_left: selection_range.start_col == col,
                            selection_right: selection_range.end_col == col,
                            fill_handle_cell: selection_range.end_row == row
                                && selection_range.end_col == col,
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
    media_preview: Option<(String, MediaType)>,
    row: usize,
    col: usize,
    width: u16,
    height: u16,
    selected: bool,
    mode: CellInteractionMode,
    in_selection: bool,
    in_multi_cell_selection: bool,
    selection_top: bool,
    selection_bottom: bool,
    selection_left: bool,
    selection_right: bool,
    fill_handle_cell: bool,
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
    let multi_cell_selection_class = if in_selection && in_multi_cell_selection {
        " multi-cell-selection"
    } else {
        ""
    };
    let selection_top_class = if in_selection && selection_top {
        " selection-top"
    } else {
        ""
    };
    let selection_bottom_class = if in_selection && selection_bottom {
        " selection-bottom"
    } else {
        ""
    };
    let selection_left_class = if in_selection && selection_left {
        " selection-left"
    } else {
        ""
    };
    let selection_right_class = if in_selection && selection_right {
        " selection-right"
    } else {
        ""
    };
    let class = format!(
        "cell{}{}{}{}{}{}{}{}",
        status_class,
        selected_class,
        selection_class,
        multi_cell_selection_class,
        selection_top_class,
        selection_bottom_class,
        selection_left_class,
        selection_right_class
    );
    let style = format!("width: {}px; height: {}px;", width, height);
    let media_preview = media_preview.filter(|_| mode != CellInteractionMode::FormulaEdit);
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

    if let Some((media_uri, media_type)) = media_preview {
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
                        if state.fill_dragging.is_some() {
                            state.update_fill_drag(row, col);
                        } else if state.selecting {
                            state.extend_selection(row, col);
                        }
                    });
                },
                onkeydown: move |event| {
                    handle_cell_keydown(state, event, row, col);
                },
                onpaste: move |event| {
                    event.prevent_default();
                    paste_into_selection(state, row, col);
                },
                if media_type == MediaType::Image {
                    img {
                        class: "cell-image-preview",
                        src: "{media_uri}",
                        alt: ""
                    }
                } else if media_type == MediaType::Video {
                    video {
                        class: "cell-image-preview",
                        src: "{media_uri}",
                        controls: true,
                        preload: "metadata"
                    }
                }
                if fill_handle_cell && mode != CellInteractionMode::FormulaEdit {
                    div {
                        class: "cell-fill-handle",
                        onmousedown: move |event| {
                            event.prevent_default();
                            event.stop_propagation();
                            state.with_mut(|state| state.begin_fill_drag(row, col));
                        }
                    }
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
                        if state.fill_dragging.is_some() {
                            state.update_fill_drag(row, col);
                        } else if state.selecting {
                            state.extend_selection(row, col);
                        }
                    });
                },
                onkeydown: move |event| {
                    handle_cell_keydown(state, event, row, col);
                },
                onpaste: move |event| {
                    event.prevent_default();
                    paste_into_selection(state, row, col);
                },
                "{value}"
                if fill_handle_cell && mode != CellInteractionMode::FormulaEdit {
                    div {
                        class: "cell-fill-handle",
                        onmousedown: move |event| {
                            event.prevent_default();
                            event.stop_propagation();
                            state.with_mut(|state| state.begin_fill_drag(row, col));
                        }
                    }
                }
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
                    if state.fill_dragging.is_some() {
                        state.update_fill_drag(row, col);
                    } else if state.selecting {
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
                state.with_mut(|state| state.commit_formula_buffer());
                queue_or_spawn_provider_work(state, row, col);
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
        if fill_handle_cell && mode != CellInteractionMode::FormulaEdit {
            div {
                class: "cell-fill-handle",
                onmousedown: move |event| {
                    event.prevent_default();
                    event.stop_propagation();
                    state.with_mut(|state| state.begin_fill_drag(row, col));
                }
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

fn cached_media_preview(
    document: &crate::backend::document::CashewDocument,
    cell: &Cell,
) -> Option<(String, MediaType)> {
    let cache_key = cell.cache_key.as_ref()?;
    let entry = document.cache.get(cache_key)?;
    if entry.status != CacheStatus::Ready {
        return None;
    }

    match &entry.value {
        CachedValue::MediaAsset(asset) if asset.media_type == MediaType::Image => Some((
            asset.data_uri.clone().unwrap_or_else(|| asset.uri.clone()),
            MediaType::Image,
        )),
        CachedValue::MediaAsset(asset) if asset.media_type == MediaType::Video => Some((
            asset.data_uri.clone().unwrap_or_else(|| asset.uri.clone()),
            MediaType::Video,
        )),
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
        Key::Backspace | Key::Delete if !editing_cell => {
            event.prevent_default();
            state.with_mut(AppState::clear_selection);
        }
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
            state.with_mut(|state| state.finish_cell_edit(row, col));
            queue_or_spawn_provider_work(state, row, col);
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
            navigator.clipboard.writeText(text).catch(() => {{
                const textarea = document.createElement("textarea");
                textarea.value = text;
                textarea.style.position = "fixed";
                textarea.style.opacity = "0";
                document.body.appendChild(textarea);
                textarea.focus();
                textarea.select();
                document.execCommand("copy");
                textarea.remove();
            }});
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

fn paste_into_selection(mut state: Signal<AppState>, row: usize, col: usize) {
    spawn(async move {
        let script = format!(
            r#"
            const editor = document.getElementById({id:?});
            const paste = window.__cashewLastPaste || "";
            if (!editor) {{
                return paste;
            }}

            return paste;
            "#,
            id = format!("cell-{row}-{col}")
        );

        if let Ok(text) = dioxus::document::eval(&script).await {
            let text = text.as_str().unwrap_or_default().to_string();
            state.with_mut(|state| state.paste_selection(&text));
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

pub(crate) fn prepare_provider_work(
    state: &mut AppState,
    row: usize,
    col: usize,
    approval_required: bool,
) -> Option<ProviderWork> {
    state
        .prepare_llm_for_cell(row, col)
        .map(ProviderWork::Llm)
        .or_else(|| {
            state
                .prepare_generate_image_for_cell(row, col, approval_required)
                .map(ProviderWork::GenerateImage)
        })
        .or_else(|| {
            state
                .prepare_generate_video_for_cell(row, col, approval_required)
                .map(ProviderWork::GenerateVideo)
        })
        .or_else(|| {
            state
                .prepare_concatenate_video_for_cell(row, col)
                .map(ProviderWork::ConcatenateVideo)
        })
}

pub(crate) fn spawn_provider_work(state: Signal<AppState>, work: Option<ProviderWork>) {
    match work {
        Some(ProviderWork::Llm((row, col, input, cache_key, request, network_call_id))) => {
            spawn(async move {
                AppState::run_llm_for_cell(
                    state,
                    row,
                    col,
                    input,
                    cache_key,
                    request,
                    network_call_id,
                )
                .await;
            });
        }
        Some(ProviderWork::GenerateImage((
            row,
            col,
            input,
            cache_key,
            request,
            network_call_id,
        ))) => {
            spawn(async move {
                AppState::run_generate_image_for_cell(
                    state,
                    row,
                    col,
                    input,
                    cache_key,
                    request,
                    network_call_id,
                )
                .await;
            });
        }
        Some(ProviderWork::GenerateVideo((
            row,
            col,
            input,
            cache_key,
            request,
            network_call_id,
        ))) => {
            spawn(async move {
                AppState::run_generate_video_for_cell(
                    state,
                    row,
                    col,
                    input,
                    cache_key,
                    request,
                    network_call_id,
                )
                .await;
            });
        }
        Some(ProviderWork::ConcatenateVideo((row, col, input, cache_key, video_inputs))) => {
            spawn(async move {
                AppState::run_concatenate_video_for_cell(
                    state,
                    row,
                    col,
                    input,
                    cache_key,
                    video_inputs,
                )
                .await;
            });
        }
        None => {}
    }
}

pub(crate) fn queue_or_spawn_provider_work(mut state: Signal<AppState>, row: usize, col: usize) {
    let approval_required = provider_requires_approval(&state.read(), row, col);
    let work = state.with_mut(|state| prepare_provider_work(state, row, col, approval_required));

    match (approval_required, work) {
        (_, None) => {}
        (true, Some(work)) => {
            state.with_mut(|state| state.queue_pending_provider_call(work));
        }
        (false, Some(work)) => {
            spawn_provider_work(state, Some(work));
        }
    }
}

fn provider_requires_approval(state: &AppState, row: usize, col: usize) -> bool {
    state
        .document
        .active_sheet()
        .and_then(|sheet| sheet.cell(row, col))
        .and_then(|cell| function_for_formula_input(&cell.input))
        .map(|function| !function.runs_without_approval)
        .unwrap_or(false)
}

pub(crate) fn map_editor_text(value: String) -> String {
    value.replace(['“', '”'], "\"").replace(['‘', '’'], "'")
}

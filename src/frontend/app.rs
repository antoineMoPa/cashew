use dioxus::prelude::Key;
use dioxus::prelude::*;

use super::components::{BottomPanel, FormulaBar, MenuBar, SettingsDialog, SheetView, StatusBar};
use super::state::{AppState, AppToast, CellInteractionMode, ResizeKind};

const APP_CSS: &str = include_str!("styles.css");

#[component]
pub fn App() -> Element {
    let mut state = use_signal(AppState::new);
    let snapshot = state.read();
    let bottom_panel_height = snapshot.bottom_panel_height;
    let toast = snapshot.toast.clone();
    drop(snapshot);

    let on_mouse_move = move |event: MouseEvent| {
        let coordinates = event.client_coordinates();
        state.with_mut(|state| {
            if let Some(resizing) = state.resizing {
                let coordinate = match resizing.kind {
                    ResizeKind::Column => coordinates.x as i32,
                    ResizeKind::Row | ResizeKind::BottomPanel => coordinates.y as i32,
                };
                state.update_resize(coordinate);
            }
        });
    };

    let on_mouse_up = move |_| {
        state.with_mut(|state| {
            state.resizing = None;
            state.finish_fill_drag();
            state.finish_selection();
            state.file_menu_open = false;
            state.edit_menu_open = false;
        });
    };

    rsx! {
        style { "{APP_CSS}" }
        main {
            class: "app",
            tabindex: "0",
            style: "--bottom-panel-height: {bottom_panel_height}px;",
            onmousemove: on_mouse_move,
            onmouseup: on_mouse_up,
            onmouseleave: on_mouse_up,
            onkeydown: move |event| {
                if let Some(shortcut) = file_shortcut(&event) {
                    event.prevent_default();
                    match shortcut {
                        FileShortcut::New => state.with_mut(AppState::new_document),
                        FileShortcut::Open => state.with_mut(AppState::open_document),
                        FileShortcut::Save => state.with_mut(AppState::save_document),
                        FileShortcut::SaveAs => state.with_mut(AppState::save_document_as),
                        FileShortcut::Settings => state.with_mut(AppState::open_settings),
                    }
                    return;
                }

                if select_all_shortcut(&event) {
                    let should_select_all = {
                        let snapshot = state.read();
                        snapshot.selected_cell_mode != CellInteractionMode::FormulaEdit
                            && !snapshot.settings_open
                    };

                    if should_select_all {
                        event.prevent_default();
                        state.with_mut(AppState::select_all_cells);
                    }
                }
            },
            MenuBar { state }
            FormulaBar { state }
            SheetView { state }
            BottomPanel { state }
            SettingsDialog { state }
            ToastViewport { toast }
            StatusBar { state }
        }
    }
}

#[component]
fn ToastViewport(toast: Option<AppToast>) -> Element {
    let Some(toast) = toast else {
        return rsx! {};
    };

    rsx! {
        div { class: "toast-viewport",
            div {
                key: "{toast.id}",
                class: "app-toast",
                "{toast.message}"
            }
        }
    }
}

enum FileShortcut {
    New,
    Open,
    Save,
    SaveAs,
    Settings,
}

fn file_shortcut(event: &KeyboardEvent) -> Option<FileShortcut> {
    let modifiers = event.modifiers();
    if !modifiers.meta() && !modifiers.ctrl() {
        return None;
    }

    match event.key() {
        Key::Character(value) if value.eq_ignore_ascii_case("n") && !modifiers.shift() => {
            Some(FileShortcut::New)
        }
        Key::Character(value) if value.eq_ignore_ascii_case("o") && !modifiers.shift() => {
            Some(FileShortcut::Open)
        }
        Key::Character(value) if value.eq_ignore_ascii_case("s") && modifiers.shift() => {
            Some(FileShortcut::SaveAs)
        }
        Key::Character(value) if value.eq_ignore_ascii_case("s") => Some(FileShortcut::Save),
        Key::Character(value) if value == "," && !modifiers.shift() => Some(FileShortcut::Settings),
        _ => None,
    }
}

fn select_all_shortcut(event: &KeyboardEvent) -> bool {
    let modifiers = event.modifiers();
    if !modifiers.meta() && !modifiers.ctrl() {
        return false;
    }

    matches!(event.key(), Key::Character(value) if value.eq_ignore_ascii_case("a"))
}

use dioxus::prelude::*;

use super::super::state::AppState;

#[cfg(target_os = "macos")]
const MENU_SHORTCUT_MODIFIER: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const MENU_SHORTCUT_MODIFIER: &str = "Ctrl";

#[component]
pub(crate) fn MenuBar(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let file_menu_open = snapshot.file_menu_open;
    drop(snapshot);

    rsx! {
        header { class: "top-shell",
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
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::new_document),
                                span { class: "menu-item-label", "New" }
                                span { class: "menu-shortcut", "{MENU_SHORTCUT_MODIFIER}+N" }
                            }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::open_document),
                                span { class: "menu-item-label", "Open..." }
                                span { class: "menu-shortcut", "{MENU_SHORTCUT_MODIFIER}+O" }
                            }
                            div { class: "menu-separator" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::save_document),
                                span { class: "menu-item-label", "Save" }
                                span { class: "menu-shortcut", "{MENU_SHORTCUT_MODIFIER}+S" }
                            }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::save_document_as),
                                span { class: "menu-item-label", "Save As..." }
                                span { class: "menu-shortcut", "{MENU_SHORTCUT_MODIFIER}+Shift+S" }
                            }
                            div { class: "menu-separator" }
                            button { class: "menu-item", onclick: move |_| state.with_mut(AppState::open_settings),
                                span { class: "menu-item-label", "Settings..." }
                                span { class: "menu-shortcut", "{MENU_SHORTCUT_MODIFIER}+," }
                            }
                        }
                    }
                }
                button { class: "menu-trigger disabled", "Edit" }
            }
        }
    }
}

use dioxus::prelude::*;

use super::components::{FormulaBar, MenuBar, SheetView, StatusBar};
use super::state::{AppState, ResizeKind};

const APP_CSS: &str = include_str!("styles.css");

#[component]
pub fn App() -> Element {
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

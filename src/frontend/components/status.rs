use dioxus::prelude::*;

use super::super::state::AppState;

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

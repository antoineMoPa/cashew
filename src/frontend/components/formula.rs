use dioxus::prelude::Key;
use dioxus::prelude::*;

use crate::backend::document::cell_key;
use crate::backend::formulas::{FormulaFunction, matching_functions};

use super::super::state::{AppState, should_show_completions};
use super::sheet::{
    accept_highlighted_formula_completion, map_editor_text, prepare_provider_work,
    spawn_provider_work,
};

#[component]
pub(crate) fn FormulaBar(mut state: Signal<AppState>) -> Element {
    let snapshot = state.read();
    let (row, col) = snapshot.selected_cell;
    let address = cell_key(row, col);
    let formula_input = snapshot.formula_input.clone();
    let formula_input_revision = snapshot.formula_input_revision;
    let completion_index = snapshot.completion_index;
    drop(snapshot);
    let formula_input_key = format!("{row}-{col}-{formula_input_revision}");
    let formula_matches = matching_functions(&formula_input);
    let highlighted_completion = formula_matches
        .get(completion_index.min(formula_matches.len().saturating_sub(1)))
        .copied();
    let autocomplete_suffix = highlighted_completion
        .and_then(|function| formula_completion_suffix(&formula_input, function))
        .unwrap_or_default();

    rsx! {
        section { class: "formula-bar",
            div { class: "name-box", "{address}" }
            div { class: "fx-label", "fx" }
            div { class: "formula-input-wrap",
                div {
                    class: "formula-autocomplete",
                    span { class: "formula-autocomplete-prefix", "{formula_input}" }
                    span { class: "formula-autocomplete-suffix", "{autocomplete_suffix}" }
                }
                input {
                    key: "{formula_input_key}",
                    class: "formula-input",
                    initial_value: "{formula_input}",
                    autocomplete: "off",
                    autocorrect: "off",
                    autocapitalize: "off",
                    spellcheck: "false",
                    onfocus: move |_| state.with_mut(|state| {
                        state.completions_open = should_show_completions(&state.formula_input);
                    }),
                    onblur: move |_| {
                        let work = state.with_mut(|state| {
                            state.commit_formula_buffer();
                            let (row, col) = state.selected_cell;
                            prepare_provider_work(state, row, col)
                        });
                        spawn_provider_work(state, work);
                    },
                    onkeydown: move |event| {
                        let matches = matching_functions(&state.read().formula_input);
                        match event.key() {
                            Key::ArrowUp => {
                                if !matches.is_empty() {
                                    event.prevent_default();
                                    state.with_mut(|state| {
                                        state.move_completion_selection(-1, matches.len());
                                    });
                                }
                            }
                            Key::ArrowDown => {
                                if !matches.is_empty() {
                                    event.prevent_default();
                                    state.with_mut(|state| {
                                        state.move_completion_selection(1, matches.len());
                                    });
                                }
                            }
                            Key::Tab => {
                                let accepted = state.with_mut(|state| {
                                    accept_highlighted_formula_completion(state)
                                });
                                if accepted {
                                    event.prevent_default();
                                }
                            }
                            Key::Enter => {
                                event.prevent_default();
                                let work = state.with_mut(|state| {
                                    state.commit_formula_buffer();
                                    state.finish_formula_edit();
                                    let (row, col) = state.selected_cell;
                                    prepare_provider_work(state, row, col)
                                });
                                spawn_provider_work(state, work);
                            }
                            _ => {}
                        }
                    },
                    oninput: move |event| {
                        let value = map_editor_text(event.value());
                        state.with_mut(|state| state.set_formula_buffer(value));
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
    let selected_index = snapshot.completion_index;
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
            for (index, function) in matches.into_iter().enumerate() {
                {
                    let selected_class = if index == selected_index { " selected" } else { "" };
                    let class = format!("completion-item{selected_class}");
                    rsx! {
                button {
                    class,
                    onmousedown: move |event| {
                        event.prevent_default();
                        event.stop_propagation();
                        state.with_mut(|state| state.insert_formula(function));
                    },
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
    }
}

fn formula_completion_suffix(input: &str, function: FormulaFunction) -> Option<String> {
    let typed = input.trim_start();
    if typed.len() <= 1 {
        return None;
    }

    let insert_text = function.insert_text;
    insert_text
        .get(typed.len()..)
        .filter(|_| insert_text[..typed.len()].eq_ignore_ascii_case(typed))
        .map(str::to_string)
}

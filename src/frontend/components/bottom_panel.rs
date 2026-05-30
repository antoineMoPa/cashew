use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::md_content_icons::MdContentCopy};

use crate::backend::formulas::{
    FORMULA_FUNCTIONS, FormulaFunction, FormulaModelDoc, formula_example_for_function,
    function_for_formula_input, matching_functions, models_for_function,
    related_functions_for_function,
};

use super::super::state::{
    AppState, BottomPanelTab, NetworkCallRecord, NetworkCallStatus, ResizeDrag, ResizeKind,
};

#[component]
pub(crate) fn BottomPanel(mut state: Signal<AppState>) -> Element {
    let selected_doc_function = use_signal(|| None::<FormulaFunction>);
    let snapshot = state.read();
    let active_tab = snapshot.bottom_panel_tab;
    let formula_input = snapshot.formula_input.clone();
    let calls = snapshot.network_calls.clone();
    let bottom_panel_height = snapshot.bottom_panel_height;
    drop(snapshot);

    rsx! {
        section { class: "bottom-panel",
            div { class: "bottom-tabs",
                div {
                    class: "bottom-panel-resize-handle",
                    onmousedown: move |event| {
                        event.prevent_default();
                        event.stop_propagation();
                        let start = event.client_coordinates().y as i32;
                        state.with_mut(|state| {
                            state.resizing = Some(ResizeDrag {
                                kind: ResizeKind::BottomPanel,
                                index: 0,
                                start,
                                original: bottom_panel_height,
                            });
                        });
                    }
                }
                div { class: "bottom-tabs-row",
                    TabButton {
                        label: "Docs",
                        active: active_tab == BottomPanelTab::FunctionDocs,
                        onclick: move |_| state.with_mut(|state| state.set_bottom_panel_tab(BottomPanelTab::FunctionDocs)),
                    }
                    TabButton {
                        label: "Network Calls",
                        active: active_tab == BottomPanelTab::NetworkCalls,
                        onclick: move |_| state.with_mut(|state| state.set_bottom_panel_tab(BottomPanelTab::NetworkCalls)),
                    }
                }
            }
            div { class: "bottom-panel-body",
                match active_tab {
                    BottomPanelTab::FunctionDocs => rsx! {
                        DocsPanel { state, selected_doc_function, formula_input }
                    },
                    BottomPanelTab::NetworkCalls => rsx! {
                        NetworkCallsPanel { calls }
                    },
                }
            }
        }
    }
}

#[component]
fn TabButton(label: &'static str, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let class = if active {
        "bottom-tab active"
    } else {
        "bottom-tab"
    };

    rsx! {
        button {
            class,
            onclick: move |event| onclick.call(event),
            "{label}"
        }
    }
}

#[component]
fn DocsPanel(
    state: Signal<AppState>,
    mut selected_doc_function: Signal<Option<FormulaFunction>>,
    formula_input: String,
) -> Element {
    if formula_input.trim().is_empty() {
        if let Some(function) = *selected_doc_function.read() {
            return rsx! {
                FunctionDocsPanel {
                    state,
                    function,
                    on_navigate: move |function| selected_doc_function.set(Some(function)),
                }
            };
        }

        return rsx! {
            AllDocsPanel {
                on_navigate: move |function| selected_doc_function.set(Some(function)),
            }
        };
    }

    let function = function_for_formula_input(&formula_input);

    if let Some(function) = function {
        return rsx! {
            FunctionDocsPanel {
                state,
                function,
                on_navigate: move |function| selected_doc_function.set(Some(function)),
            }
        };
    }

    let matches = matching_functions(&formula_input);

    if matches.is_empty() {
        return rsx! {
            div { class: "panel-empty",
                "Select or type a formula to see function documentation."
            }
        };
    }

    rsx! {
        DocsCompletionsPanel {
            formula_input,
            matches,
            on_navigate: move |function| selected_doc_function.set(Some(function)),
        }
    }
}

#[component]
fn DocsCompletionsPanel(
    formula_input: String,
    matches: Vec<FormulaFunction>,
    on_navigate: EventHandler<FormulaFunction>,
) -> Element {
    rsx! {
        div { class: "function-docs",
            div { class: "doc-header",
                div {
                    div { class: "doc-title", "Completions" }
                    div { class: "doc-summary", "Possible formulas for {formula_input}" }
                }
                div { class: "doc-muted", "{matches.len()} matches" }
            }
            div { class: "doc-index completion-doc-index",
                for function in matches {
                    button {
                        class: "doc-card doc-completion-card",
                        onmousedown: move |event| {
                            event.prevent_default();
                            event.stop_propagation();
                        },
                        onclick: move |_| {
                            on_navigate.call(function);
                        },
                        div { class: "doc-card-head",
                            div { class: "doc-card-title", "{function.name}" }
                            code { class: "doc-card-signature", "{function.signature}" }
                        }
                        div { class: "doc-summary", "{function.summary}" }
                        div { class: "doc-description", "{function.details}" }
                    }
                }
            }
        }
    }
}

#[component]
fn FunctionDocsPanel(
    state: Signal<AppState>,
    function: FormulaFunction,
    on_navigate: EventHandler<FormulaFunction>,
) -> Element {
    let models = models_for_function(function);
    let related = related_functions_for_function(function);
    let selected_model = use_signal(|| None::<String>);
    let active_model_id = selected_model
        .read()
        .clone()
        .filter(|candidate| models.iter().any(|model| model.id == candidate))
        .or_else(|| {
            models
                .iter()
                .find(|model| model.default)
                .map(|model| model.id.to_string())
        });
    let example_formula = formula_example_for_function(function, active_model_id.as_deref());

    rsx! {
        div { class: "function-docs",
            div { class: "doc-header",
                div {
                    div { class: "doc-title", "{function.name}" }
                    div { class: "doc-summary", "{function.summary}" }
                }
                div { class: "doc-signature-row",
                    pre { class: "doc-signature", "{example_formula}" }
                    CopyFormulaButton { state, text: example_formula.clone() }
                }
            }
            div { class: "doc-grid",
                DocArguments { function }
                DocModels { selected_model, active_model_id, models }
                div { class: "doc-side-sections",
                    DocNotes { function }
                    DocSeeAlso { related, on_navigate }
                }
            }
        }
    }
}

#[component]
fn CopyFormulaButton(mut state: Signal<AppState>, text: String) -> Element {
    rsx! {
        div { class: "doc-copy-control",
            button {
                class: "doc-signature-copy",
                title: "Copy formula",
                onmousedown: move |event| {
                    event.prevent_default();
                    event.stop_propagation();
                },
                onclick: move |event| {
                    event.stop_propagation();
                    copy_text_to_clipboard(text.clone());
                    state.with_mut(|state| state.show_toast("Formula copied"));
                },
                Icon {
                    icon: MdContentCopy,
                    width: 18,
                    height: 18,
                    fill: "currentColor",
                }
                span { class: "sr-only", "Copy formula" }
            }
        }
    }
}

fn copy_text_to_clipboard(text: String) {
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

#[component]
fn DocSeeAlso(
    related: Vec<FormulaFunction>,
    on_navigate: EventHandler<FormulaFunction>,
) -> Element {
    rsx! {
        section { class: "doc-section",
            h3 { "See Also" }
            if related.is_empty() {
                div { class: "doc-muted", "No related formulas documented yet." }
            } else {
                div { class: "see-also-list",
                    for function in related {
                        button {
                            class: "see-also-item",
                            onmousedown: move |event| {
                                event.prevent_default();
                                event.stop_propagation();
                            },
                            onclick: move |_| on_navigate.call(function),
                            code { "{function.name}" }
                            div { class: "doc-description", "{function.summary}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AllDocsPanel(on_navigate: EventHandler<FormulaFunction>) -> Element {
    rsx! {
        div { class: "function-docs",
            div { class: "doc-header",
                div {
                    div { class: "doc-title", "All docs" }
                    div { class: "doc-summary", "No cell is selected. Browse every available formula here." }
                }
                div { class: "doc-muted", "{FORMULA_FUNCTIONS.len()} formulas" }
            }
            div { class: "doc-index",
                for function in FORMULA_FUNCTIONS {
                    button {
                        class: "doc-card doc-nav-card",
                        onmousedown: move |event| {
                            event.prevent_default();
                            event.stop_propagation();
                        },
                        onclick: move |_| on_navigate.call(*function),
                        div { class: "doc-card-head",
                            div { class: "doc-card-title", "{function.name}" }
                            code { class: "doc-card-signature", "{function.signature}" }
                        }
                        div { class: "doc-summary", "{function.summary}" }
                        div { class: "doc-description", "{function.details}" }
                    }
                }
            }
        }
    }
}

#[component]
fn DocArguments(function: FormulaFunction) -> Element {
    rsx! {
        section { class: "doc-section",
            h3 { "Parameters" }
            if function.arguments.is_empty() {
                div { class: "doc-muted", "No parameters documented yet." }
            } else {
                div { class: "doc-table",
                    for argument in function.arguments {
                        div { class: "doc-row",
                            div {
                                code { "{argument.name}" }
                                span { class: "doc-chip", if argument.required { "required" } else { "optional" } }
                            }
                            div { class: "doc-kind", "{argument.kind}" }
                            div { class: "doc-description", "{argument.description}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DocModels(
    mut selected_model: Signal<Option<String>>,
    active_model_id: Option<String>,
    models: Vec<FormulaModelDoc>,
) -> Element {
    rsx! {
        section { class: "doc-section",
            h3 { "Models" }
            if models.is_empty() {
                div { class: "doc-muted", "No model list for this function." }
            } else {
                div { class: "model-list",
                    for model in models {
                        {
                            let is_selected = active_model_id
                                .as_deref()
                                .map(|value| value == model.id)
                                .unwrap_or(false);
                            let class = if is_selected {
                                "model-item selected"
                            } else {
                                "model-item"
                            };

                            rsx! {
                                button {
                                    class,
                                    onmousedown: move |event| {
                                        event.prevent_default();
                                        event.stop_propagation();
                                    },
                                    onclick: move |_| {
                                        selected_model.set(Some(model.id.to_string()));
                                    },
                                    div { class: "model-heading",
                                        code { "{model.id}" }
                                        if model.default {
                                            span { class: "doc-chip default", "default" }
                                        }
                                    }
                                    div { class: "model-label", "{model.label}" }
                                    div { class: "doc-description", "{model.description}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DocNotes(function: FormulaFunction) -> Element {
    rsx! {
        section { class: "doc-section",
            h3 { "Notes" }
            if function.notes.is_empty() {
                div { class: "doc-muted", "{function.details}" }
            } else {
                ul { class: "doc-notes",
                    for note in function.notes {
                        li { "{note}" }
                    }
                }
            }
        }
    }
}

#[component]
fn NetworkCallsPanel(calls: Vec<NetworkCallRecord>) -> Element {
    if calls.is_empty() {
        return rsx! {
            div { class: "panel-empty",
                "Provider requests will appear here when formulas call fal."
            }
        };
    }

    rsx! {
        div { class: "network-list",
            for call in calls.iter().rev() {
                NetworkCallItem { call: call.clone() }
            }
        }
    }
}

#[component]
fn NetworkCallItem(call: NetworkCallRecord) -> Element {
    let status = match call.status {
        NetworkCallStatus::PendingApproval => "pending",
        NetworkCallStatus::Running => "running",
        NetworkCallStatus::Completed => "completed",
        NetworkCallStatus::Failed => "failed",
    };
    let body = serde_json::to_string_pretty(&call.request_body)
        .unwrap_or_else(|_| "<unserializable request>".to_string());

    rsx! {
        article { class: "network-call",
            div { class: "network-call-header",
                div {
                    span { class: "network-cell", "{call.cell}" }
                    span { class: "network-function", "{call.function_name}" }
                    span { class: "network-provider", "{call.provider}" }
                }
                span { class: "network-status {status}", "{status}" }
            }
            div { class: "network-url", "{call.url}" }
            if !call.image_inputs.is_empty() {
                div { class: "network-images",
                    for image in call.image_inputs {
                        img {
                            class: "network-image",
                            src: "{image}",
                            alt: "request input image"
                        }
                    }
                }
            }
            if let Some(error) = call.error_message {
                div { class: "network-error",
                    div { class: "network-error-title", "Error output" }
                    pre { class: "network-error-message", "{error}" }
                }
            }
            pre { class: "network-body", "{body}" }
        }
    }
}

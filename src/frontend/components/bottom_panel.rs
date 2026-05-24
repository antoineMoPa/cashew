use dioxus::prelude::*;

use crate::backend::formulas::{
    FormulaFunction, FormulaModelDoc, formula_example_for_function, function_for_formula_input,
    models_for_function,
};

use super::super::state::{
    AppState, BottomPanelTab, NetworkCallRecord, NetworkCallStatus, ResizeDrag, ResizeKind,
};

#[component]
pub(crate) fn BottomPanel(mut state: Signal<AppState>) -> Element {
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
                        FunctionDocsPanel { formula_input }
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
fn FunctionDocsPanel(formula_input: String) -> Element {
    let function = function_for_formula_input(&formula_input);

    let Some(function) = function else {
        return rsx! {
            div { class: "panel-empty",
                "Select or type a formula to see function documentation."
            }
        };
    };

    let models = models_for_function(function);
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
                code { class: "doc-signature", "{example_formula}" }
            }
            div { class: "doc-grid",
                DocArguments { function }
                DocModels { selected_model, active_model_id, models }
                DocNotes { function }
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
            pre { class: "network-body", "{body}" }
        }
    }
}

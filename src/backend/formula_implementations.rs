use super::providers::openrouter::OpenRouterRequest;

mod concatenate_video;
mod evaluation;
mod generate_image;
mod generate_video;
mod json_extract;
mod llm;
mod math;
mod segment;

pub use concatenate_video::concatenate_video_inputs_for_sheet;
pub use evaluation::{evaluate_formula_for_sheet, format_number, parse_cell_reference};
pub use generate_image::generate_image_request_for_sheet;
pub use generate_video::generate_video_request_for_sheet;
pub use llm::{llm_request_for_sheet, parse_llm_output};
pub use segment::segment_request_for_sheet;

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaValue {
    Number(f64),
    Text(String),
    Cached {
        value: String,
        cache_key: Option<String>,
    },
    Pending(String),
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmOutputMode {
    Text,
    ListDown,
    ListRight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub function_name: &'static str,
    pub output_mode: LlmOutputMode,
    pub request: OpenRouterRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmFormulaOutput {
    Text(String),
    List(Vec<String>),
}

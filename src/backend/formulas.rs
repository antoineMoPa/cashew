use std::sync::OnceLock;

use super::providers::{
    fal_image::image_model_docs,
    fal_video::video_model_docs,
    fal_video::{
        video_model_default_duration, video_model_requires_end_image,
        video_model_supports_aspect_ratio,
    },
    openrouter::DEFAULT_MODEL as DEFAULT_LLM_MODEL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaFunction {
    pub name: &'static str,
    pub signature: &'static str,
    pub insert_text: &'static str,
    pub runs_without_approval: bool,
    pub summary: &'static str,
    pub details: &'static str,
    pub arguments: &'static [FormulaArgumentDoc],
    pub models: &'static [FormulaModelDoc],
    pub notes: &'static [&'static str],
    pub implementation: FormulaImplementation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaArgumentDoc {
    pub name: &'static str,
    pub kind: &'static str,
    pub required: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaModelDoc {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaImplementation {
    ProviderAi { provider: &'static str },
    Math(MathFunction),
    ConcatenateText,
    LocalVideoConcat,
    JsonExtract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFunction {
    Sum,
    Product,
    Average,
    Min,
    Max,
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormulaTopic {
    LlmText,
    MediaGeneration,
    Arithmetic,
}

const GENERATE_IMAGE_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "prompt",
        kind: "text",
        required: true,
        description: "Prompt text or a cell reference resolving to prompt text.",
    },
    FormulaArgumentDoc {
        name: "model",
        kind: "model id",
        required: true,
        description: "Supported fal image model id.",
    },
    FormulaArgumentDoc {
        name: "quality?",
        kind: "low | medium | high",
        required: false,
        description: "Optional quality for openai/gpt-image-2. Defaults to medium.",
    },
    FormulaArgumentDoc {
        name: "image...",
        kind: "URL or data URI",
        required: false,
        description: "Optional image input cells for edit/image-to-image models.",
    },
];

const SEGMENT_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "image",
        kind: "URL or data URI",
        required: true,
        description: "Source image URL or a cell reference resolving to one.",
    },
    FormulaArgumentDoc {
        name: "prompt?",
        kind: "text",
        required: false,
        description: "Optional segmentation prompt. Defaults to wheel.",
    },
    FormulaArgumentDoc {
        name: "point_prompts?",
        kind: "JSON array",
        required: false,
        description: "Optional JSON array of point prompt objects with x, y, label, and object_id fields.",
    },
    FormulaArgumentDoc {
        name: "box_prompts?",
        kind: "JSON array",
        required: false,
        description: "Optional JSON array of box prompt objects with x_min, y_min, x_max, y_max, and object_id fields.",
    },
    FormulaArgumentDoc {
        name: "apply_mask?",
        kind: "boolean",
        required: false,
        description: "Whether to apply the mask on the source image. Defaults to true.",
    },
    FormulaArgumentDoc {
        name: "output_format?",
        kind: "jpeg | png | webp",
        required: false,
        description: "Optional mask image format. Defaults to png.",
    },
    FormulaArgumentDoc {
        name: "return_multiple_masks?",
        kind: "boolean",
        required: false,
        description: "Return multiple mask candidates when true.",
    },
    FormulaArgumentDoc {
        name: "max_masks?",
        kind: "integer",
        required: false,
        description: "Maximum number of masks to return when multiple masks are enabled. Defaults to 3.",
    },
    FormulaArgumentDoc {
        name: "include_scores?",
        kind: "boolean",
        required: false,
        description: "Include per-mask confidence scores when true.",
    },
    FormulaArgumentDoc {
        name: "include_boxes?",
        kind: "boolean",
        required: false,
        description: "Include per-mask bounding boxes when true.",
    },
];

const LLM_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "prompt",
        kind: "text",
        required: true,
        description: "Prompt text or a cell reference resolving to prompt text.",
    },
    FormulaArgumentDoc {
        name: "model",
        kind: "model id",
        required: false,
        description: "OpenRouter model id routed through fal. Defaults to Gemini 2.5 Flash.",
    },
    FormulaArgumentDoc {
        name: "image...",
        kind: "URL or data URI",
        required: false,
        description: "Optional image inputs. Put these before the optional system_prompt.",
    },
    FormulaArgumentDoc {
        name: "system_prompt",
        kind: "text",
        required: false,
        description: "Optional system instruction text. Place it after any image inputs.",
    },
];

const LLM_MODELS: &[FormulaModelDoc] = &[FormulaModelDoc {
    id: DEFAULT_LLM_MODEL,
    label: "Gemini 2.5 Flash",
    description: "Default OpenRouter model id sent through fal openrouter/router.",
    default: true,
}];

const LLM_SPILL_NOTES: &[&str] = &[
    "Cache-first: identical formulas with identical resolved inputs reuse stored results.",
    "Image inputs are resolved before the provider request is built.",
    "The response is parsed into cells after the provider returns.",
];

const GENERATE_VIDEO_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "prompt",
        kind: "text",
        required: true,
        description: "Prompt text describing the desired motion.",
    },
    FormulaArgumentDoc {
        name: "start_image",
        kind: "URL or data URI",
        required: true,
        description: "First frame image URL or a cell reference resolving to one.",
    },
    FormulaArgumentDoc {
        name: "end_image?",
        kind: "URL or data URI",
        required: false,
        description: "Optional second frame image for Kling O1 first-frame last-frame models.",
    },
    FormulaArgumentDoc {
        name: "model?",
        kind: "model id",
        required: false,
        description: "Optional fal video model id. Defaults to Veo 3.1 reference-to-video.",
    },
    FormulaArgumentDoc {
        name: "duration?",
        kind: "model-specific seconds",
        required: false,
        description: "Optional clip duration in seconds. Allowed values depend on the selected model.",
    },
    FormulaArgumentDoc {
        name: "aspect_ratio?",
        kind: "16:9 | 9:16 | auto",
        required: false,
        description: "Optional output aspect ratio when the selected model supports it. Kling ignores this.",
    },
];

const CONCATENATE_VIDEO_ARGUMENTS: &[FormulaArgumentDoc] = &[FormulaArgumentDoc {
    name: "video...",
    kind: "URL, path, or data URI",
    required: true,
    description: "Two or more video inputs to concatenate in order.",
}];

const NUMBER_ARGUMENTS: &[FormulaArgumentDoc] = &[FormulaArgumentDoc {
    name: "number",
    kind: "number",
    required: true,
    description: "Numeric literal, arithmetic expression, or numeric cell reference.",
}];

const TWO_NUMBER_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "left",
        kind: "number",
        required: true,
        description: "Left numeric value.",
    },
    FormulaArgumentDoc {
        name: "right",
        kind: "number",
        required: true,
        description: "Right numeric value.",
    },
];

const CONCATENATE_ARGUMENTS: &[FormulaArgumentDoc] = &[FormulaArgumentDoc {
    name: "text",
    kind: "text",
    required: true,
    description: "One or more text values, cell references, or quoted string literals.",
}];

const JSON_EXTRACT_ARGUMENTS: &[FormulaArgumentDoc] = &[
    FormulaArgumentDoc {
        name: "input",
        kind: "text",
        required: true,
        description: "JSON text, a quoted JSON literal, or a cell reference resolving to JSON text.",
    },
    FormulaArgumentDoc {
        name: "path",
        kind: "JSONPath",
        required: true,
        description: "JSONPath query string, for example $.user.name or $.items[0].title.",
    },
];

pub const FORMULA_FUNCTIONS: &[FormulaFunction] = &[
    FormulaFunction {
        name: "GENERATEIMAGE",
        signature: "GENERATEIMAGE(prompt, model, quality?, image...)",
        insert_text: "=GENERATEIMAGE(prompt, \"flux/dev\")",
        runs_without_approval: false,
        summary: "Generate or edit an image through fal.",
        details: "Runs through fal image endpoints. The selected model determines the request contract, endpoint pair, and whether quality and multiple image inputs are supported.",
        arguments: GENERATE_IMAGE_ARGUMENTS,
        models: &[],
        notes: &[
            "Cache-first: identical formulas with identical resolved inputs reuse stored results.",
            "Image inputs are resolved before the provider request is built.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.image",
        },
    },
    FormulaFunction {
        name: "SEGMENT",
        signature: "SEGMENT(image, prompt?, point_prompts?, box_prompts?, apply_mask?, output_format?, return_multiple_masks?, max_masks?, include_scores?, include_boxes?)",
        insert_text: "=SEGMENT($, \"wheel\")",
        runs_without_approval: false,
        summary: "Segment objects in an image through fal SAM 3.",
        details: "Runs through fal.ai's SAM 3 image segmentation endpoint. The first returned mask is stored in the cell while the full response remains in the document cache.",
        arguments: SEGMENT_ARGUMENTS,
        models: &[],
        notes: &[
            "Cache-first: identical formulas with identical resolved inputs reuse stored results.",
            "Point and box prompt arguments accept JSON arrays or JSON objects stored in cells.",
            "The primary mask preview is shown in the cell and the full response is preserved in cache metadata.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.segment",
        },
    },
    FormulaFunction {
        name: "GENERATEVIDEO",
        signature: "GENERATEVIDEO(prompt, start_image, end_image?, model?, duration?, aspect_ratio?)",
        insert_text: "=GENERATEVIDEO(prompt, $, \"fal-ai/veo3.1/reference-to-video\", 8, \"16:9\")",
        runs_without_approval: false,
        summary: "Generate or reuse a video clip from prompt and media inputs.",
        details: "Runs through fal video endpoints. The selected model determines whether the request uses image_url, image_urls, start_image_url, or start_image_url plus end_image_url, and which duration/aspect-ratio options are valid. For Kling O1 first-frame last-frame models, place the end image immediately after the start image.",
        arguments: GENERATE_VIDEO_ARGUMENTS,
        models: &[],
        notes: &[
            "Default model: fal-ai/veo3.1/reference-to-video.",
            "Kling O1 first-frame last-frame models use the second image argument as the end frame.",
            "Kling v3 models infer aspect ratio from the source image and ignore the aspect_ratio argument.",
            "Cache-first: identical formulas with identical resolved inputs reuse stored results.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.video",
        },
    },
    FormulaFunction {
        name: "CONCATENATEVIDEO",
        signature: "CONCATENATEVIDEO(video, ...)",
        insert_text: "=CONCATENATEVIDEO(video_a, video_b)",
        runs_without_approval: true,
        summary: "Concatenate generated clips into a longer video.",
        details: "Concatenates two or more clips locally with ffmpeg. If ffmpeg is not installed, the formula returns an install warning.",
        arguments: CONCATENATE_VIDEO_ARGUMENTS,
        models: &[],
        notes: &["Runs locally and stores the concatenated result in the document cache."],
        implementation: FormulaImplementation::LocalVideoConcat,
    },
    FormulaFunction {
        name: "LLM",
        signature: "LLM(prompt, model?, image..., system_prompt?)",
        insert_text: "=LLM(prompt, \"google/gemini-2.5-flash\", $, system_prompt)",
        runs_without_approval: true,
        summary: "Generate or transform text through fal OpenRouter.",
        details: "Runs through fal endpoint openrouter/router for text-only requests and openrouter/router/vision when image inputs are provided.",
        arguments: LLM_ARGUMENTS,
        models: LLM_MODELS,
        notes: &[
            "The model argument accepts OpenRouter model ids routed through fal.",
            "The listed model is the default used when the model argument is empty.",
            "Image inputs must be URLs, data URIs, or cells that resolve to one.",
            "When you provide images, place them before the optional system prompt.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.openrouter",
        },
    },
    FormulaFunction {
        name: "LLM_LIST_DOWN",
        signature: "LLM_LIST_DOWN(prompt, model?, image..., system_prompt?)",
        insert_text: "=LLM_LIST_DOWN(prompt, \"google/gemini-2.5-flash\", $, system_prompt)",
        runs_without_approval: true,
        summary: "Generate a vertical list through fal OpenRouter.",
        details: "Runs through fal endpoint openrouter/router or openrouter/router/vision, then spills the response into a single column.",
        arguments: LLM_ARGUMENTS,
        models: LLM_MODELS,
        notes: LLM_SPILL_NOTES,
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.openrouter",
        },
    },
    FormulaFunction {
        name: "LLM_LIST_RIGHT",
        signature: "LLM_LIST_RIGHT(prompt, model?, image..., system_prompt?)",
        insert_text: "=LLM_LIST_RIGHT(prompt, \"google/gemini-2.5-flash\", $, system_prompt)",
        runs_without_approval: true,
        summary: "Generate a horizontal list through fal OpenRouter.",
        details: "Runs through fal endpoint openrouter/router or openrouter/router/vision, then spills the response into a single row.",
        arguments: LLM_ARGUMENTS,
        models: LLM_MODELS,
        notes: LLM_SPILL_NOTES,
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.openrouter",
        },
    },
    FormulaFunction {
        name: "JSONEXTRACT",
        signature: "JSONEXTRACT(input, path)",
        insert_text: "=JSONEXTRACT(input, \"$.field\")",
        runs_without_approval: true,
        summary: "Extract a value from JSON text.",
        details: "Parses JSON input and evaluates a JSONPath query. Strings, numbers, and booleans return text; arrays and objects return compact JSON; null and missing paths return empty text. Queries matching multiple values return an error.",
        arguments: JSON_EXTRACT_ARGUMENTS,
        models: &[],
        notes: &[
            "JSON input can come from a quoted literal or a text/cached cell reference.",
            "Missing paths and null values return empty text.",
            "JSONEXTRACT is for single-value extraction; JSONPath queries that match multiple nodes are rejected.",
        ],
        implementation: FormulaImplementation::JsonExtract,
    },
    FormulaFunction {
        name: "CONCATENATE",
        signature: "CONCATENATE(text, ...)",
        insert_text: "=CONCATENATE(text1, text2)",
        runs_without_approval: true,
        summary: "Join text values together.",
        details: "Concatenates text literals and text cell references into a single string.",
        arguments: CONCATENATE_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::ConcatenateText,
    },
    FormulaFunction {
        name: "SUM",
        signature: "SUM(number, ...)",
        insert_text: "=SUM(number, ...)",
        runs_without_approval: true,
        summary: "Add numbers together.",
        details: "Supports numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Sum),
    },
    FormulaFunction {
        name: "PRODUCT",
        signature: "PRODUCT(number, ...)",
        insert_text: "=PRODUCT(number, ...)",
        runs_without_approval: true,
        summary: "Multiply numbers together.",
        details: "Supports numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Product),
    },
    FormulaFunction {
        name: "AVERAGE",
        signature: "AVERAGE(number, ...)",
        insert_text: "=AVERAGE(number, ...)",
        runs_without_approval: true,
        summary: "Average numbers.",
        details: "Supports numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Average),
    },
    FormulaFunction {
        name: "MIN",
        signature: "MIN(number, ...)",
        insert_text: "=MIN(number, ...)",
        runs_without_approval: true,
        summary: "Return the smallest number.",
        details: "Supports numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Min),
    },
    FormulaFunction {
        name: "MAX",
        signature: "MAX(number, ...)",
        insert_text: "=MAX(number, ...)",
        runs_without_approval: true,
        summary: "Return the largest number.",
        details: "Supports numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Max),
    },
    FormulaFunction {
        name: "ADD",
        signature: "ADD(left, right)",
        insert_text: "=ADD(left, right)",
        runs_without_approval: true,
        summary: "Add two numbers.",
        details: "Equivalent to the + operator for numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Add),
    },
    FormulaFunction {
        name: "SUBTRACT",
        signature: "SUBTRACT(left, right)",
        insert_text: "=SUBTRACT(left, right)",
        runs_without_approval: true,
        summary: "Subtract one number from another.",
        details: "Equivalent to the - operator for numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Subtract),
    },
    FormulaFunction {
        name: "MULTIPLY",
        signature: "MULTIPLY(left, right)",
        insert_text: "=MULTIPLY(left, right)",
        runs_without_approval: true,
        summary: "Multiply two numbers.",
        details: "Equivalent to the * operator for numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Multiply),
    },
    FormulaFunction {
        name: "DIVIDE",
        signature: "DIVIDE(left, right)",
        insert_text: "=DIVIDE(left, right)",
        runs_without_approval: true,
        summary: "Divide one number by another.",
        details: "Equivalent to the / operator for numeric literals, arithmetic expressions, and numeric cell references.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Divide),
    },
];

pub fn matching_functions(input: &str) -> Vec<FormulaFunction> {
    let Some(query) = formula_query(input) else {
        return Vec::new();
    };

    FORMULA_FUNCTIONS
        .iter()
        .copied()
        .filter(|function| function.name.starts_with(&query))
        .collect()
}

pub fn function_for_formula_input(input: &str) -> Option<FormulaFunction> {
    let expression = input.trim_start().strip_prefix('=')?.trim_start();
    let name = expression
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>();

    if name.is_empty() {
        return None;
    }

    FORMULA_FUNCTIONS
        .iter()
        .copied()
        .find(|function| function.name.eq_ignore_ascii_case(&name))
}

pub fn models_for_function(function: FormulaFunction) -> Vec<FormulaModelDoc> {
    if function.name == "GENERATEIMAGE" {
        static IMAGE_MODELS: OnceLock<Vec<FormulaModelDoc>> = OnceLock::new();
        return IMAGE_MODELS
            .get_or_init(|| {
                image_model_docs()
                    .into_iter()
                    .map(|model| FormulaModelDoc {
                        id: Box::leak(model.id.into_boxed_str()),
                        label: Box::leak(model.label.into_boxed_str()),
                        description: Box::leak(model.description.into_boxed_str()),
                        default: model.default,
                    })
                    .collect()
            })
            .clone();
    }

    if function.name == "GENERATEVIDEO" {
        static VIDEO_MODELS: OnceLock<Vec<FormulaModelDoc>> = OnceLock::new();
        return VIDEO_MODELS
            .get_or_init(|| {
                video_model_docs()
                    .into_iter()
                    .map(|model| FormulaModelDoc {
                        id: Box::leak(model.id.into_boxed_str()),
                        label: Box::leak(model.label.into_boxed_str()),
                        description: Box::leak(model.description.into_boxed_str()),
                        default: model.default,
                    })
                    .collect()
            })
            .clone();
    }

    if matches!(function.name, "LLM" | "LLM_LIST_DOWN" | "LLM_LIST_RIGHT") {
        return LLM_MODELS.to_vec();
    }

    function.models.to_vec()
}

pub fn formula_example_for_function(
    function: FormulaFunction,
    selected_model: Option<&str>,
) -> String {
    let models = models_for_function(function);
    let model_id = selected_model
        .filter(|candidate| models.iter().any(|model| model.id == *candidate))
        .or_else(|| {
            models
                .iter()
                .find(|model| model.default)
                .map(|model| model.id)
        });

    let Some(model_id) = model_id else {
        return function.insert_text.to_string();
    };

    match function.name {
        "GENERATEIMAGE" => format!("=GENERATEIMAGE(prompt, \"{model_id}\")"),
        "SEGMENT" => "=SEGMENT($, \"wheel\")".to_string(),
        "GENERATEVIDEO" => {
            let duration = video_model_default_duration(model_id).unwrap_or(8);
            if video_model_requires_end_image(model_id) {
                format!("=GENERATEVIDEO(prompt, $, $, \"{model_id}\", {duration})")
            } else if video_model_supports_aspect_ratio(model_id) {
                format!("=GENERATEVIDEO(prompt, $, \"{model_id}\", {duration}, \"16:9\")")
            } else {
                format!("=GENERATEVIDEO(prompt, $, \"{model_id}\", {duration})")
            }
        }
        "LLM" => format!("=LLM(prompt, \"{model_id}\", $, system_prompt)"),
        "LLM_LIST_DOWN" => {
            format!("=LLM_LIST_DOWN(prompt, \"{model_id}\", $, system_prompt)")
        }
        "LLM_LIST_RIGHT" => {
            format!("=LLM_LIST_RIGHT(prompt, \"{model_id}\", $, system_prompt)")
        }
        _ => function.insert_text.to_string(),
    }
}

pub fn related_functions_for_function(function: FormulaFunction) -> Vec<FormulaFunction> {
    let Some(topic) = formula_topic(function) else {
        return Vec::new();
    };

    FORMULA_FUNCTIONS
        .iter()
        .copied()
        .filter(|candidate| candidate.name != function.name)
        .filter(|candidate| formula_topic(*candidate) == Some(topic))
        .collect()
}

fn formula_topic(function: FormulaFunction) -> Option<FormulaTopic> {
    match function.name {
        "LLM" | "LLM_LIST_DOWN" | "LLM_LIST_RIGHT" => Some(FormulaTopic::LlmText),
        "GENERATEIMAGE" | "SEGMENT" | "GENERATEVIDEO" | "CONCATENATEVIDEO" => {
            Some(FormulaTopic::MediaGeneration)
        }
        "SUM" | "PRODUCT" | "AVERAGE" | "MIN" | "MAX" | "ADD" | "SUBTRACT" | "MULTIPLY"
        | "DIVIDE" => Some(FormulaTopic::Arithmetic),
        _ => None,
    }
}

fn formula_query(input: &str) -> Option<String> {
    let trimmed = input.trim_start();
    let query = trimmed.strip_prefix('=')?;

    if query.contains('(') {
        return None;
    }

    Some(query.trim().to_ascii_uppercase())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_math_functions_for_completion() {
        let matches = matching_functions("=SU");

        assert!(matches.iter().any(|function| function.name == "SUM"));
        assert!(matches.iter().any(|function| function.name == "SUBTRACT"));
    }

    #[test]
    fn removed_prompt_specific_functions() {
        assert!(
            !FORMULA_FUNCTIONS
                .iter()
                .any(|function| function.name == "PROMPT")
        );
        assert!(
            !FORMULA_FUNCTIONS
                .iter()
                .any(|function| function.name == "STORYBOARD")
        );
    }

    #[test]
    fn detects_function_from_partial_formula() {
        assert_eq!(
            function_for_formula_input("=generateimage(").map(|function| function.name),
            Some("GENERATEIMAGE")
        );
    }

    #[test]
    fn generate_image_docs_include_supported_models() {
        let function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEIMAGE")
            .unwrap();
        let models = models_for_function(*function);

        assert!(models.iter().any(|model| model.id == "flux/dev"));
        assert!(models.iter().any(|model| model.id == "openai/gpt-image-2"));
        assert!(function.arguments.iter().any(|arg| arg.name == "quality?"));
    }

    #[test]
    fn generate_video_docs_include_first_last_frame_models() {
        let function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEVIDEO")
            .unwrap();
        let models = models_for_function(*function);

        assert!(
            models
                .iter()
                .any(|model| model.id == "fal-ai/kling-video/o1/standard/image-to-video")
        );
        assert!(
            models
                .iter()
                .any(|model| model.id == "fal-ai/kling-video/o1/image-to-video")
        );
        assert!(
            function
                .arguments
                .iter()
                .any(|arg| arg.name == "end_image?")
        );
    }

    #[test]
    fn segment_docs_include_segmentation_specific_arguments() {
        let function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "SEGMENT")
            .unwrap();

        assert!(!function.runs_without_approval);
        assert!(
            function
                .arguments
                .iter()
                .any(|arg| arg.name == "point_prompts?")
        );
        assert!(
            function
                .arguments
                .iter()
                .any(|arg| arg.name == "box_prompts?")
        );
        assert!(
            function
                .arguments
                .iter()
                .any(|arg| arg.name == "output_format?")
        );
    }

    #[test]
    fn expensive_provider_formulas_require_approval() {
        let image_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEIMAGE")
            .unwrap();
        let video_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEVIDEO")
            .unwrap();
        let segment_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "SEGMENT")
            .unwrap();
        let llm_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM")
            .unwrap();
        let llm_list_down = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM_LIST_DOWN")
            .unwrap();
        let llm_list_right = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM_LIST_RIGHT")
            .unwrap();

        assert!(!image_function.runs_without_approval);
        assert!(!video_function.runs_without_approval);
        assert!(!segment_function.runs_without_approval);
        assert!(llm_function.runs_without_approval);
        assert!(llm_list_down.runs_without_approval);
        assert!(llm_list_right.runs_without_approval);
    }

    #[test]
    fn formula_examples_reflect_selected_models() {
        let image_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEIMAGE")
            .unwrap();
        let video_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "GENERATEVIDEO")
            .unwrap();

        assert_eq!(
            formula_example_for_function(*image_function, Some("openai/gpt-image-2")),
            r#"=GENERATEIMAGE(prompt, "openai/gpt-image-2")"#
        );
        assert_eq!(
            formula_example_for_function(
                *video_function,
                Some("fal-ai/kling-video/v3/standard/image-to-video")
            ),
            r#"=GENERATEVIDEO(prompt, $, "fal-ai/kling-video/v3/standard/image-to-video", 5)"#
        );
        assert_eq!(
            formula_example_for_function(
                *video_function,
                Some("fal-ai/kling-video/o1/standard/image-to-video")
            ),
            r#"=GENERATEVIDEO(prompt, $, $, "fal-ai/kling-video/o1/standard/image-to-video", 5)"#
        );
        let segment_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "SEGMENT")
            .unwrap();
        assert_eq!(
            formula_example_for_function(*segment_function, None),
            r#"=SEGMENT($, "wheel")"#
        );
        let llm_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM")
            .unwrap();
        assert_eq!(
            formula_example_for_function(*llm_function, Some("google/gemini-2.5-flash")),
            r#"=LLM(prompt, "google/gemini-2.5-flash", $, system_prompt)"#
        );
        let llm_list_down = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM_LIST_DOWN")
            .unwrap();
        assert_eq!(
            formula_example_for_function(*llm_list_down, Some("google/gemini-2.5-flash")),
            r#"=LLM_LIST_DOWN(prompt, "google/gemini-2.5-flash", $, system_prompt)"#
        );
        let llm_list_right = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM_LIST_RIGHT")
            .unwrap();
        assert_eq!(
            formula_example_for_function(*llm_list_right, Some("google/gemini-2.5-flash")),
            r#"=LLM_LIST_RIGHT(prompt, "google/gemini-2.5-flash", $, system_prompt)"#
        );
    }

    #[test]
    fn related_functions_include_llm_spill_variants() {
        let llm_function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM")
            .unwrap();
        let related = related_functions_for_function(*llm_function);

        assert!(
            related
                .iter()
                .any(|function| function.name == "LLM_LIST_DOWN")
        );
        assert!(
            related
                .iter()
                .any(|function| function.name == "LLM_LIST_RIGHT")
        );

        let llm_list_down = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name == "LLM_LIST_DOWN")
            .unwrap();
        let related = related_functions_for_function(*llm_list_down);

        assert!(related.iter().any(|function| function.name == "LLM"));
        assert!(
            related
                .iter()
                .any(|function| function.name == "LLM_LIST_RIGHT")
        );
    }
}

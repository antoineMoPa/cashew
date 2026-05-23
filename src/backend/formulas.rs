use super::providers::{
    fal_image::{DEFAULT_MODEL as DEFAULT_IMAGE_MODEL, OPENAI_GPT_IMAGE_2_MODEL},
    openrouter::DEFAULT_MODEL as DEFAULT_LLM_MODEL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaFunction {
    pub name: &'static str,
    pub signature: &'static str,
    pub insert_text: &'static str,
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
    NoopAi { placeholder: &'static str },
    ProviderAi { provider: &'static str },
    Math(MathFunction),
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

const GENERATE_IMAGE_MODELS: &[FormulaModelDoc] = &[
    FormulaModelDoc {
        id: DEFAULT_IMAGE_MODEL,
        label: "FLUX Dev",
        description: "Fast fal image generation. Supports text-to-image and one image input.",
        default: true,
    },
    FormulaModelDoc {
        id: OPENAI_GPT_IMAGE_2_MODEL,
        label: "GPT Image 2",
        description: "Higher quality image generation/editing. Supports multiple image inputs.",
        default: false,
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
        description: "OpenRouter model id routed through fal.",
    },
    FormulaArgumentDoc {
        name: "system_prompt",
        kind: "text",
        required: false,
        description: "Optional system instruction text.",
    },
];

const LLM_MODELS: &[FormulaModelDoc] = &[FormulaModelDoc {
    id: DEFAULT_LLM_MODEL,
    label: "Gemini 2.5 Flash",
    description: "Default OpenRouter model id sent through fal openrouter/router.",
    default: true,
}];

const NUMBER_ARGUMENTS: &[FormulaArgumentDoc] = &[FormulaArgumentDoc {
    name: "number",
    kind: "number",
    required: true,
    description: "Numeric literal or numeric cell reference.",
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

pub const FORMULA_FUNCTIONS: &[FormulaFunction] = &[
    FormulaFunction {
        name: "GENERATEIMAGE",
        signature: "GENERATEIMAGE(prompt, model, quality?, image...)",
        insert_text: "=GENERATEIMAGE(prompt, \"flux/dev\")",
        summary: "Generate or edit an image through fal.",
        details: "Use model flux/dev for text-to-image speed or openai/gpt-image-2 for quality. OpenAI defaults to medium quality, and you can override it with low, medium, or high before any image URLs.",
        arguments: GENERATE_IMAGE_ARGUMENTS,
        models: GENERATE_IMAGE_MODELS,
        notes: &[
            "Cache-first: identical formulas with identical resolved inputs reuse stored results.",
            "Image inputs are resolved before the provider request is built.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.image",
        },
    },
    FormulaFunction {
        name: "GENERATEVIDEO",
        signature: "GENERATEVIDEO(prompt, image, settings)",
        insert_text: "=GENERATEVIDEO(prompt, image, settings)",
        summary: "Generate or reuse a video clip from prompt and media inputs.",
        details: "No-op for now. Later, this will support cached storyboard-to-video workflows.",
        arguments: &[],
        models: &[],
        notes: &["Not implemented yet."],
        implementation: FormulaImplementation::NoopAi {
            placeholder: "AI video generation is not implemented yet.",
        },
    },
    FormulaFunction {
        name: "CONCATENATEVIDEO",
        signature: "CONCATENATEVIDEO(video_range)",
        insert_text: "=CONCATENATEVIDEO(video_range)",
        summary: "Concatenate generated clips into a longer video.",
        details: "No-op for now. Later, this will assemble cached clips into scenes or complete drafts.",
        arguments: &[],
        models: &[],
        notes: &["Not implemented yet."],
        implementation: FormulaImplementation::NoopAi {
            placeholder: "Video concatenation is not implemented yet.",
        },
    },
    FormulaFunction {
        name: "LLM",
        signature: "LLM(prompt, model, system_prompt)",
        insert_text: "=LLM(prompt, \"google/gemini-2.5-flash\", system_prompt)",
        summary: "Generate or transform text through fal OpenRouter.",
        details: "Runs through fal endpoint openrouter/router using the saved FAL key.",
        arguments: LLM_ARGUMENTS,
        models: LLM_MODELS,
        notes: &[
            "The model argument accepts OpenRouter model ids routed through fal.",
            "The listed model is the default used when the model argument is empty.",
        ],
        implementation: FormulaImplementation::ProviderAi {
            provider: "fal.openrouter",
        },
    },
    FormulaFunction {
        name: "SUM",
        signature: "SUM(number, ...)",
        insert_text: "=SUM(number, ...)",
        summary: "Add numbers together.",
        details: "Currently supports numeric literal arguments only. Cell references and ranges will come later.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Sum),
    },
    FormulaFunction {
        name: "PRODUCT",
        signature: "PRODUCT(number, ...)",
        insert_text: "=PRODUCT(number, ...)",
        summary: "Multiply numbers together.",
        details: "Currently supports numeric literal arguments only.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Product),
    },
    FormulaFunction {
        name: "AVERAGE",
        signature: "AVERAGE(number, ...)",
        insert_text: "=AVERAGE(number, ...)",
        summary: "Average numbers.",
        details: "Currently supports numeric literal arguments only.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Average),
    },
    FormulaFunction {
        name: "MIN",
        signature: "MIN(number, ...)",
        insert_text: "=MIN(number, ...)",
        summary: "Return the smallest number.",
        details: "Currently supports numeric literal arguments only.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Min),
    },
    FormulaFunction {
        name: "MAX",
        signature: "MAX(number, ...)",
        insert_text: "=MAX(number, ...)",
        summary: "Return the largest number.",
        details: "Currently supports numeric literal arguments only.",
        arguments: NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Max),
    },
    FormulaFunction {
        name: "ADD",
        signature: "ADD(left, right)",
        insert_text: "=ADD(left, right)",
        summary: "Add two numbers.",
        details: "Equivalent to the + operator for numeric literals.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Add),
    },
    FormulaFunction {
        name: "SUBTRACT",
        signature: "SUBTRACT(left, right)",
        insert_text: "=SUBTRACT(left, right)",
        summary: "Subtract one number from another.",
        details: "Equivalent to the - operator for numeric literals.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Subtract),
    },
    FormulaFunction {
        name: "MULTIPLY",
        signature: "MULTIPLY(left, right)",
        insert_text: "=MULTIPLY(left, right)",
        summary: "Multiply two numbers.",
        details: "Equivalent to the * operator for numeric literals.",
        arguments: TWO_NUMBER_ARGUMENTS,
        models: &[],
        notes: &[],
        implementation: FormulaImplementation::Math(MathFunction::Multiply),
    },
    FormulaFunction {
        name: "DIVIDE",
        signature: "DIVIDE(left, right)",
        insert_text: "=DIVIDE(left, right)",
        summary: "Divide one number by another.",
        details: "Equivalent to the / operator for numeric literals.",
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

        assert!(function.models.iter().any(|model| model.id == "flux/dev"));
        assert!(
            function
                .models
                .iter()
                .any(|model| model.id == "openai/gpt-image-2")
        );
        assert!(function.arguments.iter().any(|arg| arg.name == "quality?"));
    }
}

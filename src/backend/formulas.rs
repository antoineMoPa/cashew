#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaFunction {
    pub name: &'static str,
    pub signature: &'static str,
    pub insert_text: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
    pub implementation: FormulaImplementation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaImplementation {
    NoopAi { placeholder: &'static str },
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

pub const FORMULA_FUNCTIONS: &[FormulaFunction] = &[
    FormulaFunction {
        name: "GENERATEIMAGE",
        signature: "GENERATEIMAGE(prompt, reference)",
        insert_text: "=GENERATEIMAGE(prompt, reference)",
        summary: "Generate or reuse an image from prompt inputs.",
        details: "No-op for now. Later, resolved prompt and reference inputs will determine the cache key before any provider call.",
        implementation: FormulaImplementation::NoopAi {
            placeholder: "AI image generation is not implemented yet.",
        },
    },
    FormulaFunction {
        name: "GENERATEVIDEO",
        signature: "GENERATEVIDEO(prompt, image, settings)",
        insert_text: "=GENERATEVIDEO(prompt, image, settings)",
        summary: "Generate or reuse a video clip from prompt and media inputs.",
        details: "No-op for now. Later, this will support cached storyboard-to-video workflows.",
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
        implementation: FormulaImplementation::NoopAi {
            placeholder: "Video concatenation is not implemented yet.",
        },
    },
    FormulaFunction {
        name: "LLM",
        signature: "LLM(prompt, model, system_prompt)",
        insert_text: "=LLM(prompt, \"google/gemini-2.5-flash\", system_prompt)",
        summary: "Generate or transform text through fal OpenRouter.",
        details: "Set up for fal endpoint openrouter/router. No-op in cell evaluation until async provider execution and cache writes are wired.",
        implementation: FormulaImplementation::NoopAi {
            placeholder: "LLM via fal OpenRouter is configured but execution is not wired yet.",
        },
    },
    FormulaFunction {
        name: "SUM",
        signature: "SUM(number, ...)",
        insert_text: "=SUM(number, ...)",
        summary: "Add numbers together.",
        details: "Currently supports numeric literal arguments only. Cell references and ranges will come later.",
        implementation: FormulaImplementation::Math(MathFunction::Sum),
    },
    FormulaFunction {
        name: "PRODUCT",
        signature: "PRODUCT(number, ...)",
        insert_text: "=PRODUCT(number, ...)",
        summary: "Multiply numbers together.",
        details: "Currently supports numeric literal arguments only.",
        implementation: FormulaImplementation::Math(MathFunction::Product),
    },
    FormulaFunction {
        name: "AVERAGE",
        signature: "AVERAGE(number, ...)",
        insert_text: "=AVERAGE(number, ...)",
        summary: "Average numbers.",
        details: "Currently supports numeric literal arguments only.",
        implementation: FormulaImplementation::Math(MathFunction::Average),
    },
    FormulaFunction {
        name: "MIN",
        signature: "MIN(number, ...)",
        insert_text: "=MIN(number, ...)",
        summary: "Return the smallest number.",
        details: "Currently supports numeric literal arguments only.",
        implementation: FormulaImplementation::Math(MathFunction::Min),
    },
    FormulaFunction {
        name: "MAX",
        signature: "MAX(number, ...)",
        insert_text: "=MAX(number, ...)",
        summary: "Return the largest number.",
        details: "Currently supports numeric literal arguments only.",
        implementation: FormulaImplementation::Math(MathFunction::Max),
    },
    FormulaFunction {
        name: "ADD",
        signature: "ADD(left, right)",
        insert_text: "=ADD(left, right)",
        summary: "Add two numbers.",
        details: "Equivalent to the + operator for numeric literals.",
        implementation: FormulaImplementation::Math(MathFunction::Add),
    },
    FormulaFunction {
        name: "SUBTRACT",
        signature: "SUBTRACT(left, right)",
        insert_text: "=SUBTRACT(left, right)",
        summary: "Subtract one number from another.",
        details: "Equivalent to the - operator for numeric literals.",
        implementation: FormulaImplementation::Math(MathFunction::Subtract),
    },
    FormulaFunction {
        name: "MULTIPLY",
        signature: "MULTIPLY(left, right)",
        insert_text: "=MULTIPLY(left, right)",
        summary: "Multiply two numbers.",
        details: "Equivalent to the * operator for numeric literals.",
        implementation: FormulaImplementation::Math(MathFunction::Multiply),
    },
    FormulaFunction {
        name: "DIVIDE",
        signature: "DIVIDE(left, right)",
        insert_text: "=DIVIDE(left, right)",
        summary: "Divide one number by another.",
        details: "Equivalent to the / operator for numeric literals.",
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
}

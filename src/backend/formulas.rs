#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormulaFunction {
    pub name: &'static str,
    pub signature: &'static str,
    pub insert_text: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
}

pub const FORMULA_FUNCTIONS: &[FormulaFunction] = &[
    FormulaFunction {
        name: "GENERATEIMAGE",
        signature: "GENERATEIMAGE(prompt, reference)",
        insert_text: "=GENERATEIMAGE(prompt, reference)",
        summary: "Generate or reuse an image from prompt inputs.",
        details: "Planned cached media formula. The resolved prompt and reference inputs will determine the cache key before any provider call.",
    },
    FormulaFunction {
        name: "GENERATEVIDEO",
        signature: "GENERATEVIDEO(prompt, image, settings)",
        insert_text: "=GENERATEVIDEO(prompt, image, settings)",
        summary: "Generate or reuse a video clip from prompt and media inputs.",
        details: "Planned cached media formula for storyboard-to-video workflows. Provider execution is not implemented yet.",
    },
    FormulaFunction {
        name: "CONCATENATEVIDEO",
        signature: "CONCATENATEVIDEO(video_range)",
        insert_text: "=CONCATENATEVIDEO(video_range)",
        summary: "Concatenate generated clips into a longer video.",
        details: "Planned cached composition formula for assembling scenes or complete drafts from generated clips.",
    },
    FormulaFunction {
        name: "PROMPT",
        signature: "PROMPT(instruction, source)",
        insert_text: "=PROMPT(instruction, source)",
        summary: "Transform prompt text with an instruction.",
        details: "Planned helper formula for prompt wrangling before media generation.",
    },
    FormulaFunction {
        name: "STORYBOARD",
        signature: "STORYBOARD(scenario, style)",
        insert_text: "=STORYBOARD(scenario, style)",
        summary: "Draft storyboard beats from a scenario and style guide.",
        details: "Planned helper formula for turning scenario text into shot-level rows.",
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

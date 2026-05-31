use super::{
    LlmFormulaOutput, LlmOutputMode, LlmRequest,
    evaluation::{
        parse_function_call, resolve_text_argument, split_formula_arguments,
        strip_markdown_code_fence, strip_numbered_prefix,
    },
};
use crate::backend::{
    document::Sheet,
    providers::openrouter::{DEFAULT_MODEL, OpenRouterRequest},
};

pub fn llm_request_for_sheet(input: &str, sheet: &Sheet) -> Result<Option<LlmRequest>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    let Some(output_mode) = llm_output_mode_for_name(&name) else {
        return Ok(None);
    };

    let args = split_formula_arguments(args)?;
    if args.is_empty() {
        return Err(
            "LLM expects prompt, optional model, optional image inputs, optional system_prompt"
                .to_string(),
        );
    }

    let prompt = resolve_text_argument(&args[0], sheet)?;
    let model = args
        .get(1)
        .map(|arg| resolve_text_argument(arg, sheet))
        .transpose()?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let extra_args = args
        .iter()
        .skip(2)
        .map(|arg| resolve_text_argument(arg, sheet))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    let (image_urls, system_prompt) = split_llm_extra_arguments(&extra_args)?;

    let mut request = OpenRouterRequest::new(prompt)
        .with_model(model)
        .with_image_urls(image_urls);
    if let Some(system_prompt) = llm_system_prompt_for_mode(output_mode, system_prompt) {
        request = request.with_system_prompt(system_prompt);
    }

    Ok(Some(LlmRequest {
        function_name: llm_function_name_for_mode(output_mode),
        output_mode,
        request,
    }))
}

fn llm_output_mode_for_name(name: &str) -> Option<LlmOutputMode> {
    if name.eq_ignore_ascii_case("LLM") {
        return Some(LlmOutputMode::Text);
    }

    if name.eq_ignore_ascii_case("LLM_LIST_DOWN") {
        return Some(LlmOutputMode::ListDown);
    }

    if name.eq_ignore_ascii_case("LLM_LIST_RIGHT") {
        return Some(LlmOutputMode::ListRight);
    }

    None
}

fn llm_function_name_for_mode(mode: LlmOutputMode) -> &'static str {
    match mode {
        LlmOutputMode::Text => "LLM",
        LlmOutputMode::ListDown => "LLM_LIST_DOWN",
        LlmOutputMode::ListRight => "LLM_LIST_RIGHT",
    }
}

fn llm_system_prompt_for_mode(mode: LlmOutputMode, user_prompt: Option<String>) -> Option<String> {
    let instructions = match mode {
        LlmOutputMode::Text => return user_prompt,
        LlmOutputMode::ListDown | LlmOutputMode::ListRight => {
            "Return valid JSON as a one-dimensional array of strings. Expected format: [\"first item\", \"second item\"]. Do not return comma-separated quoted strings. Do not use markdown fences, bullets, or numbering."
        }
    };

    match user_prompt {
        Some(user_prompt) if !user_prompt.trim().is_empty() => {
            Some(format!("{user_prompt}\n\n{instructions}"))
        }
        Some(_) | None => Some(instructions.to_string()),
    }
}

fn split_llm_extra_arguments(args: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    if args.is_empty() {
        return Ok((Vec::new(), None));
    }

    if args.len() == 1 {
        return if llm_image_input(&args[0]) {
            Ok((vec![args[0].clone()], None))
        } else {
            Ok((Vec::new(), Some(args[0].clone())))
        };
    }

    if llm_image_input(args.last().unwrap()) {
        if args.iter().all(|arg| llm_image_input(arg)) {
            return Ok((args.to_vec(), None));
        }

        return Err("LLM image inputs must come before the optional system_prompt".to_string());
    }

    let image_urls = args[..args.len() - 1]
        .iter()
        .map(|arg| {
            if llm_image_input(arg) {
                Ok(arg.clone())
            } else {
                Err(
                    "LLM image inputs must be URLs, data URIs, or cells that resolve to one"
                        .to_string(),
                )
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((image_urls, Some(args.last().cloned().unwrap())))
}

fn llm_image_input(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("data:")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
}

pub fn parse_llm_output(mode: LlmOutputMode, output: &str) -> Result<LlmFormulaOutput, String> {
    match mode {
        LlmOutputMode::Text => Ok(LlmFormulaOutput::Text(output.to_string())),
        LlmOutputMode::ListDown | LlmOutputMode::ListRight => {
            parse_llm_list_output(output).map(LlmFormulaOutput::List)
        }
    }
}

fn parse_llm_list_output(output: &str) -> Result<Vec<String>, String> {
    let trimmed = strip_markdown_code_fence(output).trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(values) = serde_json::from_str::<Vec<String>>(trimmed) {
        return Ok(values
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect());
    }

    Ok(trimmed
        .lines()
        .map(llm_list_item_from_line)
        .filter(|value| !value.is_empty())
        .collect())
}

fn llm_list_item_from_line(line: &str) -> String {
    let trimmed = line.trim();
    let trimmed = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("• "))
        .or_else(|| strip_numbered_prefix(trimmed))
        .unwrap_or(trimmed);

    trimmed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{
        document::Sheet,
        formula_implementations::{LlmFormulaOutput, LlmOutputMode},
        providers::openrouter::DEFAULT_MODEL,
    };

    #[test]
    fn builds_llm_request_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("LLM", 2, 2);
        sheet.set_cell_input(0, 0, "Say hi".to_string());

        let request = llm_request_for_sheet(
            r#"=LLM($A1, "google/gemini-2.5-flash", "", "Only answer in one sentence")"#,
            &sheet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(request.function_name, "LLM");
        assert_eq!(request.output_mode, LlmOutputMode::Text);
        assert_eq!(request.request.prompt, "Say hi");
        assert_eq!(request.request.model, "google/gemini-2.5-flash");
        assert_eq!(
            request.request.system_prompt.as_deref(),
            Some("Only answer in one sentence")
        );
        assert!(request.request.image_urls.is_empty());
    }

    #[test]
    fn builds_llm_vision_request_from_image_inputs() {
        let mut sheet = Sheet::new("LLM Vision", 2, 3);
        sheet.set_cell_input(0, 0, "Describe this image".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/image.png".to_string());
        sheet.set_cell_input(0, 2, "Keep it brief".to_string());

        let request = llm_request_for_sheet(r#"=LLM(A1, "", B1, C1)"#, &sheet)
            .unwrap()
            .unwrap();

        assert_eq!(request.function_name, "LLM");
        assert_eq!(request.output_mode, LlmOutputMode::Text);
        assert_eq!(request.request.prompt, "Describe this image");
        assert_eq!(request.request.model, DEFAULT_MODEL);
        assert_eq!(
            request.request.image_urls,
            vec!["https://example.com/image.png"]
        );
        assert_eq!(
            request.request.system_prompt.as_deref(),
            Some("Keep it brief")
        );
        assert_eq!(
            request.request.endpoint(),
            crate::backend::providers::openrouter::VISION_ENDPOINT
        );
    }

    #[test]
    fn builds_llm_spill_request_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("LLM Spill", 2, 3);
        sheet.set_cell_input(0, 0, "List the objects".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/image.png".to_string());

        let request = llm_request_for_sheet(r#"=LLM_LIST_RIGHT(A1, "", B1)"#, &sheet)
            .unwrap()
            .unwrap();

        assert_eq!(request.function_name, "LLM_LIST_RIGHT");
        assert_eq!(request.output_mode, LlmOutputMode::ListRight);
        assert_eq!(
            request.request.endpoint(),
            crate::backend::providers::openrouter::VISION_ENDPOINT
        );
        assert!(
            request
                .request
                .system_prompt
                .as_deref()
                .unwrap()
                .contains("one-dimensional array of strings")
        );
        assert!(
            request
                .request
                .system_prompt
                .as_deref()
                .unwrap()
                .contains(r#"Expected format: ["first item", "second item"]"#)
        );
        assert!(
            request
                .request
                .system_prompt
                .as_deref()
                .unwrap()
                .contains("Do not return comma-separated quoted strings")
        );
    }

    #[test]
    fn parses_llm_list_output_from_json_and_bullets() {
        assert_eq!(
            parse_llm_output(LlmOutputMode::ListDown, r#"["cat","chair"]"#).unwrap(),
            LlmFormulaOutput::List(vec!["cat".to_string(), "chair".to_string()])
        );
        assert_eq!(
            parse_llm_output(LlmOutputMode::ListDown, "- cat\n2. chair\n\n* lamp").unwrap(),
            LlmFormulaOutput::List(vec![
                "cat".to_string(),
                "chair".to_string(),
                "lamp".to_string(),
            ])
        );
    }
}

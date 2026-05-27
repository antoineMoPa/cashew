use super::{
    document::{CellValue, Sheet},
    fill::parse_cell_reference_parts,
    formulas::{FORMULA_FUNCTIONS, FormulaImplementation, MathFunction},
    providers::{
        fal_image::{
            DEFAULT_MODEL as DEFAULT_IMAGE_MODEL, GenerateImageRequest, parse_image_quality,
        },
        fal_segment::{
            SegmentBoxPrompt, SegmentImageRequest, SegmentOutputFormat, SegmentPointPrompt,
        },
        fal_video::{GenerateVideoRequest, video_model_id_is_supported},
        openrouter::{DEFAULT_MODEL, OpenRouterRequest},
    },
};
use serde_json::Value;
use serde_json_path::JsonPath;

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

#[cfg(test)]
fn evaluate_formula(input: &str) -> Result<FormulaValue, String> {
    evaluate_formula_with_sheet(input, None)
}

pub fn evaluate_formula_for_sheet(input: &str, sheet: &Sheet) -> Result<FormulaValue, String> {
    evaluate_formula_with_sheet(input, Some(sheet))
}

fn evaluate_formula_with_sheet(input: &str, sheet: Option<&Sheet>) -> Result<FormulaValue, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();

    if expression.is_empty() {
        return Err("Formula is empty".to_string());
    }

    if let Some((name, args)) = parse_function_call(expression)? {
        let function = FORMULA_FUNCTIONS
            .iter()
            .find(|function| function.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| format!("Unknown function {name}"))?;

        return match function.implementation {
            FormulaImplementation::ProviderAi { provider } => Ok(FormulaValue::Pending(format!(
                "{provider} request is ready to run"
            ))),
            FormulaImplementation::LocalVideoConcat => Ok(FormulaValue::Pending(
                "Local video concatenation is ready to run".to_string(),
            )),
            FormulaImplementation::ConcatenateText => {
                let values = parse_text_arguments(args, sheet)?;
                Ok(FormulaValue::Text(values.concat()))
            }
            FormulaImplementation::JsonExtract => {
                let values = split_formula_arguments(args)?;
                if values.len() != 2 {
                    return Err("JSONEXTRACT expects input and path".to_string());
                }

                let input = match sheet {
                    Some(sheet) => resolve_text_argument(&values[0], sheet)?,
                    None => resolve_text_argument_without_sheet(&values[0])?,
                };
                let path = match sheet {
                    Some(sheet) => resolve_text_argument(&values[1], sheet)?,
                    None => resolve_text_argument_without_sheet(&values[1])?,
                };
                let extracted = json_extract(&input, &path)?;
                Ok(FormulaValue::Text(extracted))
            }
            FormulaImplementation::Math(math) => {
                let values = parse_numeric_arguments(args, sheet)?;
                evaluate_math_function(math, &values)
            }
        };
    }

    if let Some(value) = resolve_single_cell_reference(expression, sheet)? {
        return Ok(value);
    }

    ExpressionParser::new(expression, sheet)
        .parse()
        .map(FormulaValue::Number)
}

pub fn llm_request_for_sheet(
    input: &str,
    sheet: &Sheet,
) -> Result<Option<OpenRouterRequest>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("LLM") {
        return Ok(None);
    }

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
    if let Some(system_prompt) = system_prompt {
        request = request.with_system_prompt(system_prompt);
    }

    Ok(Some(request))
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

        return Err(
            "LLM image inputs must come before the optional system_prompt".to_string(),
        );
    }

    let image_urls = args[..args.len() - 1]
        .iter()
        .map(|arg| {
            if llm_image_input(arg) {
                Ok(arg.clone())
            } else {
                Err("LLM image inputs must be URLs, data URIs, or cells that resolve to one"
                    .to_string())
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

pub fn generate_image_request_for_sheet(
    input: &str,
    sheet: &Sheet,
) -> Result<Option<GenerateImageRequest>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("GENERATEIMAGE") {
        return Ok(None);
    }

    let args = split_formula_arguments(args)?;
    if args.len() < 2 {
        return Err(
            "GENERATEIMAGE expects prompt, model, optional quality, optional image URLs"
                .to_string(),
        );
    }

    let prompt = resolve_text_argument(&args[0], sheet)?;
    let model = resolve_text_argument(&args[1], sheet)?.trim().to_string();
    let model = if model.is_empty() {
        DEFAULT_IMAGE_MODEL.to_string()
    } else {
        model
    };
    let mut remaining_args = args.iter().skip(2);
    let quality = remaining_args
        .next()
        .map(|arg| resolve_text_argument(arg, sheet))
        .transpose()?
        .and_then(|value| parse_image_quality(&value));
    let image_urls = if quality.is_some() {
        remaining_args
            .map(|arg| resolve_text_argument(arg, sheet))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .collect::<Vec<_>>()
    } else {
        args.iter()
            .skip(2)
            .map(|arg| resolve_text_argument(arg, sheet))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .collect::<Vec<_>>()
    };

    GenerateImageRequest::new(prompt, model, quality, image_urls)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub fn segment_request_for_sheet(
    input: &str,
    sheet: &Sheet,
) -> Result<Option<SegmentImageRequest>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("SEGMENT") {
        return Ok(None);
    }

    let args = split_formula_arguments(args)?;
    if args.is_empty() || args.len() > 10 {
        return Err(
            "SEGMENT expects image, optional prompt, optional point_prompts, optional box_prompts, optional apply_mask, optional output_format, optional return_multiple_masks, optional max_masks, optional include_scores, optional include_boxes"
                .to_string(),
        );
    }

    let image_url = resolve_text_argument(&args[0], sheet)?;
    let prompt = args
        .get(1)
        .map(|arg| resolve_text_argument(arg, sheet))
        .transpose()?
        .unwrap_or_default();
    let point_prompts = args
        .get(2)
        .map(|arg| resolve_segment_point_prompts(arg, sheet))
        .transpose()?
        .unwrap_or_default();
    let box_prompts = args
        .get(3)
        .map(|arg| resolve_segment_box_prompts(arg, sheet))
        .transpose()?
        .unwrap_or_default();
    let apply_mask = args
        .get(4)
        .map(|arg| resolve_optional_bool_argument(arg, sheet))
        .transpose()?
        .flatten();
    let output_format = args
        .get(5)
        .map(|arg| resolve_segment_output_format(arg, sheet))
        .transpose()?
        .flatten();
    let return_multiple_masks = args
        .get(6)
        .map(|arg| resolve_optional_bool_argument(arg, sheet))
        .transpose()?
        .flatten();
    let max_masks = args
        .get(7)
        .map(|arg| resolve_optional_u32_argument(arg, sheet))
        .transpose()?
        .flatten();
    let include_scores = args
        .get(8)
        .map(|arg| resolve_optional_bool_argument(arg, sheet))
        .transpose()?
        .flatten();
    let include_boxes = args
        .get(9)
        .map(|arg| resolve_optional_bool_argument(arg, sheet))
        .transpose()?
        .flatten();

    SegmentImageRequest::new(
        image_url,
        prompt,
        point_prompts,
        box_prompts,
        apply_mask,
        output_format,
        return_multiple_masks,
        max_masks,
        include_scores,
        include_boxes,
    )
    .map(Some)
    .map_err(|error| error.to_string())
}

pub fn generate_video_request_for_sheet(
    input: &str,
    sheet: &Sheet,
) -> Result<Option<GenerateVideoRequest>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("GENERATEVIDEO") {
        return Ok(None);
    }

    let args = split_formula_arguments(args)?;
    if args.len() < 2 || args.len() > 5 {
        return Err(
            "GENERATEVIDEO expects prompt, image, optional model, optional duration, optional aspect_ratio"
                .to_string(),
        );
    }

    let prompt = resolve_text_argument(&args[0], sheet)?;
    let image_url = resolve_text_argument(&args[1], sheet)?;

    let mut model = None;
    let mut duration = None;
    let mut aspect_ratio = None;

    for arg in args.iter().skip(2) {
        let value = resolve_text_argument(arg, sheet)?;
        let value = value.trim();
        if value.is_empty() {
            continue;
        }

        if model.is_none() && (video_model_id_is_supported(value) || value.starts_with("fal-ai/")) {
            model = Some(value.to_string());
            continue;
        }

        if duration.is_none() {
            if let Ok(parsed) = value.parse::<u32>() {
                duration = Some(parsed);
                continue;
            }
        }

        if aspect_ratio.is_none() {
            aspect_ratio = Some(value.to_string());
            continue;
        }

        return Err(
            "GENERATEVIDEO arguments after image must be model, duration, and aspect_ratio"
                .to_string(),
        );
    }

    GenerateVideoRequest::new(prompt, image_url, model, duration, aspect_ratio)
        .map(Some)
        .map_err(|error| error.to_string())
}

pub fn concatenate_video_inputs_for_sheet(
    input: &str,
    sheet: &Sheet,
) -> Result<Option<Vec<String>>, String> {
    let expression = input
        .trim_start()
        .strip_prefix('=')
        .ok_or_else(|| "Formula must start with =".to_string())?
        .trim();
    let Some((name, args)) = parse_function_call(expression)? else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("CONCATENATEVIDEO") {
        return Ok(None);
    }

    let args = split_formula_arguments(args)?;
    if args.len() < 2 {
        return Err("CONCATENATEVIDEO expects at least two video inputs".to_string());
    }

    let videos = args
        .iter()
        .map(|arg| resolve_text_argument(arg, sheet))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if videos.len() < 2 {
        return Err("CONCATENATEVIDEO expects at least two non-empty video inputs".to_string());
    }

    Ok(Some(videos))
}

fn parse_function_call(expression: &str) -> Result<Option<(String, &str)>, String> {
    let Some(open) = expression.find('(') else {
        return Ok(None);
    };

    let name = expression[..open].trim();
    if !is_function_name(name) {
        return Ok(None);
    }

    if !expression.ends_with(')') {
        return Err("Function call is missing a closing parenthesis".to_string());
    }

    let args = &expression[open + 1..expression.len() - 1];
    Ok(Some((name.to_ascii_uppercase(), args)))
}

fn is_function_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_alphabetic()
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn parse_numeric_arguments(args: &str, sheet: Option<&Sheet>) -> Result<Vec<f64>, String> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }

    args.split(',')
        .map(|arg| ExpressionParser::new(arg.trim(), sheet).parse())
        .collect()
}

fn parse_text_arguments(args: &str, sheet: Option<&Sheet>) -> Result<Vec<String>, String> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }

    let arguments = split_formula_arguments(args)?;
    match sheet {
        Some(sheet) => arguments
            .into_iter()
            .map(|arg| resolve_text_argument(&arg, sheet))
            .collect(),
        None => arguments
            .into_iter()
            .map(|arg| resolve_text_argument_without_sheet(&arg))
            .collect(),
    }
}

fn resolve_single_cell_reference(
    expression: &str,
    sheet: Option<&Sheet>,
) -> Result<Option<FormulaValue>, String> {
    let Some(sheet) = sheet else {
        return Ok(None);
    };

    let Ok((row, col)) = parse_cell_reference(expression) else {
        return Ok(None);
    };

    let Some(cell) = sheet.cell(row, col) else {
        return Ok(Some(FormulaValue::Empty));
    };

    let value = match &cell.value {
        CellValue::Empty => FormulaValue::Empty,
        CellValue::Text(value) => FormulaValue::Text(value.clone()),
        CellValue::FormulaPending { message } => FormulaValue::Pending(message.clone()),
        CellValue::Cached(value) => FormulaValue::Cached {
            value: value.clone(),
            cache_key: cell.cache_key.clone(),
        },
        CellValue::Error(error) => return Err(format!("{expression} has an error: {error}")),
    };

    Ok(Some(value))
}

fn split_formula_arguments(args: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escape = false;

    for character in args.chars() {
        if escape {
            current.push(character);
            escape = false;
            continue;
        }

        match character {
            '\\' if in_string => {
                current.push(character);
                escape = true;
            }
            '"' => {
                in_string = !in_string;
                current.push(character);
            }
            ',' if !in_string => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(character),
        }
    }

    if in_string {
        return Err("String argument is missing a closing quote".to_string());
    }

    parts.push(current.trim().to_string());
    Ok(parts)
}

fn resolve_text_argument(arg: &str, sheet: &Sheet) -> Result<String, String> {
    let arg = arg.trim();
    if arg.len() >= 2 && arg.starts_with('"') && arg.ends_with('"') {
        return Ok(arg[1..arg.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"));
    }

    if let Ok((row, col)) = parse_cell_reference(arg) {
        return match sheet.cell(row, col).map(|cell| &cell.value) {
            Some(CellValue::Text(value)) => Ok(value.clone()),
            Some(CellValue::Cached(value)) => Ok(value.clone()),
            Some(CellValue::FormulaPending { message }) => Ok(message.clone()),
            Some(CellValue::Error(error)) => Err(format!("{arg} has an error: {error}")),
            Some(CellValue::Empty) | None => Ok(String::new()),
        };
    }

    Ok(arg.to_string())
}

fn resolve_text_argument_without_sheet(arg: &str) -> Result<String, String> {
    let arg = arg.trim();
    if arg.len() >= 2 && arg.starts_with('"') && arg.ends_with('"') {
        return Ok(arg[1..arg.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"));
    }

    Ok(arg.to_string())
}

fn resolve_segment_point_prompts(
    arg: &str,
    sheet: &Sheet,
) -> Result<Vec<SegmentPointPrompt>, String> {
    let value = resolve_text_argument(arg, sheet)?;
    parse_segment_prompt_list(&value, "point prompt")
}

fn resolve_segment_box_prompts(arg: &str, sheet: &Sheet) -> Result<Vec<SegmentBoxPrompt>, String> {
    let value = resolve_text_argument(arg, sheet)?;
    parse_segment_prompt_list(&value, "box prompt")
}

fn parse_segment_prompt_list<T>(value: &str, kind: &str) -> Result<Vec<T>, String>
where
    T: serde::de::DeserializeOwned,
{
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(values) = serde_json::from_str::<Vec<T>>(trimmed) {
        return Ok(values);
    }

    serde_json::from_str::<T>(trimmed)
        .map(|value| vec![value])
        .map_err(|error| format!("Could not parse {kind} JSON: {error}"))
}

fn resolve_optional_bool_argument(arg: &str, sheet: &Sheet) -> Result<Option<bool>, String> {
    let value = resolve_text_argument(arg, sheet)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    trimmed
        .parse::<bool>()
        .map(Some)
        .map_err(|error| format!("Could not parse boolean value `{trimmed}`: {error}"))
}

fn resolve_segment_output_format(
    arg: &str,
    sheet: &Sheet,
) -> Result<Option<SegmentOutputFormat>, String> {
    let value = resolve_text_argument(arg, sheet)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let format = if trimmed.eq_ignore_ascii_case("jpeg") {
        Some(SegmentOutputFormat::Jpeg)
    } else if trimmed.eq_ignore_ascii_case("png") {
        Some(SegmentOutputFormat::Png)
    } else if trimmed.eq_ignore_ascii_case("webp") {
        Some(SegmentOutputFormat::Webp)
    } else {
        None
    };

    format
        .ok_or_else(|| {
            format!("Unsupported SEGMENT output_format `{trimmed}`. Use jpeg, png, or webp.")
        })
        .map(Some)
}

fn resolve_optional_u32_argument(arg: &str, sheet: &Sheet) -> Result<Option<u32>, String> {
    let value = resolve_text_argument(arg, sheet)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    trimmed
        .parse::<u32>()
        .map(Some)
        .map_err(|error| format!("Could not parse integer value `{trimmed}`: {error}"))
}

fn json_extract(input: &str, path: &str) -> Result<String, String> {
    let parsed: Value = serde_json::from_str(input.trim())
        .map_err(|error| format!("Could not parse JSON: {error}"))?;
    let trimmed_path = path.trim();
    if trimmed_path.is_empty() {
        return Err("JSON path cannot be empty".to_string());
    }

    let query = JsonPath::parse(trimmed_path)
        .map_err(|error| format!("Could not parse JSONPath: {error}"))?;
    let matches = query.query(&parsed).all();
    let value = match matches.as_slice() {
        [] => return Ok(String::new()),
        [value] => *value,
        _ => return Err("JSONEXTRACT path must resolve to at most one value".to_string()),
    };

    if value.is_null() {
        return Ok(String::new());
    }

    json_value_to_text(value)
}

fn json_value_to_text(value: &Value) -> Result<String, String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).map_err(|error| format!("Could not serialize JSON: {error}"))
        }
    }
}

fn evaluate_math_function(function: MathFunction, args: &[f64]) -> Result<FormulaValue, String> {
    let require_count = |count: usize| {
        if args.len() == count {
            Ok(())
        } else {
            Err(format!("Expected {count} arguments, got {}", args.len()))
        }
    };

    let number = match function {
        MathFunction::Sum => args.iter().sum(),
        MathFunction::Product => args.iter().product(),
        MathFunction::Average => {
            if args.is_empty() {
                return Err("AVERAGE requires at least one argument".to_string());
            }
            args.iter().sum::<f64>() / args.len() as f64
        }
        MathFunction::Min => args
            .iter()
            .copied()
            .reduce(f64::min)
            .ok_or_else(|| "MIN requires at least one argument".to_string())?,
        MathFunction::Max => args
            .iter()
            .copied()
            .reduce(f64::max)
            .ok_or_else(|| "MAX requires at least one argument".to_string())?,
        MathFunction::Add => {
            require_count(2)?;
            args[0] + args[1]
        }
        MathFunction::Subtract => {
            require_count(2)?;
            args[0] - args[1]
        }
        MathFunction::Multiply => {
            require_count(2)?;
            args[0] * args[1]
        }
        MathFunction::Divide => {
            require_count(2)?;
            if args[1] == 0.0 {
                return Err("Cannot divide by zero".to_string());
            }
            args[0] / args[1]
        }
    };

    Ok(FormulaValue::Number(number))
}

struct ExpressionParser<'a> {
    input: &'a str,
    position: usize,
    sheet: Option<&'a Sheet>,
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str, sheet: Option<&'a Sheet>) -> Self {
        Self {
            input,
            position: 0,
            sheet,
        }
    }

    fn parse(mut self) -> Result<f64, String> {
        let value = self.parse_expression()?;
        self.skip_whitespace();

        if self.position == self.input.len() {
            Ok(value)
        } else {
            Err(format!(
                "Unexpected token near {}",
                &self.input[self.position..]
            ))
        }
    }

    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_term()?;

        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.parse_term()?;
            } else if self.consume('-') {
                value -= self.parse_term()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut value = self.parse_factor()?;

        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.parse_factor()?;
            } else if self.consume('/') {
                let divisor = self.parse_factor()?;
                if divisor == 0.0 {
                    return Err("Cannot divide by zero".to_string());
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_factor(&mut self) -> Result<f64, String> {
        self.skip_whitespace();

        if self.consume('(') {
            let value = self.parse_expression()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err("Expected closing parenthesis".to_string());
            }
            return Ok(value);
        }

        if self.consume('-') {
            return Ok(-self.parse_factor()?);
        }

        if self
            .peek()
            .is_some_and(|character| character == '$' || character.is_ascii_alphabetic())
        {
            return self.parse_cell_reference();
        }

        self.parse_number()
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        self.skip_whitespace();
        let start = self.position;

        while let Some(character) = self.peek() {
            if character.is_ascii_digit() || character == '.' {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }

        if start == self.position {
            return Err("Expected a number".to_string());
        }

        self.input[start..self.position]
            .parse()
            .map_err(|_| "Invalid number".to_string())
    }

    fn parse_cell_reference(&mut self) -> Result<f64, String> {
        let start = self.position;
        self.consume('$');

        let column_start = self.position;
        while let Some(character) = self.peek() {
            if character.is_ascii_alphabetic() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }

        if column_start == self.position {
            self.position = start;
            return self.parse_number();
        }

        self.consume('$');

        let row_start = self.position;
        while let Some(character) = self.peek() {
            if character.is_ascii_digit() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }

        if row_start == self.position {
            return Err("Cell reference is missing a row number".to_string());
        }

        let reference = &self.input[start..self.position];
        let (row, col) = parse_cell_reference(reference)?;
        let Some(sheet) = self.sheet else {
            return Err(format!("Cannot resolve cell reference {reference}"));
        };

        match sheet.cell(row, col).map(|cell| &cell.value) {
            Some(CellValue::Text(value)) => value
                .trim()
                .parse()
                .map_err(|_| format!("{reference} does not contain a number")),
            Some(CellValue::Empty) | None => Ok(0.0),
            Some(CellValue::FormulaPending { .. }) => Err(format!(
                "{reference} is pending and cannot be used as a number"
            )),
            Some(CellValue::Cached(value)) => value
                .trim()
                .parse()
                .map_err(|_| format!("{reference} does not contain a number")),
            Some(CellValue::Error(error)) => Err(format!("{reference} has an error: {error}")),
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
    }
}

pub fn parse_cell_reference(reference: &str) -> Result<(usize, usize), String> {
    let parts = parse_cell_reference_parts(reference)?;
    Ok((parts.row, parts.col))
}

pub fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_basic_arithmetic_with_precedence() {
        assert_eq!(evaluate_formula("=1+2*3"), Ok(FormulaValue::Number(7.0)));
        assert_eq!(evaluate_formula("=(1+2)*3"), Ok(FormulaValue::Number(9.0)));
        assert_eq!(evaluate_formula("=-4/2"), Ok(FormulaValue::Number(-2.0)));
    }

    #[test]
    fn evaluates_named_math_functions() {
        assert_eq!(
            evaluate_formula("=SUM(1, 2, 3*4)"),
            Ok(FormulaValue::Number(15.0))
        );
        let mut sheet = Sheet::new("Math", 2, 2);
        sheet.set_cell_input(0, 0, "2".to_string());
        sheet.set_cell_input(0, 1, "3".to_string());
        assert_eq!(
            evaluate_formula_for_sheet("=SUM(A1, B1, 5)", &sheet),
            Ok(FormulaValue::Number(10.0))
        );
        assert_eq!(
            evaluate_formula("=AVERAGE(2, 4, 6)"),
            Ok(FormulaValue::Number(4.0))
        );
        assert_eq!(
            evaluate_formula("=DIVIDE(10, 2)"),
            Ok(FormulaValue::Number(5.0))
        );
    }

    #[test]
    fn provider_formulas_are_runnable() {
        assert_eq!(
            evaluate_formula("=GENERATEIMAGE(A1, A2)"),
            Ok(FormulaValue::Pending(
                "fal.image request is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=SEGMENT(A1)"),
            Ok(FormulaValue::Pending(
                "fal.segment request is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=LLM(A1, A2)"),
            Ok(FormulaValue::Pending(
                "fal.openrouter request is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=GENERATEVIDEO(A1, A2)"),
            Ok(FormulaValue::Pending(
                "fal.video request is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=CONCATENATEVIDEO(A1, A2)"),
            Ok(FormulaValue::Pending(
                "Local video concatenation is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula(r#"=CONCATENATE("hello", " ", "world")"#),
            Ok(FormulaValue::Text("hello world".to_string()))
        );
    }

    #[test]
    fn direct_cell_references_preserve_text_values() {
        let mut sheet = Sheet::new("Text", 1, 1);
        sheet.set_cell_input(0, 0, "hello".to_string());

        assert_eq!(
            evaluate_formula_for_sheet("=A1", &sheet),
            Ok(FormulaValue::Text("hello".to_string()))
        );
    }

    #[test]
    fn concatenates_text_values_from_cells_and_literals() {
        let mut sheet = Sheet::new("Text", 2, 2);
        sheet.set_cell_input(0, 0, "Hello".to_string());
        sheet.set_cell_input(0, 1, "world".to_string());

        assert_eq!(
            evaluate_formula_for_sheet(r#"=CONCATENATE(A1, " ", B1, "!")"#, &sheet),
            Ok(FormulaValue::Text("Hello world!".to_string()))
        );
    }

    #[test]
    fn direct_cell_references_preserve_cached_values() {
        let mut sheet = Sheet::new("Media", 1, 1);
        sheet.set_cell_value_with_cache(
            0,
            0,
            "https://example.com/image.png".to_string(),
            CellValue::Cached("https://example.com/image.png".to_string()),
            Some("cache-key".to_string()),
        );

        assert_eq!(
            evaluate_formula_for_sheet("=$A1", &sheet),
            Ok(FormulaValue::Cached {
                value: "https://example.com/image.png".to_string(),
                cache_key: Some("cache-key".to_string()),
            })
        );
    }

    #[test]
    fn extracts_json_values_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("JSON", 2, 2);
        let json = r#"{"user":{"name":"Ada","active":true,"details":{"age":37}},"tags":["ai","art"]}"#;
        sheet.set_cell_value_with_cache(
            0,
            0,
            json.to_string(),
            CellValue::Cached(json.to_string()),
            Some("json-cache".to_string()),
        );

        assert_eq!(
            evaluate_formula_for_sheet(r#"=JSONEXTRACT(A1, "$.user.name")"#, &sheet),
            Ok(FormulaValue::Text("Ada".to_string()))
        );
        assert_eq!(
            evaluate_formula(
                r#"=JSONEXTRACT("{\"user\":{\"name\":\"Ada\",\"active\":true,\"details\":{\"age\":37}},\"tags\":[\"ai\",\"art\"]}", "$.user")"#
            ),
            Ok(FormulaValue::Text(
                r#"{"active":true,"details":{"age":37},"name":"Ada"}"#.to_string()
            ))
        );
    }

    #[test]
    fn extracts_json_arrays_and_returns_empty_text_for_missing_paths() {
        let mut sheet = Sheet::new("JSON Edge", 2, 2);
        sheet.set_cell_input(0, 0, r#"{"items":[[{"name":"first"}],[{"name":"second"}]],"empty":null}"#.to_string());

        assert_eq!(
            evaluate_formula_for_sheet(r#"=JSONEXTRACT(A1, "$.items[0][0].name")"#, &sheet),
            Ok(FormulaValue::Text("first".to_string()))
        );
        assert_eq!(
            evaluate_formula_for_sheet(r#"=JSONEXTRACT(A1, "$.items[1]")"#, &sheet),
            Ok(FormulaValue::Text(r#"[{"name":"second"}]"#.to_string()))
        );
        assert_eq!(
            evaluate_formula_for_sheet(r#"=JSONEXTRACT(A1, "$.missing.path")"#, &sheet),
            Ok(FormulaValue::Text(String::new()))
        );
        assert_eq!(
            evaluate_formula_for_sheet(r#"=JSONEXTRACT(A1, "$.empty")"#, &sheet),
            Ok(FormulaValue::Text(String::new()))
        );
    }

    #[test]
    fn rejects_invalid_json_paths() {
        assert_eq!(
            evaluate_formula(r#"=JSONEXTRACT("{\"items\":[1]}", "")"#),
            Err("JSON path cannot be empty".to_string())
        );
        assert_eq!(
            evaluate_formula(r#"=JSONEXTRACT("{\"items\":[1,2]}", "$.items[*]")"#),
            Err("JSONEXTRACT path must resolve to at most one value".to_string())
        );
    }

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

        assert_eq!(request.prompt, "Say hi");
        assert_eq!(request.model, "google/gemini-2.5-flash");
        assert_eq!(request.system_prompt.as_deref(), Some("Only answer in one sentence"));
        assert!(request.image_urls.is_empty());
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

        assert_eq!(request.prompt, "Describe this image");
        assert_eq!(request.model, DEFAULT_MODEL);
        assert_eq!(request.image_urls, vec!["https://example.com/image.png"]);
        assert_eq!(request.system_prompt.as_deref(), Some("Keep it brief"));
        assert_eq!(request.endpoint(), crate::backend::providers::openrouter::VISION_ENDPOINT);
    }

    #[test]
    fn builds_generate_image_request_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("Image", 2, 3);
        sheet.set_cell_input(0, 0, "A moody cabin".to_string());
        sheet.set_cell_input(0, 1, "openai/gpt-image-2".to_string());
        sheet.set_cell_input(0, 2, "https://example.com/ref.png".to_string());

        let request = generate_image_request_for_sheet("=GENERATEIMAGE($A1, $B1, $C1)", &sheet)
            .unwrap()
            .unwrap();

        assert_eq!(request.prompt, "A moody cabin");
        assert_eq!(request.model, "openai/gpt-image-2");
        assert_eq!(request.endpoint, "openai/gpt-image-2/edit");
        assert_eq!(
            request.input["image_urls"][0],
            "https://example.com/ref.png"
        );
        assert_eq!(request.input["quality"], "medium");
        assert_eq!(request.quality.as_deref(), Some("medium"));
    }

    #[test]
    fn builds_generate_image_request_with_explicit_quality() {
        let mut sheet = Sheet::new("Image", 2, 3);
        sheet.set_cell_input(0, 0, "A moody cabin".to_string());
        sheet.set_cell_input(0, 2, "https://example.com/ref.png".to_string());

        let request = generate_image_request_for_sheet(
            r#"=GENERATEIMAGE($A1, "openai/gpt-image-2", "high", $C1)"#,
            &sheet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(request.input["quality"], "high");
        assert_eq!(request.quality.as_deref(), Some("high"));
        assert_eq!(
            request.input["image_urls"][0],
            "https://example.com/ref.png"
        );
    }

    #[test]
    fn routes_flux_generate_image_with_reference_image_to_image_to_image() {
        let mut sheet = Sheet::new("Image", 2, 3);
        sheet.set_cell_input(0, 0, "Add a car".to_string());
        sheet.set_cell_input(0, 2, "https://example.com/ref.png".to_string());

        let request =
            generate_image_request_for_sheet(r#"=GENERATEIMAGE($A1, "flux/dev", $C1)"#, &sheet)
                .unwrap()
                .unwrap();

        assert_eq!(request.model, "flux/dev");
        assert_eq!(request.endpoint, "fal-ai/flux/dev/image-to-image");
        assert_eq!(request.input["image_url"], "https://example.com/ref.png");
    }

    #[test]
    fn builds_segment_request_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("Segment", 2, 10);
        sheet.set_cell_input(0, 0, "https://example.com/image.png".to_string());
        sheet.set_cell_input(0, 1, "A wheel".to_string());
        sheet.set_cell_input(
            0,
            2,
            r#"[{"x":10,"y":20,"label":1,"object_id":2}]"#.to_string(),
        );
        sheet.set_cell_input(
            0,
            3,
            r#"{"x_min":1,"y_min":2,"x_max":30,"y_max":40}"#.to_string(),
        );
        sheet.set_cell_input(0, 4, "false".to_string());
        sheet.set_cell_input(0, 5, "webp".to_string());
        sheet.set_cell_input(0, 6, "true".to_string());
        sheet.set_cell_input(0, 7, "4".to_string());
        sheet.set_cell_input(0, 8, "true".to_string());
        sheet.set_cell_input(0, 9, "false".to_string());

        let request = segment_request_for_sheet(
            r#"=SEGMENT($A1, $B1, $C1, $D1, $E1, $F1, $G1, $H1, $I1, $J1)"#,
            &sheet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(request.endpoint, "fal-ai/sam-3/image");
        assert_eq!(request.prompt, "A wheel");
        assert_eq!(request.output_format, "webp");
        assert_eq!(request.input["apply_mask"], false);
        assert_eq!(request.input["return_multiple_masks"], true);
        assert_eq!(request.input["max_masks"], 4);
        assert_eq!(request.input["point_prompts"][0]["object_id"], 2);
        assert_eq!(request.input["box_prompts"][0]["x_max"], 30);
    }

    #[test]
    fn builds_generate_video_request_from_literals_and_cell_refs() {
        let mut sheet = Sheet::new("Video", 2, 2);
        sheet.set_cell_input(0, 0, "A toy robot waves".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/robot.png".to_string());

        let request = generate_video_request_for_sheet(
            r#"=GENERATEVIDEO($A1, $B1, "fal-ai/veo2/image-to-video", 6, "9:16")"#,
            &sheet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(request.prompt, "A toy robot waves");
        assert_eq!(request.image_url, "https://example.com/robot.png");
        assert_eq!(request.model, "fal-ai/veo2/image-to-video");
        assert_eq!(request.duration, 6);
        assert_eq!(request.aspect_ratio.as_deref(), Some("9:16"));
    }

    #[test]
    fn resolves_concatenate_video_inputs() {
        let mut sheet = Sheet::new("Video", 2, 2);
        sheet.set_cell_input(0, 0, "https://example.com/a.mp4".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/b.mp4".to_string());

        let inputs = concatenate_video_inputs_for_sheet(r#"=CONCATENATEVIDEO($A1, $B1)"#, &sheet)
            .unwrap()
            .unwrap();

        assert_eq!(
            inputs,
            vec![
                "https://example.com/a.mp4".to_string(),
                "https://example.com/b.mp4".to_string()
            ]
        );
    }

    #[test]
    fn parses_absolute_cell_references() {
        assert_eq!(parse_cell_reference("$A1"), Ok((0, 0)));
        assert_eq!(parse_cell_reference("B$12"), Ok((11, 1)));
        assert_eq!(parse_cell_reference("$AA10"), Ok((9, 26)));
    }

    #[test]
    fn rejects_overflowing_cell_references() {
        let reference = format!("{}1", "Z".repeat(32));
        let error = parse_cell_reference(&reference).unwrap_err();
        assert_eq!(error, "Cell reference column is too large");
    }
}

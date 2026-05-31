use super::{FormulaValue, json_extract::json_extract, math::evaluate_math_function};
use crate::backend::{
    document::{CellValue, Sheet},
    fill::parse_cell_reference_parts,
    formulas::{FORMULA_FUNCTIONS, FormulaImplementation},
};

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

pub(super) fn strip_numbered_prefix(value: &str) -> Option<&str> {
    let mut digits = 0usize;
    for character in value.chars() {
        if character.is_ascii_digit() {
            digits += 1;
        } else {
            break;
        }
    }

    if digits == 0 {
        return None;
    }

    let remainder = &value[digits..];
    remainder
        .strip_prefix(". ")
        .or_else(|| remainder.strip_prefix(") "))
}

pub(super) fn strip_markdown_code_fence(output: &str) -> &str {
    let trimmed = output.trim();
    let Some(without_opening) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let without_language = without_opening
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or(without_opening);
    without_language
        .strip_suffix("```")
        .unwrap_or(without_language)
        .trim()
}

pub(super) fn parse_function_call(expression: &str) -> Result<Option<(String, &str)>, String> {
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

pub(super) fn split_formula_arguments(args: &str) -> Result<Vec<String>, String> {
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

pub(super) fn resolve_text_argument(arg: &str, sheet: &Sheet) -> Result<String, String> {
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
            evaluate_formula("=LLM_LIST_DOWN(A1, A2)"),
            Ok(FormulaValue::Pending(
                "fal.openrouter request is ready to run".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=LLM_LIST_RIGHT(A1, A2)"),
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

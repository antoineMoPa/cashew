use super::formulas::{FormulaImplementation, FormulaValue, MathFunction, FORMULA_FUNCTIONS};

#[derive(Debug, Clone, PartialEq)]
pub enum FormulaValue {
    Number(f64),
    Pending(String),
}

pub fn evaluate_formula(input: &str) -> Result<FormulaValue, String> {
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
            FormulaImplementation::NoopAi { placeholder } => {
                Ok(FormulaValue::Pending(placeholder.to_string()))
            }
            FormulaImplementation::Math(math) => {
                let values = parse_numeric_arguments(args)?;
                evaluate_math_function(math, &values)
            }
        };
    }

    ExpressionParser::new(expression)
        .parse()
        .map(FormulaValue::Number)
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

fn parse_numeric_arguments(args: &str) -> Result<Vec<f64>, String> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }

    args.split(',')
        .map(|arg| ExpressionParser::new(arg.trim()).parse())
        .collect()
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
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
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
    fn ai_formulas_are_explicit_noops() {
        assert_eq!(
            evaluate_formula("=GENERATEIMAGE(A1, A2)"),
            Ok(FormulaValue::Pending(
                "AI image generation is not implemented yet.".to_string()
            ))
        );
        assert_eq!(
            evaluate_formula("=LLM(A1, A2)"),
            Ok(FormulaValue::Pending(
                "LLM generation is not implemented yet.".to_string()
            ))
        );
    }
}

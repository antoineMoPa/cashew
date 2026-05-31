use super::FormulaValue;
use crate::backend::formulas::MathFunction;

pub(super) fn evaluate_math_function(
    function: MathFunction,
    args: &[f64],
) -> Result<FormulaValue, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_named_math_functions() {
        assert_eq!(
            evaluate_math_function(MathFunction::Sum, &[1.0, 2.0, 12.0]),
            Ok(FormulaValue::Number(15.0))
        );
        assert_eq!(
            evaluate_math_function(MathFunction::Average, &[2.0, 4.0, 6.0]),
            Ok(FormulaValue::Number(4.0))
        );
        assert_eq!(
            evaluate_math_function(MathFunction::Divide, &[10.0, 2.0]),
            Ok(FormulaValue::Number(5.0))
        );
    }
}

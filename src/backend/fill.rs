use super::document::{Sheet, column_name};

const NUMBER_SERIES_STEP_TOLERANCE: f64 = 0.000_000_001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellReferenceParts {
    pub row: usize,
    pub col: usize,
    pub row_absolute: bool,
    pub col_absolute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

impl FillRange {
    pub fn new(anchor: (usize, usize), end: (usize, usize)) -> Self {
        Self {
            start_row: anchor.0.min(end.0),
            start_col: anchor.1.min(end.1),
            end_row: anchor.0.max(end.0),
            end_col: anchor.1.max(end.1),
        }
    }

    pub fn width(&self) -> usize {
        self.end_col - self.start_col + 1
    }

    pub fn height(&self) -> usize {
        self.end_row - self.start_row + 1
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        (self.start_row..=self.end_row).contains(&row)
            && (self.start_col..=self.end_col).contains(&col)
    }
}

pub fn fill_region(
    sheet: &mut Sheet,
    source: FillRange,
    target_end: (usize, usize),
) -> Result<FillRange, String> {
    sheet.ensure_size(
        source.end_row.max(target_end.0) + 1,
        source.end_col.max(target_end.1) + 1,
    );

    let target = FillRange::new((source.start_row, source.start_col), target_end);
    if target == source {
        return Ok(target);
    }

    let number_series = number_series_for_fill(sheet, source, target);
    let mut updates = Vec::new();
    for row in target.start_row..=target.end_row {
        for col in target.start_col..=target.end_col {
            if source.contains(row, col) {
                continue;
            }

            let input = if let Some(series) = &number_series {
                series.input_for(row, col)
            } else {
                let source_row = source.start_row + ((row - source.start_row) % source.height());
                let source_col = source.start_col + ((col - source.start_col) % source.width());
                let row_delta = row as isize - source_row as isize;
                let col_delta = col as isize - source_col as isize;
                offset_formula_references(
                    sheet
                        .cell(source_row, source_col)
                        .map(|cell| cell.input.as_str())
                        .unwrap_or_default(),
                    row_delta,
                    col_delta,
                )?
            };
            updates.push((row, col, input));
        }
    }

    for (row, col, input) in updates {
        sheet.set_cell_input_without_recalculate(row, col, input);
    }

    sheet.recalculate_formulas();
    Ok(target)
}

#[derive(Debug, Clone, Copy)]
enum NumberSeries {
    Vertical {
        start_row: usize,
        col: usize,
        first: f64,
        step: f64,
        whole_numbers: bool,
    },
    Horizontal {
        row: usize,
        start_col: usize,
        first: f64,
        step: f64,
        whole_numbers: bool,
    },
}

impl NumberSeries {
    fn input_for(&self, row: usize, col: usize) -> String {
        let (offset, first, step, whole_numbers) = match *self {
            NumberSeries::Vertical {
                start_row,
                col: series_col,
                first,
                step,
                whole_numbers,
            } => {
                debug_assert_eq!(series_col, col);
                (row - start_row, first, step, whole_numbers)
            }
            NumberSeries::Horizontal {
                row: series_row,
                start_col,
                first,
                step,
                whole_numbers,
            } => {
                debug_assert_eq!(series_row, row);
                (col - start_col, first, step, whole_numbers)
            }
        };
        let value = first + step * offset as f64;

        if whole_numbers {
            format!("{:.0}", value)
        } else {
            value.to_string()
        }
    }
}

fn number_series_for_fill(
    sheet: &Sheet,
    source: FillRange,
    target: FillRange,
) -> Option<NumberSeries> {
    if source.width() == 1 && target.width() == 1 && source.height() >= 2 {
        let values = numeric_inputs_for_column(sheet, source)?;
        let (first, step, whole_numbers) = arithmetic_series_parts(&values)?;
        return Some(NumberSeries::Vertical {
            start_row: source.start_row,
            col: source.start_col,
            first,
            step,
            whole_numbers,
        });
    }

    if source.height() == 1 && target.height() == 1 && source.width() >= 2 {
        let values = numeric_inputs_for_row(sheet, source)?;
        let (first, step, whole_numbers) = arithmetic_series_parts(&values)?;
        return Some(NumberSeries::Horizontal {
            row: source.start_row,
            start_col: source.start_col,
            first,
            step,
            whole_numbers,
        });
    }

    None
}

fn numeric_inputs_for_column(sheet: &Sheet, source: FillRange) -> Option<Vec<NumericInput>> {
    (source.start_row..=source.end_row)
        .map(|row| numeric_input_at(sheet, row, source.start_col))
        .collect()
}

fn numeric_inputs_for_row(sheet: &Sheet, source: FillRange) -> Option<Vec<NumericInput>> {
    (source.start_col..=source.end_col)
        .map(|col| numeric_input_at(sheet, source.start_row, col))
        .collect()
}

fn numeric_input_at(sheet: &Sheet, row: usize, col: usize) -> Option<NumericInput> {
    sheet
        .cell(row, col)
        .and_then(|cell| NumericInput::from_text(&cell.input))
}

#[derive(Debug, Clone, Copy)]
struct NumericInput {
    value: f64,
    whole_number: bool,
}

impl NumericInput {
    fn from_text(input: &str) -> Option<Self> {
        let text = input.trim();
        if text.is_empty() || text.starts_with('=') {
            return None;
        }

        let value = text.parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }

        Some(Self {
            value,
            whole_number: text.parse::<i64>().is_ok(),
        })
    }
}

fn arithmetic_series_parts(values: &[NumericInput]) -> Option<(f64, f64, bool)> {
    let [first, second, rest @ ..] = values else {
        return None;
    };
    let step = second.value - first.value;
    let has_constant_step = rest
        .iter()
        .scan(second.value, |previous, current| {
            let difference = current.value - *previous;
            *previous = current.value;
            Some((difference - step).abs() <= NUMBER_SERIES_STEP_TOLERANCE)
        })
        .all(|matches| matches);

    if !has_constant_step {
        return None;
    }

    let whole_numbers = values.iter().all(|input| input.whole_number) && step.fract() == 0.0;
    Some((first.value, step, whole_numbers))
}

pub fn parse_cell_reference_parts(reference: &str) -> Result<CellReferenceParts, String> {
    let mut chars = reference.trim().chars().peekable();

    let mut col_absolute = false;
    if chars.peek() == Some(&'$') {
        col_absolute = true;
        chars.next();
    }

    let mut col = 0usize;
    let mut saw_column = false;
    while let Some(character) = chars.peek().copied() {
        if character.is_ascii_alphabetic() {
            saw_column = true;
            let letter_value = (character.to_ascii_uppercase() as u8 - b'A' + 1) as usize;
            col = col
                .checked_mul(26)
                .and_then(|value| value.checked_add(letter_value))
                .ok_or_else(|| "Cell reference column is too large".to_string())?;
            chars.next();
        } else {
            break;
        }
    }

    if !saw_column {
        return Err("Cell reference is missing a column".to_string());
    }

    let mut row_absolute = false;
    if chars.peek() == Some(&'$') {
        row_absolute = true;
        chars.next();
    }

    let row_text = chars.collect::<String>();
    if row_text.is_empty() || !row_text.chars().all(|character| character.is_ascii_digit()) {
        return Err("Cell reference is missing a row number".to_string());
    }

    let row = row_text
        .parse::<usize>()
        .map_err(|_| "Cell row is invalid".to_string())?;
    if row == 0 {
        return Err("Cell row must be 1 or greater".to_string());
    }

    Ok(CellReferenceParts {
        row: row - 1,
        col: col - 1,
        row_absolute,
        col_absolute,
    })
}

pub fn offset_formula_references(
    input: &str,
    row_delta: isize,
    col_delta: isize,
) -> Result<String, String> {
    let trimmed_start = input.trim_start();
    if !trimmed_start.starts_with('=') {
        return Ok(input.to_string());
    }

    let prefix_len = input.len() - trimmed_start.len();
    let prefix = &input[..prefix_len];
    let expression = &trimmed_start[1..];
    let rewritten = rewrite_formula_expression(expression, row_delta, col_delta)?;
    Ok(format!("{prefix}={rewritten}"))
}

fn rewrite_formula_expression(
    expression: &str,
    row_delta: isize,
    col_delta: isize,
) -> Result<String, String> {
    let mut rewritten = String::with_capacity(expression.len());
    let mut index = 0;

    while index < expression.len() {
        let Some(character) = expression[index..].chars().next() else {
            break;
        };

        if character == '"' || character == '\'' {
            let end = skip_quoted_text(expression, index, character);
            rewritten.push_str(&expression[index..end]);
            index = end;
            continue;
        }

        if let Some((reference, end)) = parse_reference_at(expression, index) {
            rewritten.push_str(&shift_reference(reference, row_delta, col_delta)?);
            index = end;
            continue;
        }

        rewritten.push(character);
        index += character.len_utf8();
    }

    Ok(rewritten)
}

fn parse_reference_at(expression: &str, start: usize) -> Option<(CellReferenceParts, usize)> {
    let first = expression[start..].chars().next()?;
    if !(first == '$' || first.is_ascii_alphabetic()) {
        return None;
    }

    if start > 0 {
        let previous = expression[..start].chars().next_back()?;
        if previous.is_ascii_alphanumeric() || previous == '_' || previous == '$' {
            return None;
        }
    }

    let bytes = expression.as_bytes();
    let mut index = start;

    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }

    let column_start = index;
    while matches!(bytes.get(index), Some(b'A'..=b'Z') | Some(b'a'..=b'z')) {
        index += 1;
    }

    if index == column_start {
        return None;
    }

    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }

    let row_start = index;
    while matches!(bytes.get(index), Some(b'0'..=b'9')) {
        index += 1;
    }

    if index == row_start {
        return None;
    }

    if let Some(next) = expression[index..].chars().next() {
        if next.is_ascii_alphanumeric() || next == '_' || next == '$' {
            return None;
        }
    }

    let parts = parse_cell_reference_parts(&expression[start..index]).ok()?;
    Some((parts, index))
}

fn shift_reference(
    reference: CellReferenceParts,
    row_delta: isize,
    col_delta: isize,
) -> Result<String, String> {
    let row = if reference.row_absolute {
        reference.row
    } else {
        reference
            .row
            .checked_add_signed(row_delta)
            .ok_or_else(|| "Cell reference moved outside the sheet".to_string())?
    };
    let col = if reference.col_absolute {
        reference.col
    } else {
        reference
            .col
            .checked_add_signed(col_delta)
            .ok_or_else(|| "Cell reference moved outside the sheet".to_string())?
    };

    Ok(format!(
        "{}{}{}{}",
        if reference.col_absolute { "$" } else { "" },
        column_name(col),
        if reference.row_absolute { "$" } else { "" },
        row + 1
    ))
}

fn skip_quoted_text(expression: &str, start: usize, quote: char) -> usize {
    let bytes = expression.as_bytes();
    let mut index = start + quote.len_utf8();

    while index < expression.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(expression.len());
            continue;
        }

        if bytes[index] == quote as u8 {
            return index + quote.len_utf8();
        }

        index += 1;
    }

    expression.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_absolute_cell_references_and_absolute_axes() {
        let reference = parse_cell_reference_parts("$B$12").unwrap();
        assert_eq!(reference.row, 11);
        assert_eq!(reference.col, 1);
        assert!(reference.row_absolute);
        assert!(reference.col_absolute);
    }

    #[test]
    fn offsets_formula_references_for_single_cell_copy() {
        let rewritten = offset_formula_references("=A1+$B1+C$1+$D$2", 1, 2).unwrap();
        assert_eq!(rewritten, "=C2+$B2+E$1+$D$2");
    }

    #[test]
    fn offsets_formula_references_preserve_text_and_quotes() {
        let rewritten = offset_formula_references(r#"=LLM("see A1", A1, $B1, C$1)"#, 1, 2).unwrap();
        assert_eq!(rewritten, r#"=LLM("see A1", C2, $B2, E$1)"#);
    }

    #[test]
    fn fills_single_cell_as_a_copy_with_offsets() {
        let mut sheet = Sheet::new("Fill", 3, 3);
        sheet.set_cell_input(0, 0, "=A1+$B1".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((0, 0), (0, 0)), (1, 1)).unwrap();

        assert_eq!(filled, FillRange::new((0, 0), (1, 1)));
        assert_eq!(
            sheet.cell(0, 0).map(|cell| cell.input.as_str()),
            Some("=A1+$B1")
        );
        assert_eq!(
            sheet.cell(0, 1).map(|cell| cell.input.as_str()),
            Some("=B1+$B1")
        );
        assert_eq!(
            sheet.cell(1, 0).map(|cell| cell.input.as_str()),
            Some("=A2+$B2")
        );
        assert_eq!(
            sheet.cell(1, 1).map(|cell| cell.input.as_str()),
            Some("=B2+$B2")
        );
    }

    #[test]
    fn fills_relative_references_across_columns_and_rows() {
        let mut sheet = Sheet::new("Fill", 18, 5);
        sheet.set_cell_input(15, 2, "=C16".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((15, 2), (15, 2)), (16, 3)).unwrap();

        assert_eq!(filled, FillRange::new((15, 2), (16, 3)));
        assert_eq!(
            sheet.cell(15, 3).map(|cell| cell.input.as_str()),
            Some("=D16")
        );
        assert_eq!(
            sheet.cell(16, 2).map(|cell| cell.input.as_str()),
            Some("=C17")
        );
    }

    #[test]
    fn fills_row_patterns_by_repeating_the_source_region() {
        let mut sheet = Sheet::new("Fill", 1, 5);
        sheet.set_cell_input(0, 0, "one".to_string());
        sheet.set_cell_input(0, 1, "=A1".to_string());
        sheet.set_cell_input(0, 2, "two".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((0, 0), (0, 2)), (0, 4)).unwrap();

        assert_eq!(filled, FillRange::new((0, 0), (0, 4)));
        assert_eq!(
            sheet.cell(0, 3).map(|cell| cell.input.as_str()),
            Some("one")
        );
        assert_eq!(
            sheet.cell(0, 4).map(|cell| cell.input.as_str()),
            Some("=D1")
        );
    }

    #[test]
    fn fills_numeric_row_patterns_as_arithmetic_series() {
        let mut sheet = Sheet::new("Fill", 1, 5);
        sheet.set_cell_input(0, 0, "1".to_string());
        sheet.set_cell_input(0, 1, "2".to_string());
        sheet.set_cell_input(0, 2, "3".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((0, 0), (0, 2)), (0, 4)).unwrap();

        assert_eq!(filled, FillRange::new((0, 0), (0, 4)));
        assert_eq!(sheet.cell(0, 3).map(|cell| cell.input.as_str()), Some("4"));
        assert_eq!(sheet.cell(0, 4).map(|cell| cell.input.as_str()), Some("5"));
    }

    #[test]
    fn fills_column_patterns_by_repeating_the_source_region() {
        let mut sheet = Sheet::new("Fill", 5, 1);
        sheet.set_cell_input(0, 0, "start".to_string());
        sheet.set_cell_input(1, 0, "=A1".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((0, 0), (1, 0)), (4, 0)).unwrap();

        assert_eq!(filled, FillRange::new((0, 0), (4, 0)));
        assert_eq!(
            sheet.cell(2, 0).map(|cell| cell.input.as_str()),
            Some("start")
        );
        assert_eq!(
            sheet.cell(3, 0).map(|cell| cell.input.as_str()),
            Some("=A3")
        );
        assert_eq!(
            sheet.cell(4, 0).map(|cell| cell.input.as_str()),
            Some("start")
        );
    }

    #[test]
    fn fills_numeric_column_patterns_as_arithmetic_series() {
        let mut sheet = Sheet::new("Fill", 5, 1);
        sheet.set_cell_input(0, 0, "1".to_string());
        sheet.set_cell_input(1, 0, "2".to_string());
        sheet.set_cell_input(2, 0, "3".to_string());
        let filled = fill_region(&mut sheet, FillRange::new((0, 0), (2, 0)), (4, 0)).unwrap();

        assert_eq!(filled, FillRange::new((0, 0), (4, 0)));
        assert_eq!(sheet.cell(3, 0).map(|cell| cell.input.as_str()), Some("4"));
        assert_eq!(sheet.cell(4, 0).map(|cell| cell.input.as_str()), Some("5"));
    }

    #[test]
    fn fills_grid_patterns_by_tiling_the_selected_region() {
        let mut sheet = Sheet::new("Fill", 4, 4);
        sheet.set_cell_input(0, 0, "r1c1".to_string());
        sheet.set_cell_input(0, 1, "=A1".to_string());
        sheet.set_cell_input(1, 0, "=A1".to_string());
        sheet.set_cell_input(1, 1, "=B1".to_string());

        fill_region(&mut sheet, FillRange::new((0, 0), (1, 1)), (3, 3)).unwrap();

        assert_eq!(
            sheet.cell(2, 2).map(|cell| cell.input.as_str()),
            Some("r1c1")
        );
        assert_eq!(
            sheet.cell(2, 3).map(|cell| cell.input.as_str()),
            Some("=C3")
        );
        assert_eq!(
            sheet.cell(3, 2).map(|cell| cell.input.as_str()),
            Some("=C3")
        );
        assert_eq!(
            sheet.cell(3, 3).map(|cell| cell.input.as_str()),
            Some("=D3")
        );
    }
}

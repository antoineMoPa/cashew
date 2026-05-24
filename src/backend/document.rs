use std::{collections::BTreeMap, fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::{
    cache::CacheEntry,
    formula_implementations::{
        FormulaValue, evaluate_formula_for_sheet, format_number, parse_cell_reference,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CashewDocument {
    pub version: u32,
    pub title: String,
    pub sheets: Vec<Sheet>,
    pub cache: BTreeMap<String, CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sheet {
    pub name: String,
    pub rows: usize,
    pub cols: usize,
    #[serde(default)]
    pub column_widths: BTreeMap<String, u16>,
    #[serde(default)]
    pub row_heights: BTreeMap<String, u16>,
    pub cells: BTreeMap<String, Cell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cell {
    pub input: String,
    pub value: CellValue,
    pub cache_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum CellValue {
    Empty,
    Text(String),
    FormulaPending { message: String },
    Cached(String),
    Error(String),
}

impl CashewDocument {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            version: 1,
            title: title.into(),
            sheets: vec![Sheet::new("Default", 12, 8)],
            cache: BTreeMap::new(),
        }
    }

    pub fn load_json(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let json = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save_json(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn active_sheet(&self) -> Option<&Sheet> {
        self.sheets.first()
    }

    pub fn active_sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.sheets.first_mut()
    }
}

impl Default for CashewDocument {
    fn default() -> Self {
        Self::new("Untitled Cashew")
    }
}

impl Sheet {
    pub fn new(name: impl Into<String>, rows: usize, cols: usize) -> Self {
        Self {
            name: name.into(),
            rows,
            cols,
            column_widths: BTreeMap::new(),
            row_heights: BTreeMap::new(),
            cells: BTreeMap::new(),
        }
    }

    pub fn set_cell_input(&mut self, row: usize, col: usize, input: String) {
        self.ensure_size(row + 1, col + 1);

        let (value, cache_key) = if input.trim().is_empty() {
            (CellValue::Empty, None)
        } else if input.trim_start().starts_with('=') {
            formula_value_for_input(&input, self)
        } else {
            (CellValue::Text(input.clone()), None)
        };

        self.cells.insert(
            cell_key(row, col),
            Cell {
                input,
                value,
                cache_key,
            },
        );
        self.recalculate_formulas();
    }

    pub fn recalculate_formulas(&mut self) {
        const MAX_PASSES: usize = 8;

        for _ in 0..MAX_PASSES {
            let formulas = self
                .cells
                .iter()
                .filter_map(|(key, cell)| {
                    (cell.input.trim_start().starts_with('=')
                        && (cell.cache_key.is_none()
                            || is_direct_cell_reference_formula(&cell.input)))
                    .then(|| (key.clone(), cell.input.clone()))
                })
                .collect::<Vec<_>>();

            let mut changed = false;
            for (key, input) in formulas {
                let (value, cache_key) = formula_value_for_input(&input, self);
                if let Some(cell) = self.cells.get_mut(&key) {
                    changed |= cell.value != value || cell.cache_key != cache_key;
                    cell.value = value;
                    cell.cache_key = cache_key;
                }
            }

            if !changed {
                break;
            }
        }
    }

    pub fn set_cell_value_with_cache(
        &mut self,
        row: usize,
        col: usize,
        input: String,
        value: CellValue,
        cache_key: Option<String>,
    ) {
        self.ensure_size(row + 1, col + 1);
        self.cells.insert(
            cell_key(row, col),
            Cell {
                input,
                value,
                cache_key,
            },
        );
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.get(&cell_key(row, col))
    }

    pub fn column_width(&self, col: usize) -> u16 {
        self.column_widths
            .get(&col.to_string())
            .copied()
            .unwrap_or(DEFAULT_COLUMN_WIDTH)
    }

    pub fn set_column_width(&mut self, col: usize, width: u16) {
        self.ensure_size(self.rows, col + 1);
        self.column_widths.insert(col.to_string(), width);
    }

    pub fn row_height(&self, row: usize) -> u16 {
        self.row_heights
            .get(&row.to_string())
            .copied()
            .unwrap_or(DEFAULT_ROW_HEIGHT)
    }

    pub fn set_row_height(&mut self, row: usize, height: u16) {
        self.ensure_size(row + 1, self.cols);
        self.row_heights.insert(row.to_string(), height);
    }

    pub fn ensure_size(&mut self, rows: usize, cols: usize) {
        self.rows = self.rows.max(rows);
        self.cols = self.cols.max(cols);
    }
}

fn formula_value_for_input(input: &str, sheet: &Sheet) -> (CellValue, Option<String>) {
    match evaluate_formula_for_sheet(input, sheet) {
        Ok(FormulaValue::Number(number)) => (CellValue::Text(format_number(number)), None),
        Ok(FormulaValue::Text(value)) => (CellValue::Text(value), None),
        Ok(FormulaValue::Cached { value, cache_key }) => (CellValue::Cached(value), cache_key),
        Ok(FormulaValue::Pending(message)) => (CellValue::FormulaPending { message }, None),
        Ok(FormulaValue::Empty) => (CellValue::Empty, None),
        Err(error) => (CellValue::Error(error), None),
    }
}

fn is_direct_cell_reference_formula(input: &str) -> bool {
    let Some(expression) = input.trim_start().strip_prefix('=') else {
        return false;
    };

    parse_cell_reference(expression.trim()).is_ok()
}

pub const DEFAULT_COLUMN_WIDTH: u16 = 128;
pub const DEFAULT_ROW_HEIGHT: u16 = 28;

pub fn cell_key(row: usize, col: usize) -> String {
    format!("{}{}", column_name(col), row + 1)
}

pub fn column_name(mut col: usize) -> String {
    let mut name = String::new();
    col += 1;

    while col > 0 {
        let rem = (col - 1) % 26;
        name.insert(0, (b'A' + rem as u8) as char);
        col = (col - 1) / 26;
    }

    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::cache::stable_cache_key;

    #[test]
    fn default_sheet_is_named_default() {
        let document = CashewDocument::default();

        assert_eq!(
            document.active_sheet().map(|sheet| sheet.name.as_str()),
            Some("Default")
        );
    }

    #[test]
    fn document_round_trips_as_json() {
        let mut document = CashewDocument::new("Movie draft");
        document.active_sheet_mut().unwrap().set_cell_input(
            0,
            0,
            "=GENERATEIMAGE(A1,A2)".to_string(),
        );

        let json = serde_json::to_string_pretty(&document).unwrap();
        let parsed: CashewDocument = serde_json::from_str(&json).unwrap();

        assert_eq!(document, parsed);
    }

    #[test]
    fn cache_key_changes_with_inputs() {
        let first = stable_cache_key("=GENERATEIMAGE(A1,A2)", &["prompt".into(), "ref-a".into()]);
        let second = stable_cache_key("=GENERATEIMAGE(A1,A2)", &["prompt".into(), "ref-b".into()]);

        assert_ne!(first, second);
    }

    #[test]
    fn column_names_follow_spreadsheet_conventions() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
    }

    #[test]
    fn formulas_can_use_cell_references() {
        let mut sheet = Sheet::new("Math", 2, 2);
        sheet.set_cell_input(0, 0, "2".to_string());
        sheet.set_cell_input(0, 1, "3".to_string());
        sheet.set_cell_input(1, 0, "=$A1+B1".to_string());

        assert_eq!(
            sheet.cell(1, 0).map(|cell| &cell.value),
            Some(&CellValue::Text("5".to_string()))
        );
    }

    #[test]
    fn recalculates_formulas_after_referenced_cell_changes() {
        let mut sheet = Sheet::new("Math", 2, 4);
        sheet.set_cell_input(1, 2, "2".to_string());
        sheet.set_cell_input(1, 3, "=$C2*3".to_string());
        sheet.set_cell_input(1, 2, "5".to_string());

        assert_eq!(
            sheet.cell(1, 3).map(|cell| &cell.value),
            Some(&CellValue::Text("15".to_string()))
        );
    }

    #[test]
    fn recalculation_preserves_cached_provider_results() {
        let mut sheet = Sheet::new("LLM", 1, 2);
        sheet.set_cell_value_with_cache(
            0,
            0,
            "=LLM(B1)".to_string(),
            CellValue::Cached("cached".to_string()),
            Some("cache-key".to_string()),
        );
        sheet.recalculate_formulas();

        assert_eq!(
            sheet
                .cell(0, 0)
                .map(|cell| (&cell.value, cell.cache_key.as_deref())),
            Some((&CellValue::Cached("cached".to_string()), Some("cache-key")))
        );
    }

    #[test]
    fn direct_reference_formulas_recalculate_for_cached_values() {
        let mut sheet = Sheet::new("Media", 1, 2);
        sheet.set_cell_value_with_cache(
            0,
            0,
            "https://example.com/a.png".to_string(),
            CellValue::Cached("https://example.com/a.png".to_string()),
            Some("cache-a".to_string()),
        );
        sheet.set_cell_input(0, 1, "=$A1".to_string());

        assert_eq!(
            sheet
                .cell(0, 1)
                .map(|cell| (&cell.value, cell.cache_key.as_deref())),
            Some((
                &CellValue::Cached("https://example.com/a.png".to_string()),
                Some("cache-a")
            ))
        );

        sheet.set_cell_value_with_cache(
            0,
            0,
            "https://example.com/b.png".to_string(),
            CellValue::Cached("https://example.com/b.png".to_string()),
            Some("cache-b".to_string()),
        );
        sheet.recalculate_formulas();

        assert_eq!(
            sheet
                .cell(0, 1)
                .map(|cell| (&cell.value, cell.cache_key.as_deref())),
            Some((
                &CellValue::Cached("https://example.com/b.png".to_string()),
                Some("cache-b")
            ))
        );
    }
}

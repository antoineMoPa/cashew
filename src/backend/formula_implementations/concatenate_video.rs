use super::evaluation::{parse_function_call, resolve_text_argument, split_formula_arguments};
use crate::backend::document::Sheet;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

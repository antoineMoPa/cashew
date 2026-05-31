use super::evaluation::{parse_function_call, resolve_text_argument, split_formula_arguments};
use crate::backend::{
    document::Sheet,
    providers::fal_segment::{
        SegmentBoxPrompt, SegmentImageRequest, SegmentOutputFormat, SegmentPointPrompt,
    },
};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

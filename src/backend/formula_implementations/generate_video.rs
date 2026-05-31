use super::evaluation::{parse_function_call, resolve_text_argument, split_formula_arguments};
use crate::backend::{
    document::Sheet,
    providers::fal_video::{
        DEFAULT_MODEL, GenerateVideoRequest, video_model_id_is_supported,
        video_model_requires_end_image, video_model_supports_aspect_ratio,
    },
};

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
    if args.len() < 2 || args.len() > 6 {
        return Err(
            "GENERATEVIDEO expects prompt, start_image, optional end_image, optional model, optional duration, optional aspect_ratio"
                .to_string(),
        );
    }

    let prompt = resolve_text_argument(&args[0], sheet)?;
    let start_image_url = resolve_text_argument(&args[1], sheet)?;

    let trailing_values = args
        .iter()
        .skip(2)
        .map(|arg| resolve_text_argument(arg, sheet))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();

    let model_indices = trailing_values
        .iter()
        .enumerate()
        .filter(|(_, value)| video_model_id_is_supported(value) || value.starts_with("fal-ai/"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if model_indices.len() > 1 {
        return Err("GENERATEVIDEO arguments can include only one model id".to_string());
    }

    let model_index = model_indices.first().copied();
    let model = model_index.map(|index| trailing_values[index].clone());
    let model = model.filter(|value| !value.is_empty());

    if model.as_deref().is_some_and(video_model_requires_end_image) {
        let Some(index) = model_index else {
            unreachable!();
        };
        if index != 1 {
            return Err(
                "GENERATEVIDEO models that use a second image require the end image immediately before the model"
                    .to_string(),
            );
        }

        let end_image_url = trailing_values[0].clone();
        if end_image_url.is_empty() {
            return Err(
                "GENERATEVIDEO models that use a second image require a non-empty end image"
                    .to_string(),
            );
        }

        let mut remaining = trailing_values.iter().skip(2).cloned().collect::<Vec<_>>();
        while remaining.last().is_some_and(|value| value.is_empty()) {
            remaining.pop();
        }
        if remaining.iter().any(|value| value.is_empty()) {
            return Err(
                "GENERATEVIDEO arguments after the end image and model must not skip positions"
                    .to_string(),
            );
        }
        if remaining.len() > 1 {
            return Err(
                "GENERATEVIDEO models that use a second image accept only an optional duration after the model"
                    .to_string(),
            );
        }

        let duration = if let Some(value) = remaining.first() {
            Some(
                value
                    .parse::<u32>()
                    .map_err(|_| "GENERATEVIDEO duration must be numeric".to_string())?,
            )
        } else {
            None
        };

        return GenerateVideoRequest::new(
            prompt,
            start_image_url,
            Some(end_image_url),
            model,
            duration,
            None,
        )
        .map(Some)
        .map_err(|error| error.to_string());
    }

    let remaining = if let Some(index) = model_index {
        let mut values = trailing_values
            .iter()
            .enumerate()
            .filter(|(current_index, _)| *current_index != index)
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        while values.last().is_some_and(|value| value.is_empty()) {
            values.pop();
        }
        values
    } else {
        let mut values = trailing_values.clone();
        while values.last().is_some_and(|value| value.is_empty()) {
            values.pop();
        }
        values
    };
    if remaining.iter().any(|value| value.is_empty()) {
        return Err("GENERATEVIDEO arguments must not skip positions".to_string());
    }

    let supports_aspect_ratio = model
        .as_deref()
        .map(video_model_supports_aspect_ratio)
        .unwrap_or_else(|| video_model_supports_aspect_ratio(DEFAULT_MODEL));

    let mut duration = None;
    let mut aspect_ratio = None;

    if let Some(first) = remaining.first() {
        if let Ok(parsed) = first.parse::<u32>() {
            duration = Some(parsed);
            if let Some(second) = remaining.get(1) {
                if !supports_aspect_ratio {
                    return Err("GENERATEVIDEO model does not accept an aspect ratio".to_string());
                }
                aspect_ratio = Some(second.clone());
                if remaining.len() > 2 {
                    return Err(
                        "GENERATEVIDEO arguments after the model must be duration and aspect_ratio"
                            .to_string(),
                    );
                }
            }
        } else if supports_aspect_ratio {
            aspect_ratio = Some(first.clone());
            if remaining.len() > 1 {
                return Err(
                    "GENERATEVIDEO arguments after the model must be duration and aspect_ratio"
                        .to_string(),
                );
            }
        } else {
            return Err("GENERATEVIDEO model does not accept an aspect ratio".to_string());
        }
    }

    GenerateVideoRequest::new(prompt, start_image_url, None, model, duration, aspect_ratio)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::document::Sheet;

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
        assert_eq!(request.start_image_url, "https://example.com/robot.png");
        assert_eq!(request.model, "fal-ai/veo2/image-to-video");
        assert_eq!(request.duration, 6);
        assert_eq!(request.aspect_ratio.as_deref(), Some("9:16"));
    }

    #[test]
    fn builds_first_last_frame_generate_video_request() {
        let mut sheet = Sheet::new("Video", 2, 3);
        sheet.set_cell_input(0, 0, "A blooming tree".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/start.png".to_string());
        sheet.set_cell_input(0, 2, "https://example.com/end.png".to_string());

        let request = generate_video_request_for_sheet(
            r#"=GENERATEVIDEO($A1, $B1, $C1, "fal-ai/kling-video/o1/standard/image-to-video", 5)"#,
            &sheet,
        )
        .unwrap()
        .unwrap();

        assert_eq!(request.start_image_url, "https://example.com/start.png");
        assert_eq!(
            request.end_image_url.as_deref(),
            Some("https://example.com/end.png")
        );
        assert_eq!(
            request.model,
            "fal-ai/kling-video/o1/standard/image-to-video"
        );
        assert_eq!(request.duration, 5);
        assert!(request.aspect_ratio.is_none());
        assert_eq!(
            request.input["end_image_url"],
            "https://example.com/end.png"
        );
    }

    #[test]
    fn rejects_out_of_order_first_last_frame_generate_video_request() {
        let mut sheet = Sheet::new("Video", 2, 3);
        sheet.set_cell_input(0, 0, "A blooming tree".to_string());
        sheet.set_cell_input(0, 1, "https://example.com/start.png".to_string());
        sheet.set_cell_input(0, 2, "https://example.com/end.png".to_string());

        let error = generate_video_request_for_sheet(
            r#"=GENERATEVIDEO($A1, $B1, "fal-ai/kling-video/o1/standard/image-to-video", $C1, 5)"#,
            &sheet,
        )
        .unwrap_err();

        assert!(error.contains("require the end image immediately before the model"));
    }
}

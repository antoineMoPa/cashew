use super::evaluation::{parse_function_call, resolve_text_argument, split_formula_arguments};
use crate::backend::{
    document::Sheet,
    providers::fal_image::{
        DEFAULT_MODEL as DEFAULT_IMAGE_MODEL, GenerateImageRequest, parse_image_quality,
    },
};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::document::Sheet;

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
}

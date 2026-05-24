#![allow(dead_code)]

use std::{sync::OnceLock, time::Duration};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::settings::UserSettings;

pub const PROVIDER_NAME: &str = "fal.image";
pub const DEFAULT_MODEL: &str = "flux/dev";
pub const OPENAI_GPT_IMAGE_2_MODEL: &str = "openai/gpt-image-2";

const MODEL_CATALOG_JSON: &str = include_str!("fal_image_models.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageQuality {
    Low,
    Medium,
    High,
}

impl ImageQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FalImageClient {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
struct QueueSubmitResponse {
    request_id: String,
    status_url: String,
    response_url: String,
}

#[derive(Debug, Deserialize)]
struct QueueStatusResponse {
    status: String,
    #[serde(default)]
    queue_position: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_queue_logs")]
    logs: Vec<QueueLogEntry>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_type: Option<String>,
    #[serde(default)]
    response_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QueueLogEntry {
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ImageRequestContract {
    FluxDev,
    #[serde(rename = "openai_gpt_image_2")]
    OpenaiGptImage2,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageModelSpec {
    id: String,
    label: String,
    description: String,
    default: bool,
    queue_api_base: String,
    endpoint_text: String,
    endpoint_edit: String,
    request_contract: ImageRequestContract,
    max_input_images: usize,
    default_output_format: String,
    supports_quality: bool,
    default_quality: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageModelDoc {
    pub id: String,
    pub label: String,
    pub description: String,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerateImageRequest {
    pub prompt: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    pub endpoint: String,
    pub queue_api_base: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FalImageResponse {
    pub images: Vec<FalImage>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FalImage {
    pub url: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

fn deserialize_queue_logs<'de, D>(deserializer: D) -> Result<Vec<QueueLogEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<QueueLogEntry>>::deserialize(deserializer).map(|logs| logs.unwrap_or_default())
}

pub fn parse_image_quality(value: &str) -> Option<ImageQuality> {
    match value.trim() {
        "low" => Some(ImageQuality::Low),
        "medium" => Some(ImageQuality::Medium),
        "high" => Some(ImageQuality::High),
        _ => None,
    }
}

fn image_model_catalog() -> &'static [ImageModelSpec] {
    static CATALOG: OnceLock<Vec<ImageModelSpec>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(MODEL_CATALOG_JSON)
                .expect("fal_image_models.json should be valid image model metadata")
        })
        .as_slice()
}

fn default_image_model_spec() -> &'static ImageModelSpec {
    image_model_catalog()
        .iter()
        .find(|model| model.default)
        .unwrap_or_else(|| &image_model_catalog()[0])
}

fn image_model_spec(model: &str) -> Option<&'static ImageModelSpec> {
    image_model_catalog()
        .iter()
        .find(|candidate| candidate.id == model)
}

pub fn image_model_docs() -> Vec<ImageModelDoc> {
    image_model_catalog()
        .iter()
        .map(|model| ImageModelDoc {
            id: model.id.clone(),
            label: model.label.clone(),
            description: model.description.clone(),
            default: model.default,
        })
        .collect()
}

pub fn image_model_id_is_supported(model: &str) -> bool {
    image_model_spec(model.trim()).is_some()
}

impl FalImageClient {
    pub fn from_settings_or_env() -> anyhow::Result<Self> {
        let settings = UserSettings::load_default()?;
        let api_key = settings
            .fal_key_or_env()
            .ok_or_else(|| anyhow::anyhow!("FAL key is not configured"))?;
        Ok(Self::new(api_key))
    }

    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_key: api_key.into(),
        }
    }

    pub async fn run(&self, request: &GenerateImageRequest) -> anyhow::Result<FalImageResponse> {
        let submit_url = format!(
            "{}/{}",
            request.queue_api_base.trim_end_matches('/'),
            request.endpoint
        );
        let submitted = self
            .http
            .post(&submit_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&request.input)
            .send()
            .await?;
        let submitted: QueueSubmitResponse =
            decode_json_response(submitted, "submit fal image queue request").await?;

        let response: FalImageResponse = loop {
            let status = self
                .http
                .get(&submitted.status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;
            let status: QueueStatusResponse =
                decode_json_response(status, "read fal image queue status").await?;

            match status.status.as_str() {
                "COMPLETED" => {
                    let response_url = status
                        .response_url
                        .as_deref()
                        .unwrap_or(&submitted.response_url);
                    let response = self
                        .http
                        .get(response_url)
                        .header("Authorization", format!("Key {}", self.api_key))
                        .send()
                        .await?;
                    break decode_json_response(response, "fetch fal image queue response").await?;
                }
                "IN_QUEUE" | "IN_PROGRESS" => {
                    let _ = status.queue_position;
                    let _ = status
                        .logs
                        .iter()
                        .map(|log| log.message.len())
                        .sum::<usize>();
                    std::thread::sleep(Duration::from_secs(1));
                }
                other => {
                    let message = status
                        .error
                        .or(status.error_type)
                        .unwrap_or_else(|| format!("fal queue returned unexpected status {other}"));
                    anyhow::bail!(message);
                }
            }
        };

        if response.images.is_empty() {
            anyhow::bail!("fal image response did not include any images");
        }

        Ok(response)
    }
}

async fn decode_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    context: &str,
) -> anyhow::Result<T> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!(
            "{context} failed with HTTP {status}: {}",
            body_excerpt(&body)
        );
    }

    serde_json::from_str(&body).map_err(|error| {
        anyhow::anyhow!(
            "{context} returned an unexpected response: {error}; body: {}",
            body_excerpt(&body)
        )
    })
}

fn body_excerpt(body: &str) -> String {
    const MAX_CHARS: usize = 800;
    let excerpt = body.chars().take(MAX_CHARS).collect::<String>();
    if body.chars().count() > MAX_CHARS {
        format!("{excerpt}...")
    } else {
        excerpt
    }
}

impl GenerateImageRequest {
    pub fn new(
        prompt: impl Into<String>,
        model: impl Into<String>,
        quality: Option<ImageQuality>,
        image_urls: Vec<String>,
    ) -> anyhow::Result<Self> {
        let prompt = prompt.into();
        let model = model.into();
        let spec = select_image_model_spec(&model)?;
        let endpoint = select_image_endpoint(spec, image_urls.is_empty());
        let quality = select_image_quality(spec, quality)?;
        let input = build_image_request_input(spec, &prompt, quality, image_urls)?;

        Ok(Self {
            prompt,
            model: spec.id.clone(),
            quality: quality.map(|value| value.as_str().to_string()),
            endpoint,
            queue_api_base: spec.queue_api_base.clone(),
            input,
        })
    }
}

fn select_image_model_spec(model: &str) -> anyhow::Result<&'static ImageModelSpec> {
    let model = if model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        model.trim()
    };

    image_model_spec(model).ok_or_else(|| {
        anyhow::anyhow!(
            "Unsupported GENERATEIMAGE model {model}. Use one of: {}",
            image_model_catalog()
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

fn select_image_endpoint(model: &ImageModelSpec, is_text_only: bool) -> String {
    if is_text_only {
        model.endpoint_text.clone()
    } else {
        model.endpoint_edit.clone()
    }
}

fn select_image_quality(
    model: &ImageModelSpec,
    quality: Option<ImageQuality>,
) -> anyhow::Result<Option<ImageQuality>> {
    if !model.supports_quality {
        return Ok(None);
    }

    if let Some(quality) = quality {
        return Ok(Some(quality));
    }

    let Some(default_quality) = model.default_quality.as_deref() else {
        return Ok(None);
    };

    parse_image_quality(default_quality)
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("Invalid default image quality in model catalog"))
}

fn build_image_request_input(
    model: &ImageModelSpec,
    prompt: &str,
    quality: Option<ImageQuality>,
    image_urls: Vec<String>,
) -> anyhow::Result<Value> {
    if image_urls.len() > model.max_input_images {
        anyhow::bail!(
            "{} supports up to {} input image{}",
            model.id,
            model.max_input_images,
            if model.max_input_images == 1 { "" } else { "s" }
        );
    }

    match (&model.request_contract, image_urls.as_slice()) {
        (ImageRequestContract::FluxDev, []) => Ok(json!({
            "prompt": prompt,
            "num_images": 1,
            "output_format": model.default_output_format,
            "acceleration": "regular"
        })),
        (ImageRequestContract::FluxDev, [image_url]) => Ok(json!({
            "prompt": prompt,
            "image_url": image_url,
            "strength": 0.95,
            "num_images": 1,
            "output_format": model.default_output_format,
            "acceleration": "regular"
        })),
        (ImageRequestContract::FluxDev, _) => {
            anyhow::bail!("{} supports exactly one input image in edit mode", model.id)
        }
        (ImageRequestContract::OpenaiGptImage2, []) => Ok(json!({
            "prompt": prompt,
            "num_images": 1,
            "quality": quality.map(ImageQuality::as_str),
            "output_format": model.default_output_format
        })),
        (ImageRequestContract::OpenaiGptImage2, _) => Ok(json!({
            "prompt": prompt,
            "image_urls": image_urls,
            "num_images": 1,
            "quality": quality.map(ImageQuality::as_str),
            "output_format": model.default_output_format
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_flux_without_images_to_text_to_image() {
        let request = GenerateImageRequest::new("make it", "flux/dev", None, Vec::new()).unwrap();

        assert_eq!(request.endpoint, "fal-ai/flux/dev");
        assert_eq!(request.queue_api_base, "https://queue.fal.run");
        assert_eq!(request.input["prompt"], "make it");
        assert!(request.input.get("image_url").is_none());
    }

    #[test]
    fn routes_openai_with_images_to_edit() {
        let request = GenerateImageRequest::new(
            "edit it",
            "openai/gpt-image-2",
            None,
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(request.endpoint, "openai/gpt-image-2/edit");
        assert_eq!(request.quality.as_deref(), Some("medium"));
        assert_eq!(request.input["image_urls"][0], "https://example.com/a.png");
        assert_eq!(request.input["image_urls"][1], "https://example.com/b.png");
    }

    #[test]
    fn routes_flux_with_images_to_image_to_image() {
        let request = GenerateImageRequest::new(
            "add a car",
            "flux/dev",
            None,
            vec!["https://example.com/ref.png".to_string()],
        )
        .unwrap();

        assert_eq!(request.model, "flux/dev");
        assert_eq!(request.endpoint, "fal-ai/flux/dev/image-to-image");
        assert_eq!(request.input["image_url"], "https://example.com/ref.png");
        assert!(request.input.get("image_urls").is_none());
    }

    #[test]
    fn rejects_unsupported_models() {
        let error = GenerateImageRequest::new(
            "edit it",
            "unsupported",
            None,
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string(),
            ],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unsupported GENERATEIMAGE model")
        );
    }
}

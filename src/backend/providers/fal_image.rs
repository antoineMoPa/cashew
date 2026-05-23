#![allow(dead_code)]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::backend::settings::UserSettings;

pub const PROVIDER_NAME: &str = "fal.image";
pub const DEFAULT_MODEL: &str = "flux/dev";
pub const OPENAI_GPT_IMAGE_2_MODEL: &str = "openai/gpt-image-2";
pub const DEFAULT_OPENAI_IMAGE_QUALITY: ImageQuality = ImageQuality::Medium;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FalImageModel {
    FluxDev,
    OpenAiGptImage2,
}

impl FalImageModel {
    fn as_str(self) -> &'static str {
        match self {
            Self::FluxDev => "flux/dev",
            Self::OpenAiGptImage2 => OPENAI_GPT_IMAGE_2_MODEL,
        }
    }
}

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

pub fn parse_image_quality(value: &str) -> Option<ImageQuality> {
    match value.trim() {
        "low" => Some(ImageQuality::Low),
        "medium" => Some(ImageQuality::Medium),
        "high" => Some(ImageQuality::High),
        _ => None,
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

fn deserialize_queue_logs<'de, D>(deserializer: D) -> Result<Vec<QueueLogEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<QueueLogEntry>>::deserialize(deserializer).map(|logs| logs.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerateImageRequest {
    pub prompt: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    pub endpoint: String,
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
        eprintln!(
            "[fal.image] sending request endpoint={} model={} prompt_len={} input={}",
            request.endpoint,
            request.model,
            request.prompt.len(),
            serde_json::to_string(&request.input)
                .unwrap_or_else(|_| "<unserializable>".to_string())
        );

        let submit_url = format!("https://queue.fal.run/{}", request.endpoint);
        let submitted = self
            .http
            .post(&submit_url)
            .header("Authorization", format!("Key {}", self.api_key))
            .json(&request.input)
            .send()
            .await?;
        let submitted: QueueSubmitResponse =
            decode_json_response(submitted, "submit fal image queue request").await?;

        eprintln!(
            "[fal.image] queued request request_id={} status_url={} response_url={}",
            submitted.request_id, submitted.status_url, submitted.response_url
        );

        let response: FalImageResponse = loop {
            let status = self
                .http
                .get(&submitted.status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;
            let status: QueueStatusResponse =
                decode_json_response(status, "read fal image queue status").await?;

            eprintln!(
                "[fal.image] queue status request_id={} status={} queue_position={:?} logs={} error={:?} error_type={:?}",
                submitted.request_id,
                status.status,
                status.queue_position,
                status.logs.len(),
                status.error,
                status.error_type
            );

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
                    std::thread::sleep(Duration::from_secs(1));
                }
                other => {
                    let message = status
                        .error
                        .unwrap_or_else(|| format!("fal queue returned unexpected status {other}"));
                    anyhow::bail!(message);
                }
            }
        };

        eprintln!(
            "[fal.image] received response endpoint={} images={} prompt={:?} seed={:?}",
            request.endpoint,
            response.images.len(),
            response.prompt,
            response.seed
        );

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
        anyhow::bail!("{context} failed with HTTP {status}: {}", body_excerpt(&body));
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
        let model = parse_model(&model.into())?;
        let endpoint = endpoint_for_model(&model, !image_urls.is_empty())?;
        let quality = match (model, quality) {
            (FalImageModel::OpenAiGptImage2, None) => Some(DEFAULT_OPENAI_IMAGE_QUALITY),
            (_, quality) => quality,
        };
        let input = input_for_request(&model, &prompt, quality, image_urls)?;

        Ok(Self {
            prompt,
            model: model.as_str().to_string(),
            quality: quality.map(|quality| quality.as_str().to_string()),
            endpoint,
            input,
        })
    }
}

fn parse_model(model: &str) -> anyhow::Result<FalImageModel> {
    match model.trim() {
        "" | DEFAULT_MODEL => Ok(FalImageModel::FluxDev),
        OPENAI_GPT_IMAGE_2_MODEL => Ok(FalImageModel::OpenAiGptImage2),
        other => anyhow::bail!(
            "Unsupported GENERATEIMAGE model {other}. Use flux/dev or openai/gpt-image-2"
        ),
    }
}

fn endpoint_for_model(model: &FalImageModel, has_images: bool) -> anyhow::Result<String> {
    match (model, has_images) {
        (FalImageModel::FluxDev, false) => Ok("fal-ai/flux/dev".to_string()),
        (FalImageModel::FluxDev, true) => Ok("fal-ai/flux/dev/image-to-image".to_string()),
        (FalImageModel::OpenAiGptImage2, false) => Ok("openai/gpt-image-2".to_string()),
        (FalImageModel::OpenAiGptImage2, true) => Ok("openai/gpt-image-2/edit".to_string()),
    }
}

fn input_for_request(
    model: &FalImageModel,
    prompt: &str,
    quality: Option<ImageQuality>,
    image_urls: Vec<String>,
) -> anyhow::Result<Value> {
    match (model, image_urls.as_slice()) {
        (FalImageModel::FluxDev, []) => Ok(json!({
            "prompt": prompt,
            "num_images": 1,
            "output_format": "jpeg",
            "acceleration": "regular"
        })),
        (FalImageModel::FluxDev, [image_url]) => Ok(json!({
            "prompt": prompt,
            "image_url": image_url,
            "strength": 0.95,
            "num_images": 1,
            "output_format": "jpeg",
            "acceleration": "regular"
        })),
        (FalImageModel::FluxDev, _) => {
            anyhow::bail!("flux/dev image editing supports one input image")
        }
        (FalImageModel::OpenAiGptImage2, []) => Ok(json!({
            "prompt": prompt,
            "num_images": 1,
            "quality": quality.unwrap_or(DEFAULT_OPENAI_IMAGE_QUALITY).as_str(),
            "output_format": "png"
        })),
        (FalImageModel::OpenAiGptImage2, _) => Ok(json!({
            "prompt": prompt,
            "image_urls": image_urls,
            "num_images": 1,
            "quality": quality.unwrap_or(DEFAULT_OPENAI_IMAGE_QUALITY).as_str(),
            "output_format": "png"
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

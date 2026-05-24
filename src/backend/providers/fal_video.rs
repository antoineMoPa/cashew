#![allow(dead_code)]

use std::{sync::OnceLock, time::Duration};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::settings::UserSettings;

pub const PROVIDER_NAME: &str = "fal.video";
pub const DEFAULT_MODEL: &str = "fal-ai/veo3.1/reference-to-video";

const MODEL_CATALOG_JSON: &str = include_str!("fal_video_models.json");

#[derive(Debug, Clone)]
pub struct FalVideoClient {
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
enum InputImageField {
    ImageUrl,
    ImageUrls,
    StartImageUrl,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DurationFormat {
    Plain,
    SuffixS,
}

#[derive(Debug, Clone, Deserialize)]
struct VideoModelSpec {
    id: String,
    label: String,
    description: String,
    default: bool,
    queue_api_base: String,
    input_image_field: InputImageField,
    duration_format: DurationFormat,
    min_duration: u32,
    max_duration: u32,
    default_duration: u32,
    supports_aspect_ratio: bool,
    default_aspect_ratio: Option<String>,
    #[serde(default)]
    allowed_aspect_ratios: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VideoModelDoc {
    pub id: String,
    pub label: String,
    pub description: String,
    pub default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerateVideoRequest {
    pub prompt: String,
    pub image_url: String,
    pub model: String,
    pub duration: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    pub endpoint: String,
    pub queue_api_base: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FalVideoResponse {
    pub video: FalVideoFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FalVideoFile {
    pub url: String,
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

fn video_model_catalog() -> &'static [VideoModelSpec] {
    static CATALOG: OnceLock<Vec<VideoModelSpec>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str(MODEL_CATALOG_JSON)
                .expect("fal_video_models.json should be valid video model metadata")
        })
        .as_slice()
}

fn default_model_spec() -> &'static VideoModelSpec {
    video_model_catalog()
        .iter()
        .find(|model| model.default)
        .unwrap_or_else(|| &video_model_catalog()[0])
}

fn video_model_spec(model: &str) -> Option<&'static VideoModelSpec> {
    video_model_catalog()
        .iter()
        .find(|candidate| candidate.id == model)
}

pub fn video_model_id_is_supported(model: &str) -> bool {
    video_model_spec(model.trim()).is_some()
}

pub fn video_model_docs() -> Vec<VideoModelDoc> {
    video_model_catalog()
        .iter()
        .map(|model| VideoModelDoc {
            id: model.id.clone(),
            label: model.label.clone(),
            description: model.description.clone(),
            default: model.default,
        })
        .collect()
}

pub fn default_video_duration() -> u32 {
    default_model_spec().default_duration
}

pub fn default_video_aspect_ratio() -> Option<&'static str> {
    default_model_spec().default_aspect_ratio.as_deref()
}

pub fn supported_video_aspect_ratios() -> Vec<String> {
    default_model_spec().allowed_aspect_ratios.clone()
}

impl FalVideoClient {
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
                .timeout(Duration::from_secs(900))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api_key: api_key.into(),
        }
    }

    pub async fn run(&self, request: &GenerateVideoRequest) -> anyhow::Result<FalVideoResponse> {
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
            decode_json_response(submitted, "submit fal video queue request").await?;

        loop {
            let status = self
                .http
                .get(&submitted.status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;
            let status: QueueStatusResponse =
                decode_json_response(status, "read fal video queue status").await?;

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
                    return decode_json_response(response, "fetch fal video queue response").await;
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
        }
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

impl GenerateVideoRequest {
    pub fn new(
        prompt: impl Into<String>,
        image_url: impl Into<String>,
        model: Option<String>,
        duration: Option<u32>,
        aspect_ratio: Option<String>,
    ) -> anyhow::Result<Self> {
        let prompt = prompt.into();
        let image_url = image_url.into().trim().to_string();
        if image_url.is_empty() {
            anyhow::bail!("GENERATEVIDEO requires a source image URL or data URI");
        }

        let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let spec = video_model_spec(&model).ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported GENERATEVIDEO model {model}. Use one of: {}",
                video_model_catalog()
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

        let duration = duration.unwrap_or(spec.default_duration);
        if duration < spec.min_duration || duration > spec.max_duration {
            anyhow::bail!(
                "Unsupported GENERATEVIDEO duration {duration} for model {}. Use {} through {}",
                spec.id,
                spec.min_duration,
                spec.max_duration
            );
        }

        let aspect_ratio = if spec.supports_aspect_ratio {
            let aspect_ratio = aspect_ratio
                .or_else(|| spec.default_aspect_ratio.clone())
                .ok_or_else(|| anyhow::anyhow!("GENERATEVIDEO aspect ratio is required"))?;
            if !spec
                .allowed_aspect_ratios
                .iter()
                .any(|value| value == &aspect_ratio)
            {
                anyhow::bail!(
                    "Unsupported GENERATEVIDEO aspect ratio {aspect_ratio} for model {}. Use {}",
                    spec.id,
                    spec.allowed_aspect_ratios.join(", ")
                );
            }
            Some(aspect_ratio)
        } else {
            None
        };

        let duration_value = match spec.duration_format {
            DurationFormat::Plain => serde_json::Value::String(duration.to_string()),
            DurationFormat::SuffixS => serde_json::Value::String(format!("{duration}s")),
        };

        let mut input = json!({
            "prompt": prompt,
            "duration": duration_value,
        });

        if let Some(aspect_ratio) = &aspect_ratio {
            input["aspect_ratio"] = serde_json::Value::String(aspect_ratio.clone());
        }

        match spec.input_image_field {
            InputImageField::ImageUrl => {
                input["image_url"] = serde_json::Value::String(image_url.clone());
            }
            InputImageField::ImageUrls => {
                input["image_urls"] =
                    serde_json::Value::Array(vec![serde_json::Value::String(image_url.clone())]);
            }
            InputImageField::StartImageUrl => {
                input["start_image_url"] = serde_json::Value::String(image_url.clone());
            }
        }

        Ok(Self {
            prompt,
            image_url,
            model: spec.id.clone(),
            duration,
            aspect_ratio,
            endpoint: spec.id.clone(),
            queue_api_base: spec.queue_api_base.clone(),
            input,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_default_veo31_request() {
        let request = GenerateVideoRequest::new(
            "Camera slowly pushes in",
            "https://example.com/input.png",
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(request.endpoint, "fal-ai/veo3.1/reference-to-video");
        assert_eq!(request.queue_api_base, "https://queue.fal.run");
        assert_eq!(request.duration, 8);
        assert_eq!(request.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(
            request.input["image_urls"][0],
            "https://example.com/input.png"
        );
        assert_eq!(request.input["duration"], "8s");
    }

    #[test]
    fn builds_kling_request_without_aspect_ratio() {
        let request = GenerateVideoRequest::new(
            "Animate it",
            "https://example.com/input.png",
            Some("fal-ai/kling-video/v3/standard/image-to-video".to_string()),
            Some(10),
            Some("16:9".to_string()),
        )
        .unwrap();

        assert_eq!(
            request.endpoint,
            "fal-ai/kling-video/v3/standard/image-to-video"
        );
        assert_eq!(
            request.input["start_image_url"],
            "https://example.com/input.png"
        );
        assert!(request.input.get("aspect_ratio").is_none());
        assert_eq!(request.aspect_ratio, None);
    }

    #[test]
    fn rejects_invalid_duration_for_model() {
        let error = GenerateVideoRequest::new(
            "Animate it",
            "https://example.com/input.png",
            Some("fal-ai/veo3.1/reference-to-video".to_string()),
            Some(12),
            None,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unsupported GENERATEVIDEO duration")
        );
    }
}

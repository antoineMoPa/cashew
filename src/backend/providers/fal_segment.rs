#![allow(dead_code)]

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::providers::fal_image::FalImage;
use crate::backend::settings::UserSettings;

pub const PROVIDER_NAME: &str = "fal.segment";
pub const DEFAULT_MODEL: &str = "fal-ai/sam-3/image";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentOutputFormat {
    Jpeg,
    Png,
    Webp,
}

impl SegmentOutputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum SegmentPointLabel {
    Background = 0,
    #[default]
    Foreground = 1,
}

impl From<SegmentPointLabel> for u8 {
    fn from(value: SegmentPointLabel) -> Self {
        match value {
            SegmentPointLabel::Background => 0,
            SegmentPointLabel::Foreground => 1,
        }
    }
}

impl TryFrom<u8> for SegmentPointLabel {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Background),
            1 => Ok(Self::Foreground),
            other => Err(format!("Invalid point label {other}; expected 0 or 1")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentPointPrompt {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub label: SegmentPointLabel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentBoxPrompt {
    pub x_min: i32,
    pub y_min: i32,
    pub x_max: i32,
    pub y_max: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_index: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct QueueSubmitResponse {
    request_id: String,
    status_url: String,
    response_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct QueueStatusResponse {
    status: String,
    #[serde(default)]
    queue_position: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_type: Option<String>,
    #[serde(default)]
    response_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentMaskMetadata {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(rename = "box", default, skip_serializing_if = "Option::is_none")]
    pub box_: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SegmentImageRequest {
    pub image_url: String,
    pub prompt: String,
    pub point_prompts: Vec<SegmentPointPrompt>,
    pub box_prompts: Vec<SegmentBoxPrompt>,
    pub apply_mask: bool,
    pub output_format: String,
    pub return_multiple_masks: bool,
    pub max_masks: u32,
    pub include_scores: bool,
    pub include_boxes: bool,
    pub endpoint: String,
    pub queue_api_base: String,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FalSegmentResponse {
    #[serde(default)]
    pub image: Option<FalImage>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub masks: Vec<FalImage>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub metadata: Vec<SegmentMaskMetadata>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub scores: Vec<f64>,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    pub boxes: Vec<Vec<f64>>,
}

#[derive(Debug, Clone)]
pub struct FalSegmentClient {
    http: reqwest::Client,
    api_key: String,
}

impl FalSegmentClient {
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

    pub async fn run(&self, request: &SegmentImageRequest) -> anyhow::Result<FalSegmentResponse> {
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
            decode_json_response(submitted, "submit fal segment queue request").await?;

        let response: FalSegmentResponse = loop {
            let status = self
                .http
                .get(&submitted.status_url)
                .header("Authorization", format!("Key {}", self.api_key))
                .send()
                .await?;
            let status: QueueStatusResponse =
                decode_json_response(status, "read fal segment queue status").await?;

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
                    break decode_json_response(response, "fetch fal segment queue response")
                        .await?;
                }
                "IN_QUEUE" | "IN_PROGRESS" => {
                    let _ = status.queue_position;
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

        if response
            .image
            .as_ref()
            .map(|image| image.url.is_empty())
            .unwrap_or(true)
            && response.masks.is_empty()
        {
            anyhow::bail!("fal segment response did not include any mask images");
        }

        Ok(response)
    }
}

impl SegmentImageRequest {
    pub fn new(
        image_url: impl Into<String>,
        prompt: impl Into<String>,
        point_prompts: Vec<SegmentPointPrompt>,
        box_prompts: Vec<SegmentBoxPrompt>,
        apply_mask: Option<bool>,
        output_format: Option<SegmentOutputFormat>,
        return_multiple_masks: Option<bool>,
        max_masks: Option<u32>,
        include_scores: Option<bool>,
        include_boxes: Option<bool>,
    ) -> anyhow::Result<Self> {
        let image_url = image_url.into();
        if image_url.trim().is_empty() {
            anyhow::bail!("SEGMENT expects a non-empty image URL");
        }

        let prompt = prompt.into();
        let prompt = if prompt.trim().is_empty() {
            "wheel".to_string()
        } else {
            prompt
        };
        let output_format = output_format.unwrap_or(SegmentOutputFormat::Png);
        let max_masks = max_masks.unwrap_or(3);
        if max_masks == 0 {
            anyhow::bail!("SEGMENT max_masks must be greater than zero");
        }

        let input = json!({
            "image_url": image_url.clone(),
            "prompt": prompt.clone(),
            "point_prompts": point_prompts.clone(),
            "box_prompts": box_prompts.clone(),
            "apply_mask": apply_mask.unwrap_or(true),
            "output_format": output_format.as_str(),
            "return_multiple_masks": return_multiple_masks.unwrap_or(false),
            "max_masks": max_masks,
            "include_scores": include_scores.unwrap_or(false),
            "include_boxes": include_boxes.unwrap_or(false),
        });

        Ok(Self {
            image_url,
            prompt,
            point_prompts,
            box_prompts,
            apply_mask: apply_mask.unwrap_or(true),
            output_format: output_format.as_str().to_string(),
            return_multiple_masks: return_multiple_masks.unwrap_or(false),
            max_masks,
            include_scores: include_scores.unwrap_or(false),
            include_boxes: include_boxes.unwrap_or(false),
            endpoint: DEFAULT_MODEL.to_string(),
            queue_api_base: "https://queue.fal.run".to_string(),
            input,
        })
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

fn deserialize_null_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: DeserializeOwned,
{
    Option::<Vec<T>>::deserialize(deserializer).map(|value| value.unwrap_or_default())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_default_segment_request() {
        let request = SegmentImageRequest::new(
            "https://example.com/image.png",
            "",
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(request.endpoint, DEFAULT_MODEL);
        assert_eq!(request.queue_api_base, "https://queue.fal.run");
        assert_eq!(request.output_format, "png");
        assert_eq!(request.input["prompt"], "wheel");
        assert_eq!(request.input["apply_mask"], true);
        assert_eq!(request.input["return_multiple_masks"], false);
    }

    #[test]
    fn builds_segment_request_with_prompts() {
        let request = SegmentImageRequest::new(
            "https://example.com/image.png",
            "wheel",
            vec![SegmentPointPrompt {
                x: 10,
                y: 20,
                label: SegmentPointLabel::Foreground,
                object_id: Some(3),
                frame_index: None,
            }],
            vec![SegmentBoxPrompt {
                x_min: 1,
                y_min: 2,
                x_max: 30,
                y_max: 40,
                object_id: None,
                frame_index: Some(4),
            }],
            Some(false),
            Some(SegmentOutputFormat::Webp),
            Some(true),
            Some(5),
            Some(true),
            Some(true),
        )
        .unwrap();

        assert_eq!(request.input["output_format"], "webp");
        assert_eq!(request.input["max_masks"], 5);
        assert_eq!(request.input["point_prompts"][0]["label"], 1);
        assert_eq!(request.input["box_prompts"][0]["frame_index"], 4);
    }
}

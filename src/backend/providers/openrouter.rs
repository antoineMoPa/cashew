#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::backend::settings::UserSettings;

pub const ENDPOINT: &str = "https://fal.run/openrouter/router";
pub const VISION_ENDPOINT: &str = "https://fal.run/openrouter/router/vision";
pub const MODEL_ID: &str = "openrouter/router";
pub const DEFAULT_MODEL: &str = "google/gemini-2.5-flash";
pub const PROVIDER_NAME: &str = "fal.openrouter";

#[derive(Debug, Clone)]
pub struct OpenRouterClient {
    http: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenRouterRequest {
    pub prompt: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenRouterResponse {
    pub output: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub partial: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageInfo {
    #[serde(default)]
    pub prompt_tokens: Option<u32>,
    #[serde(default)]
    pub completion_tokens: Option<u32>,
    #[serde(default)]
    pub total_tokens: Option<u32>,
    #[serde(default)]
    pub cost: Option<f64>,
}

impl OpenRouterClient {
    pub fn from_settings_or_env() -> anyhow::Result<Self> {
        let settings = UserSettings::load_default()?;
        let api_key = settings
            .fal_key_or_env()
            .ok_or_else(|| anyhow::anyhow!("FAL key is not configured"))?;
        Ok(Self::new(api_key))
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let api_key =
            std::env::var("FAL_KEY").map_err(|_| anyhow::anyhow!("FAL_KEY is not set"))?;
        Ok(Self::new(api_key))
    }

    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }

    pub async fn run(&self, request: &OpenRouterRequest) -> anyhow::Result<OpenRouterResponse> {
        let response = self
            .http
            .post(request.endpoint())
            .header("Authorization", format!("Key {}", self.api_key))
            .json(request)
            .send()
            .await?
            .error_for_status()?
            .json::<OpenRouterResponse>()
            .await?;

        if let Some(error) = response.error.as_deref() {
            anyhow::bail!(error.to_string());
        }

        Ok(response)
    }
}

impl OpenRouterRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: DEFAULT_MODEL.to_string(),
            image_urls: Vec::new(),
            system_prompt: None,
            reasoning: None,
            temperature: Some(1.0),
            max_tokens: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_image_urls(mut self, image_urls: Vec<String>) -> Self {
        self.image_urls = image_urls;
        self
    }

    pub fn with_system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    pub fn endpoint(&self) -> &'static str {
        if self.image_urls.is_empty() {
            ENDPOINT
        } else {
            VISION_ENDPOINT
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_required_fields() {
        let request = OpenRouterRequest::new("hello").with_model("openai/gpt-4.1");
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["prompt"], "hello");
        assert_eq!(json["model"], "openai/gpt-4.1");
        assert_eq!(json["temperature"], 1.0);
        assert!(json.get("image_urls").is_none());
        assert!(json.get("system_prompt").is_none());
    }

    #[test]
    fn request_serializes_vision_inputs() {
        let request = OpenRouterRequest::new("hello")
            .with_model("google/gemini-2.5-flash")
            .with_image_urls(vec!["https://example.com/image.png".to_string()]);
        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(request.endpoint(), VISION_ENDPOINT);
        assert_eq!(json["image_urls"][0], "https://example.com/image.png");
    }

    #[test]
    fn response_deserializes_usage() {
        let response: OpenRouterResponse = serde_json::from_str(
            r#"{
                "output": "done",
                "usage": {
                    "prompt_tokens": 4,
                    "completion_tokens": 6,
                    "total_tokens": 10,
                    "cost": 0.001
                }
            }"#,
        )
        .unwrap();

        assert_eq!(response.output, "done");
        assert_eq!(response.usage.unwrap().total_tokens, Some(10));
    }
}

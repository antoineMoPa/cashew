use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheEntry {
    pub key: String,
    pub status: CacheStatus,
    pub value: CachedValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheStatus {
    Ready,
    Pending,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum CachedValue {
    Text(String),
    Json(serde_json::Value),
    MediaAsset(MediaAsset),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaAsset {
    pub provider: String,
    pub media_type: MediaType,
    pub uri: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaType {
    Image,
    Video,
    Audio,
    Other(String),
}

pub fn stable_cache_key(formula: &str, resolved_inputs: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(formula.trim().as_bytes());
    hasher.update([0]);

    for input in resolved_inputs {
        hasher.update(input.as_bytes());
        hasher.update([0]);
    }

    format!("{:x}", hasher.finalize())
}

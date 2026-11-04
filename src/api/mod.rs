pub mod bootstrap;
pub mod cache;
pub mod client;
pub mod creds;
pub mod download;
pub mod models;
pub mod task;

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Envelope<T> {
    pub success: bool,
    #[serde(rename = "responseCode", default)]
    pub response_code: Option<i64>,
    #[serde(rename = "responseMessage", default)]
    pub response_message: Option<String>,
    #[serde(default)]
    pub data: Option<T>,
}

impl<T> Envelope<T> {
    #[allow(dead_code)]
    pub fn message(&self) -> &str {
        self.response_message.as_deref().unwrap_or("unknown error")
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    #[error("{message}{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Api { code: Option<i64>, message: String },
    #[error("{0}")]
    Network(String),
}

pub const CODE_INVALID_SESSION: i64 = 61;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum StreamTime {
    Seconds(f64),
    Unlimited(String),
}

pub mod response_codes {
    #[allow(dead_code)]
    pub const STREAMING_FAMILY: [i64; 4] = [46, 227, 283, 316];
}

pub type ApiResult<T> = Result<T, ApiError>;

#[allow(dead_code)]
pub fn envelope_to_error<T>(env: &Envelope<T>, http_status: u16) -> ApiError {
    if !env.success {
        return ApiError::Api {
            code: env.response_code,
            message: env.message().to_string(),
        };
    }
    ApiError::Api {
        code: Some(http_status as i64),
        message: format!("unexpected envelope shape (HTTP {http_status})"),
    }
}

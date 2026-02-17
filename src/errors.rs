//! Provider error types.

use thiserror::Error;

/// Errors that can occur during provider operations
#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Transaction rejected: {0}")]
    Rejection(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::HttpError(e.to_string())
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(e: serde_json::Error) -> Self {
        ProviderError::SerializationError(e.to_string())
    }
}

/// Errors that can occur during event source operations
#[derive(Error, Debug)]
pub enum EventSourceError {
    #[error("HTTP error: {0}")]
    Http(String),
}

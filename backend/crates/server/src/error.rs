//! Errors as the API reports them: a status code and `{"error": "..."}`.

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
    pub fn unknown_job() -> Self {
        Self::not_found("Unknown job")
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status).json(serde_json::json!({ "error": self.message }))
    }
}

impl From<schemgen_core::Error> for ApiError {
    fn from(e: schemgen_core::Error) -> Self {
        let status = if e.is_user_error() {
            StatusCode::BAD_REQUEST
        } else if matches!(e, schemgen_core::Error::Cancelled) {
            StatusCode::CONFLICT
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        Self::new(status, e.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        Self::internal(e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

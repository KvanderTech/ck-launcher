use regex::Regex;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
    pub recoverable: bool,
}

impl LauncherError {
    pub fn internal(details: impl AsRef<str>) -> Self {
        Self {
            code: "internal_error".to_owned(),
            message: "An unexpected launcher error occurred.".to_owned(),
            details: Some(sanitize(details.as_ref())),
            recoverable: false,
        }
    }

    pub fn invalid_path() -> Self {
        Self {
            code: "invalid_path".to_owned(),
            message: "The requested path is outside the launcher data directory.".to_owned(),
            details: None,
            recoverable: false,
        }
    }

    pub fn storage_unavailable() -> Self {
        Self {
            code: "storage_unavailable".to_owned(),
            message: "Launcher storage is unavailable.".to_owned(),
            details: None,
            recoverable: true,
        }
    }
}

impl fmt::Display for LauncherError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LauncherError {}

fn sanitize(details: &str) -> String {
    let bearer = Regex::new(r"(?i)\bBearer\s+[^\s,;]+")
        .expect("Bearer redaction pattern is valid")
        .replace_all(details, "Bearer [REDACTED]");
    let named_token = Regex::new(
        r#"(?i)\b(access_token|refresh_token)(\s*[:=]\s*)(?:\"[^\"]*\"|'[^']*'|[^\s,;&]+)"#,
    )
    .expect("named token redaction pattern is valid");

    named_token
        .replace_all(&bearer, "$1$2[REDACTED]")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::LauncherError;

    #[test]
    fn internal_error_redacts_bearer_tokens_before_serialization() {
        let error = LauncherError::internal("request failed: Bearer abc.def.ghi");
        let serialized = serde_json::to_string(&error).expect("error serializes");

        assert!(!serialized.contains("abc.def.ghi"));
        assert!(serialized.contains("[REDACTED]"));
    }

    #[test]
    fn internal_error_redacts_named_tokens_without_changing_ordinary_details() {
        let error = LauncherError::internal(
            "database unavailable; access_token=access-secret; refresh_token: refresh-secret",
        );

        let details = error.details.expect("sanitized details are retained");
        assert_eq!(
            details,
            "database unavailable; access_token=[REDACTED]; refresh_token: [REDACTED]"
        );
    }
}

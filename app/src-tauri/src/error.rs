use regex::Regex;
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherError {
    code: String,
    message: String,
    details: Option<String>,
    recoverable: bool,
}

impl LauncherError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        details: Option<String>,
        recoverable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: details.as_deref().map(sanitize),
            recoverable,
        }
    }

    pub fn internal(details: impl AsRef<str>) -> Self {
        Self::new(
            "internal_error",
            "An unexpected launcher error occurred.",
            Some(details.as_ref().to_owned()),
            false,
        )
    }

    pub fn invalid_path() -> Self {
        Self::new(
            "invalid_path",
            "The requested path is outside the launcher data directory.",
            None,
            false,
        )
    }

    pub fn storage_unavailable() -> Self {
        Self::new(
            "storage_unavailable",
            "Launcher storage is unavailable.",
            None,
            true,
        )
    }

    pub fn account_state_inconsistent() -> Self {
        Self::new(
            "account_state_inconsistent",
            "Account storage could not be restored consistently. Sign in again or remove the account.",
            None,
            true,
        )
    }

    pub fn metadata_unavailable() -> Self {
        Self::new(
            "metadata_unavailable",
            "Official Minecraft version metadata is unavailable.",
            None,
            true,
        )
    }

    pub fn metadata_invalid() -> Self {
        Self::new(
            "metadata_invalid",
            "Official Minecraft version metadata is invalid.",
            None,
            false,
        )
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    pub fn recoverable(&self) -> bool {
        self.recoverable
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
        r#"(?i)\b(access_token|refresh_token)(\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s,;&]+)"#,
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

        let details = error.details().expect("sanitized details are retained");
        assert_eq!(
            details,
            "database unavailable; access_token=[REDACTED]; refresh_token: [REDACTED]"
        );
    }

    #[test]
    fn public_error_construction_sanitizes_all_serialized_details() {
        let error = LauncherError::new(
            "network_error",
            "Request failed.",
            Some(
                "Bearer abc.def.ghi; access_token=access-secret; refresh_token=refresh-secret"
                    .to_owned(),
            ),
            true,
        );
        let serialized = serde_json::to_string(&error).expect("error serializes");

        assert!(!serialized.contains("abc.def.ghi"));
        assert!(!serialized.contains("access-secret"));
        assert!(!serialized.contains("refresh-secret"));
        assert_eq!(error.code(), "network_error");
        assert_eq!(
            error.details(),
            Some("Bearer [REDACTED]; access_token=[REDACTED]; refresh_token=[REDACTED]")
        );
    }

    #[test]
    fn token_redaction_handles_escaped_quotes_and_preserves_boundary_safe_text() {
        let error = LauncherError::internal(
            r#"request timed out; access_token="abc\"def"; refresh_token="ghi\"jkl"; access_token_label=ordinary"#,
        );

        assert_eq!(
            error.details(),
            Some(
                "request timed out; access_token=[REDACTED]; refresh_token=[REDACTED]; access_token_label=ordinary"
            )
        );
    }
}

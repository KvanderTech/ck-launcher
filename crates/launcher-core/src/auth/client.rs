use crate::{error::LauncherError, storage::AccountSummary};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::{Duration, Instant};

const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_ENDPOINT: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_ENDPOINT: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_ENDPOINT: &str = "https://api.minecraftservices.com/launcher/login";
const MINECRAFT_LEGACY_LOGIN_ENDPOINT: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE_ENDPOINT: &str = "https://api.minecraftservices.com/minecraft/profile";
const XBOX_CONTRACT_VERSION: &str = "1";

pub struct OAuthTokens {
    access_token: String,
    refresh_token: crate::storage::credentials::RefreshToken,
}

impl OAuthTokens {
    pub(crate) fn new(access_token: impl Into<String>, refresh_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            refresh_token: crate::storage::credentials::RefreshToken::new(refresh_token),
        }
    }

    pub(crate) fn access_token(&self) -> &str {
        &self.access_token
    }

    pub(crate) fn refresh_token(&self) -> &crate::storage::credentials::RefreshToken {
        &self.refresh_token
    }
}

pub struct XboxToken {
    token: String,
}

impl XboxToken {
    pub(crate) fn new(token: impl Into<String>, _user_hash: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

pub struct XstsToken {
    token: String,
    user_hash: String,
}

impl XstsToken {
    pub(crate) fn new(token: impl Into<String>, user_hash: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            user_hash: user_hash.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

#[derive(Clone)]
pub struct MinecraftAccess {
    token: String,
    expires_at: Instant,
}

impl MinecraftAccess {
    #[cfg(test)]
    pub(crate) fn new(token: impl Into<String>) -> Self {
        Self::with_lifetime(token, Duration::from_secs(3_600))
    }

    fn with_lifetime(token: impl Into<String>, lifetime: Duration) -> Self {
        Self {
            token: token.into(),
            expires_at: Instant::now()
                .checked_add(lifetime)
                .unwrap_or_else(Instant::now),
        }
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn is_valid(&self) -> bool {
        Instant::now()
            .checked_add(Duration::from_secs(30))
            .is_some_and(|minimum| self.expires_at > minimum)
    }
}

#[async_trait]
pub trait MicrosoftApi: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokens, LauncherError>;
    async fn refresh_token(
        &self,
        refresh_token: &crate::storage::credentials::RefreshToken,
    ) -> Result<OAuthTokens, LauncherError>;
    async fn xbox_live(&self, access_token: &str) -> Result<XboxToken, LauncherError>;
    async fn xsts(&self, xbox: &XboxToken) -> Result<XstsToken, LauncherError>;
    async fn minecraft(&self, xsts: &XstsToken) -> Result<MinecraftAccess, LauncherError>;
    async fn profile(&self, token: &MinecraftAccess) -> Result<AccountSummary, LauncherError>;
}

pub struct HttpMicrosoftApi {
    client: Client,
    client_id: String,
}

impl HttpMicrosoftApi {
    pub fn new(client_id: impl Into<String>) -> Result<Self, LauncherError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| auth_network_error())?;
        Ok(Self {
            client,
            client_id: client_id.into(),
        })
    }

    fn xbox_request(&self, body: &XboxAuthenticateRequest<'_>) -> reqwest::RequestBuilder {
        self.client
            .post(XBOX_ENDPOINT)
            .header("x-xbl-contract-version", XBOX_CONTRACT_VERSION)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
    }

    fn xsts_request(&self, body: &XstsRequest<'_>) -> reqwest::RequestBuilder {
        self.client
            .post(XSTS_ENDPOINT)
            .header("x-xbl-contract-version", XBOX_CONTRACT_VERSION)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
    }

    #[cfg(test)]
    fn minecraft_request(&self, body: &MinecraftLoginRequest) -> reqwest::RequestBuilder {
        self.minecraft_request_to(MINECRAFT_LOGIN_ENDPOINT, body)
    }

    fn minecraft_request_to(
        &self,
        endpoint: &str,
        body: &MinecraftLoginRequest,
    ) -> reqwest::RequestBuilder {
        self.client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
    }

    #[cfg(test)]
    fn minecraft_legacy_request(
        &self,
        body: &MinecraftLegacyLoginRequest,
    ) -> reqwest::RequestBuilder {
        self.minecraft_legacy_request_to(MINECRAFT_LEGACY_LOGIN_ENDPOINT, body)
    }

    fn minecraft_legacy_request_to(
        &self,
        endpoint: &str,
        body: &MinecraftLegacyLoginRequest,
    ) -> reqwest::RequestBuilder {
        self.client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .json(body)
    }

    async fn minecraft_with_endpoints(
        &self,
        xsts: &XstsToken,
        primary_endpoint: &str,
        legacy_endpoint: &str,
    ) -> Result<MinecraftAccess, LauncherError> {
        let identity_token = format!("XBL3.0 x={};{}", xsts.user_hash, xsts.token);
        let body = MinecraftLoginRequest {
            x_token: identity_token.clone(),
            platform: "PC_LAUNCHER",
        };
        let primary = self
            .minecraft_request_to(primary_endpoint, &body)
            .send()
            .await
            .map_err(|_| auth_network_error())?;
        // Minecraft currently exposes two service endpoints in deployed launchers.
        // A compatibility retry is limited to an uninformative 403 from the newer one.
        let response: MinecraftLoginResponse = if primary.status() == StatusCode::FORBIDDEN {
            let legacy = MinecraftLegacyLoginRequest { identity_token };
            response_json(
                self.minecraft_legacy_request_to(legacy_endpoint, &legacy)
                    .send()
                    .await
                    .map_err(|_| auth_network_error())?,
                "minecraft_auth_failed",
            )
            .await?
        } else {
            response_json(primary, "minecraft_auth_failed").await?
        };
        Ok(MinecraftAccess::with_lifetime(
            response.access_token,
            Duration::from_secs(response.expires_in),
        ))
    }
}

#[async_trait]
impl MicrosoftApi for HttpMicrosoftApi {
    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokens, LauncherError> {
        let response: OAuthResponse = response_json(
            self.client
                .post(TOKEN_ENDPOINT)
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("code", code),
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect_uri),
                    ("code_verifier", verifier),
                ])
                .send()
                .await
                .map_err(|_| auth_network_error())?,
            "auth_exchange_failed",
        )
        .await?;
        Ok(OAuthTokens::new(
            response.access_token,
            response.refresh_token,
        ))
    }

    async fn refresh_token(
        &self,
        refresh_token: &crate::storage::credentials::RefreshToken,
    ) -> Result<OAuthTokens, LauncherError> {
        let response: OAuthResponse = response_json(
            self.client
                .post(TOKEN_ENDPOINT)
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    ("refresh_token", refresh_token.expose_secret()),
                    ("grant_type", "refresh_token"),
                    ("scope", "XboxLive.signin offline_access"),
                ])
                .send()
                .await
                .map_err(|_| auth_network_error())?,
            "auth_refresh_failed",
        )
        .await?;
        Ok(OAuthTokens::new(
            response.access_token,
            response.refresh_token,
        ))
    }

    async fn xbox_live(&self, access_token: &str) -> Result<XboxToken, LauncherError> {
        let body = XboxAuthenticateRequest {
            properties: XboxProperties {
                auth_method: "RPS",
                site_name: "user.auth.xboxlive.com",
                rps_ticket: format!("d={access_token}"),
            },
            relying_party: "http://auth.xboxlive.com",
            token_type: "JWT",
        };
        let response: XboxResponse = response_json(
            self.xbox_request(&body)
                .send()
                .await
                .map_err(|_| auth_network_error())?,
            "xbox_auth_failed",
        )
        .await?;
        let user_hash = response.user_hash()?.to_owned();
        Ok(XboxToken::new(response.token, user_hash))
    }

    async fn xsts(&self, xbox: &XboxToken) -> Result<XstsToken, LauncherError> {
        let body = XstsRequest {
            properties: XstsProperties {
                sandbox_id: "RETAIL",
                user_tokens: [xbox.token.as_str()],
            },
            relying_party: "rp://api.minecraftservices.com/",
            token_type: "JWT",
        };
        let response: XboxResponse = response_json(
            self.xsts_request(&body)
                .send()
                .await
                .map_err(|_| auth_network_error())?,
            "xsts_auth_failed",
        )
        .await?;
        let user_hash = response.user_hash()?.to_owned();
        Ok(XstsToken::new(response.token, user_hash))
    }

    async fn minecraft(&self, xsts: &XstsToken) -> Result<MinecraftAccess, LauncherError> {
        self.minecraft_with_endpoints(
            xsts,
            MINECRAFT_LOGIN_ENDPOINT,
            MINECRAFT_LEGACY_LOGIN_ENDPOINT,
        )
        .await
    }

    async fn profile(&self, token: &MinecraftAccess) -> Result<AccountSummary, LauncherError> {
        let response = self
            .client
            .get(MINECRAFT_PROFILE_ENDPOINT)
            .bearer_auth(&token.token)
            .send()
            .await
            .map_err(|_| auth_network_error())?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(minecraft_not_owned());
        }
        let profile: MinecraftProfile = response_json(response, "minecraft_profile_failed").await?;
        Ok(AccountSummary {
            id: profile.id.clone(),
            minecraft_name: profile.name,
            minecraft_uuid: profile.id.clone(),
            head_url: Some(format!("https://mc-heads.net/avatar/{}/64", profile.id)),
            is_active: false,
        })
    }
}

async fn response_json<T: DeserializeOwned>(
    response: reqwest::Response,
    code: &'static str,
) -> Result<T, LauncherError> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if code == "auth_exchange_failed" {
            return Err(oauth_exchange_error(status, &body));
        }
        return Err(service_auth_error(code, status, &body));
    }
    response.json().await.map_err(|_| {
        LauncherError::new(
            code,
            "Microsoft account authentication returned an invalid response.",
            None,
            true,
        )
    })
}

#[derive(Debug, Deserialize)]
struct XboxErrorResponse {
    #[serde(rename = "XErr", default)]
    xerr: Option<i64>,
}

fn service_auth_error(code: &'static str, status: StatusCode, body: &str) -> LauncherError {
    let xerr = serde_json::from_str::<XboxErrorResponse>(body)
        .ok()
        .and_then(|value| value.xerr);
    let minecraft_message = minecraft_error_message(body);
    // Minecraft has returned this diagnostic under different JSON field names over time.
    // Scan only for classification; never expose the untrusted response body or tokens.
    let invalid_registration = body
        .to_ascii_lowercase()
        .contains("invalid app registration");
    let (public_code, message) = match (code, xerr, invalid_registration) {
        ("xbox_auth_failed", _, _) => (
            "xbox_auth_failed",
            "Microsoft OAuth завершён, но Xbox Live отклонил вход.",
        ),
        ("xsts_auth_failed", Some(2_148_916_233), _) => (
            "xbox_profile_required",
            "У этой учётной записи нет профиля Xbox. Создайте его на xbox.com и повторите вход.",
        ),
        ("xsts_auth_failed", Some(2_148_916_235), _) => (
            "xbox_region_restricted",
            "Xbox Live недоступен для региона этой учётной записи.",
        ),
        ("xsts_auth_failed", Some(2_148_916_236), _) => (
            "xbox_adult_verification_required",
            "Xbox требует подтверждения возраста для этой учётной записи.",
        ),
        ("xsts_auth_failed", Some(2_148_916_237), _)
        | ("xsts_auth_failed", Some(2_148_916_238), _) => (
            "xbox_family_required",
            "Детскую учётную запись нужно добавить в семейную группу Xbox и разрешить сетевую игру.",
        ),
        ("xsts_auth_failed", _, _) => (
            "xsts_auth_failed",
            "Xbox Security Token Service отклонил учётную запись.",
        ),
        ("minecraft_auth_failed", _, true) => (
            "minecraft_app_registration_required",
            "Minecraft Services отклонил Client ID лаунчера (Invalid app registration).",
        ),
        ("minecraft_auth_failed", _, false) => (
            "minecraft_auth_failed",
            "Xbox вход выполнен, но Minecraft Services не выдал игровой токен.",
        ),
        ("minecraft_profile_failed", _, _) => (
            "minecraft_profile_failed",
            "Minecraft Services не удалось получить профиль Java Edition.",
        ),
        _ => (code, "Microsoft account authentication failed."),
    };
    LauncherError::new(
        public_code,
        message,
        Some(match xerr {
            Some(value) => format!("Xbox XErr {value}; HTTP {}", status.as_u16()),
            None => match minecraft_message.as_deref() {
                Some(message) => format!("HTTP {}: {message}", status.as_u16()),
                None => format!("HTTP {}", status.as_u16()),
            },
        }),
        true,
    )
}

fn minecraft_error_message(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    // These are the documented/observed diagnostic fields. Deliberately ignore every
    // other field so credentials accidentally echoed by a service can never reach the UI.
    [
        "errorMessage",
        "developerMessage",
        "message",
        "error_description",
        "error",
        "errorType",
    ]
    .into_iter()
    .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
    .map(sanitize_service_message)
    .filter(|message| !message.is_empty())
}

fn sanitize_service_message(message: &str) -> String {
    message
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect::<String>()
}

#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
    #[serde(default)]
    error_codes: Vec<i64>,
}

fn oauth_exchange_error(status: StatusCode, body: &str) -> LauncherError {
    let response = serde_json::from_str::<OAuthErrorResponse>(body).ok();
    let description = response
        .as_ref()
        .map(|value| value.error_description.as_str())
        .unwrap_or_default();
    let aadsts = response
        .as_ref()
        .and_then(|value| value.error_codes.first().copied())
        .or_else(|| extract_aadsts(description));
    let error = response
        .as_ref()
        .map(|value| value.error.as_str())
        .unwrap_or_default();

    let (public_code, message) = match aadsts {
        Some(7000218) => (
            "auth_public_client_disabled",
            "В Microsoft Entra включите «Разрешить общедоступные клиентские потоки» и повторите вход.",
        ),
        Some(50011) => (
            "auth_redirect_mismatch",
            "В Microsoft Entra добавьте URI перенаправления http://localhost для мобильного и классического приложения.",
        ),
        Some(70000) | Some(70008) => (
            "auth_code_expired",
            "Код Microsoft истёк или уже использован. Повторите вход.",
        ),
        _ if error == "invalid_grant" => (
            "auth_code_rejected",
            "Microsoft отклонил код входа. Повторите авторизацию.",
        ),
        _ => (
            "auth_exchange_failed",
            "Microsoft не выдал токен для этого приложения. Проверьте параметры публичного клиента в Entra.",
        ),
    };
    LauncherError::new(
        public_code,
        message,
        aadsts.map(|value| format!("Microsoft AADSTS{value}; HTTP {}", status.as_u16())),
        true,
    )
}

fn extract_aadsts(description: &str) -> Option<i64> {
    let start = description.find("AADSTS")? + "AADSTS".len();
    let digits = description[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse().ok()
}

#[derive(Deserialize)]
struct OAuthResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxAuthenticateRequest<'a> {
    properties: XboxProperties,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XboxProperties {
    #[serde(rename = "AuthMethod")]
    auth_method: &'static str,
    #[serde(rename = "SiteName")]
    site_name: &'static str,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsRequest<'a> {
    properties: XstsProperties<'a>,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsProperties<'a> {
    sandbox_id: &'a str,
    user_tokens: [&'a str; 1],
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: DisplayClaims,
}

impl XboxResponse {
    fn user_hash(&self) -> Result<&str, LauncherError> {
        self.display_claims
            .xui
            .first()
            .map(|claim| claim.user_hash.as_str())
            .ok_or_else(|| {
                LauncherError::new(
                    "xbox_auth_failed",
                    "Xbox authentication returned no user identity.",
                    None,
                    false,
                )
            })
    }
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Deserialize)]
struct XuiClaim {
    #[serde(rename = "uhs")]
    user_hash: String,
}

#[derive(Serialize)]
struct MinecraftLoginRequest {
    #[serde(rename = "xtoken")]
    x_token: String,
    platform: &'static str,
}

#[derive(Serialize)]
struct MinecraftLegacyLoginRequest {
    #[serde(rename = "identityToken")]
    identity_token: String,
}

#[derive(Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
}

fn auth_network_error() -> LauncherError {
    LauncherError::new(
        "auth_network_error",
        "Microsoft account authentication could not reach the service.",
        None,
        true,
    )
}

#[cfg(test)]
mod oauth_error_tests {
    use super::*;

    fn request_json(request: &reqwest::Request) -> serde_json::Value {
        serde_json::from_slice(
            request
                .body()
                .and_then(reqwest::Body::as_bytes)
                .expect("JSON request body"),
        )
        .expect("valid JSON request")
    }

    fn assert_json_headers(request: &reqwest::Request) {
        assert_eq!(
            request.headers().get(reqwest::header::ACCEPT),
            Some(&reqwest::header::HeaderValue::from_static(
                "application/json"
            ))
        );
        assert_eq!(
            request.headers().get(reqwest::header::CONTENT_TYPE),
            Some(&reqwest::header::HeaderValue::from_static(
                "application/json"
            ))
        );
    }

    #[test]
    fn wire_requests_match_xbox_xsts_and_minecraft_contracts() {
        let api = HttpMicrosoftApi::new("public-client-id").expect("HTTP client");
        let xbox_body = XboxAuthenticateRequest {
            properties: XboxProperties {
                auth_method: "RPS",
                site_name: "user.auth.xboxlive.com",
                rps_ticket: "d=microsoft-access".to_owned(),
            },
            relying_party: "http://auth.xboxlive.com",
            token_type: "JWT",
        };
        let xbox = api.xbox_request(&xbox_body).build().expect("Xbox request");
        assert_eq!(xbox.url().as_str(), XBOX_ENDPOINT);
        assert_json_headers(&xbox);
        assert_eq!(
            xbox.headers().get("x-xbl-contract-version"),
            Some(&reqwest::header::HeaderValue::from_static("1"))
        );
        assert_eq!(
            request_json(&xbox),
            serde_json::json!({
                "Properties": {
                    "AuthMethod": "RPS",
                    "SiteName": "user.auth.xboxlive.com",
                    "RpsTicket": "d=microsoft-access"
                },
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            })
        );

        let xsts_body = XstsRequest {
            properties: XstsProperties {
                sandbox_id: "RETAIL",
                user_tokens: ["xbox-user-token"],
            },
            relying_party: "rp://api.minecraftservices.com/",
            token_type: "JWT",
        };
        let xsts = api.xsts_request(&xsts_body).build().expect("XSTS request");
        assert_eq!(xsts.url().as_str(), XSTS_ENDPOINT);
        assert_json_headers(&xsts);
        assert_eq!(
            xsts.headers().get("x-xbl-contract-version"),
            Some(&reqwest::header::HeaderValue::from_static("1"))
        );
        assert_eq!(
            request_json(&xsts),
            serde_json::json!({
                "Properties": {
                    "SandboxId": "RETAIL",
                    "UserTokens": ["xbox-user-token"]
                },
                "RelyingParty": "rp://api.minecraftservices.com/",
                "TokenType": "JWT"
            })
        );

        let minecraft_body = MinecraftLoginRequest {
            x_token: "XBL3.0 x=user-hash;xsts-token".to_owned(),
            platform: "PC_LAUNCHER",
        };
        let minecraft = api
            .minecraft_request(&minecraft_body)
            .build()
            .expect("Minecraft request");
        assert_eq!(minecraft.url().as_str(), MINECRAFT_LOGIN_ENDPOINT);
        assert_json_headers(&minecraft);
        assert_eq!(
            request_json(&minecraft),
            serde_json::json!({
                "xtoken": "XBL3.0 x=user-hash;xsts-token",
                "platform": "PC_LAUNCHER"
            })
        );

        let legacy_body = MinecraftLegacyLoginRequest {
            identity_token: "XBL3.0 x=user-hash;xsts-token".to_owned(),
        };
        let legacy = api
            .minecraft_legacy_request(&legacy_body)
            .build()
            .expect("legacy Minecraft request");
        assert_eq!(legacy.url().as_str(), MINECRAFT_LEGACY_LOGIN_ENDPOINT);
        assert_json_headers(&legacy);
        assert_eq!(
            request_json(&legacy),
            serde_json::json!({
                "identityToken": "XBL3.0 x=user-hash;xsts-token"
            })
        );
    }

    #[test]
    fn public_client_error_is_actionable_without_exposing_response_body() {
        let error = oauth_exchange_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_client","error_description":"AADSTS7000218: client_secret required","error_codes":[7000218]}"#,
        );
        assert_eq!(error.code(), "auth_public_client_disabled");
        let serialized = serde_json::to_string(&error).expect("serializes");
        assert!(serialized.contains("AADSTS7000218"));
        assert!(!serialized.contains("client_secret required"));
    }

    #[test]
    fn redirect_error_is_mapped_to_the_exact_registered_uri() {
        let error = oauth_exchange_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_grant","error_description":"AADSTS50011: redirect mismatch"}"#,
        );
        assert_eq!(error.code(), "auth_redirect_mismatch");
    }

    #[test]
    fn xsts_error_explains_missing_xbox_profile_without_exposing_body() {
        let error = service_auth_error(
            "xsts_auth_failed",
            StatusCode::UNAUTHORIZED,
            r#"{"XErr":2148916233,"Message":"private diagnostic"}"#,
        );
        assert_eq!(error.code(), "xbox_profile_required");
        let serialized = serde_json::to_string(&error).expect("serializes");
        assert!(serialized.contains("2148916233"));
        assert!(!serialized.contains("private diagnostic"));
    }

    #[test]
    fn minecraft_registration_error_is_recognized_in_any_known_response_field() {
        for body in [
            r#"{"errorMessage":"Invalid app registration, see https://aka.ms/AppRegInfo"}"#,
            r#"{"developerMessage":"INVALID APP REGISTRATION"}"#,
            r#"{"message":"Invalid app registration"}"#,
        ] {
            let error = service_auth_error("minecraft_auth_failed", StatusCode::FORBIDDEN, body);
            assert_eq!(error.code(), "minecraft_app_registration_required");
        }
    }

    #[test]
    fn minecraft_error_exposes_only_a_bounded_known_diagnostic_field() {
        let error = service_auth_error(
            "minecraft_auth_failed",
            StatusCode::FORBIDDEN,
            r#"{"error":"Forbidden","access_token":"must-not-leak"}"#,
        );
        let serialized = serde_json::to_string(&error).expect("error serializes");
        assert!(serialized.contains("HTTP 403: Forbidden"));
        assert!(!serialized.contains("must-not-leak"));
    }
}

pub(crate) fn minecraft_not_owned() -> LauncherError {
    LauncherError::new(
        "minecraft_not_owned",
        "This Microsoft account does not own Minecraft: Java Edition.",
        None,
        false,
    )
}

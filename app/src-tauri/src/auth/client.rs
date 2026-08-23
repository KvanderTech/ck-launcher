use crate::{error::LauncherError, storage::AccountSummary};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBOX_ENDPOINT: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_ENDPOINT: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_ENDPOINT: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_PROFILE_ENDPOINT: &str = "https://api.minecraftservices.com/minecraft/profile";

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

pub struct MinecraftAccess {
    token: String,
}

impl MinecraftAccess {
    pub(crate) fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
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
            self.client
                .post(XBOX_ENDPOINT)
                .json(&body)
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
            self.client
                .post(XSTS_ENDPOINT)
                .json(&body)
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
        let body = MinecraftLoginRequest {
            identity_token: format!("XBL3.0 x={};{}", xsts.user_hash, xsts.token),
        };
        let response: MinecraftLoginResponse = response_json(
            self.client
                .post(MINECRAFT_LOGIN_ENDPOINT)
                .json(&body)
                .send()
                .await
                .map_err(|_| auth_network_error())?,
            "minecraft_auth_failed",
        )
        .await?;
        Ok(MinecraftAccess::new(response.access_token))
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
        return Err(LauncherError::new(
            code,
            "Microsoft account authentication failed.",
            None,
            true,
        ));
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
    #[serde(rename = "identityToken")]
    identity_token: String,
}

#[derive(Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
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

pub(crate) fn minecraft_not_owned() -> LauncherError {
    LauncherError::new(
        "minecraft_not_owned",
        "This Microsoft account does not own Minecraft: Java Edition.",
        None,
        false,
    )
}

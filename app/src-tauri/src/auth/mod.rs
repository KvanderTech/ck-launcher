pub mod client;
pub mod loopback;
pub mod pkce;

use crate::{
    error::LauncherError,
    storage::{credentials::CredentialStore, AccountSummary, Storage},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use client::{minecraft_not_owned, HttpMicrosoftApi, MicrosoftApi};
use loopback::CallbackReceiver;
use rand::RngCore;
use std::{sync::Arc, time::Duration};
use url::Url;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);
const AUTHORIZE_ENDPOINT: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";

pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), LauncherError>;
}

pub struct AuthSession {
    callback: CallbackReceiver,
    verifier: String,
    redirect_uri: String,
}

impl AuthSession {
    pub fn receive_code(&mut self) -> Result<String, LauncherError> {
        self.callback.receive()
    }
}

pub struct AuthService {
    client_id: Option<String>,
    api: Arc<dyn MicrosoftApi>,
    storage: Storage,
    credentials: Arc<dyn CredentialStore>,
    opener: Arc<dyn BrowserOpener>,
}

impl AuthService {
    pub fn new(
        client_id: Option<String>,
        api: Arc<dyn MicrosoftApi>,
        storage: Storage,
        credentials: Arc<dyn CredentialStore>,
        opener: Arc<dyn BrowserOpener>,
    ) -> Self {
        Self {
            client_id: client_id.filter(|value| !value.trim().is_empty()),
            api,
            storage,
            credentials,
            opener,
        }
    }

    pub fn production(
        storage: Storage,
        credentials: Arc<dyn CredentialStore>,
    ) -> Result<Self, LauncherError> {
        let client_id = std::env::var("CK_LAUNCHER_MICROSOFT_CLIENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let api = Arc::new(HttpMicrosoftApi::new(
            client_id.clone().unwrap_or_default(),
        )?);
        Ok(Self::new(
            client_id,
            api,
            storage,
            credentials,
            Arc::new(SystemBrowserOpener),
        ))
    }

    pub fn begin_login(&self) -> Result<AuthSession, LauncherError> {
        let client_id = self.client_id.as_deref().ok_or_else(|| {
            LauncherError::new(
                "auth_not_configured",
                "Microsoft sign-in is not configured for this launcher build.",
                None,
                false,
            )
        })?;
        let pkce = pkce::generate_pkce();
        let state = random_state();
        let callback = CallbackReceiver::bind(&state, CALLBACK_TIMEOUT)?;
        let redirect_uri = callback.redirect_uri();
        let mut authorization_url = Url::parse(AUTHORIZE_ENDPOINT).map_err(|_| {
            LauncherError::new(
                "auth_unavailable",
                "Microsoft sign-in is unavailable.",
                None,
                true,
            )
        })?;
        authorization_url
            .query_pairs_mut()
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", "XboxLive.signin offline_access")
            .append_pair("state", &state)
            .append_pair("code_challenge", pkce.challenge())
            .append_pair("code_challenge_method", "S256")
            .append_pair("prompt", "select_account");
        self.opener.open(authorization_url.as_str())?;

        Ok(AuthSession {
            callback,
            verifier: pkce.verifier().to_owned(),
            redirect_uri,
        })
    }

    pub async fn complete_login(
        &self,
        session: AuthSession,
        code: &str,
    ) -> Result<AccountSummary, LauncherError> {
        let oauth = self
            .api
            .exchange_code(code, &session.verifier, &session.redirect_uri)
            .await?;
        let xbox = self.api.xbox_live(oauth.access_token()).await?;
        let xsts = self.api.xsts(&xbox).await?;
        let minecraft = self.api.minecraft(&xsts).await?;
        let mut profile = self.api.profile(&minecraft).await.map_err(|error| {
            if matches!(
                error.code(),
                "minecraft_profile_not_found" | "minecraft_not_owned"
            ) {
                minecraft_not_owned()
            } else {
                error
            }
        })?;
        profile.is_active = true;

        self.credentials.save(&profile.id, oauth.refresh_token())?;
        if let Err(error) = self.storage.upsert_account(&profile).await {
            let _ = self.credentials.delete(&profile.id);
            return Err(error);
        }
        Ok(profile)
    }
}

fn random_state() -> String {
    let mut entropy = [0_u8; 32];
    rand::rng().fill_bytes(&mut entropy);
    URL_SAFE_NO_PAD.encode(entropy)
}

pub struct SystemBrowserOpener;

impl BrowserOpener for SystemBrowserOpener {
    fn open(&self, url: &str) -> Result<(), LauncherError> {
        open_system_browser(url)
    }
}

#[cfg(windows)]
fn open_system_browser(url: &str) -> Result<(), LauncherError> {
    use std::{iter::once, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let operation: Vec<u16> = std::ffi::OsStr::new("open")
        .encode_wide()
        .chain(once(0))
        .collect();
    let target: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        return Err(browser_unavailable());
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_system_browser(_url: &str) -> Result<(), LauncherError> {
    Err(browser_unavailable())
}

fn browser_unavailable() -> LauncherError {
    LauncherError::new(
        "auth_browser_unavailable",
        "The system browser could not be opened for Microsoft sign-in.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{AuthService, BrowserOpener};
    use crate::{
        auth::client::{MicrosoftApi, MinecraftAccess, OAuthTokens, XboxToken, XstsToken},
        error::LauncherError,
        storage::{
            credentials::{CredentialStore, InMemoryCredentialStore},
            AccountSummary, Storage,
        },
    };
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingOpener(Mutex<Vec<String>>);

    impl BrowserOpener for RecordingOpener {
        fn open(&self, url: &str) -> Result<(), LauncherError> {
            self.0.lock().expect("opener lock").push(url.to_owned());
            Ok(())
        }
    }

    struct MockMicrosoftApi {
        calls: Mutex<Vec<&'static str>>,
        profile_error: bool,
    }

    #[async_trait]
    impl MicrosoftApi for MockMicrosoftApi {
        async fn exchange_code(
            &self,
            code: &str,
            verifier: &str,
            redirect_uri: &str,
        ) -> Result<OAuthTokens, LauncherError> {
            assert_eq!(code, "oauth-code");
            assert!(!verifier.is_empty());
            assert!(redirect_uri.starts_with("http://127.0.0.1:"));
            self.calls.lock().expect("calls lock").push("exchange_code");
            Ok(OAuthTokens::new("oauth-access", "refresh-secret"))
        }

        async fn xbox_live(&self, access_token: &str) -> Result<XboxToken, LauncherError> {
            assert_eq!(access_token, "oauth-access");
            self.calls.lock().expect("calls lock").push("xbox_live");
            Ok(XboxToken::new("xbox-token", "user-hash"))
        }

        async fn xsts(&self, xbox: &XboxToken) -> Result<XstsToken, LauncherError> {
            assert_eq!(xbox.token(), "xbox-token");
            self.calls.lock().expect("calls lock").push("xsts");
            Ok(XstsToken::new("xsts-token", "user-hash"))
        }

        async fn minecraft(&self, xsts: &XstsToken) -> Result<MinecraftAccess, LauncherError> {
            assert_eq!(xsts.token(), "xsts-token");
            self.calls.lock().expect("calls lock").push("minecraft");
            Ok(MinecraftAccess::new("minecraft-access"))
        }

        async fn profile(&self, token: &MinecraftAccess) -> Result<AccountSummary, LauncherError> {
            assert_eq!(token.token(), "minecraft-access");
            self.calls.lock().expect("calls lock").push("profile");
            if self.profile_error {
                return Err(LauncherError::new(
                    "minecraft_profile_not_found",
                    "Profile missing.",
                    None,
                    false,
                ));
            }
            Ok(AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Player".to_owned(),
                minecraft_uuid: "minecraft-uuid".to_owned(),
                head_url: Some("https://example.test/head.png".to_owned()),
                is_active: false,
            })
        }
    }

    #[test]
    fn complete_login_runs_exact_exchange_order_and_splits_persistence() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:").await.expect("storage");
            let credentials = Arc::new(InMemoryCredentialStore::default());
            let api = Arc::new(MockMicrosoftApi {
                calls: Mutex::new(Vec::new()),
                profile_error: false,
            });
            let opener = Arc::new(RecordingOpener::default());
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                api.clone(),
                storage.clone(),
                credentials.clone(),
                opener.clone(),
            );

            let session = service.begin_login().expect("login begins");
            assert_eq!(opener.0.lock().expect("opener lock").len(), 1);
            let profile = service
                .complete_login(session, "oauth-code")
                .await
                .expect("login completes");

            assert_eq!(
                *api.calls.lock().expect("calls lock"),
                ["exchange_code", "xbox_live", "xsts", "minecraft", "profile"]
            );
            assert!(profile.is_active);
            assert_eq!(
                storage.list_accounts().await.expect("accounts"),
                vec![profile]
            );
            assert_eq!(
                credentials
                    .get("stable-account-id")
                    .expect("credential reads")
                    .expect("credential exists")
                    .expose_secret(),
                "refresh-secret"
            );
        });
    }

    #[test]
    fn profile_not_found_maps_to_stable_minecraft_not_owned_error() {
        tauri::async_runtime::block_on(async {
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                Arc::new(MockMicrosoftApi {
                    calls: Mutex::new(Vec::new()),
                    profile_error: true,
                }),
                Storage::connect("sqlite::memory:").await.expect("storage"),
                Arc::new(InMemoryCredentialStore::default()),
                Arc::new(RecordingOpener::default()),
            );

            let session = service.begin_login().expect("login begins");
            let error = service
                .complete_login(session, "oauth-code")
                .await
                .expect_err("unowned profile is rejected");
            assert_eq!(error.code(), "minecraft_not_owned");
        });
    }

    #[test]
    fn missing_client_id_fails_before_opening_browser() {
        let opener = Arc::new(RecordingOpener::default());
        let service = AuthService::new(
            None,
            Arc::new(MockMicrosoftApi {
                calls: Mutex::new(Vec::new()),
                profile_error: false,
            }),
            tauri::async_runtime::block_on(Storage::connect("sqlite::memory:")).expect("storage"),
            Arc::new(InMemoryCredentialStore::default()),
            opener.clone(),
        );

        let error = match service.begin_login() {
            Err(error) => error,
            Ok(_) => panic!("configuration is required"),
        };
        assert_eq!(error.code(), "auth_not_configured");
        assert!(opener.0.lock().expect("opener lock").is_empty());
    }
}

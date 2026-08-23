pub mod client;
pub mod loopback;
pub mod pkce;

use crate::{
    downloads::DownloadCancellationToken,
    error::LauncherError,
    storage::{
        credentials::CredentialStore, AccountMutationCoordinator, AccountStore, AccountSummary,
        Storage,
    },
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use client::{minecraft_not_owned, HttpMicrosoftApi, MicrosoftApi};
use loopback::CallbackReceiver;
use rand::RngCore;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
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
    cancel: DownloadCancellationToken,
    active_login: Arc<Mutex<Option<DownloadCancellationToken>>>,
}

impl AuthSession {
    pub fn receive_code(&mut self) -> Result<String, LauncherError> {
        self.callback.receive_cancellable(&self.cancel)
    }
}

impl Drop for AuthSession {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active_login.lock() {
            if active
                .as_ref()
                .is_some_and(|current| current.same_operation(&self.cancel))
            {
                *active = None;
            }
        }
    }
}

pub struct AuthService {
    client_id: Option<String>,
    api: Arc<dyn MicrosoftApi>,
    storage: Arc<dyn AccountStore>,
    credentials: Arc<dyn CredentialStore>,
    opener: Arc<dyn BrowserOpener>,
    mutations: Arc<AccountMutationCoordinator>,
    token_cache: Mutex<Option<CachedMinecraftAccess>>,
    active_login: Arc<Mutex<Option<DownloadCancellationToken>>>,
}

struct CachedMinecraftAccess {
    account_id: String,
    access: client::MinecraftAccess,
}

pub(crate) struct RefreshedMinecraftAccount {
    pub account: AccountSummary,
    access_token: client::MinecraftAccess,
}

impl RefreshedMinecraftAccount {
    pub(crate) fn into_parts(self) -> (AccountSummary, String) {
        (self.account, self.access_token.token().to_owned())
    }
}

impl AuthService {
    pub fn new(
        client_id: Option<String>,
        api: Arc<dyn MicrosoftApi>,
        storage: Arc<dyn AccountStore>,
        credentials: Arc<dyn CredentialStore>,
        opener: Arc<dyn BrowserOpener>,
        mutations: Arc<AccountMutationCoordinator>,
    ) -> Self {
        Self {
            client_id: client_id.filter(|value| !value.trim().is_empty()),
            api,
            storage,
            credentials,
            opener,
            mutations,
            token_cache: Mutex::new(None),
            active_login: Arc::new(Mutex::new(None)),
        }
    }

    pub fn production(
        storage: Storage,
        credentials: Arc<dyn CredentialStore>,
        mutations: Arc<AccountMutationCoordinator>,
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
            Arc::new(storage),
            credentials,
            Arc::new(SystemBrowserOpener),
            mutations,
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
        let mut active_login = self
            .active_login
            .lock()
            .map_err(|_| LauncherError::internal("Microsoft login state is unavailable"))?;
        if active_login
            .as_ref()
            .is_some_and(|current| !current.is_cancelled())
        {
            return Err(LauncherError::new(
                "auth_in_progress",
                "Microsoft sign-in is already in progress.",
                None,
                true,
            ));
        }
        let cancel = DownloadCancellationToken::new();
        *active_login = Some(cancel.clone());
        drop(active_login);
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
        let session = AuthSession {
            callback,
            verifier: pkce.verifier().to_owned(),
            redirect_uri,
            cancel,
            active_login: self.active_login.clone(),
        };
        self.opener.open(authorization_url.as_str())?;

        Ok(session)
    }

    pub fn cancel_login(&self) -> Result<(), LauncherError> {
        if let Some(cancel) = self
            .active_login
            .lock()
            .map_err(|_| LauncherError::internal("Microsoft login state is unavailable"))?
            .as_ref()
        {
            cancel.cancel();
        }
        Ok(())
    }

    pub async fn complete_login(
        &self,
        session: AuthSession,
        code: &str,
    ) -> Result<AccountSummary, LauncherError> {
        let cancel = session.cancel.clone();
        let oauth = await_auth_or_cancel(
            &cancel,
            self.api
                .exchange_code(code, &session.verifier, &session.redirect_uri),
        )
        .await?;
        let xbox = await_auth_or_cancel(&cancel, self.api.xbox_live(oauth.access_token())).await?;
        let xsts = await_auth_or_cancel(&cancel, self.api.xsts(&xbox)).await?;
        let minecraft = await_auth_or_cancel(&cancel, self.api.minecraft(&xsts)).await?;
        let mut profile = await_auth_or_cancel(&cancel, self.api.profile(&minecraft))
            .await
            .map_err(|error| {
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

        let _mutation = self.mutations.lock().await;
        let previous = self.credentials.get(&profile.id)?;
        self.credentials.save(&profile.id, oauth.refresh_token())?;
        if let Err(error) = self.storage.upsert_account(&profile).await {
            let restoration = match previous.as_ref() {
                Some(token) => self.credentials.save(&profile.id, token),
                None => self.credentials.delete(&profile.id),
            };
            if restoration.is_err() {
                return Err(LauncherError::account_state_inconsistent());
            }
            return Err(error);
        }
        self.cache_access(&profile.id, minecraft)?;
        Ok(profile)
    }

    pub(crate) async fn refresh_active_minecraft_account(
        &self,
    ) -> Result<RefreshedMinecraftAccount, LauncherError> {
        self.refresh_active_minecraft_account_cancellable(DownloadCancellationToken::new())
            .await
    }

    pub(crate) async fn refresh_active_minecraft_account_cancellable(
        &self,
        cancel: DownloadCancellationToken,
    ) -> Result<RefreshedMinecraftAccount, LauncherError> {
        let _mutation = self.mutations.lock().await;
        ensure_auth_not_cancelled(&cancel)?;
        let account = self
            .storage
            .list_accounts()
            .await?
            .into_iter()
            .find(|account| account.is_active)
            .ok_or_else(|| {
                LauncherError::new(
                    "account_required",
                    "Sign in to launch Minecraft.",
                    None,
                    true,
                )
            })?;
        if let Some(access_token) = self.cached_access(&account.id)? {
            return Ok(RefreshedMinecraftAccount {
                account,
                access_token,
            });
        }
        let refresh = self.credentials.get(&account.id)?.ok_or_else(|| {
            LauncherError::new(
                "account_reauthentication_required",
                "Sign in again to launch Minecraft.",
                None,
                true,
            )
        })?;
        let oauth = await_auth_or_cancel(&cancel, self.api.refresh_token(&refresh))
            .await
            .map_err(|error| {
                if error.code() == "download_cancelled" {
                    return error;
                }
                LauncherError::new(
                    "account_reauthentication_required",
                    "Sign in again to launch Minecraft.",
                    None,
                    true,
                )
            })?;
        let xbox = await_auth_or_cancel(&cancel, self.api.xbox_live(oauth.access_token())).await?;
        let xsts = await_auth_or_cancel(&cancel, self.api.xsts(&xbox)).await?;
        let access_token = await_auth_or_cancel(&cancel, self.api.minecraft(&xsts)).await?;
        self.credentials.save(&account.id, oauth.refresh_token())?;
        self.cache_access(&account.id, access_token.clone())?;
        Ok(RefreshedMinecraftAccount {
            account,
            access_token,
        })
    }

    fn cached_access(
        &self,
        account_id: &str,
    ) -> Result<Option<client::MinecraftAccess>, LauncherError> {
        let mut cache = self
            .token_cache
            .lock()
            .map_err(|_| LauncherError::storage_unavailable())?;
        let valid = cache
            .as_ref()
            .is_some_and(|cached| cached.account_id == account_id && cached.access.is_valid());
        if valid {
            Ok(cache.as_ref().map(|cached| cached.access.clone()))
        } else {
            *cache = None;
            Ok(None)
        }
    }

    fn cache_access(
        &self,
        account_id: &str,
        access: client::MinecraftAccess,
    ) -> Result<(), LauncherError> {
        *self
            .token_cache
            .lock()
            .map_err(|_| LauncherError::storage_unavailable())? = Some(CachedMinecraftAccess {
            account_id: account_id.to_owned(),
            access,
        });
        Ok(())
    }
}

async fn await_auth_or_cancel<T>(
    cancel: &DownloadCancellationToken,
    future: impl std::future::Future<Output = Result<T, LauncherError>>,
) -> Result<T, LauncherError> {
    let cancelled = cancel.cancelled();
    futures_util::pin_mut!(cancelled, future);
    match futures_util::future::select(cancelled, future).await {
        futures_util::future::Either::Left(_) => Err(operation_cancelled()),
        futures_util::future::Either::Right((result, _)) => result,
    }
}

fn ensure_auth_not_cancelled(cancel: &DownloadCancellationToken) -> Result<(), LauncherError> {
    if cancel.is_cancelled() {
        Err(operation_cancelled())
    } else {
        Ok(())
    }
}

fn operation_cancelled() -> LauncherError {
    LauncherError::new(
        "download_cancelled",
        "The operation was cancelled.",
        None,
        true,
    )
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
        commands::accounts::AccountService,
        error::LauncherError,
        storage::{
            credentials::{CredentialStore, InMemoryCredentialStore, RefreshToken},
            AccountMutationCoordinator, AccountStore, AccountSummary, Storage,
        },
    };
    use async_trait::async_trait;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::sync::Semaphore;

    struct ControlledAccountStore {
        accounts: Mutex<Vec<AccountSummary>>,
        block_upsert: bool,
        block_delete: bool,
        upsert_entered: Semaphore,
        delete_entered: Semaphore,
        release_upsert: Semaphore,
        release_delete: Semaphore,
    }

    impl ControlledAccountStore {
        fn new(accounts: Vec<AccountSummary>, block_upsert: bool, block_delete: bool) -> Self {
            Self {
                accounts: Mutex::new(accounts),
                block_upsert,
                block_delete,
                upsert_entered: Semaphore::new(0),
                delete_entered: Semaphore::new(0),
                release_upsert: Semaphore::new(0),
                release_delete: Semaphore::new(0),
            }
        }

        async fn wait_for_upsert(&self) {
            self.upsert_entered
                .acquire()
                .await
                .expect("upsert signal")
                .forget();
        }

        async fn wait_for_delete(&self) {
            self.delete_entered
                .acquire()
                .await
                .expect("delete signal")
                .forget();
        }

        fn release_upsert(&self) {
            self.release_upsert.add_permits(1);
        }

        fn release_delete(&self) {
            self.release_delete.add_permits(1);
        }
    }

    #[async_trait]
    impl AccountStore for ControlledAccountStore {
        async fn upsert_account(&self, account: &AccountSummary) -> Result<(), LauncherError> {
            self.upsert_entered.add_permits(1);
            if self.block_upsert {
                self.release_upsert
                    .acquire()
                    .await
                    .expect("upsert release")
                    .forget();
            }
            let mut accounts = self.accounts.lock().expect("accounts lock");
            accounts.retain(|existing| existing.id != account.id);
            accounts.push(account.clone());
            Ok(())
        }

        async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
            Ok(self.accounts.lock().expect("accounts lock").clone())
        }

        async fn set_active_account(&self, _account_id: &str) -> Result<(), LauncherError> {
            unreachable!("test does not switch accounts")
        }

        async fn delete_account(&self, account_id: &str) -> Result<(), LauncherError> {
            self.delete_entered.add_permits(1);
            if self.block_delete {
                self.release_delete
                    .acquire()
                    .await
                    .expect("delete release")
                    .forget();
            }
            self.accounts
                .lock()
                .expect("accounts lock")
                .retain(|account| account.id != account_id);
            Ok(())
        }
    }

    struct FailingAccountStore {
        accounts: Mutex<Vec<AccountSummary>>,
    }

    #[async_trait]
    impl AccountStore for FailingAccountStore {
        async fn upsert_account(&self, _account: &AccountSummary) -> Result<(), LauncherError> {
            Err(LauncherError::storage_unavailable())
        }

        async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
            Ok(self.accounts.lock().expect("accounts lock").clone())
        }

        async fn set_active_account(&self, _account_id: &str) -> Result<(), LauncherError> {
            unreachable!("test does not switch accounts")
        }

        async fn delete_account(&self, _account_id: &str) -> Result<(), LauncherError> {
            unreachable!("test does not delete accounts")
        }
    }

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
        refresh_error: bool,
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
            let redirect = url::Url::parse(redirect_uri).expect("redirect parses");
            assert_eq!(redirect.host_str(), Some("localhost"));
            assert_eq!(redirect.path(), "/callback");
            assert!(redirect.port().is_some());
            self.calls.lock().expect("calls lock").push("exchange_code");
            Ok(OAuthTokens::new("oauth-access", "refresh-secret"))
        }

        async fn refresh_token(
            &self,
            refresh_token: &RefreshToken,
        ) -> Result<OAuthTokens, LauncherError> {
            assert!(matches!(
                refresh_token.expose_secret(),
                "refresh-secret" | "old-refresh-secret"
            ));
            self.calls.lock().expect("calls lock").push("refresh_token");
            if self.refresh_error {
                return Err(LauncherError::new(
                    "oauth_exchange_failed",
                    "The token endpoint rejected the refresh token.",
                    Some("refresh_token=must-not-leak".to_owned()),
                    true,
                ));
            }
            Ok(OAuthTokens::new(
                "refreshed-oauth-access",
                "rotated-refresh-secret",
            ))
        }

        async fn xbox_live(&self, access_token: &str) -> Result<XboxToken, LauncherError> {
            assert!(matches!(
                access_token,
                "oauth-access" | "refreshed-oauth-access"
            ));
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
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let credentials = Arc::new(InMemoryCredentialStore::default());
            let api = Arc::new(MockMicrosoftApi {
                calls: Mutex::new(Vec::new()),
                profile_error: false,
                refresh_error: false,
            });
            let opener = Arc::new(RecordingOpener::default());
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                api.clone(),
                storage.clone(),
                credentials.clone(),
                opener.clone(),
                Arc::new(AccountMutationCoordinator::default()),
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
    fn launch_refreshes_the_active_account_internally_and_rotates_the_credential() {
        tauri::async_runtime::block_on(async {
            let account = AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Player".to_owned(),
                minecraft_uuid: "minecraft-uuid".to_owned(),
                head_url: None,
                is_active: true,
            };
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            storage.upsert_account(&account).await.expect("account");
            let credentials = Arc::new(InMemoryCredentialStore::default());
            credentials
                .save(&account.id, &RefreshToken::new("refresh-secret"))
                .expect("credential");
            let api = Arc::new(MockMicrosoftApi {
                calls: Mutex::new(Vec::new()),
                profile_error: false,
                refresh_error: false,
            });
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                api.clone(),
                storage,
                credentials.clone(),
                Arc::new(RecordingOpener::default()),
                Arc::new(AccountMutationCoordinator::default()),
            );

            let refreshed = service
                .refresh_active_minecraft_account()
                .await
                .expect("refresh");
            let (public, access_token) = refreshed.into_parts();
            let cached = service
                .refresh_active_minecraft_account()
                .await
                .expect("cached token");
            let (cached_public, cached_access_token) = cached.into_parts();

            assert_eq!(public, account);
            assert_eq!(access_token, "minecraft-access");
            assert_eq!(cached_public, account);
            assert_eq!(cached_access_token, "minecraft-access");
            assert_eq!(
                *api.calls.lock().expect("calls"),
                ["refresh_token", "xbox_live", "xsts", "minecraft"]
            );
            assert_eq!(
                credentials
                    .get("stable-account-id")
                    .expect("read")
                    .expect("rotated")
                    .expose_secret(),
                "rotated-refresh-secret"
            );
        });
    }

    #[test]
    fn rejected_refresh_maps_to_reauthentication_without_exposing_the_token() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let account = AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Player".to_owned(),
                minecraft_uuid: "minecraft-uuid".to_owned(),
                head_url: None,
                is_active: true,
            };
            storage
                .upsert_account(&account)
                .await
                .expect("account saves");
            let credentials = Arc::new(InMemoryCredentialStore::default());
            credentials
                .save(&account.id, &RefreshToken::new("refresh-secret"))
                .expect("credential saves");
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                Arc::new(MockMicrosoftApi {
                    calls: Mutex::new(Vec::new()),
                    profile_error: false,
                    refresh_error: true,
                }),
                storage,
                credentials,
                Arc::new(RecordingOpener::default()),
                Arc::new(AccountMutationCoordinator::default()),
            );

            let error = match service.refresh_active_minecraft_account().await {
                Ok(_) => panic!("refresh rejection requires sign-in"),
                Err(error) => error,
            };
            let serialized = serde_json::to_string(&error).expect("error serializes");

            assert_eq!(error.code(), "account_reauthentication_required");
            assert!(!serialized.contains("refresh-secret"));
            assert!(!serialized.contains("must-not-leak"));
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
                    refresh_error: false,
                }),
                Arc::new(Storage::connect("sqlite::memory:").await.expect("storage")),
                Arc::new(InMemoryCredentialStore::default()),
                Arc::new(RecordingOpener::default()),
                Arc::new(AccountMutationCoordinator::default()),
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
    fn failed_reauthentication_restores_previous_refresh_token_and_public_account() {
        tauri::async_runtime::block_on(async {
            let existing = AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Old Player".to_owned(),
                minecraft_uuid: "old-minecraft-uuid".to_owned(),
                head_url: None,
                is_active: true,
            };
            let storage = Arc::new(FailingAccountStore {
                accounts: Mutex::new(vec![existing.clone()]),
            });
            let credentials = Arc::new(InMemoryCredentialStore::default());
            credentials
                .save(&existing.id, &RefreshToken::new("old-refresh-secret"))
                .expect("old credential saves");
            let service = AuthService::new(
                Some("public-client-id".to_owned()),
                Arc::new(MockMicrosoftApi {
                    calls: Mutex::new(Vec::new()),
                    profile_error: false,
                    refresh_error: false,
                }),
                storage.clone(),
                credentials.clone(),
                Arc::new(RecordingOpener::default()),
                Arc::new(AccountMutationCoordinator::default()),
            );

            let session = service.begin_login().expect("login begins");
            let error = service
                .complete_login(session, "oauth-code")
                .await
                .expect_err("SQLite failure aborts re-authentication");

            assert_eq!(error.code(), "storage_unavailable");
            assert_eq!(
                storage.list_accounts().await.expect("accounts"),
                vec![existing]
            );
            assert_eq!(
                credentials
                    .get("stable-account-id")
                    .expect("credential reads")
                    .expect("old credential remains")
                    .expose_secret(),
                "old-refresh-secret"
            );
        });
    }

    #[test]
    fn removal_waits_for_login_persistence_and_leaves_no_public_or_credential_orphan() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(ControlledAccountStore::new(Vec::new(), true, false));
            let credentials = Arc::new(InMemoryCredentialStore::default());
            let mutations = Arc::new(AccountMutationCoordinator::default());
            let auth = Arc::new(AuthService::new(
                Some("public-client-id".to_owned()),
                Arc::new(MockMicrosoftApi {
                    calls: Mutex::new(Vec::new()),
                    profile_error: false,
                    refresh_error: false,
                }),
                storage.clone(),
                credentials.clone(),
                Arc::new(RecordingOpener::default()),
                mutations.clone(),
            ));
            let accounts = Arc::new(AccountService::new(
                storage.clone(),
                credentials.clone(),
                mutations,
            ));
            let session = auth.begin_login().expect("login begins");
            let login = tauri::async_runtime::spawn({
                let auth = auth.clone();
                async move { auth.complete_login(session, "oauth-code").await }
            });
            storage.wait_for_upsert().await;

            let removal = tauri::async_runtime::spawn({
                let accounts = accounts.clone();
                async move { accounts.remove_account("stable-account-id").await }
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(50), storage.wait_for_delete())
                    .await
                    .is_err()
            );

            storage.release_upsert();
            login.await.expect("login task").expect("login persists");
            removal
                .await
                .expect("removal task")
                .expect("removal persists");

            assert!(storage.list_accounts().await.expect("accounts").is_empty());
            assert!(credentials
                .get("stable-account-id")
                .expect("credential reads")
                .is_none());
        });
    }

    #[test]
    fn login_waits_for_removal_persistence_and_leaves_matching_public_and_credential_data() {
        tauri::async_runtime::block_on(async {
            let existing = AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Old Player".to_owned(),
                minecraft_uuid: "old-minecraft-uuid".to_owned(),
                head_url: None,
                is_active: true,
            };
            let storage = Arc::new(ControlledAccountStore::new(
                vec![existing.clone()],
                false,
                true,
            ));
            let credentials = Arc::new(InMemoryCredentialStore::default());
            credentials
                .save(&existing.id, &RefreshToken::new("old-refresh-secret"))
                .expect("old credential saves");
            let mutations = Arc::new(AccountMutationCoordinator::default());
            let auth = Arc::new(AuthService::new(
                Some("public-client-id".to_owned()),
                Arc::new(MockMicrosoftApi {
                    calls: Mutex::new(Vec::new()),
                    profile_error: false,
                    refresh_error: false,
                }),
                storage.clone(),
                credentials.clone(),
                Arc::new(RecordingOpener::default()),
                mutations.clone(),
            ));
            let accounts = Arc::new(AccountService::new(
                storage.clone(),
                credentials.clone(),
                mutations,
            ));
            let removal = tauri::async_runtime::spawn({
                let accounts = accounts.clone();
                async move { accounts.remove_account("stable-account-id").await }
            });
            storage.wait_for_delete().await;

            let session = auth.begin_login().expect("login begins");
            let login = tauri::async_runtime::spawn({
                let auth = auth.clone();
                async move { auth.complete_login(session, "oauth-code").await }
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(50), storage.wait_for_upsert())
                    .await
                    .is_err()
            );

            storage.release_delete();
            removal
                .await
                .expect("removal task")
                .expect("removal persists");
            let profile = login.await.expect("login task").expect("login persists");

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
    fn missing_client_id_fails_before_opening_browser() {
        let opener = Arc::new(RecordingOpener::default());
        let service = AuthService::new(
            None,
            Arc::new(MockMicrosoftApi {
                calls: Mutex::new(Vec::new()),
                profile_error: false,
                refresh_error: false,
            }),
            Arc::new(
                tauri::async_runtime::block_on(Storage::connect("sqlite::memory:"))
                    .expect("storage"),
            ),
            Arc::new(InMemoryCredentialStore::default()),
            opener.clone(),
            Arc::new(AccountMutationCoordinator::default()),
        );

        let error = match service.begin_login() {
            Err(error) => error,
            Ok(_) => panic!("configuration is required"),
        };
        assert_eq!(error.code(), "auth_not_configured");
        assert!(opener.0.lock().expect("opener lock").is_empty());
    }
}

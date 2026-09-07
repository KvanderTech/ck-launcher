use crate::error::LauncherError;
use std::{
    collections::HashMap,
    fmt,
    sync::{Mutex, MutexGuard},
};

const CREDENTIAL_SERVICE: &str = "ck-launcher";

pub struct RefreshToken(String);

impl RefreshToken {
    pub(crate) fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    pub(crate) fn expose_secret(&self) -> &str {
        &self.0
    }
}

pub trait CredentialStore: Send + Sync {
    fn save(&self, account_id: &str, token: &RefreshToken) -> Result<(), LauncherError>;
    fn get(&self, account_id: &str) -> Result<Option<RefreshToken>, LauncherError>;
    fn delete(&self, account_id: &str) -> Result<(), LauncherError>;
}

#[derive(Default)]
pub struct InMemoryCredentialStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl fmt::Debug for InMemoryCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stored_accounts = self.tokens.lock().map(|tokens| tokens.len()).unwrap_or(0);
        formatter
            .debug_struct("InMemoryCredentialStore")
            .field("stored_accounts", &stored_accounts)
            .finish()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn save(&self, account_id: &str, token: &RefreshToken) -> Result<(), LauncherError> {
        self.lock_tokens()?
            .insert(account_id.to_owned(), token.expose_secret().to_owned());
        Ok(())
    }

    fn get(&self, account_id: &str) -> Result<Option<RefreshToken>, LauncherError> {
        Ok(self
            .lock_tokens()?
            .get(account_id)
            .cloned()
            .map(RefreshToken::new))
    }

    fn delete(&self, account_id: &str) -> Result<(), LauncherError> {
        self.lock_tokens()?.remove(account_id);
        Ok(())
    }
}

impl InMemoryCredentialStore {
    fn lock_tokens(&self) -> Result<MutexGuard<'_, HashMap<String, String>>, LauncherError> {
        self.tokens.lock().map_err(|_| credential_unavailable())
    }
}

#[derive(Default)]
pub struct WindowsCredentialStore;

impl CredentialStore for WindowsCredentialStore {
    fn save(&self, account_id: &str, token: &RefreshToken) -> Result<(), LauncherError> {
        keyring_entry(account_id)?
            .set_password(token.expose_secret())
            .map_err(|_| credential_unavailable())
    }

    fn get(&self, account_id: &str) -> Result<Option<RefreshToken>, LauncherError> {
        match keyring_entry(account_id)?.get_password() {
            Ok(secret) => Ok(Some(RefreshToken::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_unavailable()),
        }
    }

    fn delete(&self, account_id: &str) -> Result<(), LauncherError> {
        match keyring_entry(account_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_unavailable()),
        }
    }
}

fn keyring_entry(account_id: &str) -> Result<keyring::Entry, LauncherError> {
    keyring::Entry::new(CREDENTIAL_SERVICE, account_id).map_err(|_| credential_unavailable())
}

fn credential_unavailable() -> LauncherError {
    LauncherError::new(
        "credential_unavailable",
        "Windows Credential Manager is unavailable.",
        None,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{CredentialStore, InMemoryCredentialStore, RefreshToken};

    #[test]
    fn in_memory_store_saves_reads_and_deletes_refresh_tokens() {
        let store = InMemoryCredentialStore::default();
        let token = RefreshToken::new("refresh-secret");

        store
            .save("stable-account-id", &token)
            .expect("token saves");
        let loaded = store
            .get("stable-account-id")
            .expect("token reads")
            .expect("token exists");
        assert_eq!(loaded.expose_secret(), "refresh-secret");

        store.delete("stable-account-id").expect("token deletes");
        assert!(store
            .get("stable-account-id")
            .expect("missing token reads")
            .is_none());
    }

    #[test]
    fn credential_debug_output_never_contains_refresh_token() {
        let store = InMemoryCredentialStore::default();
        store
            .save("stable-account-id", &RefreshToken::new("refresh-secret"))
            .expect("token saves");

        let debug = format!("{store:?}");
        assert!(!debug.contains("refresh-secret"));
        assert!(debug.contains("stored_accounts"));
    }
}

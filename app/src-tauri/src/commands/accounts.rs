use crate::{
    auth::AuthService,
    error::LauncherError,
    storage::{credentials::CredentialStore, AccountSummary, Storage},
};
use std::sync::Arc;
use tauri::State;

pub struct AccountService {
    storage: Storage,
    credentials: Arc<dyn CredentialStore>,
}

impl AccountService {
    pub fn new(storage: Storage, credentials: Arc<dyn CredentialStore>) -> Self {
        Self {
            storage,
            credentials,
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
        self.storage.list_accounts().await
    }

    pub async fn set_active_account(&self, account_id: &str) -> Result<(), LauncherError> {
        self.storage.set_active_account(account_id).await
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), LauncherError> {
        let previous = self.credentials.get(account_id)?;
        self.credentials.delete(account_id)?;
        if let Err(error) = self.storage.delete_account(account_id).await {
            if let Some(token) = previous.as_ref() {
                let _ = self.credentials.save(account_id, token);
            }
            return Err(error);
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn list_accounts(
    accounts: State<'_, AccountService>,
) -> Result<Vec<AccountSummary>, LauncherError> {
    accounts.list_accounts().await
}

#[tauri::command]
pub async fn begin_microsoft_login(
    auth: State<'_, AuthService>,
) -> Result<AccountSummary, LauncherError> {
    let session = auth.begin_login()?;
    let (session, code) = tauri::async_runtime::spawn_blocking(move || {
        let mut session = session;
        let code = session.receive_code();
        (session, code)
    })
    .await
    .map_err(|_| LauncherError::internal("Microsoft callback task failed"))?;
    auth.complete_login(session, &code?).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn remove_account(
    account_id: String,
    accounts: State<'_, AccountService>,
) -> Result<(), LauncherError> {
    accounts.remove_account(&account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_active_account(
    account_id: String,
    accounts: State<'_, AccountService>,
) -> Result<(), LauncherError> {
    accounts.set_active_account(&account_id).await
}

#[cfg(test)]
mod tests {
    use super::AccountService;
    use crate::storage::{
        credentials::{CredentialStore, InMemoryCredentialStore, RefreshToken},
        AccountSummary, Storage,
    };
    use std::sync::Arc;

    #[test]
    fn removing_account_clears_public_row_and_refresh_credential() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:").await.expect("storage");
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
            let service = AccountService::new(storage, credentials.clone());

            service.remove_account(&account.id).await.expect("removes");

            assert!(service.list_accounts().await.expect("accounts").is_empty());
            assert!(credentials
                .get(&account.id)
                .expect("credential reads")
                .is_none());
        });
    }
}

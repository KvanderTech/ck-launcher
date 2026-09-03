use crate::{
    auth::AuthService,
    error::LauncherError,
    storage::{
        credentials::CredentialStore, AccountMutationCoordinator, AccountStore, AccountSummary,
    },
};
use std::sync::Arc;
use tauri::State;

pub struct AccountService {
    storage: Arc<dyn AccountStore>,
    credentials: Arc<dyn CredentialStore>,
    mutations: Arc<AccountMutationCoordinator>,
}

impl AccountService {
    pub fn new(
        storage: Arc<dyn AccountStore>,
        credentials: Arc<dyn CredentialStore>,
        mutations: Arc<AccountMutationCoordinator>,
    ) -> Self {
        Self {
            storage,
            credentials,
            mutations,
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
        let accounts = self.storage.list_accounts().await?;
        let legacy: Vec<_> = accounts
            .iter()
            .filter(|account| account.id.starts_with("offline:"))
            .map(|account| account.id.clone())
            .collect();
        if legacy.is_empty() {
            return Ok(accounts);
        }
        let _mutation = self.mutations.lock().await;
        for account_id in legacy {
            self.credentials.delete(&account_id)?;
            self.storage.delete_account(&account_id).await?;
        }
        self.storage.list_accounts().await
    }

    pub async fn set_active_account(&self, account_id: &str) -> Result<(), LauncherError> {
        let _mutation = self.mutations.lock().await;
        self.storage.set_active_account(account_id).await
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), LauncherError> {
        let _mutation = self.mutations.lock().await;
        let previous = self.credentials.get(account_id)?;
        self.credentials.delete(account_id)?;
        if let Err(error) = self.storage.delete_account(account_id).await {
            if let Some(token) = previous.as_ref() {
                if self.credentials.save(account_id, token).is_err() {
                    return Err(LauncherError::account_state_inconsistent());
                }
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
    auth: State<'_, Arc<AuthService>>,
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

#[tauri::command]
pub async fn cancel_microsoft_login(
    auth: State<'_, Arc<AuthService>>,
) -> Result<(), LauncherError> {
    auth.cancel_login()
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
    use crate::error::LauncherError;
    use crate::storage::{
        credentials::{CredentialStore, InMemoryCredentialStore, RefreshToken},
        AccountMutationCoordinator, AccountStore, AccountSummary, Storage,
    };
    use async_trait::async_trait;
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    struct FailingDeleteAccountStore {
        account: AccountSummary,
    }

    #[async_trait]
    impl AccountStore for FailingDeleteAccountStore {
        async fn upsert_account(&self, _account: &AccountSummary) -> Result<(), LauncherError> {
            unreachable!("test does not upsert accounts")
        }

        async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
            Ok(vec![self.account.clone()])
        }

        async fn set_active_account(&self, _account_id: &str) -> Result<(), LauncherError> {
            unreachable!("test does not switch accounts")
        }

        async fn delete_account(&self, _account_id: &str) -> Result<(), LauncherError> {
            Err(LauncherError::storage_unavailable())
        }
    }

    struct FailingRestoreCredentialStore {
        token: Mutex<Option<String>>,
    }

    impl CredentialStore for FailingRestoreCredentialStore {
        fn save(&self, _account_id: &str, _token: &RefreshToken) -> Result<(), LauncherError> {
            Err(LauncherError::new(
                "credential_unavailable",
                "Windows Credential Manager is unavailable.",
                None,
                true,
            ))
        }

        fn get(&self, _account_id: &str) -> Result<Option<RefreshToken>, LauncherError> {
            Ok(self
                .token
                .lock()
                .expect("credential lock")
                .clone()
                .map(RefreshToken::new))
        }

        fn delete(&self, _account_id: &str) -> Result<(), LauncherError> {
            self.token.lock().expect("credential lock").take();
            Ok(())
        }
    }

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
            let service = AccountService::new(
                Arc::new(storage),
                credentials.clone(),
                Arc::new(AccountMutationCoordinator::default()),
            );

            service.remove_account(&account.id).await.expect("removes");

            assert!(service.list_accounts().await.expect("accounts").is_empty());
            assert!(credentials
                .get(&account.id)
                .expect("credential reads")
                .is_none());
        });
    }

    #[test]
    fn failed_credential_compensation_returns_sanitized_inconsistency_error() {
        tauri::async_runtime::block_on(async {
            let account = AccountSummary {
                id: "stable-account-id".to_owned(),
                minecraft_name: "Player".to_owned(),
                minecraft_uuid: "minecraft-uuid".to_owned(),
                head_url: None,
                is_active: true,
            };
            let storage = Arc::new(FailingDeleteAccountStore {
                account: account.clone(),
            });
            let credentials = Arc::new(FailingRestoreCredentialStore {
                token: Mutex::new(Some("refresh-secret".to_owned())),
            });
            let service = AccountService::new(
                storage.clone(),
                credentials.clone(),
                Arc::new(AccountMutationCoordinator::default()),
            );

            let error = service
                .remove_account(&account.id)
                .await
                .expect_err("failed compensation is reported");
            let serialized = serde_json::to_string(&error).expect("error serializes");

            assert_eq!(error.code(), "account_state_inconsistent");
            assert!(!serialized.contains("refresh-secret"));
            assert_eq!(
                storage.list_accounts().await.expect("accounts"),
                vec![account]
            );
            assert!(credentials
                .get("stable-account-id")
                .expect("credential reads")
                .is_none());
        });
    }

    #[test]
    fn switching_active_account_waits_for_the_shared_mutation_coordinator() {
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
            let mutations = Arc::new(AccountMutationCoordinator::default());
            let held = mutations.lock().await;
            let service = Arc::new(AccountService::new(
                storage,
                Arc::new(InMemoryCredentialStore::default()),
                mutations.clone(),
            ));
            let mut switching = tauri::async_runtime::spawn({
                let service = service.clone();
                async move { service.set_active_account("stable-account-id").await }
            });

            assert!(
                tokio::time::timeout(Duration::from_millis(50), &mut switching)
                    .await
                    .is_err()
            );
            drop(held);
            switching
                .await
                .expect("switch task")
                .expect("switch proceeds after shared lock releases");
        });
    }
}

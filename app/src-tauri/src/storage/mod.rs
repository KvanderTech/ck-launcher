pub mod credentials;

use crate::error::LauncherError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, Row, SqlitePool};
use std::{fs, path::Path, str::FromStr};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherProfile {
    pub id: String,
    pub name: String,
    pub version_id: Option<String>,
    pub memory_mb: u32,
    pub game_dir: String,
    pub java_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub minecraft_name: String,
    pub minecraft_uuid: String,
    pub head_url: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildSummary {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub game_dir: String,
    pub icon_url: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledContent {
    pub id: String,
    pub build_id: String,
    pub project_id: String,
    pub version_id: String,
    pub project_type: String,
    pub title: String,
    pub filename: String,
    pub icon_url: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OfflineSkin {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub file_path: String,
    pub is_active: bool,
}

#[async_trait]
pub trait AccountStore: Send + Sync {
    async fn upsert_account(&self, account: &AccountSummary) -> Result<(), LauncherError>;
    async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError>;
    async fn set_active_account(&self, account_id: &str) -> Result<(), LauncherError>;
    async fn delete_account(&self, account_id: &str) -> Result<(), LauncherError>;
}

#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn upsert_profile(&self, profile: &LauncherProfile) -> Result<(), LauncherError>;
    async fn active_profile(&self) -> Result<Option<LauncherProfile>, LauncherError>;
    async fn update_active_profile_memory(
        &self,
        memory_mb: u32,
    ) -> Result<Option<LauncherProfile>, LauncherError>;
}

#[derive(Default)]
pub struct AccountMutationCoordinator {
    mutation: tokio::sync::Mutex<()>,
}

impl AccountMutationCoordinator {
    pub(crate) async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation.lock().await
    }
}

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self, LauncherError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|_| LauncherError::storage_unavailable())?
            .create_if_missing(true);
        Self::connect_options(options).await
    }

    pub async fn connect_file(database: &Path) -> Result<Self, LauncherError> {
        let parent = database
            .parent()
            .ok_or_else(LauncherError::storage_unavailable)?;
        fs::create_dir_all(parent).map_err(|_| LauncherError::storage_unavailable())?;
        let options = SqliteConnectOptions::new()
            .filename(database)
            .create_if_missing(true);
        Self::connect_options(options).await
    }

    async fn connect_options(options: SqliteConnectOptions) -> Result<Self, LauncherError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;

        MIGRATOR
            .run(&pool)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;

        Ok(Self { pool })
    }

    pub async fn upsert_profile(&self, profile: &LauncherProfile) -> Result<(), LauncherError> {
        sqlx::query(
            "INSERT INTO profiles (id, name, version_id, memory_mb, game_dir, java_override) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
               name = excluded.name, \
               version_id = excluded.version_id, \
               memory_mb = excluded.memory_mb, \
               game_dir = excluded.game_dir, \
               java_override = excluded.java_override",
        )
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(&profile.version_id)
        .bind(i64::from(profile.memory_mb))
        .bind(&profile.game_dir)
        .bind(&profile.java_override)
        .execute(&self.pool)
        .await
        .map_err(|_| LauncherError::storage_unavailable())?;

        Ok(())
    }

    pub async fn active_profile(&self) -> Result<Option<LauncherProfile>, LauncherError> {
        let row = sqlx::query(
            "SELECT id, name, version_id, memory_mb, game_dir, java_override \
             FROM profiles \
             ORDER BY CASE id WHEN 'default' THEN 0 ELSE 1 END, id \
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| LauncherError::storage_unavailable())?;

        row.map(profile_from_row).transpose()
    }

    pub async fn update_active_profile_memory(
        &self,
        memory_mb: u32,
    ) -> Result<Option<LauncherProfile>, LauncherError> {
        let row = sqlx::query(
            "UPDATE profiles SET memory_mb = ? \
             WHERE id = (\
               SELECT id FROM profiles \
               ORDER BY CASE id WHEN 'default' THEN 0 ELSE 1 END, id \
               LIMIT 1\
             ) \
             RETURNING id, name, version_id, memory_mb, game_dir, java_override",
        )
        .bind(i64::from(memory_mb))
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| LauncherError::storage_unavailable())?;

        row.map(profile_from_row).transpose()
    }

    pub async fn upsert_account(&self, account: &AccountSummary) -> Result<(), LauncherError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        if account.is_active {
            sqlx::query("UPDATE accounts SET is_active = 0")
                .execute(&mut *transaction)
                .await
                .map_err(|_| LauncherError::storage_unavailable())?;
        }
        sqlx::query(
            "INSERT INTO accounts (id, minecraft_name, minecraft_uuid, head_url, is_active) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
               minecraft_name = excluded.minecraft_name, \
               minecraft_uuid = excluded.minecraft_uuid, \
               head_url = excluded.head_url, \
               is_active = excluded.is_active",
        )
        .bind(&account.id)
        .bind(&account.minecraft_name)
        .bind(&account.minecraft_uuid)
        .bind(&account.head_url)
        .bind(account.is_active)
        .execute(&mut *transaction)
        .await
        .map_err(|_| LauncherError::storage_unavailable())?;
        transaction
            .commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
        let rows = sqlx::query(
            "SELECT id, minecraft_name, minecraft_uuid, head_url, is_active \
             FROM accounts ORDER BY is_active DESC, minecraft_name, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| LauncherError::storage_unavailable())?;

        rows.into_iter().map(account_from_row).collect()
    }

    pub async fn set_active_account(&self, account_id: &str) -> Result<(), LauncherError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("UPDATE accounts SET is_active = 0")
            .execute(&mut *transaction)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        let updated = sqlx::query("UPDATE accounts SET is_active = 1 WHERE id = ?")
            .bind(account_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        if updated.rows_affected() != 1 {
            return Err(LauncherError::new(
                "account_not_found",
                "The selected Minecraft account was not found.",
                None,
                false,
            ));
        }
        transaction
            .commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn delete_account(&self, account_id: &str) -> Result<(), LauncherError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        let was_active =
            sqlx::query_scalar::<_, bool>("SELECT is_active FROM accounts WHERE id = ?")
                .bind(account_id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|_| LauncherError::storage_unavailable())?
                .unwrap_or(false);
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(account_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        if was_active {
            sqlx::query(
                "UPDATE accounts SET is_active = 1 WHERE id = \
                 (SELECT id FROM accounts ORDER BY minecraft_name, id LIMIT 1)",
            )
            .execute(&mut *transaction)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        }
        transaction
            .commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn set_installation_state(
        &self,
        version_id: &str,
        state: &str,
    ) -> Result<(), LauncherError> {
        let verified_at = (state == "verified").then(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string()
        });
        sqlx::query(
            "INSERT INTO installations (version_id, state, verified_at) VALUES (?, ?, ?) \
             ON CONFLICT(version_id) DO UPDATE SET state = excluded.state, verified_at = excluded.verified_at",
        )
        .bind(version_id)
        .bind(state)
        .bind(verified_at)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn installation_state(
        &self,
        version_id: &str,
    ) -> Result<Option<String>, LauncherError> {
        sqlx::query_scalar("SELECT state FROM installations WHERE version_id = ?")
            .bind(version_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn list_builds(&self) -> Result<Vec<BuildSummary>, LauncherError> {
        let rows = sqlx::query("SELECT id, name, game_version, loader, loader_version, game_dir, icon_url, is_active FROM builds ORDER BY is_active DESC, created_at DESC")
            .fetch_all(&self.pool).await.map_err(|_| LauncherError::storage_unavailable())?;
        rows.into_iter().map(build_from_row).collect()
    }

    pub async fn upsert_build(&self, build: &BuildSummary) -> Result<(), LauncherError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        if build.is_active {
            sqlx::query("UPDATE builds SET is_active = 0")
                .execute(&mut *tx)
                .await
                .map_err(|_| LauncherError::storage_unavailable())?;
        }
        sqlx::query("INSERT INTO builds (id,name,game_version,loader,loader_version,game_dir,icon_url,is_active,created_at) VALUES (?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,game_version=excluded.game_version,loader=excluded.loader,loader_version=excluded.loader_version,game_dir=excluded.game_dir,icon_url=excluded.icon_url,is_active=excluded.is_active")
            .bind(&build.id).bind(&build.name).bind(&build.game_version).bind(&build.loader).bind(&build.loader_version).bind(&build.game_dir).bind(&build.icon_url).bind(build.is_active).bind(now_timestamp())
            .execute(&mut *tx).await.map_err(|_| LauncherError::storage_unavailable())?;
        tx.commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn select_build(&self, build_id: &str) -> Result<BuildSummary, LauncherError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        let row = sqlx::query("SELECT id,name,game_version,loader,loader_version,game_dir,icon_url,is_active FROM builds WHERE id=?")
            .bind(build_id).fetch_optional(&mut *tx).await.map_err(|_| LauncherError::storage_unavailable())?
            .ok_or_else(|| LauncherError::new("build_not_found", "The selected build was not found.", None, true))?;
        let mut build = build_from_row(row)?;
        sqlx::query("UPDATE builds SET is_active=0")
            .execute(&mut *tx)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("UPDATE builds SET is_active=1 WHERE id=?")
            .bind(build_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("UPDATE profiles SET name=?, version_id=?, game_dir=? WHERE id='default'")
            .bind(&build.name)
            .bind(&build.game_version)
            .bind(&build.game_dir)
            .execute(&mut *tx)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        tx.commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        build.is_active = true;
        Ok(build)
    }

    pub async fn delete_build(&self, build_id: &str) -> Result<(), LauncherError> {
        sqlx::query("DELETE FROM builds WHERE id=?")
            .bind(build_id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn list_installed_content(
        &self,
        build_id: &str,
    ) -> Result<Vec<InstalledContent>, LauncherError> {
        let rows = sqlx::query("SELECT id,build_id,project_id,version_id,project_type,title,filename,icon_url,enabled FROM installed_content WHERE build_id=? ORDER BY installed_at DESC")
            .bind(build_id).fetch_all(&self.pool).await.map_err(|_| LauncherError::storage_unavailable())?;
        rows.into_iter().map(content_from_row).collect()
    }

    pub async fn upsert_installed_content(
        &self,
        item: &InstalledContent,
    ) -> Result<(), LauncherError> {
        sqlx::query("INSERT INTO installed_content (id,build_id,project_id,version_id,project_type,title,filename,icon_url,enabled,installed_at) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(build_id,project_id) DO UPDATE SET version_id=excluded.version_id,project_type=excluded.project_type,title=excluded.title,filename=excluded.filename,icon_url=excluded.icon_url,enabled=excluded.enabled,installed_at=excluded.installed_at")
            .bind(&item.id).bind(&item.build_id).bind(&item.project_id).bind(&item.version_id).bind(&item.project_type).bind(&item.title).bind(&item.filename).bind(&item.icon_url).bind(item.enabled).bind(now_timestamp())
            .execute(&self.pool).await.map(|_| ()).map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn remove_installed_content(
        &self,
        build_id: &str,
        project_id: &str,
    ) -> Result<Option<InstalledContent>, LauncherError> {
        let row = sqlx::query("DELETE FROM installed_content WHERE build_id=? AND project_id=? RETURNING id,build_id,project_id,version_id,project_type,title,filename,icon_url,enabled")
            .bind(build_id).bind(project_id).fetch_optional(&self.pool).await.map_err(|_| LauncherError::storage_unavailable())?;
        row.map(content_from_row).transpose()
    }

    pub async fn list_offline_skins(
        &self,
        account_id: &str,
    ) -> Result<Vec<OfflineSkin>, LauncherError> {
        let rows = sqlx::query("SELECT id,account_id,name,file_path,is_active FROM offline_skins WHERE account_id=? ORDER BY is_active DESC,created_at DESC")
            .bind(account_id).fetch_all(&self.pool).await.map_err(|_| LauncherError::storage_unavailable())?;
        rows.into_iter().map(skin_from_row).collect()
    }

    pub async fn add_offline_skin(&self, skin: &OfflineSkin) -> Result<(), LauncherError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("UPDATE offline_skins SET is_active=0 WHERE account_id=?")
            .bind(&skin.account_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("INSERT INTO offline_skins (id,account_id,name,file_path,is_active,created_at) VALUES (?,?,?,?,1,?)")
            .bind(&skin.id).bind(&skin.account_id).bind(&skin.name).bind(&skin.file_path).bind(now_timestamp()).execute(&mut *tx).await.map_err(|_| LauncherError::storage_unavailable())?;
        tx.commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }

    pub async fn select_offline_skin(
        &self,
        account_id: &str,
        skin_id: &str,
    ) -> Result<(), LauncherError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        sqlx::query("UPDATE offline_skins SET is_active=0 WHERE account_id=?")
            .bind(account_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| LauncherError::storage_unavailable())?;
        let changed =
            sqlx::query("UPDATE offline_skins SET is_active=1 WHERE account_id=? AND id=?")
                .bind(account_id)
                .bind(skin_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| LauncherError::storage_unavailable())?;
        if changed.rows_affected() != 1 {
            return Err(LauncherError::new(
                "skin_not_found",
                "The selected skin was not found.",
                None,
                true,
            ));
        }
        tx.commit()
            .await
            .map_err(|_| LauncherError::storage_unavailable())
    }
}

fn now_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn build_from_row(row: sqlx::sqlite::SqliteRow) -> Result<BuildSummary, LauncherError> {
    Ok(BuildSummary {
        id: row
            .try_get("id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        name: row
            .try_get("name")
            .map_err(|_| LauncherError::storage_unavailable())?,
        game_version: row
            .try_get("game_version")
            .map_err(|_| LauncherError::storage_unavailable())?,
        loader: row
            .try_get("loader")
            .map_err(|_| LauncherError::storage_unavailable())?,
        loader_version: row
            .try_get("loader_version")
            .map_err(|_| LauncherError::storage_unavailable())?,
        game_dir: row
            .try_get("game_dir")
            .map_err(|_| LauncherError::storage_unavailable())?,
        icon_url: row
            .try_get("icon_url")
            .map_err(|_| LauncherError::storage_unavailable())?,
        is_active: row
            .try_get("is_active")
            .map_err(|_| LauncherError::storage_unavailable())?,
    })
}

fn content_from_row(row: sqlx::sqlite::SqliteRow) -> Result<InstalledContent, LauncherError> {
    Ok(InstalledContent {
        id: row
            .try_get("id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        build_id: row
            .try_get("build_id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        project_id: row
            .try_get("project_id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        version_id: row
            .try_get("version_id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        project_type: row
            .try_get("project_type")
            .map_err(|_| LauncherError::storage_unavailable())?,
        title: row
            .try_get("title")
            .map_err(|_| LauncherError::storage_unavailable())?,
        filename: row
            .try_get("filename")
            .map_err(|_| LauncherError::storage_unavailable())?,
        icon_url: row
            .try_get("icon_url")
            .map_err(|_| LauncherError::storage_unavailable())?,
        enabled: row
            .try_get("enabled")
            .map_err(|_| LauncherError::storage_unavailable())?,
    })
}

fn skin_from_row(row: sqlx::sqlite::SqliteRow) -> Result<OfflineSkin, LauncherError> {
    Ok(OfflineSkin {
        id: row
            .try_get("id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        account_id: row
            .try_get("account_id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        name: row
            .try_get("name")
            .map_err(|_| LauncherError::storage_unavailable())?,
        file_path: row
            .try_get("file_path")
            .map_err(|_| LauncherError::storage_unavailable())?,
        is_active: row
            .try_get("is_active")
            .map_err(|_| LauncherError::storage_unavailable())?,
    })
}

#[async_trait]
impl AccountStore for Storage {
    async fn upsert_account(&self, account: &AccountSummary) -> Result<(), LauncherError> {
        Storage::upsert_account(self, account).await
    }

    async fn list_accounts(&self) -> Result<Vec<AccountSummary>, LauncherError> {
        Storage::list_accounts(self).await
    }

    async fn set_active_account(&self, account_id: &str) -> Result<(), LauncherError> {
        Storage::set_active_account(self, account_id).await
    }

    async fn delete_account(&self, account_id: &str) -> Result<(), LauncherError> {
        Storage::delete_account(self, account_id).await
    }
}

#[async_trait]
impl ProfileStore for Storage {
    async fn upsert_profile(&self, profile: &LauncherProfile) -> Result<(), LauncherError> {
        Storage::upsert_profile(self, profile).await
    }

    async fn active_profile(&self) -> Result<Option<LauncherProfile>, LauncherError> {
        Storage::active_profile(self).await
    }

    async fn update_active_profile_memory(
        &self,
        memory_mb: u32,
    ) -> Result<Option<LauncherProfile>, LauncherError> {
        Storage::update_active_profile_memory(self, memory_mb).await
    }
}

fn profile_from_row(row: sqlx::sqlite::SqliteRow) -> Result<LauncherProfile, LauncherError> {
    let memory_mb = row
        .try_get::<i64, _>("memory_mb")
        .map_err(|_| LauncherError::storage_unavailable())?;

    Ok(LauncherProfile {
        id: row
            .try_get("id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        name: row
            .try_get("name")
            .map_err(|_| LauncherError::storage_unavailable())?,
        version_id: row
            .try_get("version_id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        memory_mb: u32::try_from(memory_mb).map_err(|_| LauncherError::storage_unavailable())?,
        game_dir: row
            .try_get("game_dir")
            .map_err(|_| LauncherError::storage_unavailable())?,
        java_override: row
            .try_get("java_override")
            .map_err(|_| LauncherError::storage_unavailable())?,
    })
}

fn account_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AccountSummary, LauncherError> {
    Ok(AccountSummary {
        id: row
            .try_get("id")
            .map_err(|_| LauncherError::storage_unavailable())?,
        minecraft_name: row
            .try_get("minecraft_name")
            .map_err(|_| LauncherError::storage_unavailable())?,
        minecraft_uuid: row
            .try_get("minecraft_uuid")
            .map_err(|_| LauncherError::storage_unavailable())?,
        head_url: row
            .try_get("head_url")
            .map_err(|_| LauncherError::storage_unavailable())?,
        is_active: row
            .try_get("is_active")
            .map_err(|_| LauncherError::storage_unavailable())?,
    })
}

#[cfg(test)]
mod tests {
    use super::{AccountSummary, LauncherProfile, Storage};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn account(id: &str, name: &str, active: bool) -> AccountSummary {
        AccountSummary {
            id: id.to_owned(),
            minecraft_name: name.to_owned(),
            minecraft_uuid: format!("uuid-{id}"),
            head_url: Some(format!("https://example.test/{id}.png")),
            is_active: active,
        }
    }

    #[test]
    fn file_storage_creates_a_clean_profile_parent_database_and_migrations() {
        tauri::async_runtime::block_on(async {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("ck-launcher-clean-storage-{unique}"));
            let database = root.join("nested/launcher.sqlite3");

            let storage = Storage::connect_file(&database)
                .await
                .expect("clean profile storage initializes");

            assert!(database.is_file());
            assert!(storage
                .active_profile()
                .await
                .expect("migration query")
                .is_none());
            drop(storage);
            fs::remove_dir_all(root).expect("cleanup");
        });
    }

    #[test]
    fn migrations_persist_and_read_the_default_profile() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:")
                .await
                .expect("in-memory storage migrates");
            let profile = LauncherProfile {
                id: "default".to_owned(),
                name: "Default".to_owned(),
                version_id: None,
                memory_mb: 4096,
                game_dir: "game".to_owned(),
                java_override: None,
            };

            storage
                .upsert_profile(&profile)
                .await
                .expect("default profile is persisted");
            let active_profile = storage
                .active_profile()
                .await
                .expect("active profile is read")
                .expect("default profile exists");

            assert_eq!(active_profile.id, "default");
            assert_eq!(active_profile.memory_mb, 4096);
        });
    }

    #[test]
    fn database_connection_errors_use_a_stable_sanitized_error() {
        tauri::async_runtime::block_on(async {
            let database_url = "not-a-database-url://access_token=top-secret";
            let error = match Storage::connect(database_url).await {
                Err(error) => error,
                Ok(_) => panic!("invalid database URL is rejected"),
            };
            let serialized = serde_json::to_string(&error).expect("error serializes");

            assert_eq!(error.code(), "storage_unavailable");
            assert!(!serialized.contains("top-secret"));
            assert!(!serialized.contains(database_url));
        });
    }

    #[test]
    fn switching_accounts_transactionally_leaves_exactly_one_active() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:").await.expect("storage");
            storage
                .upsert_account(&account("one", "One", true))
                .await
                .expect("first");
            storage
                .upsert_account(&account("two", "Two", true))
                .await
                .expect("second");

            storage.set_active_account("one").await.expect("switches");
            let accounts = storage.list_accounts().await.expect("lists");
            assert_eq!(
                accounts.iter().filter(|account| account.is_active).count(),
                1
            );
            assert!(
                accounts
                    .iter()
                    .find(|account| account.id == "one")
                    .expect("one")
                    .is_active
            );
        });
    }

    #[test]
    fn deleting_active_account_removes_the_row_and_activates_one_remaining_account() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:").await.expect("storage");
            storage
                .upsert_account(&account("one", "One", false))
                .await
                .expect("first");
            storage
                .upsert_account(&account("two", "Two", true))
                .await
                .expect("second");

            storage.delete_account("two").await.expect("deletes");
            let accounts = storage.list_accounts().await.expect("lists");
            assert_eq!(accounts, vec![account("one", "One", true)]);
        });
    }

    #[test]
    fn installation_state_round_trips_and_only_verified_has_a_timestamp() {
        tauri::async_runtime::block_on(async {
            let storage = Storage::connect("sqlite::memory:").await.expect("storage");
            storage
                .set_installation_state("1.21.6", "failed")
                .await
                .expect("failed state");
            assert_eq!(
                storage
                    .installation_state("1.21.6")
                    .await
                    .expect("state")
                    .as_deref(),
                Some("failed")
            );
            storage
                .set_installation_state("1.21.6", "verified")
                .await
                .expect("verified state");
            assert_eq!(
                storage
                    .installation_state("1.21.6")
                    .await
                    .expect("state")
                    .as_deref(),
                Some("verified")
            );
            let timestamp: Option<String> =
                sqlx::query_scalar("SELECT verified_at FROM installations WHERE version_id = ?")
                    .bind("1.21.6")
                    .fetch_one(&storage.pool)
                    .await
                    .expect("timestamp");
            assert!(timestamp.is_some());
        });
    }
}

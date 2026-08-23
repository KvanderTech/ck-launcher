use crate::error::LauncherError;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, Row, SqlitePool};
use std::str::FromStr;

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

impl Storage {
    pub async fn connect(database_url: &str) -> Result<Self, LauncherError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|_| LauncherError::storage_unavailable())?
            .create_if_missing(true);
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

#[cfg(test)]
mod tests {
    use super::{LauncherProfile, Storage};

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
}

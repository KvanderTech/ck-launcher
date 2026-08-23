use crate::{
    error::LauncherError,
    storage::{LauncherProfile, ProfileStore},
};
use serde::Serialize;
use std::sync::Arc;

const MEMORY_STEP_MB: u32 = 512;
const MAX_MEMORY_MB: u64 = 32_768;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettingsStatus {
    pub memory_mb: u32,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub step_memory_mb: u32,
}

pub fn clamp_memory(requested_mb: u32, physical_mb: u64) -> u32 {
    let safe_max = ((physical_mb.saturating_mul(3) / 4).min(MAX_MEMORY_MB)
        / u64::from(MEMORY_STEP_MB))
    .saturating_mul(u64::from(MEMORY_STEP_MB))
    .max(u64::from(MEMORY_STEP_MB));
    let requested = u64::from(requested_mb.max(MEMORY_STEP_MB));
    ((requested.min(safe_max) / u64::from(MEMORY_STEP_MB)) * u64::from(MEMORY_STEP_MB)) as u32
}

pub trait PhysicalMemory: Send + Sync {
    fn physical_memory_mb(&self) -> u64;
}
pub struct SystemPhysicalMemory;
impl PhysicalMemory for SystemPhysicalMemory {
    fn physical_memory_mb(&self) -> u64 {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::SystemInformation::{
                GlobalMemoryStatusEx, MEMORYSTATUSEX,
            };
            let mut status = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            if GlobalMemoryStatusEx(&mut status) != 0 {
                return status.ullTotalPhys / (1024 * 1024);
            }
        }
        4_096
    }
}

pub struct ProfileService {
    storage: Arc<dyn ProfileStore>,
    memory: Arc<dyn PhysicalMemory>,
    default_game_dir: String,
}
impl ProfileService {
    pub fn new(
        storage: Arc<dyn ProfileStore>,
        memory: Arc<dyn PhysicalMemory>,
        default_game_dir: impl Into<String>,
    ) -> Self {
        Self {
            storage,
            memory,
            default_game_dir: default_game_dir.into(),
        }
    }
    pub async fn get_profile(&self) -> Result<LauncherProfile, LauncherError> {
        match self.storage.active_profile().await? {
            Some(profile) => Ok(profile),
            None => {
                let profile = LauncherProfile {
                    id: "default".to_owned(),
                    name: "Default".to_owned(),
                    version_id: None,
                    memory_mb: clamp_memory(4_096, self.memory.physical_memory_mb()),
                    game_dir: self.default_game_dir.clone(),
                    java_override: None,
                };
                self.storage.upsert_profile(&profile).await?;
                Ok(profile)
            }
        }
    }
    pub async fn update_profile(
        &self,
        mut profile: LauncherProfile,
    ) -> Result<LauncherProfile, LauncherError> {
        if profile.id.trim().is_empty()
            || profile.name.trim().is_empty()
            || profile.game_dir.trim().is_empty()
        {
            return Err(LauncherError::new(
                "invalid_profile",
                "Profile name and game directory are required.",
                None,
                false,
            ));
        }
        profile.memory_mb = clamp_memory(profile.memory_mb, self.memory.physical_memory_mb());
        self.storage.upsert_profile(&profile).await?;
        Ok(profile)
    }

    pub async fn update_memory(&self, memory_mb: u32) -> Result<LauncherProfile, LauncherError> {
        let mut profile = self.get_profile().await?;
        profile.memory_mb = memory_mb;
        self.update_profile(profile).await
    }

    pub async fn memory_status(&self) -> Result<MemorySettingsStatus, LauncherError> {
        let profile = self.get_profile().await?;
        let physical_mb = self.memory.physical_memory_mb();
        Ok(MemorySettingsStatus {
            memory_mb: clamp_memory(profile.memory_mb, physical_mb),
            min_memory_mb: MEMORY_STEP_MB,
            max_memory_mb: clamp_memory(u32::MAX, physical_mb),
            step_memory_mb: MEMORY_STEP_MB,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_memory, PhysicalMemory, ProfileService};
    use crate::storage::{LauncherProfile, Storage};
    use std::sync::Arc;
    struct FixedMemory(u64);
    impl PhysicalMemory for FixedMemory {
        fn physical_memory_mb(&self) -> u64 {
            self.0
        }
    }

    #[test]
    fn clamps_requested_memory_to_the_safe_512_mb_boundary() {
        assert_eq!(clamp_memory(4_096, 16_384), 4_096);
        assert_eq!(clamp_memory(20_000, 16_384), 12_288);
        assert_eq!(clamp_memory(128, 16_384), 512);
        assert_eq!(clamp_memory(2_049, 16_384), 2_048);
        assert_eq!(clamp_memory(2_048, 512), 512);
    }
    #[test]
    fn profile_updates_round_trip_with_injected_physical_memory() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let service = ProfileService::new(storage, Arc::new(FixedMemory(16_384)), "game");
            let saved = service
                .update_profile(LauncherProfile {
                    id: "default".to_owned(),
                    name: "Player".to_owned(),
                    version_id: Some("1.21.6".to_owned()),
                    memory_mb: 20_000,
                    game_dir: "game".to_owned(),
                    java_override: None,
                })
                .await
                .expect("profile saves");
            assert_eq!(saved.memory_mb, 12_288);
            assert_eq!(service.get_profile().await.expect("profile reads"), saved);
        });
    }
    #[test]
    fn profile_command_boundary_rejects_blank_name_without_host_memory() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let service = ProfileService::new(storage, Arc::new(FixedMemory(16_384)), "game");
            let error = service
                .update_profile(LauncherProfile {
                    id: "default".to_owned(),
                    name: " ".to_owned(),
                    version_id: None,
                    memory_mb: 4096,
                    game_dir: "game".to_owned(),
                    java_override: None,
                })
                .await
                .expect_err("blank profile name is rejected");
            assert_eq!(error.code(), "invalid_profile");
        });
    }

    #[test]
    fn memory_status_exposes_backend_clamped_limits_and_exact_step() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let service = ProfileService::new(storage, Arc::new(FixedMemory(16_384)), "game");

            let status = service.memory_status().await.expect("memory status");

            assert_eq!(status.memory_mb, 4_096);
            assert_eq!(status.min_memory_mb, 512);
            assert_eq!(status.max_memory_mb, 12_288);
            assert_eq!(status.step_memory_mb, 512);
        });
    }

    #[test]
    fn memory_update_preserves_the_latest_profile_fields() {
        tauri::async_runtime::block_on(async {
            let storage = Arc::new(Storage::connect("sqlite::memory:").await.expect("storage"));
            let service = ProfileService::new(storage, Arc::new(FixedMemory(16_384)), "game");
            service
                .update_profile(LauncherProfile {
                    id: "default".to_owned(),
                    name: "Player".to_owned(),
                    version_id: Some("1.21.8".to_owned()),
                    memory_mb: 4_096,
                    game_dir: "game".to_owned(),
                    java_override: None,
                })
                .await
                .expect("profile seeds");

            let saved = service.update_memory(20_000).await.expect("memory saves");

            assert_eq!(saved.version_id.as_deref(), Some("1.21.8"));
            assert_eq!(saved.memory_mb, 12_288);
        });
    }
}

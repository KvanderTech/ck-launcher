#![allow(linker_messages)]

pub mod auth;
pub mod commands;
pub mod downloads;
pub mod error;
pub mod installer;
pub mod launcher;
pub mod metadata;
mod orchestration;
pub mod paths;
pub mod profiles;
pub mod runtime;
pub mod storage;
mod webview2;

use paths::AppPaths;
use std::sync::Arc;
use storage::{credentials::WindowsCredentialStore, AccountMutationCoordinator, Storage};
use tauri::{Emitter, Manager};
use webview2::{
    check_availability, missing_runtime_instruction, show_missing_runtime_instruction,
    WindowsWebView2Registry,
};

#[tauri::command]
fn open_external_url(url: String) -> Result<(), error::LauncherError> {
    const ALLOWED: [&str; 3] = [
        "https://t.me/comfortcentr",
        "https://discord.gg/2CkZsVN8nm",
        "https://github.com/KvanderTech/ck-launcher",
    ];
    if !ALLOWED.contains(&url.as_str()) {
        return Err(error::LauncherError::new(
            "external_url_denied",
            "Эта ссылка не разрешена.",
            None,
            false,
        ));
    }
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()
        .map_err(|_| {
            error::LauncherError::new(
                "browser_open_failed",
                "Не удалось открыть системный браузер.",
                None,
                true,
            )
        })?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let webview2_availability = check_availability(&WindowsWebView2Registry);

    if let Some(instruction) = missing_runtime_instruction(webview2_availability) {
        show_missing_runtime_instruction(instruction);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            if let Some(path) = args
                .into_iter()
                .find(|arg| arg.to_ascii_lowercase().ends_with(".mrpack"))
            {
                app.state::<commands::content::PendingMrpackPath>()
                    .replace(path.clone());
                let _ = app.emit("launcher://open-mrpack", path);
            }
        }))
        .manage(webview2_availability)
        .invoke_handler(tauri::generate_handler![
            open_external_url,
            commands::accounts::list_accounts,
            commands::accounts::begin_microsoft_login,
            commands::accounts::cancel_microsoft_login,
            commands::accounts::remove_account,
            commands::accounts::set_active_account,
            commands::content::search_modrinth,
            commands::content::modrinth_project,
            commands::content::modrinth_project_versions,
            commands::content::create_build,
            commands::content::list_builds,
            commands::content::repair_build,
            commands::content::select_build,
            commands::content::rename_build,
            commands::content::choose_build_icon,
            commands::content::delete_build,
            commands::content::install_modrinth_project,
            commands::content::install_modrinth_modpack,
            commands::content::import_mrpack,
            commands::content::pending_mrpack_path,
            commands::content::list_installed_content,
            commands::content::remove_installed_content,
            commands::content::set_installed_content_enabled,
            commands::content::import_local_content,
            commands::content::open_build_folder,
            commands::content::list_build_files,
            commands::content::list_build_worlds,
            commands::content::list_build_logs,
            commands::content::read_build_log,
            commands::content::open_build_path,
            commands::content::list_offline_skins,
            commands::content::add_offline_skin,
            commands::content::delete_offline_skin,
            commands::content::select_offline_skin,
            commands::content::minecraft_cosmetics,
            commands::content::apply_minecraft_skin,
            commands::content::activate_minecraft_cape,
            commands::versions::list_game_versions,
            commands::versions::required_java_for_version,
            commands::versions::get_profile,
            commands::versions::update_profile,
            commands::versions::choose_game_directory,
            commands::versions::update_profile_memory,
            commands::versions::memory_status,
            commands::runtime::runtime_statuses,
            commands::runtime::detect_runtime,
            commands::runtime::install_runtime,
            commands::runtime::choose_runtime_path,
            commands::install::install_version,
            commands::install::cancel_operation,
            commands::install::installation_status,
            commands::launch::launch_or_install,
            commands::launch::launch_status,
            commands::launch::stop_game,
            commands::launch::read_latest_game_log,
            commands::launch::open_latest_game_log,
        ])
        .setup(|app| {
            let pending_mrpack =
                std::env::args().find(|arg| arg.to_ascii_lowercase().ends_with(".mrpack"));
            app.manage(commands::content::PendingMrpackPath(std::sync::Mutex::new(
                pending_mrpack,
            )));
            let paths = AppPaths::windows_default()?;
            paths.create_directories()?;
            let storage = tauri::async_runtime::block_on(Storage::connect_file(&paths.database))?;
            let credentials: Arc<dyn storage::credentials::CredentialStore> =
                Arc::new(WindowsCredentialStore);
            let mutations = Arc::new(AccountMutationCoordinator::default());
            let auth = Arc::new(auth::AuthService::production(
                storage.clone(),
                credentials.clone(),
                mutations.clone(),
            )?);
            let accounts = commands::accounts::AccountService::new(
                Arc::new(storage.clone()),
                credentials,
                mutations,
            );
            let metadata = Arc::new(metadata::resolver::MetadataService::production(
                paths.root.join("metadata-cache"),
            )?);
            let content = commands::content::ContentService::new(
                paths.clone(),
                storage.clone(),
                metadata.clone(),
            )?;
            let physical_memory: Arc<dyn profiles::PhysicalMemory> =
                Arc::new(profiles::SystemPhysicalMemory);
            let profiles = profiles::ProfileService::new(
                Arc::new(storage.clone()),
                physical_memory.clone(),
                paths.game.to_string_lossy(),
            );
            let runtimes = Arc::new(runtime::RuntimeManager::production(paths.runtime.clone())?);
            let downloads = Arc::new(downloads::DownloadService::new(paths.game.clone())?);
            let installer = installer::Installer::production(
                &paths,
                metadata.clone(),
                downloads,
                Arc::new(storage.clone()),
            )?;
            let operations = installer::OperationRegistry::default();
            let launch_context = Arc::new(launcher::ProductionLaunchContext::new(
                auth.clone(),
                Arc::new(storage.clone()),
                metadata.clone(),
                runtimes.clone(),
                physical_memory.clone(),
            ));
            let launcher = launcher::Launcher::production(
                launch_context,
                commands::launch::TauriGameEventSink::new(app.handle().clone(), operations.clone()),
                paths.logs.clone(),
            );
            let workflow_backend = Arc::new(orchestration::ProductionWorkflowBackend::new(
                auth.clone(),
                Arc::new(storage.clone()),
                metadata.clone(),
                runtimes.clone(),
                Arc::new(installer.clone()),
                Arc::new(launcher.clone()),
                physical_memory,
            ));
            let orchestrator = orchestration::LaunchOrchestrator::new(
                workflow_backend,
                operations.clone(),
                commands::launch::TauriWorkflowEventSink::new(app.handle().clone()),
            );

            app.manage(paths);
            app.manage(storage);
            app.manage(auth);
            app.manage(accounts);
            app.manage(metadata);
            app.manage(content);
            app.manage(profiles);
            app.manage(runtimes);
            app.manage(installer);
            app.manage(operations);
            app.manage(launcher);
            app.manage(orchestrator);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use crate::webview2::{
        check_availability, missing_runtime_instruction, WebView2Availability, WebView2Registry,
    };

    struct FakeRegistry {
        versions: Vec<Option<String>>,
    }

    impl WebView2Registry for FakeRegistry {
        fn runtime_versions(&self) -> Vec<Option<String>> {
            self.versions.clone()
        }
    }

    #[test]
    fn reports_available_when_a_registry_location_has_a_runtime_version() {
        let registry = FakeRegistry {
            versions: vec![None, Some("136.0.3240.92".to_owned())],
        };

        assert_eq!(
            check_availability(&registry),
            WebView2Availability::Available
        );
    }

    #[test]
    fn reports_missing_when_registry_locations_have_no_valid_runtime_version() {
        let registry = FakeRegistry {
            versions: vec![Some("0.0.0.0".to_owned()), Some(String::new())],
        };

        assert_eq!(check_availability(&registry), WebView2Availability::Missing);
    }

    #[test]
    fn provides_a_user_instruction_only_when_webview2_is_missing() {
        assert!(missing_runtime_instruction(WebView2Availability::Missing).is_some());
        assert!(missing_runtime_instruction(WebView2Availability::Available).is_none());
    }
}

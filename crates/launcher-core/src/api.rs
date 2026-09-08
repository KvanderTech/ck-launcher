//! Versioned, typed transport adapter shared by Qt and the transitional web UI.
use crate::{
    commands, context::AppContext, error::LauncherError, runtime::JavaRequirement,
    storage::LauncherProfile,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
fn argument<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, LauncherError> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null)).map_err(|_| {
        LauncherError::new(
            "invalid_request",
            format!("Некорректный параметр: {key}"),
            None,
            false,
        )
    })
}
fn result(value: impl Serialize) -> Result<Value, LauncherError> {
    serde_json::to_value(value).map_err(|e| LauncherError::internal(e.to_string()))
}
pub async fn dispatch(
    ctx: &AppContext,
    method: &str,
    params: Value,
) -> Result<Value, LauncherError> {
    if !params.is_object() {
        return Err(LauncherError::new(
            "invalid_request",
            "Параметры должны быть объектом.",
            None,
            false,
        ));
    }
    let mutating = matches!(
        method,
        "create_build"
            | "repair_build"
            | "select_build"
            | "rename_build"
            | "choose_build_icon"
            | "delete_build"
            | "install_modrinth_project"
            | "install_modrinth_modpack"
            | "confirm_mrpack"
            | "remove_installed_content"
            | "set_installed_content_enabled"
            | "import_local_content"
            | "update_profile"
            | "choose_game_directory"
            | "update_profile_memory"
            | "install_version"
            | "launch_or_install"
            | "install_runtime"
            | "choose_runtime_path"
            | "install_update"
    );
    let _guard = if mutating {
        let guard = ctx.mutation.try_lock().map_err(|_| busy())?;
        if ctx.operations.has_active() {
            return Err(busy());
        }
        ctx.content.reset_cancellation();
        Some(guard)
    } else {
        None
    };
    match method {
        "load_public_image" => {
            result(crate::images::load_public_image(argument(&params, "url")?).await?)
        }
        "check_update" => result(crate::updates::check_update().await?),
        "install_update" => result(
            crate::updates::install_update(
                argument::<String>(&params, "assetUrl")?,
                argument::<String>(&params, "signatureUrl")?,
                argument::<String>(&params, "installDir")?,
                argument::<u32>(&params, "launcherPid")?,
            )
            .await?,
        ),
        "open_release_page" => {
            let url: String = argument(&params, "url")?;
            crate::updates::validate_release_url(&url)?;
            result(crate::platform::open_browser(&url)?)
        }
        "preview_mrpack" => result(
            commands::content::preview_mrpack(argument(&params, "sourcePath")?, &ctx.content)
                .await?,
        ),
        "confirm_mrpack" => result(
            ctx.content
                .confirm_mrpack(argument(&params, "sha256")?)
                .await?,
        ),
        "cancel_content_operation" => {
            ctx.content.cancel();
            result(())
        }
        "hello" => result(
            serde_json::json!({"protocolVersion":1,"version":env!("CARGO_PKG_VERSION"),"ui":"qt-widgets"}),
        ),
        "open_external_url" => result(crate::platform::open_external_url(argument(
            &params, "url",
        )?)?),
        "list_accounts" => result(commands::accounts::list_accounts(&ctx.accounts).await?),
        "begin_microsoft_login" => {
            result(commands::accounts::begin_microsoft_login(&ctx.auth).await?)
        }
        "cancel_microsoft_login" => {
            result(commands::accounts::cancel_microsoft_login(&ctx.auth).await?)
        }
        "remove_account" => result(
            commands::accounts::remove_account(
                argument::<String>(&params, "accountId")?,
                &ctx.accounts,
            )
            .await?,
        ),
        "set_active_account" => result(
            commands::accounts::set_active_account(
                argument::<String>(&params, "accountId")?,
                &ctx.accounts,
            )
            .await?,
        ),
        "search_modrinth" => result(
            commands::content::search_modrinth(
                argument::<String>(&params, "query")?,
                argument::<String>(&params, "projectType")?,
                argument::<Option<String>>(&params, "gameVersion")?,
                argument::<Option<String>>(&params, "loader")?,
                argument::<Option<String>>(&params, "category")?,
                argument::<Option<String>>(&params, "environment")?,
                argument::<Option<String>>(&params, "index")?,
                argument::<u32>(&params, "offset")?,
                &ctx.content,
            )
            .await?,
        ),
        "modrinth_project" => result(
            commands::content::modrinth_project(
                argument::<String>(&params, "projectId")?,
                &ctx.content,
            )
            .await?,
        ),
        "modrinth_project_versions" => result(
            commands::content::modrinth_project_versions(
                argument::<String>(&params, "projectId")?,
                &ctx.content,
            )
            .await?,
        ),
        "create_build" => result(
            commands::content::create_build(
                argument::<String>(&params, "name")?,
                argument::<String>(&params, "gameVersion")?,
                argument::<String>(&params, "loader")?,
                &ctx.content,
            )
            .await?,
        ),
        "list_builds" => result(commands::content::list_builds(&ctx.storage).await?),
        "repair_build" => result(
            commands::content::repair_build(argument::<String>(&params, "buildId")?, &ctx.content)
                .await?,
        ),
        "select_build" => result(
            commands::content::select_build(argument::<String>(&params, "buildId")?, &ctx.storage)
                .await?,
        ),
        "rename_build" => result(
            commands::content::rename_build(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "name")?,
                &ctx.storage,
            )
            .await?,
        ),
        "choose_build_icon" => result(
            commands::content::choose_build_icon(
                argument::<String>(&params, "buildId")?,
                &ctx.storage,
            )
            .await?,
        ),
        "delete_build" => result(
            commands::content::delete_build(argument::<String>(&params, "buildId")?, &ctx.content)
                .await?,
        ),
        "install_modrinth_project" => result(
            commands::content::install_modrinth_project(
                argument::<String>(&params, "projectId")?,
                argument::<String>(&params, "buildId")?,
                argument::<Option<String>>(&params, "versionId")?,
                &ctx.content,
            )
            .await?,
        ),
        "install_modrinth_modpack" => result(
            commands::content::install_modrinth_modpack(
                argument::<String>(&params, "projectId")?,
                argument::<Option<String>>(&params, "versionId")?,
                &ctx.content,
            )
            .await?,
        ),
        "import_mrpack" => result(
            commands::content::import_mrpack(
                argument::<Option<String>>(&params, "sourcePath")?,
                &ctx.content,
            )
            .await?,
        ),
        "pending_mrpack_path" => result(commands::content::pending_mrpack_path(&ctx.pending)),
        "list_installed_content" => result(
            commands::content::list_installed_content(
                argument::<String>(&params, "buildId")?,
                &ctx.content,
            )
            .await?,
        ),
        "remove_installed_content" => result(
            commands::content::remove_installed_content(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "projectId")?,
                &ctx.storage,
            )
            .await?,
        ),
        "set_installed_content_enabled" => result(
            commands::content::set_installed_content_enabled(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "projectId")?,
                argument::<bool>(&params, "enabled")?,
                &ctx.storage,
            )
            .await?,
        ),
        "import_local_content" => result(
            commands::content::import_local_content(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "projectType")?,
                &ctx.storage,
            )
            .await?,
        ),
        "open_build_folder" => result(
            commands::content::open_build_folder(
                argument::<String>(&params, "buildId")?,
                &ctx.storage,
            )
            .await?,
        ),
        "list_build_files" => result(
            commands::content::list_build_files(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "relativePath")?,
                &ctx.storage,
            )
            .await?,
        ),
        "list_build_worlds" => result(
            commands::content::list_build_worlds(
                argument::<String>(&params, "buildId")?,
                &ctx.storage,
            )
            .await?,
        ),
        "list_build_logs" => result(
            commands::content::list_build_logs(
                argument::<String>(&params, "buildId")?,
                &ctx.storage,
            )
            .await?,
        ),
        "read_build_log" => result(
            commands::content::read_build_log(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "relativePath")?,
                &ctx.storage,
            )
            .await?,
        ),
        "open_build_path" => result(
            commands::content::open_build_path(
                argument::<String>(&params, "buildId")?,
                argument::<String>(&params, "relativePath")?,
                &ctx.storage,
            )
            .await?,
        ),
        "list_offline_skins" => result(
            commands::content::list_offline_skins(
                argument::<String>(&params, "accountId")?,
                &ctx.content,
            )
            .await?,
        ),
        "add_offline_skin" => result(
            commands::content::add_offline_skin(
                argument::<String>(&params, "accountId")?,
                &ctx.content,
            )
            .await?,
        ),
        "delete_offline_skin" => result(
            commands::content::delete_offline_skin(
                argument::<String>(&params, "accountId")?,
                argument::<String>(&params, "skinId")?,
                &ctx.content,
            )
            .await?,
        ),
        "rename_offline_skin" => result(
            commands::content::rename_offline_skin(
                argument::<String>(&params, "accountId")?,
                argument::<String>(&params, "skinId")?,
                argument::<String>(&params, "name")?,
                &ctx.content,
            )
            .await?,
        ),
        "set_offline_skin_favorite" => result(
            commands::content::set_offline_skin_favorite(
                argument::<String>(&params, "accountId")?,
                argument::<String>(&params, "skinId")?,
                argument::<bool>(&params, "isFavorite")?,
                &ctx.content,
            )
            .await?,
        ),
        "minecraft_cosmetics" => result(
            commands::content::minecraft_cosmetics(
                argument::<String>(&params, "accountId")?,
                &ctx.auth,
            )
            .await?,
        ),
        "apply_minecraft_skin" => result(
            commands::content::apply_minecraft_skin(
                argument::<String>(&params, "accountId")?,
                argument::<String>(&params, "skinId")?,
                argument::<String>(&params, "variant")?,
                &ctx.content,
                &ctx.auth,
            )
            .await?,
        ),
        "activate_minecraft_cape" => result(
            commands::content::activate_minecraft_cape(
                argument::<String>(&params, "accountId")?,
                argument::<Option<String>>(&params, "capeId")?,
                &ctx.auth,
            )
            .await?,
        ),
        "select_offline_skin" => result(
            commands::content::select_offline_skin(
                argument::<String>(&params, "accountId")?,
                argument::<String>(&params, "skinId")?,
                &ctx.content,
            )
            .await?,
        ),
        "install_version" => result(
            commands::install::install_version(
                argument::<String>(&params, "versionId")?,
                ctx.events.clone(),
                &ctx.installer,
                &ctx.profiles,
                &ctx.operations,
            )
            .await?,
        ),
        "cancel_operation" => result(
            commands::install::cancel_operation(
                argument::<String>(&params, "operationId")?,
                &ctx.operations,
            )
            .await?,
        ),
        "installation_status" => result(
            commands::install::installation_status(
                argument::<String>(&params, "operationId")?,
                &ctx.operations,
            )
            .await?,
        ),
        "launch_or_install" => result(
            commands::launch::launch_or_install(
                argument::<String>(&params, "profileId")?,
                &ctx.orchestrator,
            )
            .await?,
        ),
        "launch_status" => result(
            commands::launch::launch_status(
                argument::<String>(&params, "operationId")?,
                &ctx.launcher,
            )
            .await?,
        ),
        "stop_game" => result(
            commands::launch::stop_game(argument::<String>(&params, "operationId")?, &ctx.launcher)
                .await?,
        ),
        "open_latest_game_log" => result(commands::launch::open_latest_game_log(&ctx.paths).await?),
        "read_latest_game_log" => result(commands::launch::read_latest_game_log(&ctx.paths).await?),
        "runtime_statuses" => result(commands::runtime::runtime_statuses(&ctx.runtimes).await?),
        "detect_runtime" => result(
            commands::runtime::detect_runtime(
                argument::<JavaRequirement>(&params, "requirement")?,
                &ctx.runtimes,
            )
            .await?,
        ),
        "install_runtime" => result(
            commands::runtime::install_runtime(
                argument::<JavaRequirement>(&params, "requirement")?,
                &ctx.runtimes,
            )
            .await?,
        ),
        "choose_runtime_path" => result(
            commands::runtime::choose_runtime_path(
                argument::<JavaRequirement>(&params, "requirement")?,
                &ctx.runtimes,
            )
            .await?,
        ),
        "list_game_versions" => {
            result(commands::versions::list_game_versions(&ctx.metadata).await?)
        }
        "required_java_for_version" => result(
            commands::versions::required_java_for_version(
                argument::<String>(&params, "versionId")?,
                &ctx.metadata,
            )
            .await?,
        ),
        "get_profile" => result(commands::versions::get_profile(&ctx.profiles).await?),
        "update_profile" => result(
            commands::versions::update_profile(
                argument::<LauncherProfile>(&params, "profile")?,
                &ctx.profiles,
            )
            .await?,
        ),
        "choose_game_directory" => {
            result(commands::versions::choose_game_directory(&ctx.profiles).await?)
        }
        "update_profile_memory" => result(
            commands::versions::update_profile_memory(
                argument::<u32>(&params, "memoryMb")?,
                &ctx.profiles,
            )
            .await?,
        ),
        "memory_status" => result(commands::versions::memory_status(&ctx.profiles).await?),
        _ => Err(LauncherError::new(
            "unknown_method",
            "Неизвестная команда лаунчера.",
            None,
            false,
        )),
    }
}

fn busy() -> LauncherError {
    LauncherError::new(
        "operation_in_progress",
        "Дождитесь завершения операции или остановите игру перед изменением сборки.",
        None,
        true,
    )
}

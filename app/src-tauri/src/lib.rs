#![allow(linker_messages)]
mod webview2;
use ck_launcher_core::{api, context::AppContext, events::EventBus, paths::AppPaths};
use tauri::{Emitter, Manager};
#[tauri::command]
async fn core_request(
    method: String,
    params: serde_json::Value,
    context: tauri::State<'_, AppContext>,
) -> Result<serde_json::Value, ck_launcher_core::error::LauncherError> {
    api::dispatch(&context, &method, params).await
}
pub fn run() {
    let availability = webview2::check_availability(&webview2::WindowsWebView2Registry);
    if let Some(instruction) = webview2::missing_runtime_instruction(availability) {
        webview2::show_missing_runtime_instruction(instruction);
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            if let Some(path) = args
                .into_iter()
                .find(|p| p.to_ascii_lowercase().ends_with(".mrpack"))
            {
                if let Some(context) = app.try_state::<AppContext>() {
                    context.pending.replace(path.clone());
                    let _ = app.emit("launcher://open-mrpack", path);
                }
            }
        }))
        .invoke_handler(tauri::generate_handler![core_request])
        .setup(|app| {
            let handle = app.handle().clone();
            let events = EventBus::new(move |event, data| {
                let _ = handle.emit(event, data);
            });
            let ctx = ck_launcher_core::tasks::block_on(AppContext::new(
                AppPaths::windows_default()?,
                events,
            ))?;
            if let Some(path) =
                std::env::args().find(|p| p.to_ascii_lowercase().ends_with(".mrpack"))
            {
                ctx.pending.replace(path);
            }
            app.manage(ctx);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Could not start CK Launcher");
}

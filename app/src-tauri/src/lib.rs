#![allow(linker_messages)]

pub mod error;
pub mod paths;
pub mod storage;
mod webview2;

use paths::AppPaths;
use storage::Storage;
use tauri::Manager;
use webview2::{
    check_availability, missing_runtime_instruction, show_missing_runtime_instruction,
    WindowsWebView2Registry,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let webview2_availability = check_availability(&WindowsWebView2Registry);

    if let Some(instruction) = missing_runtime_instruction(webview2_availability) {
        show_missing_runtime_instruction(instruction);
    }

    tauri::Builder::default()
        .manage(webview2_availability)
        .setup(|app| {
            let paths = AppPaths::windows_default()?;
            paths.create_directories()?;
            let database_url = paths.database_url()?;
            let storage = tauri::async_runtime::block_on(Storage::connect(&database_url))?;

            app.manage(paths);
            app.manage(storage);
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

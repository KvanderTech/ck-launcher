#![allow(linker_messages)]

mod webview2;

use tauri::Manager;
use webview2::{check_availability, WebView2Availability, WindowsWebView2Registry};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let webview2_availability = check_availability(&WindowsWebView2Registry);

    tauri::Builder::default()
        .manage(webview2_availability)
        .setup(|app| {
            if *app.state::<WebView2Availability>() == WebView2Availability::Missing {
                eprintln!(
                    "Microsoft Edge WebView2 Runtime не найден. Установите Evergreen Runtime для продолжения."
                );
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use crate::webview2::{check_availability, WebView2Availability, WebView2Registry};

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
}

use winreg::{RegKey, HKEY};

const WEBVIEW2_CLIENT_KEY: &str =
    r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
const WEBVIEW2_CURRENT_USER_CLIENT_KEY: &str =
    r"Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebView2Availability {
    Available,
    Missing,
}

pub trait WebView2Registry {
    fn runtime_versions(&self) -> Vec<Option<String>>;
}

pub struct WindowsWebView2Registry;

impl WebView2Registry for WindowsWebView2Registry {
    fn runtime_versions(&self) -> Vec<Option<String>> {
        vec![
            read_runtime_version(winreg::enums::HKEY_LOCAL_MACHINE, WEBVIEW2_CLIENT_KEY),
            read_runtime_version(
                winreg::enums::HKEY_CURRENT_USER,
                WEBVIEW2_CURRENT_USER_CLIENT_KEY,
            ),
        ]
    }
}

pub fn check_availability(registry: &impl WebView2Registry) -> WebView2Availability {
    if registry.runtime_versions().iter().flatten().any(|version| {
        let version = version.trim();
        !version.is_empty() && version != "0.0.0.0"
    }) {
        WebView2Availability::Available
    } else {
        WebView2Availability::Missing
    }
}

fn read_runtime_version(hive: HKEY, path: &str) -> Option<String> {
    RegKey::predef(hive)
        .open_subkey(path)
        .ok()?
        .get_value("pv")
        .ok()
}

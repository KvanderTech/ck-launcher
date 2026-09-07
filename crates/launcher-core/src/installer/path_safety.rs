use std::path::{Component, Path};

pub(super) fn is_strict_windows_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| {
            let Component::Normal(component) = component else {
                return false;
            };
            component.to_str().is_some_and(is_strict_windows_component)
        })
}

fn is_strict_windows_component(component: &str) -> bool {
    if component.is_empty()
        || component.ends_with(['.', ' '])
        || component.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }
    let lowercase = component.to_ascii_lowercase();
    if lowercase.ends_with(".part") || lowercase.ends_with(".part.lock") {
        return false;
    }
    let device = lowercase
        .split('.')
        .next()
        .unwrap_or(&lowercase)
        .trim_end_matches(['.', ' ']);
    !matches!(
        device,
        "con" | "prn" | "aux" | "nul" | "clock$" | "conin$" | "conout$"
    ) && !numbered_device(device, "com")
        && !numbered_device(device, "lpt")
}

fn numbered_device(component: &str, prefix: &str) -> bool {
    component
        .strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']))
}

use crate::{
    error::LauncherError,
    installer::libraries::{rules_allowed, WindowsRuleContext},
    metadata::models::Argument,
};
use serde_json::Value;
use std::collections::BTreeMap;

const WINDOWS_OS_NAME_JVM: &str = "-Dos.name=Windows 10";
const WINDOWS_OS_VERSION_JVM: &str = "-Dos.version=10.0";

pub(super) fn resolve_modern(
    arguments: &[Argument],
    variables: &BTreeMap<&str, String>,
) -> Result<Vec<String>, LauncherError> {
    let context = WindowsRuleContext::default();
    let mut resolved = Vec::new();
    for argument in arguments {
        match argument {
            Argument::Literal(value) => resolved.push(substitute(value, variables)?),
            Argument::Conditional { rules, value } if rules_allowed(rules, &context)? => {
                for value in argument_values(value)? {
                    resolved.push(substitute(value, variables)?);
                }
            }
            Argument::Conditional { .. } => {}
        }
    }
    Ok(resolved)
}

pub(super) fn resolve_legacy(
    arguments: &str,
    variables: &BTreeMap<&str, String>,
) -> Result<Vec<String>, LauncherError> {
    tokenize_legacy(arguments)?
        .into_iter()
        .map(|value| substitute(&value, variables))
        .collect()
}

pub(super) fn safe_metadata_jvm(
    arguments: Vec<String>,
    expected_classpath: &str,
    expected_natives: &str,
    expected_launcher_name: &str,
    expected_launcher_version: &str,
) -> Result<Vec<String>, LauncherError> {
    let owned_native_path = format!("-Djava.library.path={expected_natives}");
    let allowed = [
        "-XstartOnFirstThread".to_owned(),
        "-Xss1M".to_owned(),
        "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"
            .to_owned(),
        format!("-Djna.tmpdir={expected_natives}"),
        format!("-Dorg.lwjgl.system.SharedLibraryExtractPath={expected_natives}"),
        format!("-Dio.netty.native.workdir={expected_natives}"),
        format!("-Dminecraft.launcher.brand={expected_launcher_name}"),
        format!("-Dminecraft.launcher.version={expected_launcher_version}"),
    ];
    let mut safe = Vec::new();
    let mut requests_windows_os_name = false;
    let mut requests_windows_os_version = false;
    let mut index = 0;
    while index < arguments.len() {
        let current = &arguments[index];
        if matches!(current.as_str(), "-cp" | "-classpath" | "--class-path") {
            let Some(value) = arguments.get(index + 1) else {
                return Err(argument_invalid());
            };
            if matches!(value.as_bytes().first(), Some(b'@' | b'-')) || value != expected_classpath
            {
                return Err(unsafe_argument());
            }
            index += 2;
            continue;
        }
        if current == &owned_native_path {
            index += 1;
            continue;
        }
        if current == WINDOWS_OS_NAME_JVM {
            requests_windows_os_name = true;
            index += 1;
            continue;
        }
        if current == WINDOWS_OS_VERSION_JVM {
            requests_windows_os_version = true;
            index += 1;
            continue;
        }
        if !allowed.contains(current) {
            return Err(unsafe_argument());
        }
        safe.push(current.clone());
        index += 1;
    }
    if requests_windows_os_name {
        safe.push(WINDOWS_OS_NAME_JVM.to_owned());
    }
    if requests_windows_os_version {
        safe.push(WINDOWS_OS_VERSION_JVM.to_owned());
    }
    Ok(safe)
}

fn argument_values(value: &Value) -> Result<Vec<&str>, LauncherError> {
    match value {
        Value::String(value) => Ok(vec![value]),
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().ok_or_else(argument_invalid))
            .collect(),
        _ => Err(argument_invalid()),
    }
}

fn substitute(template: &str, variables: &BTreeMap<&str, String>) -> Result<String, LauncherError> {
    let mut result = template.to_owned();
    for (placeholder, value) in variables {
        result = result.replace(&format!("${{{placeholder}}}"), value);
    }
    if result.contains("${") {
        return Err(argument_invalid());
    }
    if result.chars().any(|character| character == '\0') {
        return Err(argument_invalid());
    }
    Ok(result)
}

fn tokenize_legacy(input: &str) -> Result<Vec<String>, LauncherError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escape = false;
    for character in input.chars() {
        if escape {
            current.push(character);
            escape = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escape = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if quote.is_some() || escape {
        return Err(argument_invalid());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn argument_invalid() -> LauncherError {
    LauncherError::new(
        "launch_argument_invalid",
        "Minecraft launch metadata contains an unsupported argument.",
        None,
        false,
    )
}

fn unsafe_argument() -> LauncherError {
    LauncherError::new(
        "unsafe_launch_argument",
        "Minecraft launch metadata attempted to override a protected launcher setting.",
        None,
        false,
    )
}

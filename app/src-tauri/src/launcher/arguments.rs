use crate::{
    error::LauncherError,
    installer::libraries::{rules_allowed, WindowsRuleContext},
    metadata::models::Argument,
};
use serde_json::Value;
use std::collections::BTreeMap;

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
) -> Result<Vec<String>, LauncherError> {
    let mut safe = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let current = &arguments[index];
        if matches!(current.as_str(), "-cp" | "-classpath" | "--class-path") {
            let Some(value) = arguments.get(index + 1) else {
                return Err(argument_invalid());
            };
            if value != expected_classpath {
                return Err(unsafe_argument());
            }
            index += 2;
            continue;
        }
        if current.starts_with("-Djava.library.path=") {
            index += 1;
            continue;
        }
        if is_unsafe_jvm_argument(current) {
            return Err(unsafe_argument());
        }
        safe.push(current.clone());
        index += 1;
    }
    Ok(safe)
}

fn is_unsafe_jvm_argument(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("-xmx")
        || lower.starts_with("-xms")
        || lower.starts_with("-javaagent")
        || lower.starts_with("-agentlib")
        || lower.starts_with("-agentpath")
        || lower.starts_with("-xbootclasspath")
        || lower.starts_with("--module-path")
        || lower == "-p"
        || lower.starts_with("-djavax.net.ssl.truststore")
        || lower.starts_with("-djava.security")
        || lower.starts_with("-dlog4j.configuration")
        || lower.starts_with("-dlog4j2.configuration")
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

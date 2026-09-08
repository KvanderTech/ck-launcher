//! Extra JVM options needed by Forge, without relaxing the vanilla argument policy.
use crate::{error::LauncherError, paths::AppPaths};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn unsafe_argument() -> LauncherError {
    LauncherError::new(
        "unsafe_launch_argument",
        "Параметры Forge содержат небезопасный или неподдерживаемый аргумент Java.",
        None,
        false,
    )
}
fn filename_list(value: &str) -> bool {
    value.len() <= 4096
        && value.split(',').all(|name| {
            !name.is_empty()
                && name.len() < 256
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}
pub(super) fn extract_options(
    arguments: &mut Vec<String>,
    root: &Path,
    classpath: &[PathBuf],
    library_directory: &str,
    expected_classpath: &str,
) -> Result<Vec<String>, LauncherError> {
    let library_root = root
        .join("libraries")
        .canonicalize()
        .map_err(|_| unsafe_argument())?;
    let approved: HashSet<_> = classpath
        .iter()
        .map(|p| p.canonicalize())
        .collect::<Result<_, _>>()
        .map_err(|_| unsafe_argument())?;
    let mut remaining = Vec::new();
    let mut forge = Vec::new();
    let mut input = std::mem::take(arguments).into_iter();
    while let Some(current) = input.next() {
        match current.as_str() {
            "-p" | "--module-path" => {
                let paths = input.next().ok_or_else(unsafe_argument)?;
                if paths.len() > 32768 {
                    return Err(unsafe_argument());
                }
                let mut modules = HashSet::new();
                for path in paths.split(';') {
                    if path.is_empty() || path.contains(['*', '@']) {
                        return Err(unsafe_argument());
                    }
                    let path = Path::new(path);
                    let canonical = path.canonicalize().map_err(|_| unsafe_argument())?;
                    if !canonical.starts_with(&library_root)
                        || !approved.contains(&canonical)
                        || !modules.insert(canonical.clone())
                    {
                        return Err(unsafe_argument());
                    }
                    let relative = canonical
                        .strip_prefix(root.canonicalize().map_err(|_| unsafe_argument())?)
                        .map_err(|_| unsafe_argument())?;
                    AppPaths::new(root.to_owned()).safe_join(root, relative)?;
                }
                forge.extend([current, paths]);
            }
            "--add-modules" | "--add-opens" | "--add-exports" => {
                let value = input.next().ok_or_else(unsafe_argument)?;
                let permitted = match current.as_str() {
                    "--add-modules" => value == "ALL-MODULE-PATH",
                    "--add-opens" => [
                        "java.base/java.util.jar=cpw.mods.securejarhandler",
                        "java.base/java.lang.invoke=cpw.mods.securejarhandler",
                    ]
                    .contains(&value.as_str()),
                    _ => [
                        "java.base/sun.security.util=cpw.mods.securejarhandler",
                        "jdk.naming.dns/com.sun.jndi.dns=java.naming",
                    ]
                    .contains(&value.as_str()),
                };
                if !permitted {
                    return Err(unsafe_argument());
                }
                forge.extend([current, value]);
            }
            "-Djava.net.preferIPv6Addresses=system" => forge.push(current),
            _ if current.starts_with("-DlibraryDirectory=") => {
                if current != format!("-DlibraryDirectory={library_directory}") {
                    return Err(unsafe_argument());
                }
                forge.push(current);
            }
            _ if current.starts_with("-DlegacyClassPath=") => {
                if current != format!("-DlegacyClassPath={expected_classpath}") {
                    return Err(unsafe_argument());
                }
                forge.push(current);
            }
            _ if current.starts_with("-DignoreList=") || current.starts_with("-DmergeModules=") => {
                if !filename_list(current.split_once('=').ok_or_else(unsafe_argument)?.1) {
                    return Err(unsafe_argument());
                }
                forge.push(current);
            }
            _ => remaining.push(current),
        }
    }
    *arguments = remaining;
    Ok(forge)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_path_is_confined_to_verified_classpath_libraries() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("libraries")).unwrap();
        let module = root.path().join("libraries/bootstrap.jar");
        let other = root.path().join("other.jar");
        let rogue = root.path().join("libraries/rogue.jar");
        for file in [&module, &other, &rogue] {
            fs::write(file, b"jar").unwrap();
        }
        let libs = root.path().join("libraries").to_string_lossy().into_owned();
        let mut good = vec![
            "-p".into(),
            module.to_string_lossy().into_owned(),
            "--add-modules".into(),
            "ALL-MODULE-PATH".into(),
            "-Xss1M".into(),
        ];
        assert_eq!(
            extract_options(&mut good, root.path(), &[module.clone()], &libs, "cp")
                .unwrap()
                .len(),
            4
        );
        assert_eq!(good, ["-Xss1M"]);
        for bad in [
            other.to_string_lossy().into_owned(),
            rogue.to_string_lossy().into_owned(),
            "@args.txt".into(),
            String::new(),
        ] {
            assert!(extract_options(
                &mut vec!["-p".into(), bad],
                root.path(),
                &[module.clone()],
                &libs,
                "cp"
            )
            .is_err());
        }
        let mut malicious = vec!["-javaagent:evil.jar".into(), "-Xmx999G".into()];
        assert!(
            extract_options(&mut malicious, root.path(), &[module], &libs, "cp")
                .unwrap()
                .is_empty()
        );
        assert!(super::super::arguments::safe_metadata_jvm(
            malicious,
            "cp",
            "native",
            "CKLauncher",
            "test"
        )
        .is_err());
    }
    use std::fs;

    /// Developer-only network check. Never use a real launcher data directory:
    /// the caller must prepare an isolated instance with the public service API.
    /// Writes a DEMO launch command, without any account or production auth bypass.
    #[tokio::test]
    #[ignore = "Requires CK_FORGE_SMOKE_ROOT with an isolated, installed Forge pack and network access"]
    async fn installed_forge_builds_a_valid_demo_launch() {
        use crate::{
            context::AppContext,
            downloads::DownloadCancellationToken,
            events::EventBus,
            launcher::{build_launch, LaunchAccount, LaunchBuildRequest},
            runtime::requirement_for_version,
            storage::LauncherProfile,
        };
        let root =
            PathBuf::from(std::env::var_os("CK_FORGE_SMOKE_ROOT").expect("isolated test data"));
        let context = AppContext::new(AppPaths::new(root.clone()), EventBus::new(|_, _| {}))
            .await
            .unwrap();
        assert!(
            context.storage.list_accounts().await.unwrap().is_empty(),
            "Use isolated test data, never real accounts"
        );
        let build = context
            .storage
            .list_builds()
            .await
            .unwrap()
            .into_iter()
            .find(|build| build.loader == "forge")
            .expect("Install the test Forge pack first");
        let game_root = PathBuf::from(&build.game_dir);
        let version = context
            .metadata
            .resolved_version(&build.game_version)
            .await
            .unwrap();
        eprintln!(
            "Preparing verified Minecraft assets and libraries for {}",
            version.id
        );
        let installer = context.installer.for_game_root(game_root.clone()).unwrap();
        installer
            .install(
                "forge-network-smoke".into(),
                version.id.clone(),
                DownloadCancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(installer.is_verified_version(&version).await.unwrap());
        let runtime = context
            .runtimes
            .resolve(requirement_for_version(&version).unwrap(), None)
            .await
            .unwrap();
        let profile = LauncherProfile {
            id: "forge-demo-smoke".into(),
            name: "Forge demo smoke".into(),
            version_id: Some(version.id.clone()),
            memory_mb: 4096,
            game_dir: build.game_dir,
            java_override: None,
        };
        let mut prepared = build_launch(LaunchBuildRequest {
            account: LaunchAccount::new("CKForgeTest", "00000000000000000000000000000000", "0"),
            version,
            profile,
            runtime,
            game_root,
            physical_memory_mb: 16384,
        })
        .unwrap();
        prepared.command.args.push("--demo".into());
        let args: Vec<_> = prepared
            .command
            .args
            .iter()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        fs::write(
            root.join("forge-demo-command.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "executable": prepared.command.executable, "cwd": prepared.command.cwd, "args": args
            }))
            .unwrap(),
        )
        .unwrap();
        eprintln!("Verified Forge launch plan; demo-only command saved in the isolated test root");
    }
}

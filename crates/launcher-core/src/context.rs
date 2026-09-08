use crate::*;
use std::sync::Arc;
use storage::{credentials::WindowsCredentialStore, AccountMutationCoordinator, Storage};

pub struct AppContext {
    pub mutation: tokio::sync::Mutex<()>,
    pub paths: paths::AppPaths,
    pub storage: storage::Storage,
    pub auth: Arc<auth::AuthService>,
    pub accounts: commands::accounts::AccountService,
    pub metadata: Arc<metadata::resolver::MetadataService>,
    pub content: commands::content::ContentService,
    pub profiles: profiles::ProfileService,
    pub runtimes: Arc<runtime::RuntimeManager>,
    pub installer: installer::Installer,
    pub operations: installer::OperationRegistry,
    pub launcher: launcher::Launcher,
    pub(crate) orchestrator: orchestration::LaunchOrchestrator,
    pub pending: commands::content::PendingMrpackPath,
    pub events: events::EventBus,
}
impl AppContext {
    pub async fn new(
        paths: paths::AppPaths,
        events: events::EventBus,
    ) -> Result<Self, error::LauncherError> {
        paths.create_directories()?;
        let storage = Storage::connect_file(&paths.database).await?;
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
        let runtimes = Arc::new(
            runtime::RuntimeManager::production(paths.runtime.clone())?.with_events(events.clone()),
        );
        let content = commands::content::ContentService::new(
            paths.clone(),
            storage.clone(),
            metadata.clone(),
            runtimes.clone(),
        )?
        .with_events(events.clone());
        let physical_memory: Arc<dyn profiles::PhysicalMemory> =
            Arc::new(profiles::SystemPhysicalMemory);
        let profiles = profiles::ProfileService::new(
            Arc::new(storage.clone()),
            physical_memory.clone(),
            paths.game.to_string_lossy(),
        );
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
            commands::launch::GameEventSink::new(events.clone(), operations.clone()),
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
            commands::launch::LauncherWorkflowEventSink::new(events.clone()),
        );

        let pending = commands::content::PendingMrpackPath(std::sync::Mutex::new(None));
        Ok(Self {
            mutation: tokio::sync::Mutex::new(()),
            paths,
            storage,
            auth,
            accounts,
            metadata,
            content,
            profiles,
            runtimes,
            installer,
            operations,
            launcher,
            orchestrator,
            pending,
            events,
        })
    }
}

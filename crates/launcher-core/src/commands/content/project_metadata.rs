use super::*;
use futures_util::{stream, StreamExt};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Clone, Deserialize)]
pub(super) struct ProjectIdentity {
    id: String,
    title: String,
    icon_url: Option<String>,
}
pub(super) type IdentityCache = Arc<Mutex<HashMap<String, (Instant, Option<ProjectIdentity>)>>>;

fn project_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|c| c.is_ascii_alphanumeric())
}

impl ContentService {
    pub(super) async fn content_with_metadata(
        &self,
        build_id: &str,
    ) -> Result<Vec<InstalledContent>, LauncherError> {
        let mut items = self.storage.list_installed_content(build_id).await?;
        let ids: Vec<_> = items
            .iter()
            .filter(|item| project_id(&item.project_id) && project_id(&item.version_id))
            .map(|item| item.project_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let missing: Vec<_> = {
            let cache = self
                .project_identities
                .lock()
                .map_err(|_| LauncherError::storage_unavailable())?;
            ids.into_iter()
                .filter(|id| {
                    !cache.get(id).is_some_and(|(time, value)| {
                        time.elapsed()
                            < Duration::from_secs(if value.is_some() { 3600 } else { 60 })
                    })
                })
                .collect()
        };
        let batches: Vec<_> = missing.chunks(50).map(|chunk| chunk.to_vec()).collect();
        let mut results = stream::iter(batches.into_iter().map(|batch| async move {
            let result = async {
                let response = self
                    .client
                    .get(format!("{MODRINTH_API}/projects"))
                    .query(&[("ids", serde_json::to_string(&batch).unwrap_or_default())])
                    .timeout(Duration::from_secs(8))
                    .send()
                    .await
                    .map_err(|_| network_error())?;
                Self::response_json::<Vec<ProjectIdentity>>(response).await
            }
            .await;
            (batch, result)
        }))
        .buffer_unordered(3);
        while let Some((batch, result)) = results.next().await {
            let mut cache = self
                .project_identities
                .lock()
                .map_err(|_| LauncherError::storage_unavailable())?;
            if cache.len() > 4096 {
                cache.clear();
            }
            for id in &batch {
                cache.insert(id.clone(), (Instant::now(), None));
            }
            if let Ok(projects) = result {
                for project in projects {
                    if batch.contains(&project.id)
                        && !project.title.trim().is_empty()
                        && project.title.len() < 500
                    {
                        cache.insert(project.id.clone(), (Instant::now(), Some(project)));
                    }
                }
            }
        }
        for item in &mut items {
            let metadata = self.project_identities.lock().ok().and_then(|cache| {
                cache
                    .get(&item.project_id)
                    .and_then(|(_, value)| value.clone())
            });
            if let Some(project) = metadata {
                if item.title != project.title || item.icon_url != project.icon_url {
                    self.storage
                        .update_content_identity(
                            &item.id,
                            &item.project_id,
                            &project.title,
                            project.icon_url.as_deref(),
                        )
                        .await?;
                    item.title = project.title;
                    item.icon_url = project.icon_url;
                }
            }
        }
        Ok(items)
    }
}

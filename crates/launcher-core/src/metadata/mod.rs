pub mod models;
pub mod resolver;

#[cfg(test)]
mod tests {
    use super::resolver::MetadataService;

    #[test]
    fn stable_releases_exclude_snapshots_and_sort_newest_first() {
        let service = MetadataService::from_manifest_fixture(include_str!(
            "../tests/fixtures/version_manifest_v2.json"
        ));

        let releases =
            crate::tasks::block_on(service.stable_releases()).expect("manifest is resolved");

        assert_eq!(
            releases
                .into_iter()
                .map(|release| release.id)
                .collect::<Vec<_>>(),
            vec!["1.21.6", "1.20.4"]
        );
    }

    #[test]
    fn game_version_dto_exposes_the_documented_type_field() {
        let value = serde_json::to_value(super::models::GameVersionSummary {
            id: "1.21.6".to_owned(),
            version_type: "release".to_owned(),
            release_date: "2026-06-17T12:00:00+00:00".to_owned(),
        })
        .expect("DTO serializes");

        assert_eq!(value["type"], "release");
        assert!(value.get("versionType").is_none());
    }
}

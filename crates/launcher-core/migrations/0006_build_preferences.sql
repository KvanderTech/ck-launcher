CREATE TABLE IF NOT EXISTS build_preferences (
    build_id TEXT PRIMARY KEY REFERENCES builds(id) ON DELETE CASCADE,
    group_name TEXT NOT NULL DEFAULT '',
    java_override TEXT,
    account_id TEXT
);

INSERT OR IGNORE INTO build_preferences(build_id, java_override)
SELECT builds.id, profiles.java_override FROM builds JOIN profiles ON profiles.id='default'
WHERE builds.is_active=1 AND profiles.java_override IS NOT NULL;

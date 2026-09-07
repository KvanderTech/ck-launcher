CREATE TABLE builds (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    game_version TEXT NOT NULL,
    loader TEXT NOT NULL DEFAULT 'vanilla',
    loader_version TEXT,
    game_dir TEXT NOT NULL,
    icon_url TEXT,
    is_active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE installed_content (
    id TEXT PRIMARY KEY,
    build_id TEXT NOT NULL REFERENCES builds(id) ON DELETE CASCADE,
    project_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    project_type TEXT NOT NULL,
    title TEXT NOT NULL,
    filename TEXT NOT NULL,
    icon_url TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    installed_at TEXT NOT NULL
);

CREATE UNIQUE INDEX installed_content_build_project
ON installed_content(build_id, project_id);

CREATE TABLE offline_skins (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

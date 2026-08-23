CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    minecraft_name TEXT NOT NULL,
    minecraft_uuid TEXT NOT NULL,
    head_url TEXT,
    is_active INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version_id TEXT,
    memory_mb INTEGER NOT NULL DEFAULT 4096,
    game_dir TEXT NOT NULL,
    java_override TEXT
);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE TABLE installations (
    version_id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    verified_at TEXT
);

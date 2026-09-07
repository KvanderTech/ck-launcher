CREATE TABLE pack_sources (build_id TEXT PRIMARY KEY REFERENCES builds(id) ON DELETE CASCADE, source_json TEXT NOT NULL);

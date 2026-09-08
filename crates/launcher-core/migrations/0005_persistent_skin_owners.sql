-- A login is removable; a player's local skin library is not.
CREATE TABLE offline_skins_persistent (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    owner_uuid TEXT NOT NULL,
    name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    is_favorite INTEGER NOT NULL DEFAULT 0
);
INSERT INTO offline_skins_persistent
SELECT s.id, s.account_id, lower(replace(a.minecraft_uuid, '-', '')),
       s.name, s.file_path, s.is_active, s.created_at, s.is_favorite
FROM offline_skins s JOIN accounts a ON a.id = s.account_id;
DROP TABLE offline_skins;
ALTER TABLE offline_skins_persistent RENAME TO offline_skins;
CREATE INDEX offline_skins_owner ON offline_skins(owner_uuid);

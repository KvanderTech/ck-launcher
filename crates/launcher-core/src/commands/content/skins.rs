use super::*;
use std::io::Write;

pub(super) fn stored_path(skin: &OfflineSkin) -> Result<PathBuf, LauncherError> {
    let stored = Path::new(&skin.file_path);
    let parent = stored.parent().ok_or_else(LauncherError::invalid_path)?;
    let name = stored.file_name().ok_or_else(LauncherError::invalid_path)?;
    // Normalize directory aliases, but not the file itself: safe_destination must
    // still inspect and reject a final symlink/reparse point before any file read.
    let parent = parent
        .canonicalize()
        .map_err(|_| LauncherError::invalid_path())?;
    Ok(crate::paths::strip_verbatim_prefix(parent.join(name)))
}

pub(super) fn read_skin(path: &Path) -> Result<Vec<u8>, LauncherError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(2_000_001).read_to_end(&mut bytes))
        .map_err(|_| LauncherError::storage_unavailable())?;
    validate_skin_png(&bytes)?;
    Ok(bytes)
}

impl ContentService {
    pub(super) async fn skin_owner_key(&self, account_id: &str) -> Result<String, LauncherError> {
        let account = self
            .storage
            .list_accounts()
            .await?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or_else(|| input_error("account_not_found", "Войдите в Minecraft-аккаунт."))?;
        let uuid = account.minecraft_uuid.replace('-', "").to_ascii_lowercase();
        if uuid.len() == 32 && uuid.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(uuid)
        } else {
            // Preserve historical non-Microsoft records without ever using an unchecked path.
            Ok(format!("legacy-{:x}", Sha256::digest(uuid.as_bytes())))
        }
    }

    pub(super) async fn skin_relative(
        &self,
        account_id: &str,
        skin: &OfflineSkin,
    ) -> Result<PathBuf, LauncherError> {
        let key = self.skin_owner_key(account_id).await?;
        let new = Path::new("skins")
            .join(key)
            .join(format!("{}.png", skin.id));
        let old = Path::new("skins")
            .join(format!("{:x}", Sha256::digest(skin.account_id.as_bytes())))
            .join(format!("{}.png", skin.id));
        let stored = stored_path(skin)?;
        for relative in [new, old] {
            if security::safe_destination(&self.paths.root, &relative)? == stored {
                return Ok(relative);
            }
        }
        Err(LauncherError::invalid_path())
    }

    // Caller holds skin_mutation. FileTransaction restores the previous layout if SQLite fails.
    pub(super) async fn prepare_skin_library(&self, account_id: &str) -> Result<(), LauncherError> {
        let owner = self.skin_owner_key(account_id).await?;
        for skin in self.storage.list_offline_skins(account_id).await? {
            let old = self.skin_relative(account_id, &skin).await?;
            let destination = Path::new("skins")
                .join(&owner)
                .join(format!("{}.png", skin.id));
            if old == destination {
                continue;
            }
            let source = security::safe_destination(&self.paths.root, &old)?;
            let bytes = read_skin(&source)?;
            let path = security::safe_destination(&self.paths.root, &destination)?;
            // Never overwrite a different existing copy while migrating a library.
            if path.exists() && read_skin(&path)? != bytes {
                return Err(input_error(
                    "skin_migration_conflict",
                    "В папке скинов найдены разные копии одного скина. Исходные файлы сохранены.",
                ));
            }
            let mut staged = tempfile::NamedTempFile::new_in(&self.paths.root)
                .map_err(|_| LauncherError::storage_unavailable())?;
            staged
                .write_all(&bytes)
                .map_err(|_| LauncherError::storage_unavailable())?;
            let mut transaction = FileTransaction::new(&self.paths.root)?;
            transaction.replace(&destination, staged.path())?;
            transaction.remove(&old)?;
            self.storage
                .relocate_offline_skin(account_id, &skin.id, &path.to_string_lossy())
                .await?;
            transaction.commit();
        }
        // Pre-1.0 sign-out cascaded the database rows but left PNGs on disk. Only
        // recover files in this exact account's old directory, never another player's.
        let old_directory =
            Path::new("skins").join(format!("{:x}", Sha256::digest(account_id.as_bytes())));
        let directory = security::safe_destination(&self.paths.root, &old_directory)?;
        if !directory.is_dir() {
            return Ok(());
        }
        for entry in fs::read_dir(&directory)
            .map_err(|_| LauncherError::storage_unavailable())?
            .take(4096)
        {
            let entry = entry.map_err(|_| LauncherError::storage_unavailable())?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(id) = name.strip_suffix(".png") else {
                continue;
            };
            if !id.starts_with("skin-")
                || id.len() > 96
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                continue;
            }
            if self.storage.skin_id_exists(id).await? {
                continue;
            }
            let old = old_directory.join(name);
            let source = security::safe_destination(&self.paths.root, &old)?;
            let Ok(bytes) = read_skin(&source) else {
                continue;
            };
            let destination = Path::new("skins").join(&owner).join(name);
            let path = security::safe_destination(&self.paths.root, &destination)?;
            if path.exists() {
                continue;
            }
            let mut staged = tempfile::NamedTempFile::new_in(&self.paths.root)
                .map_err(|_| LauncherError::storage_unavailable())?;
            staged
                .write_all(&bytes)
                .map_err(|_| LauncherError::storage_unavailable())?;
            let mut transaction = FileTransaction::new(&self.paths.root)?;
            transaction.replace(&destination, staged.path())?;
            transaction.remove(&old)?;
            self.storage
                .add_offline_skin(&OfflineSkin {
                    id: id.to_owned(),
                    account_id: account_id.to_owned(),
                    name: "Восстановленный скин".to_owned(),
                    file_path: path.to_string_lossy().into_owned(),
                    is_active: false,
                    is_favorite: false,
                })
                .await?;
            transaction.commit();
        }
        Ok(())
    }

    pub(super) async fn import_skin(
        &self,
        account_id: String,
        name: String,
        bytes: &[u8],
    ) -> Result<OfflineSkinView, LauncherError> {
        validate_skin_png(bytes)?;
        let owner = self.skin_owner_key(&account_id).await?;
        let id = format!("skin-{:032x}", rand::random::<u128>());
        let relative = Path::new("skins").join(owner).join(format!("{id}.png"));
        let path = security::safe_destination(&self.paths.root, &relative)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.paths.root)
            .map_err(|_| LauncherError::storage_unavailable())?;
        staged
            .write_all(bytes)
            .map_err(|_| LauncherError::storage_unavailable())?;
        let mut transaction = FileTransaction::new(&self.paths.root)?;
        transaction.replace(&relative, staged.path())?;
        let skin = OfflineSkin {
            id,
            account_id,
            name,
            file_path: path.to_string_lossy().into_owned(),
            is_active: true,
            is_favorite: false,
        };
        self.storage.add_offline_skin(&skin).await?;
        transaction.commit();
        self.skin_view(skin)
    }
}

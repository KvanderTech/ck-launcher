use crate::error::{sanitize, LauncherError};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub fn read_tail(path: &Path) -> Result<String, LauncherError> {
    const LIMIT: u64 = 2 * 1024 * 1024;
    let mut file = File::open(path).map_err(|_| LauncherError::storage_unavailable())?;
    let length = file
        .metadata()
        .map_err(|_| LauncherError::storage_unavailable())?
        .len();
    let start = length.saturating_sub(LIMIT);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| LauncherError::storage_unavailable())?;
    let mut bytes = Vec::new();
    file.take(LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|_| LauncherError::storage_unavailable())?;
    let offset = if start > 0 {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map_or(bytes.len(), |p| p + 1)
    } else {
        0
    };
    Ok(sanitize(&String::from_utf8_lossy(&bytes[offset..])))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_log_memory_and_redacts_tokens() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        file.write_all(&vec![b'x'; 3 * 1024 * 1024]).unwrap();
        file.write_all(b"\naccess_token=secret; Bearer hidden\n")
            .unwrap();
        let text = read_tail(file.path()).unwrap();
        assert!(!text.contains("secret"));
        assert!(!text.contains("hidden"));
        assert!(text.len() < 100);
    }
}

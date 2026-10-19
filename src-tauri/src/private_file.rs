// Writing files only the current user can read, for credentials kept on disk.

use std::io::Write;
use std::path::Path;

/// Write `contents` to `path`, readable and writable by the owner only (0600)
/// on Unix. A file that already exists with wider permissions is narrowed,
/// since the mode passed at creation does not apply to an existing file.
/// Elsewhere this is a plain write.
pub fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        write_all(file, contents)
    }
    #[cfg(not(unix))]
    {
        write_all(options.open(path)?, contents)
    }
}

fn write_all(mut file: std::fs::File, contents: &[u8]) -> std::io::Result<()> {
    file.write_all(contents)?;
    file.sync_all()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_file_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        write_private(&path, b"{}").unwrap();

        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"{}");
    }

    #[test]
    fn an_existing_world_readable_file_is_narrowed_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        std::fs::write(&path, b"old and longer").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_private(&path, b"new").unwrap();

        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}

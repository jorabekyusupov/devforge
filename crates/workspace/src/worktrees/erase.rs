//! Deletion is called only after receipt/path/content review and the purging journal.
//! Never clear a Git object's attributes: local submodule objects can be hardlinked.
use std::{io, path::Path};
#[cfg(not(windows))]
pub(super) fn payload(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
#[cfg(windows)]
pub(super) fn payload(path: &Path) -> io::Result<()> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfoEx,
        SetFileInformationByHandle,
    };
    let metadata = std::fs::symlink_metadata(path)?;
    // A junction or symlink is a leaf. The opened handle also refuses reparse traversal.
    if metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        for entry in std::fs::read_dir(path)? {
            payload(&entry?.path())?;
        }
    }
    let file = match std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            #[cfg(feature = "test-support")]
            eprintln!(
                "ARCHIVE_ERASE_OPEN:{:?}:{}",
                path.file_name(),
                error.raw_os_error().unwrap_or(0)
            );
            return Err(error);
        }
    };
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: file owns a live Windows handle; the typed structure and its size match the class.
    let success = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            std::ptr::from_ref(&disposition).cast(),
            std::mem::size_of_val(&disposition) as u32,
        )
    };
    if success == 0 {
        let e = io::Error::last_os_error();
        #[cfg(feature = "test-support")]
        eprintln!(
            "ARCHIVE_ERASE_DISPOSITION:{:?}:{}",
            path.file_name(),
            e.raw_os_error().unwrap_or(0)
        );
        return Err(e);
    }
    drop(file);
    Ok(())
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "Windows-only cleanup restores an owned temp fixture attribute"
    )]
    fn removes_readonly_hardlink_without_altering_source_attributes_or_data() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source-object");
        std::fs::write(&source, b"retained source object").unwrap();
        let archive = root.path().join("retained");
        std::fs::create_dir(&archive).unwrap();
        std::fs::hard_link(&source, archive.join("git-object")).unwrap();
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        payload(&archive).unwrap();
        assert!(!archive.exists());
        assert_eq!(std::fs::read(&source).unwrap(), b"retained source object");
        assert!(std::fs::metadata(&source).unwrap().permissions().readonly());
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(source, permissions).unwrap();
    }
}

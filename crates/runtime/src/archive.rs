use crate::{
    Cancellation, Result, RuntimeError,
    package::{Archive, Limits, copy_checked},
    relative_path,
};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

struct Extractor<'a> {
    root: &'a Path,
    limits: Limits,
    cancel: &'a Cancellation,
    bytes: u64,
    names: HashSet<String>,
    links: Vec<(PathBuf, String)>,
    directories: HashSet<PathBuf>,
}
impl Extractor<'_> {
    fn name(&mut self, name: &str) -> Result<PathBuf> {
        self.cancel.check()?;
        // Archives may contain an explicit root directory.
        let relative = relative_path(name)?;
        if self.names.len() >= self.limits.entries {
            return Err(RuntimeError::LimitExceeded);
        }
        if !self
            .names
            .insert(relative.to_string_lossy().to_ascii_lowercase())
        {
            return Err(RuntimeError::UnsafeArchive);
        }
        Ok(relative)
    }
    fn file(
        &mut self,
        relative: &Path,
        size: u64,
        reader: &mut impl Read,
        mode: u32,
    ) -> Result<()> {
        if size > self.limits.unpacked_bytes.saturating_sub(self.bytes) {
            return Err(RuntimeError::LimitExceeded);
        }
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let count = copy_checked(
            reader,
            &mut output,
            self.limits.unpacked_bytes - self.bytes,
            self.cancel,
            |_| {},
        )?;
        self.bytes += count;
        if count != size {
            return Err(RuntimeError::UnsafeArchive);
        }
        output.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Strip setuid/setgid and world-write bits from publisher permissions.
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }),
            )?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        Ok(())
    }
    fn tar(&mut self, reader: impl Read) -> Result<()> {
        let mut entry_count = 0;
        let mut archive = tar::Archive::new(reader);
        for item in archive.entries().map_err(|_| RuntimeError::UnsafeArchive)? {
            entry_count += 1;
            if entry_count > self.limits.entries {
                return Err(RuntimeError::LimitExceeded);
            }
            self.cancel.check()?;
            let mut item = item.map_err(|_| RuntimeError::UnsafeArchive)?;
            let name = item
                .path()
                .map_err(|_| RuntimeError::UnsafeArchive)?
                .to_string_lossy()
                .into_owned();
            if name == "." || name == "./" {
                continue;
            }
            let kind = item.header().entry_type();
            let candidate = relative_path(&name)?;
            if kind.is_dir() && self.directories.contains(&candidate) {
                continue;
            }
            let relative = self.name(&name)?;
            if kind.is_dir() {
                self.directories.insert(relative.clone());
                fs::create_dir_all(self.root.join(relative))?;
            } else if kind.is_file() {
                let size = item.size();
                let mode = item
                    .header()
                    .mode()
                    .map_err(|_| RuntimeError::UnsafeArchive)?;
                self.file(&relative, size, &mut item, mode)?;
            } else if kind.is_symlink() {
                let target = item
                    .link_name()
                    .map_err(|_| RuntimeError::UnsafeArchive)?
                    .ok_or(RuntimeError::UnsafeArchive)?
                    .to_string_lossy()
                    .into_owned();
                self.links.push((relative, target));
            } else {
                return Err(RuntimeError::UnsafeArchive);
            }
        }
        Ok(())
    }
    fn zip(&mut self, path: &Path) -> Result<()> {
        let mut archive =
            zip::ZipArchive::new(File::open(path)?).map_err(|_| RuntimeError::UnsafeArchive)?;
        for i in 0..archive.len() {
            let mut item = archive
                .by_index(i)
                .map_err(|_| RuntimeError::UnsafeArchive)?;
            let relative = self.name(&item.name().map_err(|_| RuntimeError::UnsafeArchive)?)?;
            let mode = item.unix_mode().unwrap_or(0o644);
            if item.is_dir() {
                fs::create_dir_all(self.root.join(relative))?;
            } else if mode & 0o170000 == 0o120000 {
                if item.size() > 4096 {
                    return Err(RuntimeError::UnsafeArchive);
                }
                let mut target = String::new();
                item.take(4097)
                    .read_to_string(&mut target)
                    .map_err(|_| RuntimeError::UnsafeArchive)?;
                self.links.push((relative, target));
            } else if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 {
                return Err(RuntimeError::UnsafeArchive);
            } else {
                let size = item.size();
                self.file(&relative, size, &mut item, mode)?;
            }
        }
        Ok(())
    }
    fn links(&self) -> Result<()> {
        // Defer every link until all writes finish: no archive member can write
        // through a previously-created symlink. Targets must already resolve to
        // regular in-tree files/dirs; no dangling or escaping links.
        for (relative, target) in &self.links {
            self.cancel.check()?;
            if target.starts_with('/') || target.contains(['\\', ':', '\0']) {
                return Err(RuntimeError::UnsafeArchive);
            }
            let mut parts: Vec<_> = relative
                .parent()
                .unwrap()
                .components()
                .map(|p| p.as_os_str().to_owned())
                .collect();
            for part in target.split('/') {
                match part {
                    "" | "." => {}
                    ".." => {
                        if parts.pop().is_none() {
                            return Err(RuntimeError::UnsafeArchive);
                        }
                    }
                    _ => {
                        relative_path(part)?;
                        parts.push(part.into());
                    }
                }
            }
            let normalized: PathBuf = parts.iter().collect();
            let resolved = fs::canonicalize(self.root.join(normalized))
                .map_err(|_| RuntimeError::UnsafeArchive)?;
            if !resolved.starts_with(self.root) {
                return Err(RuntimeError::UnsafeArchive);
            }
            let link = self.root.join(relative);
            fs::create_dir_all(link.parent().unwrap())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, link)?;
            // Windows avoids symlink-creation privilege for internal file aliases.
            #[cfg(windows)]
            {
                if !resolved.is_file() {
                    return Err(RuntimeError::UnsafeArchive);
                }
                if link.exists() {
                    return Err(RuntimeError::UnsafeArchive);
                }
                fs::copy(resolved, link)?;
            }
        }
        Ok(())
    }
}
pub(crate) fn extract(
    source: &Path,
    format: &Archive,
    root: &Path,
    limits: Limits,
    cancel: &Cancellation,
) -> Result<()> {
    let mut extractor = Extractor {
        root,
        limits,
        cancel,
        bytes: 0,
        names: HashSet::new(),
        links: Vec::new(),
        directories: HashSet::new(),
    };
    match format {
        Archive::File { path } => {
            let relative = extractor.name(path)?;
            let mut file = File::open(source)?;
            extractor.file(&relative, file.metadata()?.len(), &mut file, 0o644)?;
        }
        Archive::TarGz => extractor.tar(flate2::read::GzDecoder::new(File::open(source)?))?,
        Archive::TarXz => extractor.tar(lzma_rust2::XzReader::new_mem_limit(
            File::open(source)?,
            false,
            256 * 1024,
        ))?,
        Archive::Zip => extractor.zip(source)?,
        Archive::ZipFiles { files } => {
            let mut archive = zip::ZipArchive::new(File::open(source)?)
                .map_err(|_| RuntimeError::UnsafeArchive)?;
            for (destination, member) in files {
                relative_path(member)?;
                let relative = extractor.name(destination)?;
                let mut item = archive
                    .by_name(member)
                    .map_err(|_| RuntimeError::UnsafeArchive)?;
                if item.is_dir() || item.is_symlink() {
                    return Err(RuntimeError::UnsafeArchive);
                }
                let size = item.size();
                let mode = item.unix_mode().unwrap_or(0o644);
                if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 {
                    return Err(RuntimeError::UnsafeArchive);
                }
                extractor.file(&relative, size, &mut item, mode)?;
            }
        }
        Archive::ZipTarXz { member } => {
            let mut archive = zip::ZipArchive::new(File::open(source)?)
                .map_err(|_| RuntimeError::UnsafeArchive)?;
            let mut inner = archive
                .by_name(member)
                .map_err(|_| RuntimeError::UnsafeArchive)?;
            if inner.size() > limits.download_bytes {
                return Err(RuntimeError::LimitExceeded);
            }
            let mut nested = tempfile::tempfile_in(root.parent().unwrap())?;
            copy_checked(
                &mut inner,
                &mut nested,
                limits.download_bytes,
                cancel,
                |_| {},
            )?;
            use std::io::{Seek, SeekFrom};
            nested.seek(SeekFrom::Start(0))?;
            extractor.tar(lzma_rust2::XzReader::new_mem_limit(
                nested,
                false,
                256 * 1024,
            ))?;
        }
    }
    extractor.links()?;
    cancel.check()
}

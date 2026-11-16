//! Filesystem operations for the command-line driver.
use realobfuscator::rng::Rng;
use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process,
};

/// Own the temporary path until rename succeeds. Close the handle before cleanup
/// so errors and unwinding clean up correctly on Windows as well as Unix.
struct PendingWrite {
    file: Option<File>,
    path: PathBuf,
    committed: bool,
}

impl Drop for PendingWrite {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Write beside the destination, then publish through a single rename.
/// Exclusive creation prevents following a pre-existing temporary file or symlink.
pub(super) fn atomic_write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let temporary = path.with_file_name(format!(
        ".{name}.{}.{:016x}.tmp",
        process::id(),
        Rng::random().next_u64()
    ));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let mut pending = PendingWrite {
        file: Some(file),
        path: temporary,
        committed: false,
    };
    let file = pending
        .file
        .as_mut()
        .expect("new pending write owns its file");
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(pending.file.take());
    fs::rename(&pending.path, path)?;
    pending.committed = true;
    Ok(())
}

pub(super) fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect(&path, files)?;
        } else if kind.is_file()
            && matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("lua" | "luau")
            )
            && !path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.to_ascii_lowercase().ends_with(".obf"))
        {
            files.push(path);
        }
    }
    Ok(())
}

/// Resolve missing output paths through their nearest existing ancestor as well.
pub(super) fn resolved(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.try_exists()? {
        if let Some(name) = ancestor.file_name() {
            suffix.push(name.to_owned());
        } else {
            // Deal with trailing `..` without treating it as a literal file name.
            let mut normalized = PathBuf::new();
            for component in absolute.components() {
                match component {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        normalized.pop();
                    }
                    other => normalized.push(other.as_os_str()),
                }
            }
            return resolved(&normalized);
        }
        ancestor = ancestor
            .parent()
            .ok_or_else(|| std::io::Error::other("cannot resolve output path"))?;
    }
    let mut output = fs::canonicalize(ancestor)?;
    for component in suffix.into_iter().rev() {
        output.push(component);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_publish_removes_temporary_file_and_preserves_destination() {
        let directory = env::temp_dir().join(format!(
            "realobfuscator-write-test-{}-{:016x}",
            process::id(),
            Rng::random().next_u64()
        ));
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("existing-directory");
        fs::create_dir(&destination).unwrap();
        let original = destination.join("original");
        fs::write(&original, "keep this").unwrap();
        assert!(atomic_write(&destination, "replacement").is_err());
        assert_eq!(fs::read_to_string(&original).unwrap(), "keep this");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_file(original).unwrap();
        fs::remove_dir(destination).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}

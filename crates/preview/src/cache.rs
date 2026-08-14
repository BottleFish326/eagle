use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{RwLock, RwLockReadGuard};

use tempfile::NamedTempFile;

use crate::{CacheClearReport, PreviewError};

const CACHE_DIRECTORY: &str = "thumbnails-v1";
const CACHE_MARKER: &str = ".material-eagle-thumbnail-cache";
const CACHE_MARKER_CONTENT: &str = "material-eagle-thumbnail-cache-v1\n";

#[derive(Debug)]
pub(crate) struct ThumbnailCache {
    base: PathBuf,
    root: PathBuf,
    gate: RwLock<()>,
}

impl ThumbnailCache {
    pub(crate) fn open(base: &Path) -> Result<Self, PreviewError> {
        fs::create_dir_all(base).map_err(|source| PreviewError::CacheIo {
            path: base.to_path_buf(),
            source,
        })?;
        let base = base
            .canonicalize()
            .map_err(|source| PreviewError::CacheIo {
                path: base.to_path_buf(),
                source,
            })?;
        let root = base.join(CACHE_DIRECTORY);
        if root
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(PreviewError::UnsafeCacheRoot(root));
        }
        fs::create_dir_all(&root).map_err(|source| PreviewError::CacheIo {
            path: root.clone(),
            source,
        })?;
        verify_root(&base, &root)?;
        initialize_marker(&root)?;
        Ok(Self {
            base,
            root,
            gate: RwLock::new(()),
        })
    }

    pub(crate) fn read_guard(&self) -> Result<RwLockReadGuard<'_, ()>, PreviewError> {
        self.gate
            .read()
            .map_err(|_| PreviewError::PoisonedLock("thumbnail cache"))
    }

    pub(crate) fn lookup(&self, key: &str) -> Result<Option<CacheEntry>, PreviewError> {
        let path = self.path_for(key)?;
        let Some(metadata) = entry_metadata(&path)? else {
            return Ok(None);
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PreviewError::UnsafeCacheRoot(path));
        }
        if let Ok((width, height)) = image::image_dimensions(&path) {
            Ok(Some(CacheEntry { width, height }))
        } else {
            fs::remove_file(&path).map_err(|source| PreviewError::CacheIo {
                path: path.clone(),
                source,
            })?;
            Ok(None)
        }
    }

    pub(crate) fn store(&self, key: &str, bytes: &[u8]) -> Result<PathBuf, PreviewError> {
        let path = self.path_for(key)?;
        let parent = path
            .parent()
            .ok_or_else(|| PreviewError::UnsafeCacheRoot(path.clone()))?;
        fs::create_dir_all(parent).map_err(|source| PreviewError::CacheIo {
            path: parent.to_path_buf(),
            source,
        })?;
        verify_shard(&self.root, parent)?;
        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|source| PreviewError::CacheIo {
                path: parent.to_path_buf(),
                source,
            })?;
        temporary
            .write_all(bytes)
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|source| PreviewError::CacheIo {
                path: temporary.path().to_path_buf(),
                source,
            })?;
        temporary
            .persist(&path)
            .map_err(|error| PreviewError::CacheIo {
                path: path.clone(),
                source: error.error,
            })?;
        sync_directory(parent)?;
        Ok(path)
    }

    pub(crate) fn read(&self, key: &str) -> Result<Vec<u8>, PreviewError> {
        let _guard = self.read_guard()?;
        let path = self.path_for(key)?;
        if entry_metadata(&path)?
            .is_some_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(PreviewError::UnsafeCacheRoot(path));
        }
        fs::read(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                PreviewError::MissingCacheEntry(key.to_owned())
            } else {
                PreviewError::CacheIo { path, source }
            }
        })
    }

    pub(crate) fn clear(&self) -> Result<CacheClearReport, PreviewError> {
        let _guard = self
            .gate
            .write()
            .map_err(|_| PreviewError::PoisonedLock("thumbnail cache"))?;
        verify_root(&self.base, &self.root)?;
        verify_marker(&self.root)?;
        let report = measure_cache(&self.root)?;
        fs::remove_dir_all(&self.root).map_err(|source| PreviewError::CacheIo {
            path: self.root.clone(),
            source,
        })?;
        fs::create_dir_all(&self.root).map_err(|source| PreviewError::CacheIo {
            path: self.root.clone(),
            source,
        })?;
        initialize_marker(&self.root)?;
        Ok(report)
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, PreviewError> {
        validate_key(key)?;
        let shard = self.root.join(&key[..2]);
        if entry_metadata(&shard)?
            .is_some_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_dir())
        {
            return Err(PreviewError::UnsafeCacheRoot(shard));
        }
        Ok(shard.join(format!("{key}.png")))
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Debug)]
pub(crate) struct CacheEntry {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

fn validate_key(key: &str) -> Result<(), PreviewError> {
    if key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(PreviewError::InvalidCacheKey(key.to_owned()))
    }
}

fn entry_metadata(path: &Path) -> Result<Option<fs::Metadata>, PreviewError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn verify_shard(root: &Path, shard: &Path) -> Result<(), PreviewError> {
    let metadata = shard
        .symlink_metadata()
        .map_err(|source| PreviewError::CacheIo {
            path: shard.to_path_buf(),
            source,
        })?;
    let canonical = shard
        .canonicalize()
        .map_err(|source| PreviewError::CacheIo {
            path: shard.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || canonical.parent() != Some(root) {
        return Err(PreviewError::UnsafeCacheRoot(shard.to_path_buf()));
    }
    Ok(())
}

fn verify_root(base: &Path, root: &Path) -> Result<(), PreviewError> {
    let metadata = root
        .symlink_metadata()
        .map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()));
    }
    let canonical = root
        .canonicalize()
        .map_err(|source| PreviewError::CacheIo {
            path: root.to_path_buf(),
            source,
        })?;
    if canonical.parent() != Some(base)
        || canonical
            .file_name()
            .is_none_or(|name| name != CACHE_DIRECTORY)
    {
        return Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()));
    }
    Ok(())
}

fn initialize_marker(root: &Path) -> Result<(), PreviewError> {
    let marker = root.join(CACHE_MARKER);
    if marker.exists() {
        return verify_marker(root);
    }
    fs::write(&marker, CACHE_MARKER_CONTENT).map_err(|source| PreviewError::CacheIo {
        path: marker,
        source,
    })
}

fn verify_marker(root: &Path) -> Result<(), PreviewError> {
    let marker = root.join(CACHE_MARKER);
    let contents = fs::read_to_string(&marker).map_err(|source| PreviewError::CacheIo {
        path: marker.clone(),
        source,
    })?;
    if contents == CACHE_MARKER_CONTENT {
        Ok(())
    } else {
        Err(PreviewError::UnsafeCacheRoot(root.to_path_buf()))
    }
}

fn measure_cache(root: &Path) -> Result<CacheClearReport, PreviewError> {
    let mut report = CacheClearReport::default();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| PreviewError::CacheIo {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| PreviewError::CacheIo {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| PreviewError::CacheIo {
                path: path.clone(),
                source,
            })?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push(path);
            } else if entry.file_name() != CACHE_MARKER {
                report.removed_files += 1;
                report.removed_bytes = report.removed_bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(report)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), PreviewError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| PreviewError::CacheIo {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
const fn sync_directory(_path: &Path) -> Result<(), PreviewError> {
    Ok(())
}

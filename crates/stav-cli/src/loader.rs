//! Reading source files from disk, confined to their import roots.

use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use stav::{ImportRoot, LoadError, Loader, MAX_SOURCE_LEN, RelPath};

/// A [`Loader`] that reads files from disk.
///
/// A file is read only when it is a regular file and its real path, with
/// every symbolic link resolved, lies inside the real path of its import
/// root. An import root with an empty path is the current directory.
#[derive(Debug, Clone, Default)]
pub(crate) struct DiskLoader {
    /// The directory that serves `std/<name>` from `<name>.stav`, under
    /// the same confinement as an import root. Without it there are no
    /// standard modules.
    pub(crate) std_dir: Option<PathBuf>,
}

impl Loader for DiskLoader {
    fn std_module(&self, name: &str) -> Option<Cow<'static, str>> {
        let dir = self.std_dir.as_deref()?;
        let rel = RelPath::new(format!("{name}.stav"))?;
        read(dir, &rel).ok().map(Cow::Owned)
    }

    fn read(&self, root: &ImportRoot, rel: &RelPath) -> Result<String, LoadError> {
        read(root.path(), rel)
    }
}

fn read(root: &Path, rel: &RelPath) -> Result<String, LoadError> {
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let base = fs::canonicalize(root).map_err(load_error)?;
    let real = fs::canonicalize(base.join(rel.to_path())).map_err(load_error)?;
    if !real.starts_with(&base) {
        return Err(LoadError::OutsideRoot);
    }
    let metadata = fs::metadata(&real).map_err(load_error)?;
    if !metadata.is_file() {
        return Err(LoadError::NotAFile);
    }
    if metadata.len() > MAX_SOURCE_LEN as u64 {
        return Err(LoadError::TooLarge);
    }
    let mut bytes = Vec::new();
    File::open(&real)
        .and_then(|file| file.take(MAX_SOURCE_LEN as u64 + 1).read_to_end(&mut bytes))
        .map_err(load_error)?;
    if bytes.len() > MAX_SOURCE_LEN {
        return Err(LoadError::TooLarge);
    }
    String::from_utf8(bytes).map_err(|_| LoadError::Other("not valid UTF-8".to_owned()))
}

fn load_error(error: io::Error) -> LoadError {
    match error.kind() {
        io::ErrorKind::NotFound => LoadError::NotFound,
        kind => LoadError::Other(kind.to_string()),
    }
}

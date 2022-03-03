#[derive(Debug)]
pub struct PathFixture(PathFixtureInner);

#[derive(Debug)]
enum PathFixtureInner {
    None,
    Immutable(std::path::PathBuf),
    #[cfg(feature = "path")]
    MutablePath(std::path::PathBuf),
    #[cfg(feature = "path")]
    MutableTemp {
        temp: tempfile::TempDir,
        path: std::path::PathBuf,
    },
}

impl PathFixture {
    pub fn none() -> Self {
        Self(PathFixtureInner::None)
    }

    pub fn immutable(target: &std::path::Path) -> Self {
        Self(PathFixtureInner::Immutable(target.to_owned()))
    }

    #[cfg(feature = "path")]
    pub fn mutable_temp() -> Result<Self, std::io::Error> {
        let temp = tempfile::tempdir()?;
        // We need to get the `/private` prefix on Mac so variable substitutions work
        // correctly
        let path = canonicalize(temp.path())?;
        Ok(Self(PathFixtureInner::MutableTemp { temp, path }))
    }

    #[cfg(feature = "path")]
    pub fn mutable_at(target: &std::path::Path) -> Result<Self, std::io::Error> {
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target)?;
        Ok(Self(PathFixtureInner::MutablePath(target.to_owned())))
    }

    #[cfg(feature = "path")]
    pub fn with_template(self, template_root: &std::path::Path) -> Result<Self, std::io::Error> {
        match &self.0 {
            PathFixtureInner::None | PathFixtureInner::Immutable(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Sandboxing is disabled",
                ));
            }
            PathFixtureInner::MutablePath(path) | PathFixtureInner::MutableTemp { path, .. } => {
                crate::debug!(
                    "Initializing {} from {}",
                    path.display(),
                    template_root.display()
                );
                copy_template(template_root, path)?;
            }
        }

        Ok(self)
    }

    pub fn is_mutable(&self) -> bool {
        match &self.0 {
            PathFixtureInner::None | PathFixtureInner::Immutable(_) => false,
            #[cfg(feature = "path")]
            PathFixtureInner::MutablePath(_) => true,
            #[cfg(feature = "path")]
            PathFixtureInner::MutableTemp { .. } => true,
        }
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        match &self.0 {
            PathFixtureInner::None => None,
            PathFixtureInner::Immutable(path) => Some(path.as_path()),
            #[cfg(feature = "path")]
            PathFixtureInner::MutablePath(path) => Some(path.as_path()),
            #[cfg(feature = "path")]
            PathFixtureInner::MutableTemp { path, .. } => Some(path.as_path()),
        }
    }

    /// Explicitly close to report errors
    pub fn close(self) -> Result<(), std::io::Error> {
        match self.0 {
            PathFixtureInner::None | PathFixtureInner::Immutable(_) => Ok(()),
            #[cfg(feature = "path")]
            PathFixtureInner::MutablePath(_) => Ok(()),
            #[cfg(feature = "path")]
            PathFixtureInner::MutableTemp { temp, .. } => temp.close(),
        }
    }
}

impl Default for PathFixture {
    fn default() -> Self {
        Self::none()
    }
}

/// Recursively walk a path
///
/// Note: Ignores `.keep` files
#[cfg(feature = "path")]
pub struct Walk {
    inner: walkdir::IntoIter,
}

#[cfg(feature = "path")]
impl Walk {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            inner: walkdir::WalkDir::new(path).into_iter(),
        }
    }
}

#[cfg(feature = "path")]
impl Iterator for Walk {
    type Item = Result<std::path::PathBuf, std::io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.inner.next().map(|e| {
            e.map(walkdir::DirEntry::into_path)
                .map_err(std::io::Error::from)
        }) {
            if entry.as_ref().ok().and_then(|e| e.file_name())
                != Some(std::ffi::OsStr::new(".keep"))
            {
                return Some(entry);
            }
        }
        None
    }
}

#[cfg(not(feature = "path"))]
pub struct Walk {}

#[cfg(not(feature = "path"))]
impl Walk {
    pub fn new(_path: &std::path::Path) -> Self {
        Self {}
    }
}

#[cfg(not(feature = "path"))]
impl Iterator for Walk {
    type Item = Result<std::path::PathBuf, std::io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

/// Copy a template into a [`PathFixture`]
///
/// Note: Generally you'll use [`PathFixture::with_template`] instead.
///
/// Note: Ignores `.keep` files
#[cfg(feature = "path")]
pub fn copy_template(
    source: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), std::io::Error> {
    let source = canonicalize(source)?;
    let dest = canonicalize(dest)?;

    for current in Walk::new(&source) {
        let current = current?;
        let rel = current.strip_prefix(&source).unwrap();
        let target = dest.join(rel);

        shallow_copy(&current, &target)?;
    }

    Ok(())
}

/// Copy a file system entry, without recursing
pub fn shallow_copy(
    source: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), std::io::Error> {
    let meta = source.symlink_metadata()?;
    if meta.is_dir() {
        std::fs::create_dir_all(dest)?;
    } else if meta.is_file() {
        std::fs::copy(source, dest)?;
    } else if let Ok(target) = std::fs::read_link(source) {
        symlink_to_file(dest, &target)?;
    }

    Ok(())
}

#[cfg(windows)]
fn symlink_to_file(link: &std::path::Path, target: &std::path::Path) -> Result<(), std::io::Error> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(not(windows))]
fn symlink_to_file(link: &std::path::Path, target: &std::path::Path) -> Result<(), std::io::Error> {
    std::os::unix::fs::symlink(target, link)
}

pub fn resolve_dir(path: std::path::PathBuf) -> Result<std::path::PathBuf, std::io::Error> {
    let meta = std::fs::symlink_metadata(&path)?;
    if meta.is_dir() {
        canonicalize(&path)
    } else if meta.is_file() {
        // Git might checkout symlinks as files
        let target = std::fs::read_to_string(&path)?;
        let target_path = path.parent().unwrap().join(target);
        resolve_dir(target_path)
    } else {
        canonicalize(&path)
    }
}

fn canonicalize(path: &std::path::Path) -> Result<std::path::PathBuf, std::io::Error> {
    #[cfg(feature = "path")]
    {
        dunce::canonicalize(path)
    }
    #[cfg(not(feature = "path"))]
    {
        // Hope for the best
        Ok(strip_trailing_slash(path).to_owned())
    }
}

pub fn strip_trailing_slash(path: &std::path::Path) -> &std::path::Path {
    path.components().as_path()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn strips_trailing_slash() {
        let path = std::path::Path::new("/foo/bar/");
        let rendered = path.display().to_string();
        assert_eq!(rendered.as_bytes()[rendered.len() - 1], b'/');

        let stripped = strip_trailing_slash(path);
        let rendered = stripped.display().to_string();
        assert_eq!(rendered.as_bytes()[rendered.len() - 1], b'r');
    }
}
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path;
use std::time;

use nix::sys::stat::{mode_t, Mode, SFlag};
use walkdir::DirEntry;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// The kind of filesystem object an entry represents.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileType {
    Directory,
    File,
    // Unix specific
    Link,
    Socket,
    Block,
    Character,
    Pipe,
}

impl From<SFlag> for FileType {
    fn from(value: SFlag) -> Self {
        Self::from_mode(value)
    }
}

impl FileType {
    /// Maps a Unix `st_mode` file-type flag to a [`FileType`].
    pub fn from_mode(mode: SFlag) -> Self {
        match mode {
            SFlag::S_IFBLK => FileType::Block,
            SFlag::S_IFCHR => FileType::Character,
            SFlag::S_IFDIR => FileType::Directory,
            SFlag::S_IFIFO => FileType::Pipe,
            SFlag::S_IFLNK => FileType::Link,
            SFlag::S_IFSOCK => FileType::Socket,
            _ => FileType::File,
        }
    }
}

/// Whether an entry is a plain file/directory or a symlink, and if a symlink, its target.
///
/// Note: symlink *validity* is not stored here; it is resolved lazily (with a follow-`stat`)
/// via [`JoshutoMetadata::link_valid`], so broken links are always styled correctly.
#[derive(Clone, Debug)]
pub enum LinkType {
    Normal,
    Symlink { target: String },
}

/// The filesystem metadata that requires a `stat` syscall. It is loaded lazily (on first access)
/// and then cached, so that directory listings and previews avoid `stat`-ing every entry up front.
#[derive(Clone, Debug)]
struct Detailed {
    len: u64,
    modified: time::SystemTime,
    accessed: time::SystemTime,
    mode: Mode,
    #[cfg(unix)]
    uid: u32,
    #[cfg(unix)]
    gid: u32,
    // Symlink-only fields: resolved by the follow-`stat` that `Detailed::load` already performs
    // for links (irrelevant for other file types).
    /// Whether the symlink's target could be resolved at all (false for broken links and loops).
    link_valid: bool,
    /// Whether the symlink's target is a directory.
    link_target_is_dir: bool,
}

impl Detailed {
    fn empty() -> Self {
        Self {
            len: 0,
            modified: time::SystemTime::UNIX_EPOCH,
            accessed: time::SystemTime::UNIX_EPOCH,
            mode: Mode::empty(),
            #[cfg(unix)]
            uid: 0,
            #[cfg(unix)]
            gid: 0,
            link_valid: false,
            link_target_is_dir: false,
        }
    }

    fn load(path: &path::Path) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let symlink_metadata = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => return Self::empty(),
        };
        let is_symlink = symlink_metadata.file_type().is_symlink();
        // Only symlinks need a second (follow) stat: for regular files and directories
        // `symlink_metadata` already returned the same data.
        let metadata = if is_symlink {
            fs::metadata(path)
        } else {
            Ok(symlink_metadata.clone())
        };

        let (len, modified, accessed) = match metadata.as_ref() {
            Ok(m) => (
                m.len(),
                m.modified().unwrap_or(time::SystemTime::UNIX_EPOCH),
                m.accessed().unwrap_or(time::SystemTime::UNIX_EPOCH),
            ),
            Err(_) => (
                symlink_metadata.len(),
                symlink_metadata
                    .modified()
                    .unwrap_or(time::SystemTime::UNIX_EPOCH),
                symlink_metadata
                    .accessed()
                    .unwrap_or(time::SystemTime::UNIX_EPOCH),
            ),
        };

        let mut mode = Mode::empty();
        #[cfg(unix)]
        let mut uid: u32 = 0;
        #[cfg(unix)]
        let mut gid: u32 = 0;
        if let Ok(m) = metadata.as_ref() {
            mode = Mode::from_bits_truncate(m.mode() as mode_t);
            #[cfg(unix)]
            {
                uid = m.uid();
                gid = m.gid();
            }
        }

        // For symlinks, `metadata` is the follow-`stat` of the target: its outcome tells us
        // whether the link is valid and whether the target is a directory.
        let link_valid = metadata.is_ok();
        let link_target_is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        Self {
            len,
            modified,
            accessed,
            mode,
            #[cfg(unix)]
            uid,
            #[cfg(unix)]
            gid,
            link_valid,
            link_target_is_dir,
        }
    }
}

/// Filesystem metadata for a [`JoshutoDirEntry`](super::JoshutoDirEntry).
///
/// `file_type` and `link_type` are always available: they are derived cheaply from `readdir`'s
/// `d_type` (no syscall on most local filesystems) and a `readlink` for symlinks. The remaining
/// fields (`len`, `modified`, `accessed`, `mode`, `uid`, `gid`) are fetched lazily on first access
/// and cached, so listing a large directory no longer `stat`s every entry.
#[derive(Clone)]
pub struct JoshutoMetadata {
    path: path::PathBuf,
    pub file_type: FileType,
    pub link_type: LinkType,
    detailed: RefCell<Option<Detailed>>,
    directory_size: RefCell<Option<usize>>,
    cumulative_size: RefCell<Option<u64>>,
}

impl std::fmt::Debug for JoshutoMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("JoshutoMetadata")
            .field("file_type", &self.file_type)
            .field("link_type", &self.link_type)
            .finish_non_exhaustive()
    }
}

impl JoshutoMetadata {
    /// Eagerly reads all metadata for `path` (used for the listed directory itself).
    pub fn from(path: &path::Path) -> io::Result<Self> {
        let symlink_metadata = fs::symlink_metadata(path)?;
        let file_type = {
            #[cfg(unix)]
            {
                FileType::from_mode(SFlag::from_bits_truncate(symlink_metadata.mode() as mode_t))
            }
            #[cfg(not(unix))]
            {
                let ft = symlink_metadata.file_type();
                if ft.is_dir() {
                    FileType::Directory
                } else if ft.is_symlink() {
                    FileType::Link
                } else {
                    FileType::File
                }
            }
        };
        let link_type = if symlink_metadata.file_type().is_symlink() {
            let target = fs::read_link(path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            LinkType::Symlink { target }
        } else {
            LinkType::Normal
        };
        let detailed = Detailed::load(path);
        Ok(Self {
            path: path.to_path_buf(),
            file_type,
            link_type,
            detailed: RefCell::new(Some(detailed)),
            directory_size: RefCell::new(None),
            cumulative_size: RefCell::new(None),
        })
    }

    /// Builds metadata for a directory entry without performing a `stat`: `file_type` and
    /// `link_type` are provided directly (e.g. from `readdir`'s `d_type` and a `readlink`).
    pub fn from_lightweight(path: path::PathBuf, file_type: FileType, link_type: LinkType) -> Self {
        Self {
            path,
            file_type,
            link_type,
            detailed: RefCell::new(None),
            directory_size: RefCell::new(None),
            cumulative_size: RefCell::new(None),
        }
    }

    /// Builds lightweight metadata from a `walkdir` entry, deriving `file_type` from `d_type`
    /// (avoiding a `stat` on filesystems that report it) and reading the symlink target.
    pub fn from_walkdir(direntry: &DirEntry, path: path::PathBuf) -> Self {
        let wt = direntry.file_type();
        let file_type = if wt.is_dir() {
            FileType::Directory
        } else if wt.is_symlink() {
            FileType::Link
        } else if wt.is_file() {
            FileType::File
        } else {
            // Rare: `d_type` was unavailable, so `walkdir` already `stat`ted to resolve the type.
            match direntry.metadata() {
                Ok(m) => {
                    #[cfg(unix)]
                    {
                        FileType::from_mode(SFlag::from_bits_truncate(m.mode() as mode_t))
                    }
                    #[cfg(not(unix))]
                    {
                        let ft = m.file_type();
                        if ft.is_dir() {
                            FileType::Directory
                        } else if ft.is_symlink() {
                            FileType::Link
                        } else {
                            FileType::File
                        }
                    }
                }
                Err(_) => FileType::File,
            }
        };
        let link_type = if file_type == FileType::Link {
            let target = fs::read_link(&path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            LinkType::Symlink { target }
        } else {
            LinkType::Normal
        };
        Self::from_lightweight(path, file_type, link_type)
    }

    fn with_detailed<T>(&self, f: impl FnOnce(&Detailed) -> T) -> T {
        let mut cell = self.detailed.borrow_mut();
        if cell.is_none() {
            *cell = Some(Detailed::load(&self.path));
        }
        f(cell.as_ref().unwrap())
    }

    /// Returns the entry's size in bytes, as reported by the filesystem.
    pub fn len(&self) -> u64 {
        self.with_detailed(|d| d.len)
    }

    /// Returns the number of entries in this directory, if it has been computed.
    pub fn directory_size(&self) -> Option<usize> {
        *self.directory_size.borrow()
    }

    /// Records the number of entries in this directory.
    pub fn update_directory_size(&self, size: usize) {
        *self.directory_size.borrow_mut() = Some(size);
    }

    /// Returns the total recursive size of this directory in bytes, if it has been computed.
    pub fn cumulative_size(&self) -> Option<u64> {
        *self.cumulative_size.borrow()
    }

    /// Records the total recursive size of this directory in bytes.
    pub fn update_cumulative_size(&self, size: u64) {
        *self.cumulative_size.borrow_mut() = Some(size);
    }

    /// Returns the entry's last-modified time.
    pub fn modified(&self) -> time::SystemTime {
        self.with_detailed(|d| d.modified)
    }

    /// Returns the entry's last-accessed time.
    pub fn accessed(&self) -> time::SystemTime {
        self.with_detailed(|d| d.accessed)
    }

    /// Returns the entry's permission mode.
    pub fn mode(&self) -> Mode {
        self.with_detailed(|d| d.mode)
    }

    /// Updates the entry's permission mode in memory (after a `chmod`).
    pub fn set_mode(&self, mode: Mode) {
        let mut cell = self.detailed.borrow_mut();
        if cell.is_none() {
            // Load the real metadata (not an empty placeholder) so symlink fields and
            // other lazily-resolved values stay consistent.
            *cell = Some(Detailed::load(&self.path));
        }
        cell.as_mut().unwrap().mode = mode;
    }

    /// Returns the entry's owner uid (unix).
    #[cfg(unix)]
    pub fn uid(&self) -> u32 {
        self.with_detailed(|d| d.uid)
    }

    /// Returns the entry's group gid (unix).
    #[cfg(unix)]
    pub fn gid(&self) -> u32 {
        self.with_detailed(|d| d.gid)
    }

    /// Returns the kind of filesystem object this entry is.
    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    /// Returns whether this entry is a symlink, and its target if so.
    pub fn link_type(&self) -> &LinkType {
        &self.link_type
    }

    /// Returns the symlink's target path as a string, if this entry is a symlink.
    pub fn link_target(&self) -> Option<&str> {
        match &self.link_type {
            LinkType::Symlink { target, .. } => Some(target.as_str()),
            LinkType::Normal => None,
        }
    }

    /// Returns whether a symlink's target could be resolved by a follow-`stat` (false for
    /// broken links and loops). Always `true` for non-symlink entries with readable metadata.
    pub fn link_valid(&self) -> bool {
        self.with_detailed(|d| d.link_valid)
    }

    /// Returns `true` if this entry is a directory, resolving symlink targets: a symlink
    /// pointing at a directory counts as a directory (for sorting, previews, and navigation).
    pub fn is_dir(&self) -> bool {
        match self.file_type {
            FileType::Directory => true,
            FileType::Link => self.with_detailed(|d| d.link_target_is_dir),
            _ => false,
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::OsString;

    /// Builds lightweight metadata the way `from_walkdir` does for a symlink, without
    /// needing a real `walkdir` entry: the follow-`stat` is deferred to first access.
    fn link_metadata(path: path::PathBuf, target: OsString) -> JoshutoMetadata {
        JoshutoMetadata::from_lightweight(
            path,
            FileType::Link,
            LinkType::Symlink {
                target: target.to_string_lossy().to_string(),
            },
        )
    }

    #[test]
    fn symlink_to_dir_counts_as_dir_and_resolves_validity() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let target = dir.join("target");
        fs::create_dir_all(&target).unwrap();
        let good = dir.join("good_link");
        let bad = dir.join("bad_link");
        std::os::unix::fs::symlink(&target, &good).unwrap();
        std::os::unix::fs::symlink(dir.join("nonexistent"), &bad).unwrap();

        let good_link = link_metadata(good.clone(), fs::read_link(&good).unwrap().into_os_string());
        let bad_link = link_metadata(bad.clone(), OsString::from("nonexistent"));

        let (good_is_dir, good_valid, bad_is_dir, bad_valid) = (
            good_link.is_dir(),
            good_link.link_valid(),
            bad_link.is_dir(),
            bad_link.link_valid(),
        );
        fs::remove_dir_all(&dir).unwrap();

        assert!(
            good_is_dir,
            "symlink to a directory should count as a directory"
        );
        assert!(good_valid);
        assert!(
            !bad_is_dir,
            "broken symlink should not count as a directory"
        );
        assert!(!bad_valid);
    }

    #[test]
    fn symlink_to_file_is_dir_false_but_valid() {
        let dir = std::env::temp_dir().join(format!("joshuto-test-{}", uuid::Uuid::new_v4()));
        let target = dir.join("file");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&target, b"data").unwrap();
        let link = dir.join("file_link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let link_meta = link_metadata(link.clone(), fs::read_link(&link).unwrap().into_os_string());

        let (is_dir, valid) = (link_meta.is_dir(), link_meta.link_valid());
        fs::remove_dir_all(&dir).unwrap();

        assert!(!is_dir);
        assert!(valid);
    }
}

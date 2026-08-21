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
#[derive(Clone, Debug)]
pub enum LinkType {
    Normal,
    Symlink { target: String, valid: bool },
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

        Self {
            len,
            modified,
            accessed,
            mode,
            #[cfg(unix)]
            uid,
            #[cfg(unix)]
            gid,
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
            LinkType::Symlink {
                target,
                valid: fs::metadata(path).is_ok(),
            }
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
            // `valid` is resolved lazily (a follow-stat) when the detailed metadata is loaded;
            // the cursor entry's footer will show the correct state.
            LinkType::Symlink {
                target,
                valid: true,
            }
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
            *cell = Some(Detailed::empty());
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

    /// Returns `true` if this entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type == FileType::Directory
    }
}

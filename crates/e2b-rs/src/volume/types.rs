//! Public types for the E2B volume API.
//!
//! All generated wire types stay inside the internal `volume::schema` module and are
//! never re-exported. The structs here are the stable public surface; they are
//! populated from wire types via `from_wire` helpers.

use chrono::{DateTime, Utc};

/// The type of a single entry (file, directory, or symlink) stored in a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeFileType {
    /// The entry type could not be determined.
    Unknown,
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

/// Minimal identifying information about a volume.
#[derive(Debug, Clone)]
pub struct VolumeInfo {
    /// Unique identifier for the volume.
    pub volume_id: String,
    /// Human-readable name of the volume.
    pub name: String,
}

/// A volume together with a short-lived Bearer token for the content API.
#[derive(Debug, Clone)]
pub struct VolumeAndToken {
    /// Unique identifier for the volume.
    pub volume_id: String,
    /// Human-readable name of the volume.
    pub name: String,
    /// Short-lived Bearer token; pass to `VolumeApiClient::new` when constructing a content client.
    pub token: String,
}

/// Metadata for a single entry (file, directory, or symlink) inside a volume.
#[derive(Debug, Clone)]
pub struct VolumeEntryStat {
    /// Base name of the entry (file name only, no leading path components).
    pub name: String,
    /// Absolute path of the entry within the volume.
    pub path: String,
    /// Type of the entry.
    pub file_type: VolumeFileType,
    /// Size in bytes (`0` for directories).
    pub size: i64,
    /// Unix permission bits (e.g. `0o644`).
    pub mode: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
    /// Last-access time.
    pub atime: DateTime<Utc>,
    /// Last-modification time.
    pub mtime: DateTime<Utc>,
    /// Last-status-change time.
    pub ctime: DateTime<Utc>,
    /// Symlink target path; `Some` only when `file_type == VolumeFileType::Symlink`.
    pub target: Option<String>,
}

/// Options for setting ownership and permission bits on a volume entry.
#[derive(Default, Debug, Clone)]
pub struct VolumeMetadataOpts {
    /// Owner user ID to set on the entry.
    pub uid: Option<u32>,
    /// Owner group ID to set on the entry.
    pub gid: Option<u32>,
    /// Unix permission bits to set on the entry.
    pub mode: Option<u32>,
}

/// Options for writing a file into a volume.
#[derive(Default, Debug, Clone)]
pub struct VolumeWriteOpts {
    /// Owner user ID to set on the new file.
    pub uid: Option<u32>,
    /// Owner group ID to set on the new file.
    pub gid: Option<u32>,
    /// Unix permission bits to set on the new file.
    pub mode: Option<u32>,
    /// When `true`, overwrite the file if it already exists.
    pub force: Option<bool>,
}

/// Options for reading a file from a volume.
#[derive(Default, Debug, Clone)]
pub struct VolumeReadOpts {
    /// Idle timeout in milliseconds for streaming reads. `None` uses the
    /// client's default file timeout (1 hour).
    pub stream_idle_timeout_ms: Option<u64>,
}

/// Options for listing the entries of a volume directory.
#[derive(Default, Debug, Clone)]
pub struct VolumeListOpts {
    /// Recursion depth. `None` or `Some(1)` lists only the immediate children.
    pub depth: Option<u32>,
}

/// Options for creating a directory inside a volume.
#[derive(Default, Debug, Clone)]
pub struct VolumeMakeDirOpts {
    /// Owner user ID to set on the new directory.
    pub uid: Option<u32>,
    /// Owner group ID to set on the new directory.
    pub gid: Option<u32>,
    /// Unix permission bits to set on the new directory.
    pub mode: Option<u32>,
    /// When `true`, create intermediate directories and do not error if the
    /// directory already exists (analogous to `mkdir -p`).
    pub force: Option<bool>,
}

#[allow(dead_code)] // from_wire called by Volume methods in later tasks
impl VolumeEntryStat {
    /// Map from the internal wire type [`crate::volume::schema::VolumeEntryStat`]
    /// to this public struct.
    pub(crate) fn from_wire(w: crate::volume::schema::VolumeEntryStat) -> VolumeEntryStat {
        use crate::volume::schema::VolumeEntryStatType;
        let file_type = match w.type_ {
            VolumeEntryStatType::Unknown => VolumeFileType::Unknown,
            VolumeEntryStatType::File => VolumeFileType::File,
            VolumeEntryStatType::Directory => VolumeFileType::Directory,
            VolumeEntryStatType::Symlink => VolumeFileType::Symlink,
        };
        VolumeEntryStat {
            name: w.name,
            path: w.path,
            file_type,
            size: w.size,
            mode: w.mode,
            uid: w.uid,
            gid: w.gid,
            atime: w.atime,
            mtime: w.mtime,
            ctime: w.ctime,
            target: w.target,
        }
    }
}

#[allow(dead_code)] // from_wire called by Volume API methods in later tasks
impl VolumeInfo {
    /// Map from the generated [`crate::api::schema::Volume`] wire type.
    pub(crate) fn from_wire(w: crate::api::schema::Volume) -> VolumeInfo {
        VolumeInfo {
            volume_id: w.volume_id,
            name: w.name,
        }
    }
}

#[allow(dead_code)] // from_wire called by Volume API methods in later tasks
impl VolumeAndToken {
    /// Map from the generated [`crate::api::schema::VolumeAndToken`] wire type.
    pub(crate) fn from_wire(w: crate::api::schema::VolumeAndToken) -> VolumeAndToken {
        VolumeAndToken {
            volume_id: w.volume_id,
            name: w.name,
            token: w.token,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_wire(
        type_: crate::volume::schema::VolumeEntryStatType,
    ) -> crate::volume::schema::VolumeEntryStat {
        crate::volume::schema::VolumeEntryStat {
            atime: chrono::Utc::now(),
            ctime: chrono::Utc::now(),
            mtime: chrono::Utc::now(),
            gid: 0,
            mode: 0o644,
            uid: 1000,
            name: "test.txt".to_string(),
            path: "/test.txt".to_string(),
            size: 0,
            target: None,
            type_,
        }
    }

    #[test]
    fn maps_wire_entry_stat() {
        use crate::volume::schema::VolumeEntryStatType;

        assert_eq!(
            VolumeEntryStat::from_wire(make_wire(VolumeEntryStatType::Unknown)).file_type,
            VolumeFileType::Unknown,
        );
        assert_eq!(
            VolumeEntryStat::from_wire(make_wire(VolumeEntryStatType::File)).file_type,
            VolumeFileType::File,
        );
        assert_eq!(
            VolumeEntryStat::from_wire(make_wire(VolumeEntryStatType::Directory)).file_type,
            VolumeFileType::Directory,
        );
        assert_eq!(
            VolumeEntryStat::from_wire(make_wire(VolumeEntryStatType::Symlink)).file_type,
            VolumeFileType::Symlink,
        );
    }
}

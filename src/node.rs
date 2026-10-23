use crate::dir_walker::WalkData;
use crate::platform::get_metadata;
use crate::utils::is_filtered_out_due_to_file_time;
use crate::utils::is_filtered_out_due_to_invert_regex;
use crate::utils::is_filtered_out_due_to_regex;

use std::cmp::Ordering;
use std::path::PathBuf;

#[derive(Debug, Eq, Clone)]
pub struct Node {
    pub name: PathBuf,
    // Existing: usage (blocks) or apparent per global mode
    pub size: u64,
    // NEW: always apparent size; for symlinks, the target's size (0 if dangling)
    pub size_apparent: u64,
    // NEW: lstat-based file type, for the tree style
    pub filetype: FileType,
    pub children: Vec<Node>,
    pub inode_device: Option<(u64, u64)>,
    pub depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Dir,
    File,
    Symlink,
}

#[derive(Debug, PartialEq)]
pub enum FileTime {
    Modified,
    Accessed,
    Changed,
}

impl From<crate::cli::FileTime> for FileTime {
    fn from(time: crate::cli::FileTime) -> Self {
        match time {
            crate::cli::FileTime::Modified => Self::Modified,
            crate::cli::FileTime::Accessed => Self::Accessed,
            crate::cli::FileTime::Changed => Self::Changed,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_node(
    dir: PathBuf,
    children: Vec<Node>,
    is_symlink: bool,
    is_file: bool,
    depth: usize,
    walk_data: &WalkData,
) -> Option<Node> {
    let use_apparent_size = walk_data.use_apparent_size;
    let by_filecount = walk_data.by_filecount;
    let by_filetime = &walk_data.by_filetime;

    get_metadata(
        &dir,
        use_apparent_size,
        walk_data.follow_links && is_symlink,
    )
    .map(|data| {
        let (usage, apparent, inode_device, times) = data;

        let size = if is_filtered_out_due_to_regex(walk_data.filter_regex, &dir)
            || is_filtered_out_due_to_invert_regex(walk_data.invert_filter_regex, &dir)
            || by_filecount && !is_file
            || [
                (&walk_data.filter_modified_time, times.0),
                (&walk_data.filter_accessed_time, times.1),
                (&walk_data.filter_changed_time, times.2),
            ]
            .iter()
            .any(|(filter_time, actual_time)| {
                is_filtered_out_due_to_file_time(filter_time, *actual_time)
            }) {
            0
        } else if by_filecount {
            1
        } else if by_filetime.is_some() {
            match by_filetime {
                Some(FileTime::Modified) => times.0.unsigned_abs(),
                Some(FileTime::Accessed) => times.1.unsigned_abs(),
                Some(FileTime::Changed) => times.2.unsigned_abs(),
                None => unreachable!(),
            }
        } else if use_apparent_size {
            apparent
        } else {
            usage
        };

        let filetype = if is_symlink {
            FileType::Symlink
        } else if is_file {
            FileType::File
        } else {
            FileType::Dir
        };

        Node {
            name: dir,
            size,
            // For symlinks this is the lstat size (target-path length): the
            // script's `zstat -L -A` quirk does not actually follow links.
            size_apparent: apparent,
            filetype,
            children,
            inode_device,
            depth,
        }
    })
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.size == other.size && self.children == other.children
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        self.size
            .cmp(&other.size)
            .then_with(|| self.name.cmp(&other.name))
            .then_with(|| self.children.cmp(&other.children))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

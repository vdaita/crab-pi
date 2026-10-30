//! Path-based view of the FAT32 volume.
//!
//! Paths are normalized lexically against the caller's cwd (so `.`, `..` and
//! repeated slashes never reach FAT32) and then walked from the root one
//! component at a time. The volume only supports 8.3 names; lookups are
//! case-insensitive and anything created is stored upper-case.

use crate::fat32::{self, fat32_fs_t, pi_dirent_t, pi_file_t};
use crate::kmalloc;
use crate::os::errno::*;
use crate::os::holder::OSHolder;

pub const PATH_MAX: usize = 128;

/// A normalized absolute path: starts with `/`, no `.`/`..`, no trailing slash.
#[derive(Clone, Copy)]
pub struct Path {
    buf: [u8; PATH_MAX],
    len: usize,
}

impl Path {
    pub const ROOT: Path = {
        let mut buf = [0u8; PATH_MAX];
        buf[0] = b'/';
        Path { buf, len: 1 }
    };

    pub fn as_str(&self) -> &str {
        // only ever built by copying in pieces of &str
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    pub fn is_root(&self) -> bool {
        self.len == 1
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.as_str().split('/').filter(|c| !c.is_empty())
    }

    /// Interpret `path` relative to `cwd` (ignored if `path` is absolute).
    pub fn resolve(cwd: &Path, path: &str) -> Result<Path, Errno> {
        if path.is_empty() {
            return Err(ENOENT);
        }
        let mut out = if path.starts_with('/') { Path::ROOT } else { *cwd };
        for comp in path.split('/') {
            match comp {
                "" | "." => {}
                ".." => out.pop(),
                name => out.push(name)?,
            }
        }
        Ok(out)
    }

    /// Split into (parent directory, final component). The root has an empty leaf.
    pub fn split_leaf(&self) -> (Path, &str) {
        let s = self.as_str();
        let slash = s.rfind('/').unwrap_or(0);
        let mut parent = *self;
        parent.len = slash.max(1);
        (parent, &s[slash + 1..])
    }

    fn push(&mut self, name: &str) -> Result<(), Errno> {
        let sep = if self.is_root() { 0 } else { 1 };
        if self.len + sep + name.len() > PATH_MAX {
            return Err(ENAMETOOLONG);
        }
        if sep == 1 {
            self.buf[self.len] = b'/';
        }
        self.buf[self.len + sep..self.len + sep + name.len()].copy_from_slice(name.as_bytes());
        self.len += sep + name.len();
        Ok(())
    }

    fn pop(&mut self) {
        let slash = self.as_str().rfind('/').unwrap_or(0);
        self.len = slash.max(1);
    }
}

/// A resolved directory entry plus the directory it lives in (needed to
/// write it back, rename it, or delete it).
#[derive(Clone, Copy)]
pub struct Entry {
    pub dirent: pi_dirent_t,
    pub parent: pi_dirent_t,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.dirent.is_dir_p != 0
    }
}

fn volume() -> (&'static fat32_fs_t, &'static pi_dirent_t) {
    let holder = unsafe { OSHolder::os_holder_mut() };
    (&holder.fs, &holder.root)
}

pub fn dirent_name(d: &pi_dirent_t) -> &str {
    let len = d.name.iter().position(|&b| b == 0).unwrap_or(d.name.len());
    unsafe { core::str::from_utf8_unchecked(&d.name[..len]) }
}

/// Upper-case `name` into `buf` and check that FAT32 will accept it.
fn fat_name<'a>(name: &str, buf: &'a mut [u8; 12]) -> Result<&'a str, Errno> {
    if name.len() > buf.len() {
        return Err(ENAMETOOLONG);
    }
    for (dst, src) in buf.iter_mut().zip(name.bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    let s = core::str::from_utf8(&buf[..name.len()]).map_err(|_| EINVAL)?;
    if fat32::fat32_is_valid_name(s) { Ok(s) } else { Err(EINVAL) }
}

pub fn lookup(path: &Path) -> Result<Entry, Errno> {
    let (fs, root) = volume();
    let mut entry = Entry { dirent: *root, parent: *root };
    for name in path.components() {
        if !entry.is_dir() {
            return Err(ENOTDIR);
        }
        let found = fat32::fat32_stat(fs, &entry.dirent, name);
        if found.is_null() {
            return Err(ENOENT);
        }
        entry = Entry { dirent: unsafe { *found }, parent: entry.dirent };
    }
    Ok(entry)
}

/// The directory that `path` would live in, along with the leaf name.
pub fn lookup_parent(path: &Path) -> Result<(pi_dirent_t, &str), Errno> {
    let (parent, leaf) = path.split_leaf();
    if leaf.is_empty() {
        return Err(EPERM);
    }
    let dir = lookup(&parent)?.dirent;
    if dir.is_dir_p == 0 {
        return Err(ENOTDIR);
    }
    Ok((dir, leaf))
}

pub fn create(path: &Path, is_dir: bool) -> Result<Entry, Errno> {
    let (fs, _) = volume();
    let (parent, leaf) = lookup_parent(path)?;
    let mut buf = [0u8; 12];
    let name = fat_name(leaf, &mut buf)?;

    let created = fat32::fat32_create(fs, &parent, name, is_dir as i32);
    if created.is_null() {
        return Err(ENOSPC);
    }
    let mut dirent = unsafe { *created };

    if is_dir {
        // A FAT directory needs a cluster of its own; an all-zero one reads back empty.
        let size = (fs.sectors_per_cluster * 512) as usize;
        let zeros = unsafe { kmalloc::kmalloc(size) };
        unsafe { core::ptr::write_bytes(zeros, 0, size) };
        let file = pi_file_t { data: zeros, n_data: size, n_alloc: size };
        if fat32::fat32_write(fs, &parent, name, &file) != 1 {
            return Err(EIO);
        }
        let reread = fat32::fat32_stat(fs, &parent, name);
        if reread.is_null() {
            return Err(EIO);
        }
        dirent = unsafe { *reread };
    }

    Ok(Entry { dirent, parent })
}

pub fn remove(path: &Path, want_dir: bool) -> Result<(), Errno> {
    let (fs, _) = volume();
    if path.is_root() {
        return Err(EPERM);
    }
    let entry = lookup(path)?;
    match (want_dir, entry.is_dir()) {
        (true, false) => return Err(ENOTDIR),
        (false, true) => return Err(EISDIR),
        (true, true) if fat32::fat32_readdir(fs, &entry.dirent).ndirents > 2 => return Err(ENOTEMPTY),
        _ => {}
    }
    if fat32::fat32_delete(fs, &entry.parent, dirent_name(&entry.dirent)) != 1 {
        return Err(EIO);
    }
    Ok(())
}

pub fn rename(from: &Path, to: &Path) -> Result<(), Errno> {
    let (fs, _) = volume();
    let src = lookup(from)?;
    let (dst_dir, dst_leaf) = lookup_parent(to)?;
    if dst_dir.cluster_id != src.parent.cluster_id {
        return Err(EXDEV); // fat32_rename only renames within a directory
    }
    let src_name = dirent_name(&src.dirent);
    if src_name.eq_ignore_ascii_case(dst_leaf) {
        return Ok(());
    }
    if let Ok(existing) = lookup(to) {
        if existing.is_dir() {
            return Err(EISDIR);
        }
        fat32::fat32_delete(fs, &existing.parent, dirent_name(&existing.dirent));
    }
    let mut buf = [0u8; 12];
    let dst_name = fat_name(dst_leaf, &mut buf)?;
    if fat32::fat32_rename(fs, &src.parent, src_name, dst_name) != 1 {
        return Err(EIO);
    }
    Ok(())
}

/// Read a whole file into a fresh kernel buffer.
pub fn read_file(dirent: &pi_dirent_t) -> pi_file_t {
    let (fs, _) = volume();
    unsafe { *fat32::fat32_read_dirent(fs, dirent) }
}

/// Replace a file's contents on disk.
pub fn write_file(parent: &pi_dirent_t, dirent: &pi_dirent_t, data: *mut u8, len: usize) -> Result<(), Errno> {
    let (fs, _) = volume();
    let file = pi_file_t { data, n_data: len, n_alloc: len };
    if fat32::fat32_write(fs, parent, dirent_name(dirent), &file) != 1 {
        return Err(EIO);
    }
    Ok(())
}

/// A directory's entries, serialized as `linux_dirent64` records.
pub fn list_dir(dirent: &pi_dirent_t) -> pi_file_t {
    let (fs, _) = volume();
    fat32::get_dirents64_as_file(&fat32::fat32_readdir(fs, dirent))
}

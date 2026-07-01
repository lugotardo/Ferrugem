/// Virtual File System trait layer over concrete filesystems.

pub mod ramfs;

pub use ramfs::{RamFs, InodeKind, Inode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeekFrom {
    Start(u64),
    Current(i64),
    End(i64),
}

pub struct FileHandle {
    pub inode:  usize,
    pub offset: u64,
    pub size:   u64,
}

impl FileHandle {
    pub fn new(inode: usize, size: u64) -> Self {
        Self { inode, offset: 0, size }
    }
}

/// Full Unix-style filesystem interface.
/// Concrete filesystems implement the rich API; the basic open/create/remove
/// stubs let new implementations be added incrementally.
pub trait FileSystem {
    fn name(&self) -> &'static str;

    // ── basic VFS ops (default no-ops) ────────────────────────────────────────
    fn open(&self, path: &str) -> Option<FileHandle> { let _ = path; None }
    fn create(&self, path: &str) -> Option<FileHandle> { let _ = path; None }
    fn remove(&self, path: &str) -> bool { let _ = path; false }
    fn list_names(&self, path: &str) -> &[&str] { let _ = path; &[] }

    // ── initialisation ────────────────────────────────────────────────────────
    fn init_root(&mut self);

    // ── path resolution ───────────────────────────────────────────────────────
    fn resolve(&self, path: &str, cwd: usize) -> Option<usize>;
    fn is_dir(&self, path: &str, cwd: usize) -> Option<bool>;
    fn inode_path(&self, idx: usize, buf: &mut [u8]) -> usize;
    fn get_inode(&self, idx: usize) -> Option<&Inode>;

    // ── file I/O ──────────────────────────────────────────────────────────────
    fn write_file(&mut self, path: &str, cwd: usize, data: &[u8], uid: u16, gid: u16) -> bool;
    fn read_file(&self, path: &str, cwd: usize, uid: u16, gid: u16) -> Option<&[u8]>;

    // ── directory ops ─────────────────────────────────────────────────────────
    fn mkdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16, mode: u16) -> bool;
    fn rmdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool;
    fn list_dir(&self, path: &str, cwd: usize, f: &mut dyn FnMut(&Inode));

    // ── file management ───────────────────────────────────────────────────────
    fn touch(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool;
    fn unlink(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool;
    fn rename(&mut self, src: &str, dst: &str, cwd: usize, uid: u16, gid: u16) -> bool;

    // ── permissions ───────────────────────────────────────────────────────────
    fn chmod(&mut self, path: &str, cwd: usize, mode: u16, caller_uid: u16) -> bool;
    fn chown(&mut self, path: &str, cwd: usize, new_uid: u16, new_gid: u16, caller_uid: u16) -> bool;
}

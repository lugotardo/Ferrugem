/// Filesystem layer all access goes through the FileSystem trait, not RamFs directly.

use crate::vfs::{RamFs, FileSystem, Inode};

static mut ROOT_FS: RamFs = RamFs::new();

pub fn init() {
    unsafe {
        ROOT_FS.init_root();
        ROOT_FS.mkdir("/etc",  0, 0, 0, 0o755);
        ROOT_FS.mkdir("/home", 0, 0, 0, 0o755);
        ROOT_FS.mkdir("/tmp",  0, 0, 0, 0o777);
        ROOT_FS.write_file("/etc/hostname", 0, b"ferrugem\n",           0, 0);
        ROOT_FS.write_file("/etc/motd",     0, b"Welcome to Ferrugem!\n", 0, 0);
        ROOT_FS.mkdir("/home/ferrugem",  0, 1000, 1000, 0o755);
        ROOT_FS.mkdir("/bin", 0, 0, 0, 0o755);
        #[cfg(target_arch = "riscv64")]
        ROOT_FS.write_file("/bin/hello", 0, &crate::userspace::HELLO_ELF_RV64, 0, 0);
        #[cfg(target_arch = "x86_64")]
        ROOT_FS.write_file("/bin/hello", 0, &crate::userspace::HELLO_ELF_X86, 0, 0);
    }
}

// ── backward-compat root ops ─────────────────────────────────────────────────

pub fn write(path: &str, data: &[u8]) -> bool {
    unsafe { ROOT_FS.write_file(path, 0, data, 0, 0) }
}

pub fn read(path: &str) -> Option<&'static [u8]> {
    unsafe { ROOT_FS.read_file(path, 0, 0, 0) }
}

// ── directory operations ─────────────────────────────────────────────────────

pub fn mkdir(path: &str, cwd: usize, uid: u16, gid: u16, mode: u16) -> bool {
    unsafe { ROOT_FS.mkdir(path, cwd, uid, gid, mode) }
}

pub fn rmdir(path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
    unsafe { ROOT_FS.rmdir(path, cwd, uid, gid) }
}

// ── file operations ───────────────────────────────────────────────────────────

pub fn touch(path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
    unsafe { ROOT_FS.touch(path, cwd, uid, gid) }
}

pub fn unlink(path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
    unsafe { ROOT_FS.unlink(path, cwd, uid, gid) }
}

pub fn write_as(path: &str, cwd: usize, data: &[u8], uid: u16, gid: u16) -> bool {
    unsafe { ROOT_FS.write_file(path, cwd, data, uid, gid) }
}

pub fn read_as(path: &str, cwd: usize, uid: u16, gid: u16) -> Option<&'static [u8]> {
    unsafe { ROOT_FS.read_file(path, cwd, uid, gid) }
}

// ── permission operations ─────────────────────────────────────────────────────

pub fn chmod(path: &str, cwd: usize, mode: u16, caller_uid: u16) -> bool {
    unsafe { ROOT_FS.chmod(path, cwd, mode, caller_uid) }
}

pub fn chown(path: &str, cwd: usize, new_uid: u16, new_gid: u16, caller_uid: u16) -> bool {
    unsafe { ROOT_FS.chown(path, cwd, new_uid, new_gid, caller_uid) }
}

// ── path / inode helpers ──────────────────────────────────────────────────────

pub fn resolve(path: &str, cwd: usize) -> Option<usize> {
    unsafe { ROOT_FS.resolve(path, cwd) }
}

pub fn inode_path(idx: usize, buf: &mut [u8]) -> usize {
    unsafe { ROOT_FS.inode_path(idx, buf) }
}

pub fn is_dir(path: &str, cwd: usize) -> Option<bool> {
    unsafe { ROOT_FS.is_dir(path, cwd) }
}

// ── directory listing ─────────────────────────────────────────────────────────

pub fn list_dir<F: FnMut(&Inode)>(path: &str, cwd: usize, mut f: F) {
    unsafe { ROOT_FS.list_dir(path, cwd, &mut f) }
}

// ── stat / rename ─────────────────────────────────────────────────────────────

pub fn stat_at<F: FnOnce(&Inode)>(path: &str, cwd: usize, f: F) -> bool {
    unsafe {
        if let Some(idx) = ROOT_FS.resolve(path, cwd) {
            if let Some(inode) = ROOT_FS.get_inode(idx) {
                f(inode);
                return true;
            }
        }
        false
    }
}

pub fn rename(src: &str, dst: &str, cwd: usize, uid: u16, gid: u16) -> bool {
    unsafe { ROOT_FS.rename(src, dst, cwd, uid, gid) }
}

/// Resolve `path` if it is a directory; returns its inode index.
pub fn open_dir(path: &str, cwd: usize) -> Option<usize> {
    if is_dir(path, cwd) == Some(true) { resolve(path, cwd) } else { None }
}

/// Call `f` with the Inode at `idx`. Returns false if not found.
pub fn stat_inode_by_idx<F: FnOnce(&Inode)>(idx: usize, f: F) -> bool {
    unsafe {
        match ROOT_FS.get_inode(idx) { Some(n) => { f(n); true } None => false }
    }
}

/// Iterate direct children of `dir_inode`, calling `f` with (child_idx, &child_inode).
pub fn list_dir_indexed<F: FnMut(usize, &Inode)>(dir_inode: usize, f: F) {
    unsafe { ROOT_FS.list_dir_indexed(dir_inode, f) }
}

// ── inode-indexed I/O (for file descriptor layer) ─────────────────────────────

/// Return the inode index for `path`, checking the caller's permission.
/// Returns None for directories, non-existent paths, or access denied.
pub fn open_inode(path: &str, cwd: usize, uid: u16, gid: u16, writable: bool) -> Option<usize> {
    unsafe { ROOT_FS.open_check(path, cwd, uid, gid, writable) }
}

/// Read up to `buf.len()` bytes from inode `idx` at `offset`. Returns bytes read.
pub fn inode_size(idx: usize) -> usize {
    unsafe { ROOT_FS.get_inode(idx).map(|n| n.size).unwrap_or(0) }
}

pub fn read_inode(idx: usize, offset: usize, buf: &mut [u8]) -> usize {
    unsafe { ROOT_FS.inode_read(idx, offset, buf) }
}

/// Write `data` into inode `idx` at `offset`. Returns bytes written.
pub fn write_inode(idx: usize, offset: usize, data: &[u8]) -> usize {
    unsafe { ROOT_FS.inode_write(idx, offset, data) }
}

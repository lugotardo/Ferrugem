/// RAM filesystem with directory tree, Unix permissions, and uid/gid ownership.

use super::{FileSystem, FileHandle};

pub const MAX_INODES: usize = 64;
pub const MAX_NAME:   usize = 64;
pub const MAX_DATA:   usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InodeKind { File, Dir }

#[derive(Clone, Copy)]
pub struct Inode {
    pub used:     bool,
    pub kind:     InodeKind,
    pub name:     [u8; MAX_NAME],
    pub name_len: usize,
    pub parent:   usize,
    pub uid:      u16,
    pub gid:      u16,
    pub mode:     u16,   // low 9 bits: rwxrwxrwx
    pub data:     [u8; MAX_DATA],
    pub size:     usize,
}

impl Inode {
    const fn empty() -> Self {
        Self {
            used: false, kind: InodeKind::File,
            name: [0u8; MAX_NAME], name_len: 0,
            parent: 0, uid: 0, gid: 0, mode: 0o644,
            data: [0u8; MAX_DATA], size: 0,
        }
    }
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
    fn set_name(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(MAX_NAME);
        self.name[..n].copy_from_slice(&b[..n]);
        self.name_len = n;
    }
    fn name_eq(&self, s: &str) -> bool {
        let b = s.as_bytes();
        self.name_len == b.len() && &self.name[..self.name_len] == b
    }
}

pub struct RamFs {
    inodes: [Inode; MAX_INODES],
}

impl RamFs {
    pub const fn new() -> Self {
        Self { inodes: [const { Inode::empty() }; MAX_INODES] }
    }

    pub fn init_root(&mut self) {
        let r = &mut self.inodes[0];
        r.used     = true;
        r.kind     = InodeKind::Dir;
        r.name[0]  = b'/';
        r.name_len = 1;
        r.parent   = 0;
        r.uid = 0; r.gid = 0; r.mode = 0o755;
    }

    fn alloc(&mut self) -> Option<usize> {
        for i in 1..MAX_INODES {
            if !self.inodes[i].used {
                self.inodes[i] = Inode::empty();
                self.inodes[i].used = true;
                return Some(i);
            }
        }
        None
    }

    fn lookup_in(&self, parent: usize, name: &str) -> Option<usize> {
        match name {
            "."  => return Some(parent),
            ".." => return Some(self.inodes[parent].parent),
            _    => {}
        }
        for i in 0..MAX_INODES {
            let n = &self.inodes[i];
            if n.used && n.parent == parent && i != parent && n.name_eq(name) {
                return Some(i);
            }
        }
        None
    }

    pub fn resolve(&self, path: &str, cwd: usize) -> Option<usize> {
        let mut cur = if path.starts_with('/') { 0 } else { cwd };
        let stripped = path.trim_start_matches('/');
        if stripped.is_empty() { return Some(cur); }
        for part in stripped.split('/') {
            if part.is_empty() || part == "." { continue; }
            cur = self.lookup_in(cur, part)?;
        }
        Some(cur)
    }

    fn split_parent(path: &str) -> (&str, &str) {
        let path = path.trim_end_matches('/');
        if let Some(sep) = path.rfind('/') {
            let p = if sep == 0 { "/" } else { &path[..sep] };
            (p, &path[sep + 1..])
        } else {
            ("", path)
        }
    }

    fn perm(&self, idx: usize, uid: u16, gid: u16, op: u16) -> bool {
        if uid == 0 { return true; }
        let n = &self.inodes[idx];
        let bits = if n.uid == uid      { n.mode >> 6 }
                   else if n.gid == gid { n.mode >> 3 }
                   else                 { n.mode };
        bits & op != 0
    }

    /// Unix sticky-directory removal rule: an entry inside `parent` may be
    /// removed by root, by the entry's own owner, or — only when `parent`'s
    /// sticky bit (mode & 0o1000) is *not* set — by anyone with write access
    /// to `parent`. Without this, any world-writable directory (e.g. /tmp,
    /// mode 0o777) would let any user delete files they don't own.
    fn can_delete(&self, parent: usize, target_idx: usize, uid: u16, gid: u16) -> bool {
        if uid == 0 { return true; }
        if self.inodes[target_idx].uid == uid { return true; }
        if !self.perm(parent, uid, gid, 2) { return false; }
        let sticky = self.inodes[parent].mode & 0o1000 != 0;
        !sticky || self.inodes[parent].uid == uid
    }

    // ── write / read ─────────────────────────────────────────────────────────

    pub fn write(&mut self, path: &str, cwd: usize, data: &[u8], uid: u16, gid: u16) -> bool {
        let existing = self.resolve(path, cwd);
        let idx = if let Some(i) = existing {
            if self.inodes[i].kind == InodeKind::Dir { return false; }
            if !self.perm(i, uid, gid, 2) { return false; }
            i
        } else {
            let (pp, name) = Self::split_parent(path);
            let parent = match self.resolve(if pp.is_empty() { "." } else { pp }, cwd) {
                Some(p) => p,
                None    => return false,
            };
            if !self.perm(parent, uid, gid, 2) { return false; }
            let i = match self.alloc() { Some(i) => i, None => return false };
            self.inodes[i].kind   = InodeKind::File;
            self.inodes[i].parent = parent;
            self.inodes[i].uid    = uid;
            self.inodes[i].gid    = gid;
            self.inodes[i].mode   = 0o644;
            self.inodes[i].set_name(name);
            i
        };
        let len = data.len().min(MAX_DATA);
        self.inodes[idx].data[..len].copy_from_slice(&data[..len]);
        self.inodes[idx].size = len;
        true
    }

    pub fn read_at(&self, path: &str, cwd: usize, uid: u16, gid: u16) -> Option<&[u8]> {
        let idx = self.resolve(path, cwd)?;
        if self.inodes[idx].kind == InodeKind::Dir { return None; }
        if !self.perm(idx, uid, gid, 4) { return None; }
        Some(&self.inodes[idx].data[..self.inodes[idx].size])
    }

    pub fn read_bytes(&self, inode: usize) -> &[u8] {
        &self.inodes[inode].data[..self.inodes[inode].size]
    }

    // ── mkdir / rmdir / touch / unlink ────────────────────────────────────────

    pub fn mkdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16, mode: u16) -> bool {
        let (pp, name) = Self::split_parent(path);
        let parent = match self.resolve(if pp.is_empty() { "." } else { pp }, cwd) {
            Some(p) => p,
            None    => return false,
        };
        if self.inodes[parent].kind != InodeKind::Dir { return false; }
        if !self.perm(parent, uid, gid, 2) { return false; }
        if self.lookup_in(parent, name).is_some() { return false; }
        let i = match self.alloc() { Some(i) => i, None => return false };
        self.inodes[i].kind   = InodeKind::Dir;
        self.inodes[i].parent = parent;
        self.inodes[i].uid    = uid;
        self.inodes[i].gid    = gid;
        self.inodes[i].mode   = mode & 0o1777;
        self.inodes[i].set_name(name);
        true
    }

    pub fn rmdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        let idx = match self.resolve(path, cwd) { Some(i) => i, None => return false };
        if idx == 0 { return false; }
        if self.inodes[idx].kind != InodeKind::Dir { return false; }
        for i in 0..MAX_INODES {
            if i != idx && self.inodes[i].used && self.inodes[i].parent == idx {
                return false; // not empty
            }
        }
        let parent = self.inodes[idx].parent;
        if !self.can_delete(parent, idx, uid, gid) { return false; }
        self.inodes[idx].used = false;
        true
    }

    pub fn touch(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        if self.resolve(path, cwd).is_some() { return true; }
        let (pp, name) = Self::split_parent(path);
        let parent = match self.resolve(if pp.is_empty() { "." } else { pp }, cwd) {
            Some(p) => p,
            None    => return false,
        };
        if !self.perm(parent, uid, gid, 2) { return false; }
        let i = match self.alloc() { Some(i) => i, None => return false };
        self.inodes[i].kind   = InodeKind::File;
        self.inodes[i].parent = parent;
        self.inodes[i].uid    = uid;
        self.inodes[i].gid    = gid;
        self.inodes[i].mode   = 0o644;
        self.inodes[i].set_name(name);
        true
    }

    pub fn unlink(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        let idx = match self.resolve(path, cwd) { Some(i) => i, None => return false };
        if idx == 0 { return false; }
        if self.inodes[idx].kind == InodeKind::Dir { return false; }
        let parent = self.inodes[idx].parent;
        if !self.can_delete(parent, idx, uid, gid) { return false; }
        self.inodes[idx].used = false;
        true
    }

    // ── chmod / chown ─────────────────────────────────────────────────────────

    pub fn chmod(&mut self, path: &str, cwd: usize, mode: u16, caller_uid: u16) -> bool {
        let idx = match self.resolve(path, cwd) { Some(i) => i, None => return false };
        if caller_uid != 0 && self.inodes[idx].uid != caller_uid { return false; }
        self.inodes[idx].mode = mode & 0o1777;
        true
    }

    pub fn chown(&mut self, path: &str, cwd: usize, new_uid: u16, new_gid: u16, caller_uid: u16) -> bool {
        if caller_uid != 0 { return false; }
        let idx = match self.resolve(path, cwd) { Some(i) => i, None => return false };
        self.inodes[idx].uid = new_uid;
        self.inodes[idx].gid = new_gid;
        true
    }

    // ── directory listing (callback avoids large stack allocs) ────────────────

    /// Iterate all direct children of `dir_idx`, calling `f` with (child_idx, &child_inode).
    pub fn list_dir_indexed<F: FnMut(usize, &Inode)>(&self, dir_idx: usize, mut f: F) {
        if dir_idx >= MAX_INODES || !self.inodes[dir_idx].used { return; }
        if self.inodes[dir_idx].kind != InodeKind::Dir { return; }
        for i in 0..MAX_INODES {
            let n = &self.inodes[i];
            if n.used && n.parent == dir_idx && i != dir_idx {
                f(i, n);
            }
        }
    }

    pub fn list_dir<F: FnMut(&Inode)>(&self, path: &str, cwd: usize, mut f: F) {
        let dir_idx = match self.resolve(path, cwd) { Some(i) => i, None => return };
        if self.inodes[dir_idx].kind != InodeKind::Dir { return; }
        for i in 0..MAX_INODES {
            let n = &self.inodes[i];
            if n.used && n.parent == dir_idx && i != dir_idx {
                f(n);
            }
        }
    }

    // ── path reconstruction ───────────────────────────────────────────────────

    pub fn inode_path(&self, mut idx: usize, buf: &mut [u8]) -> usize {
        if idx == 0 {
            if !buf.is_empty() { buf[0] = b'/'; }
            return 1;
        }
        // Walk up to root collecting inode indices
        let mut stack = [0usize; 16];
        let mut depth = 0usize;
        while idx != 0 && depth < 16 {
            let n = &self.inodes[idx];
            if !n.used { break; }
            stack[depth] = idx;
            depth += 1;
            if n.parent == idx { break; }
            idx = n.parent;
        }
        let mut pos = 0usize;
        for d in (0..depth).rev() {
            let n = &self.inodes[stack[d]];
            if pos < buf.len() { buf[pos] = b'/'; pos += 1; }
            let clen = n.name_len.min(buf.len().saturating_sub(pos));
            buf[pos..pos + clen].copy_from_slice(&n.name[..clen]);
            pos += clen;
        }
        pos
    }

    pub fn is_dir(&self, path: &str, cwd: usize) -> Option<bool> {
        let idx = self.resolve(path, cwd)?;
        Some(self.inodes[idx].kind == InodeKind::Dir)
    }

    pub fn get_inode(&self, idx: usize) -> Option<&Inode> {
        if idx < MAX_INODES && self.inodes[idx].used {
            Some(&self.inodes[idx])
        } else {
            None
        }
    }

    /// Return the inode index for `path` if the caller has the requested permission.
    /// Returns None for directories, missing paths, or permission failures.
    pub fn open_check(&self, path: &str, cwd: usize, uid: u16, gid: u16, write: bool) -> Option<usize> {
        let idx = self.resolve(path, cwd)?;
        if self.inodes[idx].kind == InodeKind::Dir { return None; }
        let op: u16 = if write { 2 } else { 4 };
        if !self.perm(idx, uid, gid, op) { return None; }
        Some(idx)
    }

    /// Read up to `buf.len()` bytes from inode `idx` starting at `offset`.
    pub fn inode_read(&self, idx: usize, offset: usize, buf: &mut [u8]) -> usize {
        if idx >= MAX_INODES || !self.inodes[idx].used || self.inodes[idx].kind == InodeKind::Dir {
            return 0;
        }
        let start = offset.min(self.inodes[idx].size);
        let avail = self.inodes[idx].size - start;
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&self.inodes[idx].data[start..start + n]);
        n
    }

    /// Write `data` into inode `idx` at `offset`, extending the file if needed.
    pub fn inode_write(&mut self, idx: usize, offset: usize, data: &[u8]) -> usize {
        if idx >= MAX_INODES || !self.inodes[idx].used || self.inodes[idx].kind == InodeKind::Dir {
            return 0;
        }
        let end = (offset + data.len()).min(MAX_DATA);
        let n = end.saturating_sub(offset);
        if n == 0 { return 0; }
        self.inodes[idx].data[offset..offset + n].copy_from_slice(&data[..n]);
        if end > self.inodes[idx].size { self.inodes[idx].size = end; }
        n
    }

    /// Rename/move `src` to `dst`.  Both paths are relative to `cwd`.
    /// Caller must own or be root (uid == 0) to write to both parent dirs.
    pub fn rename(&mut self, src: &str, dst: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        let src_idx = match self.resolve(src, cwd) { Some(i) => i, None => return false };
        if src_idx == 0 { return false; }
        let src_parent = self.inodes[src_idx].parent;
        if !self.perm(src_parent, uid, gid, 2) { return false; }

        let (dst_pp, dst_name) = Self::split_parent(dst);
        let dst_parent = match self.resolve(if dst_pp.is_empty() { "." } else { dst_pp }, cwd) {
            Some(p) => p,
            None    => return false,
        };
        if !self.perm(dst_parent, uid, gid, 2) { return false; }

        // If destination already exists, allow overwrite only for files.
        if let Some(dst_idx) = self.lookup_in(dst_parent, dst_name) {
            if dst_idx == src_idx { return true; }
            if self.inodes[dst_idx].kind == InodeKind::Dir { return false; }
            self.inodes[dst_idx].used = false;
        }

        self.inodes[src_idx].parent = dst_parent;
        self.inodes[src_idx].set_name(dst_name);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `RamFs` is ~262 KiB (MAX_INODES * MAX_DATA), same as production's
    // `fs::ROOT_FS` — too big for the small boot stack, so each test backs
    // it with its own `static mut` instead of a stack local.

    #[test_case]
    fn write_then_read_roundtrip() {
        static mut FS: RamFs = RamFs::new();
        let fs = unsafe { &mut FS };
        fs.init_root();
        assert!(fs.write("/hello.txt", 0, b"hi", 0, 0));
        let data = fs.read_at("/hello.txt", 0, 0, 0).expect("file should exist after write");
        assert_eq!(data, b"hi");
    }

    #[test_case]
    fn resolve_missing_path_is_none() {
        static mut FS: RamFs = RamFs::new();
        let fs = unsafe { &mut FS };
        fs.init_root();
        assert_eq!(fs.resolve("/nope", 0), None);
    }

    #[test_case]
    fn mkdir_then_write_inside_it() {
        static mut FS: RamFs = RamFs::new();
        let fs = unsafe { &mut FS };
        fs.init_root();
        assert!(fs.mkdir("/dir", 0, 0, 0, 0o755));
        assert!(fs.write("/dir/file.txt", 0, b"x", 0, 0));
        assert_eq!(fs.read_at("/dir/file.txt", 0, 0, 0), Some(&b"x"[..]));
    }
}

impl FileSystem for RamFs {
    fn name(&self) -> &'static str { "ramfs" }

    fn open(&self, path: &str) -> Option<FileHandle> {
        let idx = self.resolve(path, 0)?;
        Some(FileHandle::new(idx, self.inodes[idx].size as u64))
    }

    fn init_root(&mut self) { self.init_root(); }

    fn resolve(&self, path: &str, cwd: usize) -> Option<usize> {
        self.resolve(path, cwd)
    }

    fn is_dir(&self, path: &str, cwd: usize) -> Option<bool> {
        self.is_dir(path, cwd)
    }

    fn inode_path(&self, idx: usize, buf: &mut [u8]) -> usize {
        self.inode_path(idx, buf)
    }

    fn get_inode(&self, idx: usize) -> Option<&Inode> {
        self.get_inode(idx)
    }

    fn write_file(&mut self, path: &str, cwd: usize, data: &[u8], uid: u16, gid: u16) -> bool {
        self.write(path, cwd, data, uid, gid)
    }

    fn read_file(&self, path: &str, cwd: usize, uid: u16, gid: u16) -> Option<&[u8]> {
        self.read_at(path, cwd, uid, gid)
    }

    fn mkdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16, mode: u16) -> bool {
        self.mkdir(path, cwd, uid, gid, mode)
    }

    fn rmdir(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        self.rmdir(path, cwd, uid, gid)
    }

    fn list_dir(&self, path: &str, cwd: usize, f: &mut dyn FnMut(&Inode)) {
        self.list_dir(path, cwd, |inode| f(inode));
    }

    fn touch(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        self.touch(path, cwd, uid, gid)
    }

    fn unlink(&mut self, path: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        self.unlink(path, cwd, uid, gid)
    }

    fn rename(&mut self, src: &str, dst: &str, cwd: usize, uid: u16, gid: u16) -> bool {
        self.rename(src, dst, cwd, uid, gid)
    }

    fn chmod(&mut self, path: &str, cwd: usize, mode: u16, caller_uid: u16) -> bool {
        self.chmod(path, cwd, mode, caller_uid)
    }

    fn chown(&mut self, path: &str, cwd: usize, new_uid: u16, new_gid: u16, caller_uid: u16) -> bool {
        self.chown(path, cwd, new_uid, new_gid, caller_uid)
    }
}

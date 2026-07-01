/// Unix-compatible syscall ABI.
///
/// Numbers match Linux x86_64; RISC-V uses the same numbers.
/// Negative return values are errno values (e.g. -2 = ENOENT, -22 = EINVAL).

use crate::process::FdEntry;
use crate::vfs::InodeKind;

// ── saved user context ────────────────────────────────────────────────────────

static mut SYSCALL_USER_IP: u64 = 0;
static mut SYSCALL_USER_SP: u64 = 0;

pub fn set_user_ctx(ip: u64, sp: u64) {
    unsafe { SYSCALL_USER_IP = ip; SYSCALL_USER_SP = sp; }
}

// ── exec redirect (set by sys_execve, consumed by arch trap handler) ──────────

static mut EXEC_CTX: Option<(u64, u64, u64)> = None; // (new_ip, new_sp, new_pt_phys)

pub fn set_exec_ctx(ip: u64, sp: u64, pt: u64) {
    unsafe { EXEC_CTX = Some((ip, sp, pt)); }
}

pub fn take_exec_ctx() -> Option<(u64, u64, u64)> {
    unsafe { EXEC_CTX.take() }
}

// ── syscall numbers ───────────────────────────────────────────────────────────

pub const SYS_READ:          usize = 0;
pub const SYS_WRITE:         usize = 1;
pub const SYS_RT_SIGACTION:  usize = 13;
pub const SYS_RT_SIGPROCMASK: usize = 14;
pub const SYS_RT_SIGRETURN:  usize = 15;
pub const SYS_OPEN:          usize = 2;
pub const SYS_CLOSE:         usize = 3;
pub const SYS_STAT:          usize = 4;
pub const SYS_FSTAT:         usize = 5;
pub const SYS_LSTAT:         usize = 6;
pub const SYS_LSEEK:         usize = 8;
pub const SYS_MMAP:          usize = 9;
pub const SYS_MUNMAP:        usize = 11;
pub const SYS_BRK:           usize = 12;
pub const SYS_IOCTL:         usize = 16;
pub const SYS_PIPE:          usize = 22;
pub const SYS_YIELD:         usize = 24;
pub const SYS_DUP:           usize = 32;
pub const SYS_DUP2:          usize = 33;
pub const SYS_NANOSLEEP:     usize = 35;
pub const SYS_GETPID:        usize = 39;
pub const SYS_FCNTL:         usize = 72;
pub const SYS_GETCWD:        usize = 79;
pub const SYS_CHDIR:         usize = 80;
pub const SYS_MKDIR:         usize = 83;
pub const SYS_RMDIR:         usize = 84;
pub const SYS_UNLINK:        usize = 87;
pub const SYS_GETUID:        usize = 102;
pub const SYS_GETGID:        usize = 104;
pub const SYS_GETEUID:       usize = 107;
pub const SYS_GETEGID:       usize = 108;
pub const SYS_GETPPID:       usize = 110;
pub const SYS_FORK:          usize = 57;
pub const SYS_EXECVE:        usize = 59;
pub const SYS_EXIT:          usize = 60;
pub const SYS_WAIT4:         usize = 61;
pub const SYS_KILL:          usize = 62;
pub const SYS_UNAME:         usize = 63;
pub const SYS_CLOCK_GETTIME: usize = 228;
pub const SYS_EXIT_GROUP:    usize = 231;
pub const SYS_GETDENTS64:    usize = 217;
pub const SYS_ARCH_PRCTL:   usize = 158;
pub const SYS_GETTID:        usize = 186;
pub const SYS_TKILL:         usize = 200;
pub const SYS_FUTEX:         usize = 202;
pub const SYS_SET_ROBUST_LIST: usize = 273;
pub const SYS_SET_TID_ADDRESS: usize = 218;
pub const SYS_TGKILL:        usize = 234;
pub const SYS_MPROTECT:      usize = 10;
pub const SYS_PRLIMIT64:     usize = 302;
pub const SYS_GETRANDOM:     usize = 318;
pub const SYS_OPENAT:        usize = 257;
pub const SYS_NEWFSTATAT:    usize = 262;
pub const SYS_READLINKAT:    usize = 267;
pub const SYS_RENAMEAT:      usize = 264;
pub const SYS_UNLINKAT:      usize = 263;
pub const SYS_MKDIRAT:       usize = 258;

// ── dispatcher ────────────────────────────────────────────────────────────────

pub fn dispatch(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> isize {
    match nr {
        SYS_READ          => sys_read(a0, a1 as *mut u8, a2),
        SYS_WRITE         => sys_write(a0, a1 as *const u8, a2),
        SYS_RT_SIGACTION  => sys_rt_sigaction(a0, a1, a2, a3),
        SYS_RT_SIGPROCMASK => sys_rt_sigprocmask(a0, a1, a2, a3),
        SYS_RT_SIGRETURN  => 0,
        SYS_ARCH_PRCTL    => sys_arch_prctl(a0, a1),
        SYS_MPROTECT      => 0,  // stub: pretend all protection changes succeed
        SYS_GETTID        => sys_getpid(), // single-threaded: TID == PID
        SYS_TKILL         => sys_kill(a0, a1),
        SYS_TGKILL        => sys_kill(a1, a2), // tgkill(tgid, tid, sig) — tid==PID
        SYS_FUTEX         => sys_futex(a0, a1, a2),
        SYS_SET_TID_ADDRESS => sys_getpid(), // record tidptr, return TID
        SYS_SET_ROBUST_LIST => 0,  // stub
        SYS_PRLIMIT64     => 0,    // stub: all limits succeed
        SYS_GETRANDOM     => sys_getrandom(a0 as *mut u8, a1, a2),
        SYS_OPENAT        => sys_openat(a0 as i64, a1 as *const u8, a2, a3),
        SYS_NEWFSTATAT    => sys_newfstatat(a0 as i64, a1 as *const u8, a2 as *mut Stat, a3),
        SYS_READLINKAT    => -22, // EINVAL — no symlinks yet
        SYS_RENAMEAT      => -38, // ENOSYS
        SYS_UNLINKAT      => sys_unlinkat(a0 as i64, a1 as *const u8, a2),
        SYS_MKDIRAT       => sys_mkdirat(a0 as i64, a1 as *const u8, a2),
        SYS_OPEN          => sys_open(a0 as *const u8, a1, a2),
        SYS_CLOSE         => sys_close(a0),
        SYS_STAT          => sys_stat(a0 as *const u8, a1 as *mut Stat),
        SYS_FSTAT         => sys_fstat(a0, a1 as *mut Stat),
        SYS_LSTAT         => sys_stat(a0 as *const u8, a1 as *mut Stat),
        SYS_LSEEK         => sys_lseek(a0, a1 as i64, a2),
        SYS_MMAP          => sys_mmap(a0, a1, a2, a3),
        SYS_MUNMAP        => sys_munmap(a0, a1),
        SYS_BRK           => sys_brk(a0),
        SYS_IOCTL         => sys_ioctl(a0, a1, a2),
        SYS_PIPE          => sys_pipe(a0 as *mut i32),
        SYS_YIELD         => sys_yield(),
        SYS_DUP           => sys_dup(a0),
        SYS_DUP2          => sys_dup2(a0, a1),
        SYS_NANOSLEEP     => sys_nanosleep(a0 as *const TimeSpec),
        SYS_GETPID        => sys_getpid(),
        SYS_FCNTL         => sys_fcntl(a0, a1, a2),
        SYS_GETCWD        => sys_getcwd(a0 as *mut u8, a1),
        SYS_CHDIR         => sys_chdir(a0 as *const u8),
        SYS_MKDIR         => sys_mkdir(a0 as *const u8, a1),
        SYS_RMDIR         => sys_rmdir(a0 as *const u8),
        SYS_UNLINK        => sys_unlink(a0 as *const u8),
        SYS_GETUID | SYS_GETEUID | SYS_GETGID | SYS_GETEGID => 0,
        SYS_GETPPID       => sys_getppid(),
        SYS_FORK          => sys_fork(),
        SYS_EXECVE        => sys_execve(a0, a1, a2),
        SYS_EXIT          => sys_exit(a0 as i32),
        SYS_EXIT_GROUP    => sys_exit(a0 as i32),
        SYS_WAIT4         => sys_wait4(a0 as i64, a1 as *mut i32, a2),
        SYS_KILL          => sys_kill(a0, a1),
        SYS_UNAME         => sys_uname(a0 as *mut UtsName),
        SYS_CLOCK_GETTIME => sys_clock_gettime(a0, a1 as *mut TimeSpec),
        SYS_GETDENTS64    => sys_getdents64(a0, a1 as *mut u8, a2),
        _                 => -38, // ENOSYS
    }
}

// ── path helper ───────────────────────────────────────────────────────────────

unsafe fn read_path<'a>(ptr: *const u8) -> Option<&'a str> {
    if ptr.is_null() { return None; }
    let mut len = 0usize;
    while len < 256 && *ptr.add(len) != 0 { len += 1; }
    core::str::from_utf8(core::slice::from_raw_parts(ptr, len)).ok()
}

// ── read ──────────────────────────────────────────────────────────────────────

fn sys_read(fd: usize, buf: *mut u8, len: usize) -> isize {
    if len == 0 { return 0; }
    if buf.is_null() { return -14; } // EFAULT

    let entry = match crate::scheduler::current_fd_get(fd) {
        Some(e) => e,
        None    => return -9, // EBADF
    };

    match entry {
        FdEntry::Stdin => {
            loop {
                if let Some(b) = crate::drivers::serial::read_byte() {
                    unsafe { *buf = b };
                    return 1;
                }
                if let Some(sc) = crate::drivers::keyboard::read_scancode() {
                    if let Some(c) = crate::drivers::keyboard::scancode_to_ascii(sc) {
                        unsafe { *buf = c };
                        return 1;
                    }
                }
                crate::scheduler::block_on_tty();
            }
        }
        FdEntry::VfsFile { inode, offset, .. } => {
            let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
            let n = crate::fs::read_inode(inode, offset as usize, out);
            if n > 0 {
                crate::scheduler::current_fd_set_offset(fd, offset + n as u64);
            }
            n as isize
        }
        FdEntry::PipeRead { idx } => {
            loop {
                let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
                let n = crate::ipc::pipe_read(idx, out);
                if n > 0 { return n as isize; }
                if !crate::ipc::pipe_write_open(idx) { return 0; }
                crate::scheduler::block_on_pipe(idx);
            }
        }
        FdEntry::VfsDir { .. }              => -21, // EISDIR
        FdEntry::Stdout | FdEntry::Stderr   => -9,
        FdEntry::PipeWrite { .. }           => -9,
        FdEntry::Empty                      => -9,
    }
}

// ── write ─────────────────────────────────────────────────────────────────────

fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    if len == 0 { return 0; }
    if buf.is_null() { return -14; } // EFAULT

    let entry = match crate::scheduler::current_fd_get(fd) {
        Some(e) => e,
        None    => return -9, // EBADF
    };

    match entry {
        FdEntry::Stdout | FdEntry::Stderr => {
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            crate::drivers::serial::print_bytes(slice);
            len as isize
        }
        FdEntry::VfsFile { inode, offset, writable } => {
            if !writable { return -13; } // EACCES
            let data = unsafe { core::slice::from_raw_parts(buf, len) };
            let n = crate::fs::write_inode(inode, offset as usize, data);
            if n > 0 {
                crate::scheduler::current_fd_set_offset(fd, offset + n as u64);
            }
            n as isize
        }
        FdEntry::PipeWrite { idx } => {
            if !crate::ipc::pipe_read_open(idx) { return -32; } // EPIPE
            let data = unsafe { core::slice::from_raw_parts(buf, len) };
            let n = crate::ipc::pipe_write(idx, data);
            if n > 0 { crate::scheduler::wake_pipe_waiter(idx); }
            n as isize
        }
        FdEntry::VfsDir { .. }          => -21, // EISDIR
        FdEntry::Stdin                  => -9,
        FdEntry::PipeRead { .. }        => -9,
        FdEntry::Empty                  => -9,
    }
}

// ── open ──────────────────────────────────────────────────────────────────────

fn sys_open(path_ptr: *const u8, flags: usize, _mode: usize) -> isize {
    if path_ptr.is_null() { return -14; } // EFAULT
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2, // ENOENT
    };

    // O_WRONLY=1, O_RDWR=2, O_CREAT=0x40, O_TRUNC=0x200, O_DIRECTORY=0x10000
    let writable  = flags & 3 != 0;
    let create    = flags & 0x40 != 0;
    let truncate  = flags & 0x200 != 0;

    let cwd = crate::scheduler::current_cwd();

    if create { crate::fs::touch(path, cwd, 0, 0); }

    match crate::fs::open_inode(path, cwd, 0, 0, writable) {
        Some(inode) => {
            if truncate && writable { crate::fs::write_inode(inode, 0, &[]); }
            let entry = FdEntry::VfsFile { inode, offset: 0, writable };
            match crate::scheduler::current_fd_alloc(entry) {
                Some(fd) => fd as isize,
                None     => -24, // EMFILE
            }
        }
        None => {
            if writable { return -2; } // can't open dirs for writing
            if let Some(dir_inode) = crate::fs::open_dir(path, cwd) {
                let entry = FdEntry::VfsDir { inode: dir_inode, pos: 0 };
                match crate::scheduler::current_fd_alloc(entry) {
                    Some(fd) => fd as isize,
                    None     => -24, // EMFILE
                }
            } else {
                -2 // ENOENT
            }
        }
    }
}

// ── close ─────────────────────────────────────────────────────────────────────

fn sys_close(fd: usize) -> isize {
    match crate::scheduler::current_fd_close(fd) {
        None => -9, // EBADF
        Some(FdEntry::PipeRead  { idx }) => { crate::ipc::close_pipe_read(idx);  0 }
        Some(FdEntry::PipeWrite { idx }) => { crate::ipc::close_pipe_write(idx); 0 }
        Some(_) => 0,
    }
}

// ── stat / fstat ──────────────────────────────────────────────────────────────

#[repr(C)]
struct Stat {
    st_dev:      u64,
    st_ino:      u64,
    st_nlink:    u64,
    st_mode:     u32,
    st_uid:      u32,
    st_gid:      u32,
    _pad0:       u32,
    st_rdev:     u64,
    st_size:     i64,
    st_blksize:  i64,
    st_blocks:   i64,
    st_atime:    i64,
    st_atime_ns: i64,
    st_mtime:    i64,
    st_mtime_ns: i64,
    st_ctime:    i64,
    st_ctime_ns: i64,
    _unused:     [i64; 3],
}

unsafe fn fill_inode_stat(buf: *mut Stat, inode_idx: usize) -> isize {
    if buf.is_null() { return -14; } // EFAULT
    let ok = crate::fs::stat_inode_by_idx(inode_idx, |n| {
        core::ptr::write_bytes(buf as *mut u8, 0, core::mem::size_of::<Stat>());
        let is_dir = n.kind == InodeKind::Dir;
        (*buf).st_dev     = 1;
        (*buf).st_ino     = inode_idx as u64;
        (*buf).st_nlink   = 1;
        (*buf).st_mode    = (if is_dir { 0o040000u32 } else { 0o100000u32 }) | n.mode as u32;
        (*buf).st_uid     = n.uid as u32;
        (*buf).st_gid     = n.gid as u32;
        (*buf).st_size    = if is_dir { 4096 } else { n.size as i64 };
        (*buf).st_blksize = 4096;
        (*buf).st_blocks  = ((*buf).st_size + 511) / 512;
    });
    if ok { 0 } else { -2 } // ENOENT
}

unsafe fn fill_tty_stat(buf: *mut Stat, fd: usize) {
    core::ptr::write_bytes(buf as *mut u8, 0, core::mem::size_of::<Stat>());
    (*buf).st_dev     = 5;
    (*buf).st_ino     = fd as u64 + 1;
    (*buf).st_nlink   = 1;
    (*buf).st_mode    = 0x2190; // S_IFCHR | 0620
    (*buf).st_rdev    = (4 << 8) | 64; // major=4 (tty), minor=64 (ttyS0)
    (*buf).st_blksize = 1024;
}

fn sys_stat(path_ptr: *const u8, buf: *mut Stat) -> isize {
    if buf.is_null() { return -14; } // EFAULT
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = crate::scheduler::current_cwd();
    let idx = match crate::fs::resolve(path, cwd) {
        Some(i) => i,
        None    => return -2, // ENOENT
    };
    unsafe { fill_inode_stat(buf, idx) }
}

fn sys_fstat(fd: usize, buf: *mut Stat) -> isize {
    if buf.is_null() { return -14; } // EFAULT
    let entry = match crate::scheduler::current_fd_get(fd) {
        Some(e) => e,
        None    => return -9, // EBADF
    };
    match entry {
        FdEntry::VfsFile { inode, .. } => unsafe { fill_inode_stat(buf, inode) },
        FdEntry::VfsDir  { inode, .. } => unsafe { fill_inode_stat(buf, inode) },
        FdEntry::Stdin | FdEntry::Stdout | FdEntry::Stderr => {
            unsafe { fill_tty_stat(buf, fd); }
            0
        }
        _ => -9, // EBADF (pipes)
    }
}

// ── brk ───────────────────────────────────────────────────────────────────────

fn sys_brk(addr: usize) -> isize {
    let current = crate::scheduler::current_heap_brk();
    let pt_phys  = crate::scheduler::current_page_table_phys();

    if addr == 0 || pt_phys == 0 { return current as isize; }

    let new_brk = addr as u64;
    if new_brk <= current {
        crate::scheduler::set_heap_brk(new_brk);
        return new_brk as isize;
    }

    let page_lo = ((current + 0xFFF) & !0xFFF) as usize;
    let page_hi = ((new_brk  + 0xFFF) & !0xFFF) as usize;

    let mut va = page_lo;
    while va < page_hi {
        match crate::memory::alloc_pages(1) {
            None => return current as isize,
            Some(phys) => {
                unsafe { core::ptr::write_bytes(phys as *mut u8, 0, 4096); }
                if !crate::arch::map_user_page(
                    pt_phys, va, phys,
                    crate::arch::PROT_READ | crate::arch::PROT_WRITE | crate::arch::PROT_USER,
                ) {
                    return current as isize;
                }
            }
        }
        va += 4096;
    }

    crate::scheduler::set_heap_brk(new_brk);
    new_brk as isize
}

// ── mmap / munmap ─────────────────────────────────────────────────────────────

fn sys_mmap(addr: usize, len: usize, prot: usize, _flags: usize) -> isize {
    if len == 0 { return -22; } // EINVAL
    let pt_phys = crate::scheduler::current_page_table_phys();
    if pt_phys == 0 { return -22; }

    let n_pages = (len + 0xFFF) / 4096;

    let base_va = {
        let brk = ((crate::scheduler::current_heap_brk() + 0xFFF) & !0xFFF) as usize;
        if addr != 0 && addr >= crate::arch::USER_BASE_VA { addr } else { brk }
    };

    let arch_prot = {
        let mut p = crate::arch::PROT_USER;
        if prot & 1 != 0 { p |= crate::arch::PROT_READ; }
        if prot & 2 != 0 { p |= crate::arch::PROT_WRITE; }
        if prot & 4 != 0 { p |= crate::arch::PROT_EXEC; }
        p
    };

    let mut va = base_va;
    for _ in 0..n_pages {
        match crate::memory::alloc_pages(1) {
            None => return -12, // ENOMEM
            Some(phys) => {
                unsafe { core::ptr::write_bytes(phys as *mut u8, 0, 4096); }
                if !crate::arch::map_user_page(pt_phys, va, phys, arch_prot) {
                    return -12;
                }
            }
        }
        va += 4096;
    }

    let top = va as u64;
    if top > crate::scheduler::current_heap_brk() {
        crate::scheduler::set_heap_brk(top);
    }

    base_va as isize
}

fn sys_munmap(_addr: usize, _len: usize) -> isize { 0 }

// ── ioctl ─────────────────────────────────────────────────────────────────────

const TCGETS:     usize = 0x5401;
const TIOCGWINSZ: usize = 0x5413;

fn sys_ioctl(fd: usize, cmd: usize, arg: usize) -> isize {
    match crate::scheduler::current_fd_get(fd) {
        None => return -9, // EBADF
        Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => {}
        Some(_) => return -25, // ENOTTY
    }
    match cmd {
        TCGETS => -25, // ENOTTY — no full termios
        TIOCGWINSZ => {
            if arg == 0 { return -14; } // EFAULT
            // struct winsize: ws_row, ws_col, ws_xpixel, ws_ypixel (4 × u16)
            let ptr = arg as *mut u16;
            unsafe { *ptr = 24; *ptr.add(1) = 80; *ptr.add(2) = 0; *ptr.add(3) = 0; }
            0
        }
        _ => -25, // ENOTTY
    }
}

// ── fcntl ─────────────────────────────────────────────────────────────────────

fn sys_fcntl(fd: usize, cmd: usize, _arg: usize) -> isize {
    if crate::scheduler::current_fd_get(fd).is_none() { return -9; } // EBADF
    match cmd {
        0 => sys_dup(fd), // F_DUPFD
        1 => 0,           // F_GETFD  — no FD_CLOEXEC yet
        2 => 0,           // F_SETFD
        3 => 0o2,         // F_GETFL  — return O_RDWR stub
        4 => 0,           // F_SETFL
        _ => -22,         // EINVAL
    }
}

// ── pipe ──────────────────────────────────────────────────────────────────────

fn sys_pipe(fds_ptr: *mut i32) -> isize {
    if fds_ptr.is_null() { return -14; } // EFAULT
    let idx = match crate::ipc::alloc_pipe() {
        Some(i) => i,
        None    => return -24, // EMFILE
    };
    let r_fd = crate::scheduler::current_fd_alloc(FdEntry::PipeRead  { idx });
    let w_fd = crate::scheduler::current_fd_alloc(FdEntry::PipeWrite { idx });
    match (r_fd, w_fd) {
        (Some(r), Some(w)) => {
            unsafe { *fds_ptr = r as i32; *fds_ptr.add(1) = w as i32; }
            0
        }
        _ => {
            crate::ipc::close_pipe_read(idx);
            crate::ipc::close_pipe_write(idx);
            -24 // EMFILE
        }
    }
}

// ── dup / dup2 ────────────────────────────────────────────────────────────────

fn sys_dup(oldfd: usize) -> isize {
    match crate::scheduler::current_fd_dup(oldfd) { Some(fd) => fd as isize, None => -9 }
}

fn sys_dup2(oldfd: usize, newfd: usize) -> isize {
    if oldfd == newfd {
        return match crate::scheduler::current_fd_get(oldfd) {
            Some(_) => oldfd as isize, None => -9,
        };
    }
    match crate::scheduler::current_fd_dup2(oldfd, newfd) { Some(fd) => fd as isize, None => -9 }
}

// ── getcwd / chdir ────────────────────────────────────────────────────────────

fn sys_getcwd(buf: *mut u8, size: usize) -> isize {
    if buf.is_null() || size == 0 { return -14; } // EFAULT
    let cwd_inode = crate::scheduler::current_cwd();
    let out = unsafe { core::slice::from_raw_parts_mut(buf, size) };
    let n = crate::fs::inode_path(cwd_inode, out);
    if n == 0 || n >= size { return -34; } // ERANGE
    unsafe { *buf.add(n) = 0; }
    buf as isize
}

fn sys_chdir(path_ptr: *const u8) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = crate::scheduler::current_cwd();
    match crate::fs::open_dir(path, cwd) {
        None => -2, // ENOENT / ENOTDIR
        Some(inode) => { crate::scheduler::set_cwd(inode); 0 }
    }
}

// ── mkdir / rmdir / unlink ────────────────────────────────────────────────────

fn sys_mkdir(path_ptr: *const u8, mode: usize) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = crate::scheduler::current_cwd();
    if crate::fs::mkdir(path, cwd, 0, 0, mode as u16) { 0 } else { -17 } // EEXIST / ENOENT
}

fn sys_rmdir(path_ptr: *const u8) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = crate::scheduler::current_cwd();
    if crate::fs::rmdir(path, cwd, 0, 0) { 0 } else { -2 }
}

fn sys_unlink(path_ptr: *const u8) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = crate::scheduler::current_cwd();
    if crate::fs::unlink(path, cwd, 0, 0) { 0 } else { -2 }
}

// ── getdents64 ────────────────────────────────────────────────────────────────

fn write_dirent64(buf: &mut [u8], ino: u64, d_off: i64, name: &[u8], d_type: u8) -> Option<usize> {
    let reclen = ((19 + name.len() + 1) + 7) & !7; // 19-byte header + name + NUL, rounded to 8
    if reclen > buf.len() { return None; }
    buf[0..8].copy_from_slice(&ino.to_le_bytes());
    buf[8..16].copy_from_slice(&(d_off as u64).to_le_bytes());
    buf[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
    buf[18] = d_type;
    buf[19..19 + name.len()].copy_from_slice(name);
    buf[19 + name.len()] = 0;
    for b in &mut buf[19 + name.len() + 1..reclen] { *b = 0; }
    Some(reclen)
}

fn sys_getdents64(fd: usize, buf_ptr: *mut u8, count: usize) -> isize {
    if buf_ptr.is_null() || count == 0 { return -22; } // EINVAL
    let (dir_inode, pos) = match crate::scheduler::current_fd_get(fd) {
        Some(FdEntry::VfsDir { inode, pos }) => (inode, pos),
        Some(_) => return -20, // ENOTDIR
        None    => return -9,  // EBADF
    };

    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, count) };
    let mut written     = 0usize;
    let mut entry_idx   = 0usize;
    let mut new_pos     = pos;

    crate::fs::list_dir_indexed(dir_inode, |inode_idx, inode| {
        if entry_idx < pos { entry_idx += 1; return; }
        let d_type = if inode.kind == InodeKind::Dir { 4u8 } else { 8u8 };
        let name   = &inode.name[..inode.name_len];
        let d_off  = (new_pos + 1) as i64;
        if let Some(reclen) = write_dirent64(&mut buf[written..], inode_idx as u64, d_off, name, d_type) {
            written  += reclen;
            new_pos  += 1;
        }
        entry_idx += 1;
    });

    if written > 0 { crate::scheduler::current_fd_set_dir_pos(fd, new_pos); }
    written as isize
}

// ── execve ────────────────────────────────────────────────────────────────────

fn sys_execve(path_ptr: usize, _argv: usize, _envp: usize) -> isize {
    if path_ptr == 0 { return -14; } // EFAULT
    let path = match unsafe { read_path(path_ptr as *const u8) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };

    let cwd = crate::scheduler::current_cwd();
    let data = match crate::fs::read_as(path, cwd, 0, 0) {
        Some(d) => d,
        None    => return -2, // ENOENT
    };

    if !crate::elf::is_elf(data) { return -8; } // ENOEXEC

    match crate::scheduler::execve_current(data) {
        Some((ip, sp, pt)) => { set_exec_ctx(ip, sp, pt); 0 }
        None               => -12, // ENOMEM
    }
}

// ── yield ─────────────────────────────────────────────────────────────────────

fn sys_yield() -> isize { crate::scheduler::schedule(); 0 }

// ── nanosleep ─────────────────────────────────────────────────────────────────

#[repr(C)]
struct TimeSpec {
    tv_sec:  i64,
    tv_nsec: i64,
}

fn sys_nanosleep(req: *const TimeSpec) -> isize {
    if req.is_null() { return -14; } // EFAULT
    let (secs, nsecs) = unsafe { ((*req).tv_sec, (*req).tv_nsec) };
    if secs < 0 || nsecs < 0 || nsecs >= 1_000_000_000 { return -22; } // EINVAL
    let ticks = (secs as u64) * 100 + (nsecs as u64) / 10_000_000;
    if ticks > 0 { crate::scheduler::sleep_ticks(ticks); }
    0
}

// ── getpid / getppid ──────────────────────────────────────────────────────────

fn sys_getpid()  -> isize { crate::scheduler::current_pid()  as isize }
fn sys_getppid() -> isize { crate::scheduler::current_ppid() as isize }

// ── wait4 ─────────────────────────────────────────────────────────────────────

fn sys_wait4(pid: i64, status_ptr: *mut i32, _options: usize) -> isize {
    let target = if pid == -1 { u64::MAX } else { pid as u64 };
    crate::scheduler::waitpid(target, status_ptr)
}

// ── kill ──────────────────────────────────────────────────────────────────────

fn sys_kill(target_pid: usize, sig: usize) -> isize {
    let sig = sig as u8;
    if sig > 31 { return -22; } // EINVAL
    if crate::scheduler::raise_signal_to_pid(target_pid as u64, sig) { 0 } else { -3 } // ESRCH
}

// ── exit ──────────────────────────────────────────────────────────────────────

fn sys_exit(code: i32) -> isize { crate::scheduler::exit_current(code); 0 }

// ── uname ─────────────────────────────────────────────────────────────────────

#[repr(C)]
struct UtsName {
    sysname:  [u8; 65],
    nodename: [u8; 65],
    release:  [u8; 65],
    version:  [u8; 65],
    machine:  [u8; 65],
}

fn sys_uname(buf: *mut UtsName) -> isize {
    if buf.is_null() { return -14; } // EFAULT
    fn fill(dst: &mut [u8; 65], s: &[u8]) {
        let n = s.len().min(64); dst[..n].copy_from_slice(&s[..n]); dst[n] = 0;
    }
    unsafe {
        fill(&mut (*buf).sysname,  b"Ferrugem");
        fill(&mut (*buf).nodename, b"ferrugem");
        fill(&mut (*buf).release,  crate::version::VERSION_FULL.as_bytes());
        fill(&mut (*buf).version,  b"#1 SMP");
        #[cfg(target_arch = "x86_64")]  fill(&mut (*buf).machine, b"x86_64");
        #[cfg(target_arch = "riscv64")] fill(&mut (*buf).machine, b"riscv64");
    }
    0
}

// ── fork ──────────────────────────────────────────────────────────────────────

fn sys_fork() -> isize {
    let (ip, sp) = unsafe { (SYSCALL_USER_IP, SYSCALL_USER_SP) };
    if ip == 0 { return -38; }
    match crate::scheduler::spawn_fork(ip, sp) {
        Some(slot) => crate::scheduler::task_pid(slot) as isize,
        None       => -12, // ENOMEM
    }
}

// ── lseek ─────────────────────────────────────────────────────────────────────

fn sys_lseek(fd: usize, offset: i64, whence: usize) -> isize {
    let (inode, cur_off) = match crate::scheduler::current_fd_get(fd) {
        Some(FdEntry::VfsFile { inode, offset, .. }) => (inode, offset as i64),
        Some(FdEntry::VfsDir { .. })                 => return -21, // EISDIR
        Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => return -29, // ESPIPE
        Some(FdEntry::PipeRead { .. }) | Some(FdEntry::PipeWrite { .. })     => return -29,
        _ => return -9, // EBADF
    };
    let file_size = crate::fs::inode_size(inode) as i64;
    let new_off = match whence {
        0 => offset,            // SEEK_SET
        1 => cur_off + offset,  // SEEK_CUR
        2 => file_size + offset,// SEEK_END
        _ => return -22,
    };
    if new_off < 0 { return -22; }
    crate::scheduler::current_fd_set_offset(fd, new_off as u64);
    new_off as isize
}

// ── openat / fstatat / unlinkat / mkdirat ────────────────────────────────────
//
// The `*at` variants take a dirfd as first argument.
// AT_FDCWD (-100) means "use cwd" — same as the plain syscall.
// We don't support real dirfd-relative resolution yet: if dirfd != AT_FDCWD
// and the path is relative, we fall back to cwd (acceptable for now).

const AT_FDCWD: i64 = -100;

fn at_cwd(dirfd: i64) -> usize {
    if dirfd == AT_FDCWD {
        return crate::scheduler::current_cwd();
    }
    // Dirfd-relative resolution: look up the inode for the given fd.
    match crate::scheduler::current_fd_get(dirfd as usize) {
        Some(FdEntry::VfsDir { inode, .. }) => inode,
        Some(FdEntry::VfsFile { inode, .. }) => inode,
        _ => crate::scheduler::current_cwd(),
    }
}

fn sys_openat(dirfd: i64, path_ptr: *const u8, flags: usize, mode: usize) -> isize {
    if path_ptr.is_null() { return -14; } // EFAULT
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let writable = flags & 3 != 0;
    let create   = flags & 0x40 != 0;
    let truncate = flags & 0x200 != 0;
    let cwd = at_cwd(dirfd);

    if create { crate::fs::touch(path, cwd, 0, 0); }

    match crate::fs::open_inode(path, cwd, 0, 0, writable) {
        Some(inode) => {
            if truncate && writable { crate::fs::write_inode(inode, 0, &[]); }
            let entry = FdEntry::VfsFile { inode, offset: 0, writable };
            match crate::scheduler::current_fd_alloc(entry) {
                Some(fd) => fd as isize,
                None     => -24,
            }
        }
        None => {
            if writable { return -2; }
            if let Some(dir_inode) = crate::fs::open_dir(path, cwd) {
                let entry = FdEntry::VfsDir { inode: dir_inode, pos: 0 };
                match crate::scheduler::current_fd_alloc(entry) {
                    Some(fd) => fd as isize,
                    None     => -24,
                }
            } else {
                -2 // ENOENT
            }
        }
    }
}

fn sys_newfstatat(dirfd: i64, path_ptr: *const u8, buf: *mut Stat, _flags: usize) -> isize {
    if buf.is_null() { return -14; }
    // Empty path with AT_EMPTY_PATH (flags & 0x1000) → fstat on dirfd
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) => s,
        None    => return -14,
    };
    if path.is_empty() {
        // AT_EMPTY_PATH: fstat on the fd itself
        let fd = if dirfd == AT_FDCWD { return -22 } else { dirfd as usize };
        return sys_fstat(fd, buf);
    }
    let cwd = at_cwd(dirfd);
    let idx = match crate::fs::resolve(path, cwd) {
        Some(i) => i,
        None    => return -2,
    };
    unsafe { fill_inode_stat(buf, idx) }
}

fn sys_unlinkat(dirfd: i64, path_ptr: *const u8, flags: usize) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = at_cwd(dirfd);
    const AT_REMOVEDIR: usize = 0x200;
    if flags & AT_REMOVEDIR != 0 {
        if crate::fs::rmdir(path, cwd, 0, 0) { 0 } else { -2 }
    } else {
        if crate::fs::unlink(path, cwd, 0, 0) { 0 } else { -2 }
    }
}

fn sys_mkdirat(dirfd: i64, path_ptr: *const u8, mode: usize) -> isize {
    let path = match unsafe { read_path(path_ptr) } {
        Some(s) if !s.is_empty() => s,
        _ => return -2,
    };
    let cwd = at_cwd(dirfd);
    if crate::fs::mkdir(path, cwd, 0, 0, mode as u16) { 0 } else { -17 }
}

// ── getrandom ─────────────────────────────────────────────────────────────────

fn sys_getrandom(buf: *mut u8, len: usize, _flags: usize) -> isize {
    if buf.is_null() { return -14; } // EFAULT
    let out = unsafe { core::slice::from_raw_parts_mut(buf, len) };
    let mut seed = crate::arch::entropy_seed();
    for chunk in out.chunks_mut(8) {
        seed = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(0x6c62272e07bb0142);
        let bytes = seed.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    len as isize
}

// ── futex (minimal: FUTEX_WAIT / FUTEX_WAKE) ─────────────────────────────────

fn sys_futex(uaddr: usize, op: usize, val: usize) -> isize {
    const FUTEX_WAIT:    usize = 0;
    const FUTEX_WAKE:    usize = 1;
    const FUTEX_PRIVATE: usize = 128; // FUTEX_PRIVATE_FLAG

    let op_clean = op & !FUTEX_PRIVATE;
    match op_clean {
        FUTEX_WAIT => {
            // If *uaddr != val, return EAGAIN immediately.
            if uaddr == 0 { return -14; } // EFAULT
            let cur = unsafe { *(uaddr as *const u32) };
            if cur as usize != val { return -11; } // EAGAIN
            // For single-threaded use, just yield once; the waker will write *uaddr.
            crate::scheduler::schedule();
            0
        }
        FUTEX_WAKE => {
            // Nothing to wake for now (no real waiter list); return 0 woken.
            0
        }
        _ => -38, // ENOSYS for other futex ops
    }
}

// ── rt_sigaction / rt_sigprocmask ────────────────────────────────────────────
//
// Stub implementations: always succeed so programs that register signal handlers
// (SIGPIPE=SIG_IGN, SIGCHLD, etc.) don't receive ENOSYS and abort.
// Full delivery of signals to userspace requires sigreturn trampolines (TODO).

fn sys_rt_sigaction(sig: usize, act: usize, oact: usize, sigsetsize: usize) -> isize {
    if sig == 0 || sig > 31 { return -22; } // EINVAL
    // If caller wants old action: return zeroed struct (SIG_DFL, no mask, no flags).
    if oact != 0 {
        // struct kernel_sigaction: sa_handler (8), sa_flags (8), sa_restorer (8), sa_mask (sigsetsize)
        let total = 24 + sigsetsize.min(128);
        unsafe { core::ptr::write_bytes(oact as *mut u8, 0, total); }
    }
    let _ = act;
    0
}

fn sys_rt_sigprocmask(_how: usize, set: usize, oldset: usize, sigsetsize: usize) -> isize {
    if oldset != 0 {
        let sz = sigsetsize.min(128);
        unsafe { core::ptr::write_bytes(oldset as *mut u8, 0, sz); }
    }
    let _ = set;
    0
}

// ── arch_prctl (x86_64 only) ──────────────────────────────────────────────────

fn sys_arch_prctl(code: usize, addr: usize) -> isize {
    const ARCH_SET_GS: usize = 0x1001;
    const ARCH_SET_FS: usize = 0x1002;
    const ARCH_GET_FS: usize = 0x1003;
    const ARCH_GET_GS: usize = 0x1004;

    match code {
        ARCH_SET_FS => {
            crate::scheduler::set_fs_base(addr as u64);
            crate::arch::write_fs_base(addr as u64);
            0
        }
        ARCH_GET_FS => {
            if addr == 0 { return -14; } // EFAULT
            let val = crate::scheduler::get_fs_base();
            unsafe { *(addr as *mut u64) = val; }
            0
        }
        ARCH_SET_GS | ARCH_GET_GS => -22, // EINVAL — GS.base not exposed
        _ => -22,
    }
}

// ── clock_gettime ─────────────────────────────────────────────────────────────

fn sys_clock_gettime(clock_id: usize, ts: *mut TimeSpec) -> isize {
    if ts.is_null() { return -14; } // EFAULT
    if clock_id > 1 { return -22; } // EINVAL
    let ticks = crate::scheduler::tick_count();
    let secs  = (ticks / 100) as i64;
    let nsecs = ((ticks % 100) * 10_000_000) as i64;
    unsafe { (*ts).tv_sec = secs; (*ts).tv_nsec = nsecs; }
    0
}

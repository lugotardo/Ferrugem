/// Unix-style process model: tasks, signals, file descriptor tables.

// ── signal numbers ────────────────────────────────────────────────────────────

pub const SIGHUP:  u8 =  1;
pub const SIGINT:  u8 =  2;
pub const SIGQUIT: u8 =  3;
pub const SIGABRT: u8 =  6;
pub const SIGKILL: u8 =  9;
pub const SIGTERM: u8 = 15;
pub const SIGCHLD: u8 = 17;

/// Bitmask for signal `sig` (signals are 1-indexed; bit 0 = SIGHUP, bit 8 = SIGKILL).
pub fn sig_bit(sig: u8) -> u32 {
    if sig == 0 || sig > 31 { return 0; }
    1u32 << (sig as u32 - 1)
}

// ── file descriptor table ─────────────────────────────────────────────────────

pub const MAX_FDS: usize = 16;

#[derive(Clone, Copy)]
pub enum FdEntry {
    Empty,
    Stdin,
    Stdout,
    Stderr,
    VfsFile { inode: usize, offset: u64, writable: bool },
    VfsDir  { inode: usize, pos: usize },
    PipeRead  { idx: u8 },
    PipeWrite { idx: u8 },
    /// One end of a socketpair(2): reads from `read_idx`, writes to `write_idx`
    /// (two independent pipes wired crosswise so both ends can read and write).
    SocketPair { read_idx: u8, write_idx: u8 },
    DevNull,
    DevZero,
    DevUrandom,
}

#[derive(Clone, Copy)]
pub struct FdTable {
    pub entries: [FdEntry; MAX_FDS],
}

impl FdTable {
    pub fn new() -> Self {
        let mut t = FdTable { entries: [FdEntry::Empty; MAX_FDS] };
        t.entries[0] = FdEntry::Stdin;
        t.entries[1] = FdEntry::Stdout;
        t.entries[2] = FdEntry::Stderr;
        t
    }

    /// Find the first free slot ≥ 3 (preserving stdin/stdout/stderr).
    pub fn alloc(&mut self, entry: FdEntry) -> Option<usize> {
        for i in 3..MAX_FDS {
            if matches!(self.entries[i], FdEntry::Empty) {
                self.entries[i] = entry;
                return Some(i);
            }
        }
        None
    }

    pub fn get(&self, fd: usize) -> Option<FdEntry> {
        if fd >= MAX_FDS { return None; }
        match self.entries[fd] { FdEntry::Empty => None, e => Some(e) }
    }

    pub fn set_offset(&mut self, fd: usize, new_offset: u64) {
        if let Some(FdEntry::VfsFile { offset, .. }) = self.entries.get_mut(fd) {
            *offset = new_offset;
        }
    }

    pub fn set_dir_pos(&mut self, fd: usize, new_pos: usize) {
        if let Some(FdEntry::VfsDir { pos, .. }) = self.entries.get_mut(fd) {
            *pos = new_pos;
        }
    }

    /// Install `entry` at the specific `fd` slot, closing the existing entry first.
    /// Returns `Some(fd)` on success, `None` if `fd` is out of range.
    pub fn alloc_at(&mut self, fd: usize, entry: FdEntry) -> Option<usize> {
        if fd >= MAX_FDS { return None; }
        match self.entries[fd] {
            FdEntry::PipeRead  { idx } => crate::ipc::close_pipe_read(idx),
            FdEntry::PipeWrite { idx } => crate::ipc::close_pipe_write(idx),
            FdEntry::SocketPair { read_idx, write_idx } => {
                crate::ipc::close_pipe_read(read_idx);
                crate::ipc::close_pipe_write(write_idx);
            }
            _ => {}
        }
        self.entries[fd] = entry;
        Some(fd)
    }

    /// Free one FD; returns the old entry (caller must release pipe refs).
    pub fn close(&mut self, fd: usize) -> Option<FdEntry> {
        if fd >= MAX_FDS { return None; }
        match self.entries[fd] {
            FdEntry::Empty => None,
            e => { self.entries[fd] = FdEntry::Empty; Some(e) }
        }
    }

    /// Release all file descriptions; notifies the pipe pool for pipe ends.
    pub fn close_all(&mut self) {
        for i in 0..MAX_FDS {
            match self.entries[i] {
                FdEntry::PipeRead  { idx } => crate::ipc::close_pipe_read(idx),
                FdEntry::PipeWrite { idx } => crate::ipc::close_pipe_write(idx),
                FdEntry::SocketPair { read_idx, write_idx } => {
                    crate::ipc::close_pipe_read(read_idx);
                    crate::ipc::close_pipe_write(write_idx);
                }
                _ => {}
            }
            self.entries[i] = FdEntry::Empty;
        }
    }
}

// ── task states ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Zombie,
}

/// Architecture-neutral saved CPU context (userspace register snapshot).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CpuContext {
    pub regs:  [u64; 16],
    pub ip:    u64,
    pub sp:    u64,
    pub flags: u64,
}

impl CpuContext {
    pub const fn zero() -> Self {
        Self { regs: [0u64; 16], ip: 0, sp: 0, flags: 0 }
    }
}

// ── task ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Task {
    pub pid:             u64,
    pub ppid:            u64,
    pub state:           TaskState,
    pub context:         CpuContext,
    /// Saved kernel RSP written/read by `arch::context_switch`.
    pub kernel_sp:       u64,
    /// Top of this task's kernel stack kept in TSS.rsp0 / sscratch while running.
    pub stack_top:       u64,
    pub exit_code:       i32,
    pub is_user:         bool,
    /// Physical address of this task's root page table (0 = use kernel PT).
    pub page_table_phys: u64,

    // ── extensions ──────────────────────────────────────────────────────────
    /// Bitmask of pending signals (bit N-1 = signal N, 1-indexed).
    pub pending_signals: u32,
    /// If non-zero, the task is blocked in waitpid waiting for this PID.
    /// Use u64::MAX to mean "any child".
    pub waiting_for_pid: u64,
    /// Current program break (user-space heap end VA). 0 for kernel tasks.
    pub heap_brk:        u64,
    /// Per-task file descriptor table.
    pub fd_table:        FdTable,
    /// Current working directory inode index (0 = filesystem root).
    pub cwd:             usize,
    /// x86_64 FS.base (thread-pointer / TLS base, set via arch_prctl).
    /// Saved/restored on each context switch so each task has private TLS.
    pub fs_base:         u64,
}

static mut NEXT_PID: u64 = 1;

impl Task {
    pub fn new(ip: u64, sp: u64) -> Self {
        let pid = unsafe { let p = NEXT_PID; NEXT_PID += 1; p };
        let mut ctx = CpuContext::zero();
        ctx.ip = ip;
        ctx.sp = sp;
        Self {
            pid, ppid: 0, state: TaskState::Ready, context: ctx,
            kernel_sp: 0, stack_top: 0, exit_code: 0, is_user: false, page_table_phys: 0,
            pending_signals: 0, waiting_for_pid: 0, heap_brk: 0, fd_table: FdTable::new(), cwd: 0, fs_base: 0,
        }
    }

    pub fn new_kernel(kernel_sp: u64, stack_top: u64, ppid: u64) -> Self {
        let pid = unsafe { let p = NEXT_PID; NEXT_PID += 1; p };
        Self {
            pid, ppid, state: TaskState::Ready, context: CpuContext::zero(),
            kernel_sp, stack_top, exit_code: 0, is_user: false, page_table_phys: 0,
            pending_signals: 0, waiting_for_pid: 0, heap_brk: 0, fd_table: FdTable::new(), cwd: 0, fs_base: 0,
        }
    }

    pub fn new_user(kernel_sp: u64, stack_top: u64, page_table_phys: u64, ppid: u64) -> Self {
        let pid = unsafe { let p = NEXT_PID; NEXT_PID += 1; p };
        Self {
            pid, ppid, state: TaskState::Ready, context: CpuContext::zero(),
            kernel_sp, stack_top, exit_code: 0, is_user: true, page_table_phys,
            pending_signals: 0, waiting_for_pid: 0, heap_brk: 0, fd_table: FdTable::new(), cwd: 0, fs_base: 0,
        }
    }

    pub fn idle() -> Self {
        Self {
            pid: 0, ppid: 0, state: TaskState::Ready, context: CpuContext::zero(),
            kernel_sp: 0, stack_top: 0, exit_code: 0, is_user: false, page_table_phys: 0,
            pending_signals: 0, waiting_for_pid: 0, heap_brk: 0, fd_table: FdTable::new(), cwd: 0, fs_base: 0,
        }
    }

    /// Mark the task as dead, releasing its file descriptions.
    pub fn exit(&mut self, code: i32) {
        self.fd_table.close_all();
        self.exit_code = code;
        self.state = TaskState::Zombie;
    }

    /// Deliver `sig` to this task.
    pub fn raise_signal(&mut self, sig: u8) -> bool {
        let bit = sig_bit(sig);
        if bit == 0 { return false; }
        self.pending_signals |= bit;
        true
    }

    /// Check and atomically clear `sig`; returns true if it was pending.
    pub fn take_signal(&mut self, sig: u8) -> bool {
        let bit = sig_bit(sig);
        if self.pending_signals & bit != 0 {
            self.pending_signals &= !bit;
            true
        } else {
            false
        }
    }
}

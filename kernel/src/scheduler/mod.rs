/// Round-Robin scheduler with guard-page-protected kernel stacks, signal
/// delivery, and wait/waitpid support.
///
/// Stack layout per task (9 × 4 KiB = 1 guard + 8 stack pages):
///   [ guard page (not-present) ][ 8 × stack pages (32 KiB, grows down) ]
///
/// Slot 0 is the idle task and uses the boot stack (no guard page).
/// Slots 1-63 allocate their stacks dynamically from the frame allocator.

use crate::process::{Task, TaskState, FdEntry, SIGKILL, SIGTERM, SIGINT, SIGABRT, SIGQUIT, SIGCHLD};

const MAX_TASKS:   usize = 64;
const STACK_PAGES: usize = 8;
const STACK_SIZE:  usize = STACK_PAGES * 4096; // 32 KiB per task
const GUARD_SIZE:  usize = 4096;               // one guard page before each stack

// Initial user stack for a freshly exec'd process: holds argv/envp strings,
// the argv[]/envp[]/auxv pointer tables, and the actual call stack above that.
// 32 KiB is generous for typical shell command lines and a handful of env
// vars; setup_abi_stack() fails cleanly (None) rather than overflow if a
// caller ever exceeds it.
const USER_STACK_PAGES: usize = 8;
const USER_STACK_SIZE:  usize = USER_STACK_PAGES * 4096;
// Shared cap on both argv[] and envp[] entry counts (independent vectors,
// same bound) — kept as one constant so the fixed-size staging arrays below
// have a single source of truth.
const MAX_EXEC_VEC: usize = 64;
const MAX_ARG_LEN:  usize = 4096;

const GUARD_CANARY: u64 = 0xDEAD_C0DE_DEAD_C0DE;

static mut TASKS:      [Option<Task>; MAX_TASKS] = [const { None }; MAX_TASKS];
static mut GUARD_PHYS: [usize; MAX_TASKS]        = [0usize; MAX_TASKS];
static mut CURRENT:    usize = 0;
static mut TICK_COUNT: u64   = 0;

/// Slot of the task blocked waiting for TTY (keyboard or serial) input.
static mut TTY_WAITER: Option<usize> = None;

/// Per-pipe blocked reader: PIPE_WAITERS[pipe_idx] = blocked task slot.
static mut PIPE_WAITERS: [Option<usize>; crate::ipc::MAX_PIPES] = [None; crate::ipc::MAX_PIPES];

pub fn init() {
    unsafe { TASKS[0] = Some(Task::idle()); }
}

pub fn tick() {
    unsafe {
        TICK_COUNT += 1;
        // Check and deliver pending lethal signals to all tasks every tick.
        if TICK_COUNT % 5 == 0 { deliver_all_signals(); }
        if TICK_COUNT % 10 == 0 { schedule(); }
    }
}

pub fn schedule() {
    unsafe {
        let irqs_on = crate::arch::save_and_disable_interrupts();

        check_canary(CURRENT);

        let search_start = if CURRENT == 0 { 1 } else { (CURRENT % (MAX_TASKS - 1)) + 1 };
        let mut next = 0usize;
        for i in 0..MAX_TASKS - 1 {
            let slot = (search_start - 1 + i) % (MAX_TASKS - 1) + 1;
            if let Some(t) = TASKS[slot] {
                if t.state == TaskState::Ready { next = slot; break; }
            }
        }

        if next == CURRENT {
            crate::arch::restore_interrupt_state(irqs_on);
            return;
        }

        let prev = CURRENT;
        CURRENT  = next;

        let next_task = TASKS[next].as_ref().unwrap();
        let kstack = if next_task.is_user { next_task.stack_top } else { 0 };
        crate::arch::set_kernel_stack(kstack);

        let pt = next_task.page_table_phys;
        let pt_phys = if pt != 0 { pt } else { crate::arch::kernel_page_table_phys() };
        crate::arch::switch_address_space(pt_phys);

        let next_sp = TASKS[next].as_ref().unwrap().kernel_sp;
        let cur_sp  = &mut TASKS[prev].as_mut().unwrap().kernel_sp as *mut u64;

        // Save outgoing task's FS.base; restore incoming task's FS.base.
        TASKS[prev].as_mut().unwrap().fs_base = crate::arch::read_fs_base();
        crate::arch::context_switch(cur_sp, next_sp);
        crate::arch::write_fs_base(TASKS[next].as_ref().unwrap().fs_base);

        crate::arch::restore_interrupt_state(irqs_on);
    }
}

// ── signal delivery ───────────────────────────────────────────────────────────

/// Called every few ticks: deliver lethal signals to all tasks.
fn deliver_all_signals() {
    unsafe {
        for slot in 1..MAX_TASKS {
            let t = match TASKS[slot].as_mut() { Some(t) => t, None => continue };
            if t.state == TaskState::Zombie { continue; }
            if t.pending_signals == 0 { continue; }

            // Lethal signals: SIGKILL, SIGABRT, SIGQUIT, SIGTERM, SIGINT
            let lethal = t.take_signal(SIGKILL)
                      || t.take_signal(SIGABRT)
                      || t.take_signal(SIGQUIT)
                      || t.take_signal(SIGTERM)
                      || t.take_signal(SIGINT);
            if lethal {
                let pid = t.pid;
                t.exit(1);
                wake_child_waiters(pid, t.ppid);
                continue;
            }

            // SIGCHLD: wake if blocked in waitpid
            if t.take_signal(SIGCHLD) && t.waiting_for_pid != 0 {
                t.state = TaskState::Ready;
                t.waiting_for_pid = 0;
            }
        }
    }
}

// ── task creation ─────────────────────────────────────────────────────────────

/// Spawn a kernel function as a new schedulable task.
pub fn spawn_fn(entry: fn() -> !) -> Option<usize> {
    unsafe {
        let parent_pid = current_pid();
        for i in 1..MAX_TASKS {
            let vacant = TASKS[i].is_none()
                || matches!(TASKS[i], Some(t) if t.state == TaskState::Zombie);
            if !vacant { continue; }

            let frames = crate::memory::alloc_pages(1 + STACK_PAGES)?;
            let guard_phys = frames;
            let stack_phys = frames + GUARD_SIZE;

            crate::arch::protect_guard_page(guard_phys);
            GUARD_PHYS[i] = guard_phys;

            let canary_ptr = stack_phys as *mut u64;
            *canary_ptr = GUARD_CANARY;

            let stack_slice = core::slice::from_raw_parts_mut(stack_phys as *mut u8, STACK_SIZE);
            let kernel_sp = crate::arch::task_init_stack(stack_slice, entry);
            let stack_top = (stack_phys + STACK_SIZE) as u64;

            TASKS[i] = Some(Task::new_kernel(kernel_sp, stack_top, parent_pid));
            return Some(i);
        }
        None
    }
}

/// Spawn a userspace task from a raw program byte slice.
pub fn spawn_user(prog: &[u8]) -> Option<usize> {
    if prog.len() > 4096 { return None; }
    unsafe {
        let parent_pid = current_pid();

        let kframes     = crate::memory::alloc_pages(1 + STACK_PAGES)?;
        let guard_phys  = kframes;
        let kstack_phys = kframes + GUARD_SIZE;
        crate::arch::protect_guard_page(guard_phys);
        *(kstack_phys as *mut u64) = GUARD_CANARY;

        let code_phys   = crate::memory::alloc_pages(1)?;
        let ustack_phys = crate::memory::alloc_pages(1)?;

        let code_dst = core::slice::from_raw_parts_mut(code_phys as *mut u8, 4096);
        code_dst[..prog.len()].copy_from_slice(prog);
        core::ptr::write_bytes(code_dst.as_mut_ptr().add(prog.len()), 0, 4096 - prog.len());

        let pt_phys = crate::arch::create_process_page_table(code_phys, ustack_phys)?;

        let user_rip = crate::arch::USER_CODE_VA as u64;
        let user_rsp = crate::arch::USER_STACK_TOP as u64;

        let kstack = core::slice::from_raw_parts_mut(kstack_phys as *mut u8, STACK_SIZE);
        let kernel_sp = crate::arch::task_init_userspace_stack(kstack, user_rip, user_rsp);
        let stack_top = (kstack_phys + STACK_SIZE) as u64;

        for i in 1..MAX_TASKS {
            let vacant = TASKS[i].is_none()
                || matches!(TASKS[i], Some(t) if t.state == TaskState::Zombie);
            if !vacant { continue; }
            let mut task = Task::new_user(kernel_sp, stack_top, pt_phys, parent_pid);
            task.context.ip = user_rip;
            task.context.sp = user_rsp;
            GUARD_PHYS[i] = guard_phys;
            TASKS[i] = Some(task);
            return Some(i);
        }
        None
    }
}

/// Read a NUL-terminated string from a raw user-space pointer, bounded to
/// `max_len` bytes. The pointer is valid because the caller's page table is
/// still the active one (execve hasn't switched CR3 yet). Returns `None` if
/// `ptr` is null or the string exceeds `max_len`.
unsafe fn user_cstr<'a>(ptr: *const u8, max_len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() { return None; }
    let mut len = 0usize;
    while len < max_len {
        if *ptr.add(len) == 0 { return Some(core::slice::from_raw_parts(ptr, len)); }
        len += 1;
    }
    None // unterminated within bound — treat as malformed
}

/// Read a NUL-terminated array of `*const u8` from user space (an argv/envp
/// vector), bounded to `max_count` entries. Returns the entries as byte-string
/// slices, still pointing into the caller's (pre-exec) address space.
unsafe fn user_cstr_array<'a>(ptr: usize, max_count: usize, out: &mut [&'a [u8]; MAX_EXEC_VEC]) -> usize {
    if ptr == 0 { return 0; }
    let ptrs = ptr as *const usize;
    let mut n = 0usize;
    while n < max_count {
        let entry = *ptrs.add(n);
        if entry == 0 { break; }
        let Some(s) = user_cstr(entry as *const u8, MAX_ARG_LEN) else { break };
        out[n] = s;
        n += 1;
    }
    n
}

/// Build a System V AMD64 ABI-compliant initial stack frame on the
/// `region_size`-byte physical region `region_phys` (mapped contiguously at
/// virtual addresses `[stack_top_va - region_size, stack_top_va)`).
///
/// Memory layout (high to low within the region):
///   AT_RANDOM seed, then argv[]/envp[] strings, then the argc/argv/envp/auxv
///   table itself, all 8/16-byte aligned as required.
///
/// `argv`/`envp` are the real argument/environment vectors read from the
/// calling process (still mapped, since CR3 hasn't switched yet); pass empty
/// slices to fall back to a synthetic single-element argv of `argv0` (used
/// when spawning the initial init process, which has no caller to inherit
/// argv/envp from).
///
/// Returns the virtual address of argc (the new user RSP), or `None` if the
/// strings and tables don't fit in `region_size`.
unsafe fn setup_abi_stack(
    region_phys: usize,
    region_size: usize,
    stack_top_va: usize,
    argv0: &str,
    argv: &[&[u8]],
    envp: &[&[u8]],
    phdr_va: usize,
    phnum: usize,
    phentsize: usize,
) -> Option<usize> {
    let region_va = stack_top_va - region_size;

    // Helper: write u64 into the physical region at a given byte offset from its base.
    let w64 = |off: usize, val: u64| { *((region_phys + off) as *mut u64) = val; };

    let synthetic = [argv0.as_bytes()];
    let argv: &[&[u8]] = if argv.is_empty() { &synthetic } else { argv };

    // ── String area: write from the top of the region downward ──────────────

    // 16-byte AT_RANDOM seed at the very top of the region.
    let rand_off = region_size - 16;
    let rand_va  = region_va + rand_off;
    let seed = crate::arch::entropy_seed();
    w64(rand_off,     seed);
    w64(rand_off + 8, seed ^ 0xDEAD_BEEF_CAFE_1234);

    // argv[] and envp[] strings, each NUL-terminated and 8-byte aligned,
    // packed downward from just below AT_RANDOM. Track each string's VA so
    // the pointer table below can reference it.
    let mut argv_va = [0u64; MAX_EXEC_VEC];
    let mut envp_va = [0u64; MAX_EXEC_VEC];
    let mut off = rand_off;

    for (s, dst_va) in argv.iter().zip(argv_va.iter_mut()).take(argv.len().min(MAX_EXEC_VEC)) {
        off = (off.checked_sub(s.len() + 1)?) & !7;
        let dst = (region_phys + off) as *mut u8;
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
        *dst.add(s.len()) = 0;
        *dst_va = (region_va + off) as u64;
    }
    for (s, dst_va) in envp.iter().zip(envp_va.iter_mut()).take(envp.len().min(MAX_EXEC_VEC)) {
        off = (off.checked_sub(s.len() + 1)?) & !7;
        let dst = (region_phys + off) as *mut u8;
        core::ptr::copy_nonoverlapping(s.as_ptr(), dst, s.len());
        *dst.add(s.len()) = 0;
        *dst_va = (region_va + off) as u64;
    }

    let argc = argv.len().min(MAX_EXEC_VEC);
    let envc = envp.len().min(MAX_EXEC_VEC);

    // ── ABI table: argc / argv[] / NULL / envp[] / NULL / auxv[] ────────────
    // auxv entries we provide (type, value pairs, 8 bytes each):
    //   AT_PHDR   (3), phdr_va       — program headers VA (required for TLS init)
    //   AT_PHENT  (4), phentsize     — size of each program header entry
    //   AT_PHNUM  (5), phnum         — number of program header entries
    //   AT_PAGESZ (6), 4096
    //   AT_UID    (11), 0
    //   AT_EUID   (12), 0
    //   AT_GID    (13), 0
    //   AT_EGID   (14), 0
    //   AT_SECURE (23), 0
    //   AT_RANDOM (25), &rand_bytes
    //   AT_NULL   (0),  0
    // = 11 pairs × 16 bytes = 176 bytes
    // + argc(8) + argv[0..argc](8 each) + NULL(8) + envp[0..envc](8 each) + NULL(8)
    let table_size = 8 + (argc + 1) * 8 + (envc + 1) * 8 + 176;
    let rsp_off = off.checked_sub(table_size)? & !15; // 16-byte align
    let rsp_va  = region_va + rsp_off;

    let mut o = rsp_off;
    w64(o, argc as u64); o += 8;
    for i in 0..argc { w64(o, argv_va[i]); o += 8; }
    w64(o, 0); o += 8; // argv[argc] = NULL
    for i in 0..envc { w64(o, envp_va[i]); o += 8; }
    w64(o, 0); o += 8; // envp[envc] = NULL
    // auxv
    w64(o,      3);                 o += 8; // AT_PHDR
    w64(o, phdr_va as u64);         o += 8;
    w64(o,      4);                 o += 8; // AT_PHENT
    w64(o, phentsize as u64);       o += 8;
    w64(o,      5);                 o += 8; // AT_PHNUM
    w64(o, phnum as u64);           o += 8;
    w64(o,      6);                 o += 8; // AT_PAGESZ
    w64(o,   4096);                 o += 8;
    w64(o,     11);                 o += 8; // AT_UID
    w64(o,      0);                 o += 8;
    w64(o,     12);                 o += 8; // AT_EUID
    w64(o,      0);                 o += 8;
    w64(o,     13);                 o += 8; // AT_GID
    w64(o,      0);                 o += 8;
    w64(o,     14);                 o += 8; // AT_EGID
    w64(o,      0);                 o += 8;
    w64(o,     23);                 o += 8; // AT_SECURE
    w64(o,      0);                 o += 8;
    w64(o,     25);                 o += 8; // AT_RANDOM
    w64(o, rand_va as u64);         o += 8;
    w64(o,      0);                 o += 8; // AT_NULL
    w64(o,      0);                 // = 0
    let _ = o;

    Some(rsp_va)
}

/// Allocate and map the contiguous `USER_STACK_PAGES` physical region used for
/// a fresh process's initial stack (argv/envp/auxv + call stack above it).
/// Returns the region's physical base address.
unsafe fn map_user_init_stack(pt_phys: u64, stack_top: usize) -> Option<usize> {
    let phys = crate::memory::alloc_pages(USER_STACK_PAGES)?;
    core::ptr::write_bytes(phys as *mut u8, 0, USER_STACK_SIZE);
    let region_va = stack_top - USER_STACK_SIZE;
    for i in 0..USER_STACK_PAGES {
        let va = region_va + i * 4096;
        let page_phys = phys + i * 4096;
        if !crate::arch::map_user_page(
            pt_phys, va, page_phys,
            crate::arch::PROT_READ | crate::arch::PROT_WRITE | crate::arch::PROT_USER,
        ) {
            return None;
        }
    }
    Some(phys)
}

/// Spawn a user task by loading an ELF64 binary into a fresh address space.
/// Supports ET_EXEC and ET_DYN (PIE); ET_DYN ELFs are relocated to USER_BASE_VA.
/// Returns the task slot index, or None on OOM / invalid ELF / run queue full.
pub fn spawn_elf(data: &[u8], argv0: &str) -> Option<usize> {
    unsafe {
        let parent_pid = current_pid();

        // Kernel stack (guard + 8 pages)
        let kframes     = crate::memory::alloc_pages(1 + STACK_PAGES)?;
        let guard_phys  = kframes;
        let kstack_phys = kframes + GUARD_SIZE;
        crate::arch::protect_guard_page(guard_phys);
        *(kstack_phys as *mut u64) = GUARD_CANARY;

        // Empty address space: kernel entries only, no user pages yet
        let pt_phys = crate::arch::create_empty_process_page_table()?;

        // Parse and map ELF segments into pt_phys
        let (entry, heap_start, phdr_va, phnum, phentsize) = crate::elf::load(data, pt_phys)?;

        // Allocate and map the initial user stack region.
        // Use USER_ELF_STACK_TOP which is 256 MiB above USER_BASE_VA — well
        // above any binary's BSS so there is no overlap with PT_LOAD segments.
        let stack_top   = crate::arch::USER_ELF_STACK_TOP;
        let ustack_phys = map_user_init_stack(pt_phys, stack_top)?;

        let user_rip = entry as u64;
        let user_rsp = setup_abi_stack(
            ustack_phys, USER_STACK_SIZE, stack_top, argv0, &[], &[], phdr_va, phnum, phentsize,
        )? as u64;

        let kstack    = core::slice::from_raw_parts_mut(kstack_phys as *mut u8, STACK_SIZE);
        let kernel_sp = crate::arch::task_init_userspace_stack(kstack, user_rip, user_rsp);
        let stack_top = (kstack_phys + STACK_SIZE) as u64;

        for i in 1..MAX_TASKS {
            let vacant = TASKS[i].is_none()
                || matches!(TASKS[i], Some(t) if t.state == TaskState::Zombie);
            if !vacant { continue; }
            let mut task = Task::new_user(kernel_sp, stack_top, pt_phys, parent_pid);
            task.context.ip = user_rip;
            task.context.sp = user_rsp;
            task.heap_brk   = heap_start as u64;
            GUARD_PHYS[i]   = guard_phys;
            TASKS[i]        = Some(task);
            return Some(i);
        }
        None
    }
}

/// Fork the current user process: deep-copy the page table, inherit the FD
/// table and heap_brk, then set up a new kernel stack so the child enters
/// user mode at `user_ip` / `user_sp` and returns 0 from the fork syscall.
///
/// `regs` are the 6 syscall argument registers at the time of the call — a real
/// clone()/fork() preserves them across the fork, and libc's `__clone` trampoline
/// on x86_64 depends on that (see `task_init_fork_stack`).
///
/// Returns the new task slot, or None on OOM / run queue full.
pub fn spawn_fork(user_ip: u64, user_sp: u64, regs: [u64; 6], callee_saved: [u64; 6]) -> Option<usize> {
    unsafe {
        let parent_pid = current_pid();
        let parent_pt  = current_page_table_phys();
        let parent_brk = current_heap_brk();
        let parent_fdt = TASKS[CURRENT].as_ref()?.fd_table;
        // Read live, not the saved copy: the parent is still the running task, so
        // its FS.base MSR (TLS pointer) hasn't been flushed into TASKS[CURRENT] yet.
        let parent_fs_base = crate::arch::read_fs_base();

        // New kernel stack (guard + 8 pages)
        let kframes     = crate::memory::alloc_pages(1 + STACK_PAGES)?;
        let guard_phys  = kframes;
        let kstack_phys = kframes + GUARD_SIZE;
        crate::arch::protect_guard_page(guard_phys);
        *(kstack_phys as *mut u64) = GUARD_CANARY;

        // Deep-copy parent's address space
        let child_pt = crate::arch::clone_user_page_table(parent_pt)?;

        // Kernel stack: child resumes at user_ip with user_sp, rax=0 (fork returns 0 in child)
        let kstack    = core::slice::from_raw_parts_mut(kstack_phys as *mut u8, STACK_SIZE);
        let kernel_sp = crate::arch::task_init_fork_stack(
            kstack, user_ip, user_sp,
            regs[0], regs[1], regs[2], regs[3], regs[4], regs[5],
            callee_saved,
        );
        let stack_top = (kstack_phys + STACK_SIZE) as u64;

        let parent_cwd = current_cwd();

        for i in 1..MAX_TASKS {
            let vacant = TASKS[i].is_none()
                || matches!(TASKS[i], Some(t) if t.state == TaskState::Zombie);
            if !vacant { continue; }
            let mut task   = Task::new_user(kernel_sp, stack_top, child_pt, parent_pid);
            task.heap_brk  = parent_brk;
            task.fd_table  = parent_fdt;
            task.cwd       = parent_cwd;
            task.fs_base   = parent_fs_base;
            // Increment pipe reference counts for any pipe FDs inherited by child.
            for fd in &task.fd_table.entries {
                match fd {
                    crate::process::FdEntry::PipeRead  { idx } => crate::ipc::dup_pipe_read(*idx),
                    crate::process::FdEntry::PipeWrite { idx } => crate::ipc::dup_pipe_write(*idx),
                    crate::process::FdEntry::SocketPair { read_idx, write_idx } => {
                        crate::ipc::dup_pipe_read(*read_idx);
                        crate::ipc::dup_pipe_write(*write_idx);
                    }
                    _ => {}
                }
            }
            GUARD_PHYS[i]  = guard_phys;
            TASKS[i]       = Some(task);
            return Some(i);
        }
        None
    }
}

/// Replace the current task's address space in-place with a fresh ELF binary.
/// `argv_ptr`/`envp_ptr` are the raw argv[]/envp[] vectors passed to execve(2),
/// still pointing into the CALLER's (pre-exec) address space — read them
/// before touching the new page table below, since both are only valid until
/// the CR3 switch the trap handler performs after this function returns.
/// Returns (new_entry, new_stack_top, new_pt_phys) on success.
/// The caller (trap handler) must redirect sepc/rip and switch address space.
pub fn execve_current(data: &[u8], argv0: &str, argv_ptr: usize, envp_ptr: usize) -> Option<(u64, u64, u64)> {
    unsafe {
        let mut argv_buf: [&[u8]; MAX_EXEC_VEC] = [&[]; MAX_EXEC_VEC];
        let mut envp_buf: [&[u8]; MAX_EXEC_VEC] = [&[]; MAX_EXEC_VEC];
        let argc = user_cstr_array(argv_ptr, MAX_EXEC_VEC, &mut argv_buf);
        let envc = user_cstr_array(envp_ptr, MAX_EXEC_VEC, &mut envp_buf);

        let new_pt = crate::arch::create_empty_process_page_table()?;
        let (entry, heap_start, phdr_va, phnum, phentsize) = crate::elf::load(data, new_pt)?;

        let stack_top   = crate::arch::USER_ELF_STACK_TOP;
        let ustack_phys = map_user_init_stack(new_pt, stack_top)?;

        let user_rsp = setup_abi_stack(
            ustack_phys, USER_STACK_SIZE, stack_top, argv0,
            &argv_buf[..argc], &envp_buf[..envc], phdr_va, phnum, phentsize,
        )?;

        let task = TASKS[CURRENT].as_mut()?;
        // Old page table and its mapped frames are leaked — a page-table walker is
        // needed to free them properly; deferred until memory management matures.
        task.page_table_phys = new_pt;
        task.heap_brk        = heap_start as u64;
        task.fs_base         = 0; // clear stale TLS from the old process image
        crate::arch::write_fs_base(0);

        Some((entry as u64, user_rsp as u64, new_pt))
    }
}

/// Add a pre-built Task to the run queue.
pub fn spawn(task: Task) -> Option<usize> {
    unsafe {
        for i in 1..MAX_TASKS {
            let vacant = TASKS[i].is_none()
                || matches!(TASKS[i], Some(t) if t.state == TaskState::Zombie);
            if vacant { TASKS[i] = Some(task); return Some(i); }
        }
        None
    }
}

// ── exit & reaping ────────────────────────────────────────────────────────────

pub fn exit_current(code: i32) {
    unsafe {
        if let Some(t) = TASKS[CURRENT].as_mut() {
            let pid  = t.pid;
            let ppid = t.ppid;
            t.exit(code);               // marks Zombie, closes FDs
            wake_child_waiters(pid, ppid);
            signal_parent(ppid, SIGCHLD);
            // Wake any tasks blocked reading from a pipe whose write end just closed.
            wake_closed_pipe_readers();
        }
        schedule();
    }
}

/// Wake any task that is blocked in waitpid for `exiting_pid`.
fn wake_child_waiters(exiting_pid: u64, parent_pid: u64) {
    unsafe {
        for slot in 0..MAX_TASKS {
            if let Some(t) = TASKS[slot].as_mut() {
                if t.state != TaskState::Blocked { continue; }
                let wake = t.waiting_for_pid == exiting_pid
                    || (t.waiting_for_pid == u64::MAX && t.pid == parent_pid);
                if wake {
                    t.waiting_for_pid = 0;
                    t.state = TaskState::Ready;
                }
            }
        }
    }
}

fn signal_parent(parent_pid: u64, sig: u8) {
    unsafe {
        for slot in 0..MAX_TASKS {
            if let Some(t) = TASKS[slot].as_mut() {
                if t.pid == parent_pid {
                    t.raise_signal(sig);
                    break;
                }
            }
        }
    }
}

// ── wait/waitpid (blocking) ───────────────────────────────────────────────────

/// Block the current task until a child with `target_pid` (or any child if
/// `target_pid == u64::MAX`) exits.  Reaps the zombie and writes the exit
/// status to `*status_out` (WEXITSTATUS encoding).
/// Returns the reaped PID on success, or a negative errno:
///   -10 (ECHILD) if no matching children exist.
pub fn waitpid(target_pid: u64, status_out: *mut i32, no_hang: bool) -> isize {
    unsafe {
        let irqs_on = crate::arch::save_and_disable_interrupts();
        let cur_pid = TASKS[CURRENT].as_ref().map(|t| t.pid).unwrap_or(0);
        crate::arch::restore_interrupt_state(irqs_on);

        loop {
            let irqs_on = crate::arch::save_and_disable_interrupts();

            // Scan for a matching zombie child.
            let mut found_slot: Option<(usize, u64, i32)> = None;
            for slot in 1..MAX_TASKS {
                if let Some(t) = TASKS[slot].as_ref() {
                    if t.state != TaskState::Zombie { continue; }
                    if t.ppid != cur_pid { continue; }
                    let matches = target_pid == u64::MAX || t.pid == target_pid;
                    if matches { found_slot = Some((slot, t.pid, t.exit_code)); break; }
                }
            }

            if let Some((slot, pid, code)) = found_slot {
                TASKS[slot] = None;
                crate::arch::restore_interrupt_state(irqs_on);
                if !status_out.is_null() {
                    *status_out = (code & 0xFF) << 8; // WEXITSTATUS-compatible
                }
                return pid as isize;
            }

            // No zombie: check if any living children exist.
            let has_child = TASKS[1..].iter().any(|s| {
                s.as_ref().map(|t| t.ppid == cur_pid && t.state != TaskState::Zombie)
                 .unwrap_or(false)
            });

            if !has_child {
                crate::arch::restore_interrupt_state(irqs_on);
                return -10; // -ECHILD
            }

            if no_hang {
                crate::arch::restore_interrupt_state(irqs_on);
                return 0; // WNOHANG: no zombie yet, return immediately
            }

            // Block until a child exits (wake_child_waiters sets us Ready).
            TASKS[CURRENT].as_mut().unwrap().waiting_for_pid = target_pid;
            TASKS[CURRENT].as_mut().unwrap().state = TaskState::Blocked;
            crate::arch::restore_interrupt_state(irqs_on);
            schedule();
        }
    }
}

// ── TTY blocking ──────────────────────────────────────────────────────────────

/// Block the current task until serial or keyboard input arrives.
pub fn block_on_tty() {
    unsafe {
        let was = crate::arch::save_and_disable_interrupts();
        let has = crate::drivers::keyboard::has_input()
               || crate::drivers::serial::has_input();
        if !has {
            TTY_WAITER = Some(CURRENT);
            TASKS[CURRENT].as_mut().unwrap().state = TaskState::Blocked;
            crate::arch::restore_interrupt_state(was);
            schedule();
        } else {
            crate::arch::restore_interrupt_state(was);
        }
    }
}

/// Called from IRQ handlers (keyboard IRQ1, serial IRQ4) when new input arrives.
pub fn wake_tty_waiter() {
    unsafe {
        if let Some(slot) = TTY_WAITER.take() {
            if let Some(t) = TASKS[slot].as_mut() {
                if t.state == TaskState::Blocked { t.state = TaskState::Ready; }
            }
        }
    }
}

// ── pipe blocking ─────────────────────────────────────────────────────────────

/// Block the current task until the pipe at `pipe_idx` has data.
pub fn block_on_pipe(pipe_idx: u8) {
    unsafe {
        let was = crate::arch::save_and_disable_interrupts();
        if !crate::ipc::pipe_has_data(pipe_idx) && crate::ipc::pipe_write_open(pipe_idx) {
            PIPE_WAITERS[pipe_idx as usize] = Some(CURRENT);
            TASKS[CURRENT].as_mut().unwrap().state = TaskState::Blocked;
            crate::arch::restore_interrupt_state(was);
            schedule();
        } else {
            crate::arch::restore_interrupt_state(was);
        }
    }
}

/// Wake the task blocked reading from `pipe_idx`, if any.
pub fn wake_pipe_waiter(pipe_idx: u8) {
    unsafe {
        if let Some(slot) = PIPE_WAITERS[pipe_idx as usize].take() {
            if let Some(t) = TASKS[slot].as_mut() {
                if t.state == TaskState::Blocked { t.state = TaskState::Ready; }
            }
        }
    }
}

/// Wake any tasks blocked on pipes whose write end is now fully closed.
/// Called after a task exits and its pipe FDs are released.
fn wake_closed_pipe_readers() {
    unsafe {
        for (pipe_idx, waiter_slot) in PIPE_WAITERS.iter_mut().enumerate() {
            if waiter_slot.is_none() { continue; }
            if !crate::ipc::pipe_write_open(pipe_idx as u8) {
                if let Some(slot) = waiter_slot.take() {
                    if let Some(t) = TASKS[slot].as_mut() {
                        if t.state == TaskState::Blocked { t.state = TaskState::Ready; }
                    }
                }
            }
        }
    }
}

// ── nanosleep ─────────────────────────────────────────────────────────────────

/// Block the current task for at least `ticks` scheduler ticks.
pub fn sleep_ticks(ticks: u64) {
    let wake_at = unsafe { TICK_COUNT } + ticks;
    while unsafe { TICK_COUNT } < wake_at {
        schedule();
    }
}

// ── signal dispatch ───────────────────────────────────────────────────────────

/// Deliver `sig` to the task with `target_pid`. Returns false if not found.
pub fn raise_signal_to_pid(target_pid: u64, sig: u8) -> bool {
    unsafe {
        for slot in 0..MAX_TASKS {
            if let Some(t) = TASKS[slot].as_mut() {
                if t.pid == target_pid && t.state != TaskState::Zombie {
                    t.raise_signal(sig);
                    // Immediately wake if blocking (signal will be delivered by deliver_all_signals).
                    if t.state == TaskState::Blocked {
                        t.state = TaskState::Ready;
                    }
                    return true;
                }
            }
        }
        false
    }
}

// ── FD table helpers (used by syscall dispatch) ───────────────────────────────

pub fn current_fd_get(fd: usize) -> Option<FdEntry> {
    unsafe { TASKS[CURRENT].as_ref()?.fd_table.get(fd) }
}

pub fn current_fd_alloc(entry: FdEntry) -> Option<usize> {
    unsafe { TASKS[CURRENT].as_mut()?.fd_table.alloc(entry) }
}

/// Close FD, returning the old entry so the caller can release pipe refs.
pub fn current_fd_close(fd: usize) -> Option<FdEntry> {
    unsafe { TASKS[CURRENT].as_mut()?.fd_table.close(fd) }
}

/// Duplicate `oldfd` into the lowest available slot ≥ 3. Returns new fd or None.
pub fn current_fd_dup(oldfd: usize) -> Option<usize> {
    unsafe {
        let entry = TASKS[CURRENT].as_ref()?.fd_table.get(oldfd)?;
        TASKS[CURRENT].as_mut()?.fd_table.alloc(entry)
    }
}

/// Duplicate `oldfd` into `newfd`, closing `newfd` first if open.
/// Returns `newfd` on success, or None if oldfd is invalid or newfd is out of range.
pub fn current_fd_dup2(oldfd: usize, newfd: usize) -> Option<usize> {
    unsafe {
        let t = TASKS[CURRENT].as_mut()?;
        let entry = t.fd_table.get(oldfd)?;
        t.fd_table.alloc_at(newfd, entry)
    }
}

pub fn current_fd_set_offset(fd: usize, offset: u64) {
    unsafe {
        if let Some(t) = TASKS[CURRENT].as_mut() {
            t.fd_table.set_offset(fd, offset);
        }
    }
}

pub fn current_fd_set_dir_pos(fd: usize, pos: usize) {
    unsafe {
        if let Some(t) = TASKS[CURRENT].as_mut() {
            t.fd_table.set_dir_pos(fd, pos);
        }
    }
}

pub fn current_cwd() -> usize {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.cwd).unwrap_or(0) }
}

pub fn set_cwd(inode: usize) {
    unsafe {
        if let Some(t) = TASKS[CURRENT].as_mut() { t.cwd = inode; }
    }
}

pub fn current_heap_brk() -> u64 {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.heap_brk).unwrap_or(0) }
}

pub fn current_page_table_phys() -> u64 {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.page_table_phys).unwrap_or(0) }
}

pub fn set_heap_brk(addr: u64) {
    unsafe {
        if let Some(t) = TASKS[CURRENT].as_mut() { t.heap_brk = addr; }
    }
}

pub fn get_fs_base() -> u64 {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.fs_base).unwrap_or(0) }
}

pub fn set_fs_base(val: u64) {
    unsafe { if let Some(t) = TASKS[CURRENT].as_mut() { t.fs_base = val; } }
}

// ── info queries ──────────────────────────────────────────────────────────────

pub fn current_ppid() -> u64 {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.ppid).unwrap_or(0) }
}

pub fn current_pid() -> u64 {
    unsafe { TASKS[CURRENT].as_ref().map(|t| t.pid).unwrap_or(0) }
}

pub fn current_slot() -> usize { unsafe { CURRENT } }

pub fn task_pid(slot: usize) -> u64 {
    unsafe { TASKS[slot].as_ref().map(|t| t.pid).unwrap_or(0) }
}

pub fn tick_count() -> u64 { unsafe { TICK_COUNT } }

pub fn task_count() -> usize {
    unsafe {
        TASKS[1..].iter()
            .filter(|s| matches!(s, Some(t) if t.state != TaskState::Zombie))
            .count()
    }
}

pub fn kill_task(slot: usize) -> bool {
    unsafe {
        if slot == 0 || slot >= MAX_TASKS || slot == CURRENT { return false; }
        if let Some(t) = TASKS[slot].as_mut() {
            if t.state != TaskState::Zombie {
                t.raise_signal(SIGKILL);
                if t.state == TaskState::Blocked { t.state = TaskState::Ready; }
                return true;
            }
        }
        false
    }
}

/// Demand-page a single 4 KiB page at `va` in the current task's address space.
///
/// Called from the arch page-fault handler when an unmapped user address is
/// accessed.  Allocates a zeroed physical frame and maps it RWX+user.
/// Returns true if the mapping succeeded; false if the fault is fatal (kernel
/// fault, address below USER_BASE_VA, or OOM).
pub fn handle_user_page_fault(va: u64) -> bool {
    let pt_phys = current_page_table_phys();
    if pt_phys == 0 { return false; }  // kernel task — no user PT

    let va_page = (va as usize) & !0xFFF;
    if va_page < crate::arch::USER_BASE_VA { return false; }

    let prot = crate::arch::PROT_READ | crate::arch::PROT_WRITE
             | crate::arch::PROT_EXEC | crate::arch::PROT_USER;

    match crate::memory::alloc_pages(1) {
        None => false,
        Some(phys) => {
            unsafe { core::ptr::write_bytes(phys as *mut u8, 0, 4096); }
            let ok = crate::arch::map_user_page(pt_phys, va_page, phys, prot);
            if ok { crate::arch::flush_tlb_page(va_page); }
            ok
        }
    }
}

pub fn for_each_task<F: FnMut(usize, u64, TaskState, bool)>(mut f: F) {
    unsafe {
        for (i, slot) in TASKS.iter().enumerate() {
            if let Some(t) = slot { f(i, t.pid, t.state, t.is_user); }
        }
    }
}

// ── canary check ──────────────────────────────────────────────────────────────

unsafe fn check_canary(slot: usize) {
    if slot == 0 { return; }
    let guard = GUARD_PHYS[slot];
    if guard == 0 { return; }
    let stack_phys = guard + GUARD_SIZE;
    let canary_ptr = stack_phys as *const u64;
    if *canary_ptr != GUARD_CANARY {
        panic!("stack overflow detected on task slot {}", slot);
    }
}

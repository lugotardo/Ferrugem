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

/// Build a System V AMD64 ABI-compliant initial stack frame on `stack_page_phys`.
///
/// Memory layout (high to low within the 4 KiB page):
///   [top-24]: 16-byte AT_RANDOM seed + 8-byte padding
///   [top-32]: NUL-terminated argv[0] string, 8-byte padded
///   [rsp..]:  argc, argv[0] ptr, NULL, NULL (envp), auxv pairs, AT_NULL
///
/// `stack_top_va` is the first byte ABOVE the stack page (= USER_ELF_STACK_TOP).
/// Returns the virtual address of argc (the new user RSP).
unsafe fn setup_abi_stack(
    stack_page_phys: usize,
    stack_top_va: usize,
    argv0: &str,
    phdr_va: usize,
    phnum: usize,
    phentsize: usize,
) -> usize {
    let page_va   = stack_top_va - 4096;
    let phys_base = stack_page_phys;

    // Helper: write u64 into the physical page at a given byte offset from page start.
    let w64 = |off: usize, val: u64| { *((phys_base + off) as *mut u64) = val; };

    // ── String area: write from page top downward ────────────────────────────

    // 16-byte AT_RANDOM seed at the very top of the page.
    let rand_off = 4096 - 16;
    let rand_va  = page_va + rand_off;
    // Fill with pseudo-random data from hardware entropy.
    let seed = crate::arch::entropy_seed();
    w64(rand_off,     seed);
    w64(rand_off + 8, seed ^ 0xDEAD_BEEF_CAFE_1234);

    // argv[0] string, 8-byte aligned, below AT_RANDOM.
    let name     = argv0.as_bytes();
    let str_off  = (rand_off - name.len() - 1) & !7;
    let str_va   = page_va + str_off;
    let dst = (phys_base + str_off) as *mut u8;
    core::ptr::copy_nonoverlapping(name.as_ptr(), dst, name.len());
    *dst.add(name.len()) = 0; // NUL

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
    // + argc(8) + argv[0](8) + NULL(8) + NULL(8) = 32 bytes
    // Total = 208 bytes from rsp (round up to 224 for alignment).
    let table_size = 224usize;
    let rsp_off = (str_off - table_size) & !15; // 16-byte align
    let rsp_va  = page_va + rsp_off;

    let mut o = rsp_off;
    w64(o,      1);                 o += 8; // argc = 1
    w64(o, str_va as u64);         o += 8; // argv[0] ptr
    w64(o,      0);                 o += 8; // argv[1] = NULL
    w64(o,      0);                 o += 8; // envp[0] = NULL
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

    rsp_va
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

        // Allocate and map one user stack page.
        // Use USER_ELF_STACK_TOP which is 256 MiB above USER_BASE_VA — well
        // above any binary's BSS so there is no overlap with PT_LOAD segments.
        let stack_top      = crate::arch::USER_ELF_STACK_TOP;
        let ustack_phys    = crate::memory::alloc_pages(1)?;
        let ustack_page_va = stack_top - 4096;
        core::ptr::write_bytes(ustack_phys as *mut u8, 0, 4096);
        if !crate::arch::map_user_page(
            pt_phys, ustack_page_va, ustack_phys,
            crate::arch::PROT_READ | crate::arch::PROT_WRITE | crate::arch::PROT_USER,
        ) {
            return None;
        }

        let user_rip = entry as u64;
        let user_rsp = setup_abi_stack(ustack_phys, stack_top, argv0, phdr_va, phnum, phentsize) as u64;

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
/// Returns (new_entry, new_stack_top, new_pt_phys) on success.
/// The caller (trap handler) must redirect sepc/rip and switch address space.
pub fn execve_current(data: &[u8], argv0: &str) -> Option<(u64, u64, u64)> {
    unsafe {
        let new_pt = crate::arch::create_empty_process_page_table()?;
        let (entry, heap_start, phdr_va, phnum, phentsize) = crate::elf::load(data, new_pt)?;

        let stack_top      = crate::arch::USER_ELF_STACK_TOP;
        let ustack_phys    = crate::memory::alloc_pages(1)?;
        let ustack_page_va = stack_top - 4096;
        core::ptr::write_bytes(ustack_phys as *mut u8, 0, 4096);
        if !crate::arch::map_user_page(
            new_pt, ustack_page_va, ustack_phys,
            crate::arch::PROT_READ | crate::arch::PROT_WRITE | crate::arch::PROT_USER,
        ) {
            return None;
        }

        let user_rsp = setup_abi_stack(ustack_phys, stack_top, argv0, phdr_va, phnum, phentsize);

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

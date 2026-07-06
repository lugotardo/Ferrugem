/// Interactive shell navigation, file I/O, permissions, users, processes.

mod users;

const MAX_CMD:    usize = 256;
const HIST_DEPTH: usize = 16;

// ── history ───────────────────────────────────────────────────────────────────

struct History {
    bufs:  [[u8; MAX_CMD]; HIST_DEPTH],
    lens:  [usize; HIST_DEPTH],
    head:  usize,
    count: usize,
}

impl History {
    const fn new() -> Self {
        Self { bufs: [[0u8; MAX_CMD]; HIST_DEPTH], lens: [0usize; HIST_DEPTH], head: 0, count: 0 }
    }

    fn push(&mut self, data: &[u8]) {
        if data.is_empty() { return; }
        let len = data.len().min(MAX_CMD);
        self.bufs[self.head][..len].copy_from_slice(&data[..len]);
        self.lens[self.head] = len;
        self.head = (self.head + 1) % HIST_DEPTH;
        if self.count < HIST_DEPTH { self.count += 1; }
    }

    /// offset 0 = most recent entry.
    fn get(&self, offset: usize) -> Option<(&[u8], usize)> {
        if offset >= self.count { return None; }
        let idx = (self.head + HIST_DEPTH - 1 - offset) % HIST_DEPTH;
        Some((&self.bufs[idx], self.lens[idx]))
    }
}

static mut HISTORY: History = History::new();

// ── shell state ───────────────────────────────────────────────────────────────

struct State {
    cwd: usize,
    uid: u16,
    gid: u16,
}

impl State {
    fn new() -> Self { State { cwd: 0, uid: 0, gid: 0 } }
}

// ── main loop ─────────────────────────────────────────────────────────────────

pub fn run() -> ! {
    users::init();
    print_str(crate::version::BANNER);
    print_str("Type 'help' for available commands.\n\n");
    let mut state = State::new();
    loop {
        print_prompt(&state);
        let line = unsafe { read_line(&*core::ptr::addr_of!(HISTORY)) };
        unsafe {
            if !line.is_empty() { (*core::ptr::addr_of_mut!(HISTORY)).push(line.as_bytes()); }
        }
        execute(line.as_str(), &mut state);
    }
}

// ── prompt ────────────────────────────────────────────────────────────────────

fn print_prompt(s: &State) {
    let name = match users::lookup_by_uid(s.uid) {
        Some(u) => u.name_str(),
        None    => "?",
    };
    print_str(name);
    print_str("@ferrugem:");
    let mut buf: [u8; 256] = [0u8; 256];
    let len = crate::fs::inode_path(s.cwd, &mut buf);
    if let Ok(path) = core::str::from_utf8(&buf[..len]) {
        print_str(path);
    } else {
        print_str("/");
    }
    if s.uid == 0 { print_str("# "); } else { print_str("$ "); }
}

// ── dispatch ──────────────────────────────────────────────────────────────────

fn execute(line: &str, s: &mut State) {
    let mut parts = line.split_ascii_whitespace();
    let cmd = match parts.next() { Some(c) => c, None => return };
    match cmd {
        "help"   => cmd_help(),
        "echo"   => cmd_echo(line, s),
        "clear"  => cmd_clear(),
        "pwd"    => cmd_pwd(s),
        "cd"     => cmd_cd(parts.next(), s),
        "ls"     => { let a = parts.next(); let b = parts.next(); cmd_ls(a, b, s) }
        "mkdir"  => cmd_mkdir(parts.next(), s),
        "rmdir"  => cmd_rmdir(parts.next(), s),
        "rm"     => cmd_rm(parts.next(), s),
        "touch"  => cmd_touch(parts.next(), s),
        "cat"    => cmd_cat(parts.next(), s),
        "write"  => cmd_write(parts.next(), line, s),
        "append" => cmd_append(parts.next(), line, s),
        "cp"     => cmd_cp(parts.next(), parts.next(), s),
        "mv"     => cmd_mv(parts.next(), parts.next(), s),
        "stat"   => cmd_stat(parts.next(), s),
        "chmod"  => cmd_chmod(parts.next(), parts.next(), s),
        "chown"  => cmd_chown(parts.next(), parts.next(), s),
        "whoami" => cmd_whoami(s),
        "su"     => cmd_su(parts.next(), s),
        "users"  => cmd_users(),
        "uname"  => cmd_uname(),
        "ps"     => cmd_ps(),
        "uptime" => cmd_uptime(),
        "mem"    => cmd_mem(),
        "spawn"  => cmd_spawn(parts.next()),
        "run"    => cmd_run(parts.next()),
        "kill"   => cmd_kill(parts.next()),
        "diskinfo"  => cmd_diskinfo(),
        "diskread"  => cmd_diskread(parts.next()),
        "diskwrite" => cmd_diskwrite(parts.next(), line),
        "halt" | "exit" | "poweroff" | "shutdown" => cmd_halt(),
        ""       => {}
        other    => { print_str("unknown command: "); print_str(other); print_str("\n"); }
    }
}

// ── help ──────────────────────────────────────────────────────────────────────

fn cmd_help() {
    print_str("ferrugem Shell\n\n");
    print_str("Navigation:\n");
    print_str("  cd [path]              change directory (default: /)\n");
    print_str("  ls [-l] [path]         list directory  (-l for long format)\n");
    print_str("  pwd                    print working directory\n");
    print_str("\nFiles:\n");
    print_str("  touch <file>           create empty file\n");
    print_str("  cat <file>             print file contents\n");
    print_str("  write <file> <text>    overwrite file\n");
    print_str("  append <file> <text>   append to file\n");
    print_str("  cp <src> <dst>         copy file\n");
    print_str("  mv <src> <dst>         rename / move file\n");
    print_str("  rm <file>              delete file\n");
    print_str("  mkdir <dir>            create directory\n");
    print_str("  rmdir <dir>            remove empty directory\n");
    print_str("  stat <path>            show file metadata\n");
    print_str("\nRedirects:\n");
    print_str("  echo <text>            print text\n");
    print_str("  echo <text> > <file>   overwrite file\n");
    print_str("  echo <text> >> <file>  append to file\n");
    print_str("\nPermissions:\n");
    print_str("  chmod <mode> <file>    change mode (octal, e.g. 755)\n");
    print_str("  chown <user> <file>    change owner  (root only)\n");
    print_str("\nUsers:\n");
    print_str("  whoami                 print current user\n");
    print_str("  su [user]              switch user  (default: root)\n");
    print_str("  users                  list all users\n");
    print_str("\nSystem:\n");
    print_str("  uname                  kernel name and architecture\n");
    print_str("  uptime                 scheduler tick counter\n");
    print_str("  mem                    heap memory usage\n");
    print_str("  ps                     list running processes\n");
    print_str("  clear                  clear the screen\n");
    print_str("  halt | exit            halt the system\n");
    print_str("\nProcesses:\n");
    print_str("  spawn <task>           start a kernel task  (hello, counter)\n");
    print_str("  run <path>             load and run an ELF64 binary from VFS\n");
    print_str("  run user               start the built-in demo user program\n");
    print_str("  kill <slot>            terminate task by slot number\n");
    print_str("\nStorage (USB mass storage, x86_64 and Raspberry Pi 3 only):\n");
    print_str("  diskinfo               list attached USB disks and their capacity\n");
    print_str("  diskread <lba>         read one block, print its first 64 bytes\n");
    print_str("  diskwrite <lba> <text> write text (zero-padded) into one block\n");
    print_str("\nHistory: up/down arrows navigate command history\n");
}

// ── navigation ────────────────────────────────────────────────────────────────

fn cmd_pwd(s: &State) {
    let mut buf = [0u8; 256];
    let len = crate::fs::inode_path(s.cwd, &mut buf);
    if let Ok(path) = core::str::from_utf8(&buf[..len]) {
        print_str(path);
    } else {
        print_str("/");
    }
    print_str("\n");
}

fn cmd_cd(arg: Option<&str>, s: &mut State) {
    let path = arg.unwrap_or("/");
    match crate::fs::resolve(path, s.cwd) {
        None => { print_str("cd: no such directory: "); print_str(path); print_str("\n"); }
        Some(idx) => {
            match crate::fs::is_dir(path, s.cwd) {
                Some(true) => s.cwd = idx,
                _ => { print_str("cd: not a directory: "); print_str(path); print_str("\n"); }
            }
        }
    }
}

fn cmd_ls(first: Option<&str>, second: Option<&str>, s: &State) {
    let (long, path) = match (first, second) {
        (Some("-l"), Some(p)) => (true, p),
        (Some("-l"), None)    => (true, "."),
        (Some(p),   _)        => (false, p),
        (None,      _)        => (false, "."),
    };

    let mut found = false;
    crate::fs::list_dir(path, s.cwd, |inode| {
        found = true;
        if long {
            let m = format_mode(inode.kind == crate::vfs::InodeKind::Dir, inode.mode);
            if let Ok(ms) = core::str::from_utf8(&m) { print_str(ms); }
            print_str("  ");
            let owner = match users::lookup_by_uid(inode.uid) {
                Some(u) => u.name_str(), None => "?",
            };
            print_str(owner);
            print_str("  ");
            let group = match users::lookup_by_uid(inode.gid) {
                Some(u) => u.name_str(), None => "?",
            };
            print_str(group);
            print_str("  ");
            print_u64_padded(inode.size as u64, 6);
            print_str("  ");
        }
        print_str(inode.name_str());
        if inode.kind == crate::vfs::InodeKind::Dir { print_str("/"); }
        print_str("\n");
    });

    if !found {
        match crate::fs::resolve(path, s.cwd) {
            None => { print_str("ls: "); print_str(path); print_str(": no such file\n"); }
            Some(_) => { print_str(path); print_str("\n"); }
        }
    }
}

fn cmd_mkdir(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: mkdir <dir>\n"); return; } };
    if !crate::fs::mkdir(path, s.cwd, s.uid, s.gid, 0o755) {
        print_str("mkdir: cannot create directory: "); print_str(path); print_str("\n");
    }
}

fn cmd_rmdir(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: rmdir <dir>\n"); return; } };
    if !crate::fs::rmdir(path, s.cwd, s.uid, s.gid) {
        print_str("rmdir: cannot remove "); print_str(path);
        print_str(" (not empty, no permission, or not a directory)\n");
    }
}

fn cmd_rm(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: rm <file>\n"); return; } };
    if !crate::fs::unlink(path, s.cwd, s.uid, s.gid) {
        print_str("rm: cannot remove: "); print_str(path); print_str("\n");
    }
}

fn cmd_touch(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: touch <file>\n"); return; } };
    if !crate::fs::touch(path, s.cwd, s.uid, s.gid) {
        print_str("touch: cannot create: "); print_str(path); print_str("\n");
    }
}

fn cmd_cat(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: cat <file>\n"); return; } };
    match crate::fs::read_as(path, s.cwd, s.uid, s.gid) {
        Some(data) => {
            if let Ok(text) = core::str::from_utf8(data) {
                print_str(text);
            } else {
                print_str("<binary file>\n");
            }
        }
        None => {
            print_str("cat: "); print_str(path); print_str(": no such file or permission denied\n");
        }
    }
}

// ── file write / append / copy / move / stat ──────────────────────────────────

/// write <file> <content…>  create or overwrite with the given content.
fn cmd_write(path_arg: Option<&str>, line: &str, s: &State) {
    let path = match path_arg { Some(p) => p, None => { print_str("usage: write <file> <content>\n"); return; } };
    let content = skip_words(line, 2);
    if !crate::fs::write_as(path, s.cwd, content.as_bytes(), s.uid, s.gid) {
        print_str("write: cannot write: "); print_str(path); print_str("\n");
    }
}

/// append <file> <content…>  append a line to file (creates it if absent).
fn cmd_append(path_arg: Option<&str>, line: &str, s: &State) {
    let path = match path_arg { Some(p) => p, None => { print_str("usage: append <file> <content>\n"); return; } };
    let content = skip_words(line, 2);
    do_append(path, content, s);
}

fn do_append(path: &str, content: &str, s: &State) {
    static mut ABUF: [u8; 4096] = [0u8; 4096];
    let len = unsafe {
        let prefix = match crate::fs::read_as(path, s.cwd, s.uid, s.gid) {
            Some(existing) => {
                let l = existing.len().min(4090);
                ABUF[..l].copy_from_slice(&existing[..l]);
                l
            }
            None => 0,
        };
        let cbytes = content.as_bytes();
        let clen = cbytes.len().min(4090 - prefix);
        ABUF[prefix..prefix + clen].copy_from_slice(&cbytes[..clen]);
        ABUF[prefix + clen] = b'\n';
        prefix + clen + 1
    };
    if !crate::fs::write_as(path, s.cwd, unsafe { &ABUF[..len] }, s.uid, s.gid) {
        print_str("append: cannot write: "); print_str(path); print_str("\n");
    }
}

fn cmd_cp(src_arg: Option<&str>, dst_arg: Option<&str>, s: &State) {
    let src = match src_arg { Some(p) => p, None => { print_str("usage: cp <src> <dst>\n"); return; } };
    let dst = match dst_arg { Some(p) => p, None => { print_str("usage: cp <src> <dst>\n"); return; } };
    static mut CPBUF: [u8; 4096] = [0u8; 4096];
    let len = unsafe {
        match crate::fs::read_as(src, s.cwd, s.uid, s.gid) {
            None => { print_str("cp: cannot read: "); print_str(src); print_str("\n"); return; }
            Some(data) => {
                let l = data.len().min(4096);
                CPBUF[..l].copy_from_slice(&data[..l]);
                l
            }
        }
    };
    if !crate::fs::write_as(dst, s.cwd, unsafe { &CPBUF[..len] }, s.uid, s.gid) {
        print_str("cp: cannot write: "); print_str(dst); print_str("\n");
    }
}

fn cmd_mv(src_arg: Option<&str>, dst_arg: Option<&str>, s: &State) {
    let src = match src_arg { Some(p) => p, None => { print_str("usage: mv <src> <dst>\n"); return; } };
    let dst = match dst_arg { Some(p) => p, None => { print_str("usage: mv <src> <dst>\n"); return; } };
    if !crate::fs::rename(src, dst, s.cwd, s.uid, s.gid) {
        print_str("mv: cannot rename: "); print_str(src); print_str("\n");
    }
}

fn cmd_stat(arg: Option<&str>, s: &State) {
    let path = match arg { Some(p) => p, None => { print_str("usage: stat <path>\n"); return; } };
    let found = crate::fs::stat_at(path, s.cwd, |inode| {
        print_str("  File: "); print_str(inode.name_str()); print_str("\n");
        print_str("  Type: ");
        if inode.kind == crate::vfs::InodeKind::Dir {
            print_str("directory");
        } else {
            print_str("regular file");
        }
        print_str("\n  Mode: ");
        let m = format_mode(inode.kind == crate::vfs::InodeKind::Dir, inode.mode);
        if let Ok(ms) = core::str::from_utf8(&m) { print_str(ms); }
        print_str("\n   Uid: "); print_u64(inode.uid as u64);
        print_str("   Gid: "); print_u64(inode.gid as u64);
        print_str("\n  Size: "); print_u64(inode.size as u64); print_str(" bytes\n");
    });
    if !found {
        print_str("stat: "); print_str(path); print_str(": no such file or directory\n");
    }
}

// ── echo with optional redirect ───────────────────────────────────────────────

fn cmd_echo(line: &str, s: &State) {
    let rest = line.trim_start_matches("echo").trim_start_matches(' ');

    // Check >> before > to avoid matching the > inside >>.
    if let Some((content, path, append)) = parse_redirect(rest) {
        if path.is_empty() { print_str("echo: missing filename for redirect\n"); return; }
        if append {
            do_append(path, content, s);
        } else {
            let mut data = [0u8; 4096];
            let cbytes = content.as_bytes();
            let len = cbytes.len().min(4095);
            data[..len].copy_from_slice(&cbytes[..len]);
            data[len] = b'\n';
            if !crate::fs::write_as(path, s.cwd, &data[..len + 1], s.uid, s.gid) {
                print_str("echo: cannot write: "); print_str(path); print_str("\n");
            }
        }
        return;
    }

    print_str(rest);
    print_str("\n");
}

/// Parse `echo` body for ` >> path` or ` > path` (or `>> path` / `> path` at start).
/// Returns (content, path, is_append) or None.
fn parse_redirect(rest: &str) -> Option<(&str, &str, bool)> {
    if let Some(pos) = rest.find(" >> ") {
        return Some((&rest[..pos], rest[pos + 4..].trim(), true));
    }
    if rest.starts_with(">> ") {
        return Some(("", rest[3..].trim(), true));
    }
    if let Some(pos) = rest.find(" > ") {
        return Some((&rest[..pos], rest[pos + 3..].trim(), false));
    }
    if rest.starts_with("> ") {
        return Some(("", rest[2..].trim(), false));
    }
    None
}

fn cmd_clear() {
    crate::arch::console_clear();
}

// ── permissions ───────────────────────────────────────────────────────────────

fn cmd_chmod(mode_arg: Option<&str>, path_arg: Option<&str>, s: &State) {
    let mode_str = match mode_arg { Some(m) => m, None => { print_str("usage: chmod <mode> <file>\n"); return; } };
    let path     = match path_arg  { Some(p) => p, None => { print_str("usage: chmod <mode> <file>\n"); return; } };
    let mode = match parse_octal(mode_str) {
        Some(m) => m,
        None => { print_str("chmod: invalid mode (use octal, e.g. 755)\n"); return; }
    };
    if !crate::fs::chmod(path, s.cwd, mode, s.uid) {
        print_str("chmod: permission denied or file not found\n");
    }
}

fn cmd_chown(owner_arg: Option<&str>, path_arg: Option<&str>, s: &State) {
    let name = match owner_arg { Some(n) => n, None => { print_str("usage: chown <user> <file>\n"); return; } };
    let path = match path_arg  { Some(p) => p, None => { print_str("usage: chown <user> <file>\n"); return; } };
    let user = match users::lookup_by_name(name) {
        Some(u) => u,
        None => { print_str("chown: unknown user: "); print_str(name); print_str("\n"); return; }
    };
    if !crate::fs::chown(path, s.cwd, user.uid, user.gid, s.uid) {
        print_str("chown: permission denied (only root can chown)\n");
    }
}

// ── user control ──────────────────────────────────────────────────────────────

fn cmd_whoami(s: &State) {
    match users::lookup_by_uid(s.uid) {
        Some(u) => { print_str(u.name_str()); print_str("\n"); }
        None    => { print_str("uid="); print_u64(s.uid as u64); print_str("\n"); }
    }
}

fn cmd_su(arg: Option<&str>, s: &mut State) {
    let target_name = arg.unwrap_or("root");
    match users::lookup_by_name(target_name) {
        None => { print_str("su: user not found: "); print_str(target_name); print_str("\n"); }
        Some(u) => {
            s.uid = u.uid;
            s.gid = u.gid;
            print_str("switched to "); print_str(u.name_str()); print_str("\n");
        }
    }
}

fn cmd_users() {
    for u in users::list() {
        if !u.used { continue; }
        print_str("uid="); print_u64(u.uid as u64);
        print_str(" gid="); print_u64(u.gid as u64);
        print_str(" "); print_str(u.name_str()); print_str("\n");
    }
}

// ── system ────────────────────────────────────────────────────────────────────

fn cmd_uname() {
    print_str(crate::version::NAME);
    print_str(" ");
    print_str(crate::version::VERSION_FULL);
    print_str(" ");
    print_str(crate::arch::name());
    print_str("\n");
}

fn cmd_ps() {
    let self_slot = crate::scheduler::current_slot();
    print_str("SLOT  PID    STATE    TYPE\n");
    crate::scheduler::for_each_task(|slot, pid, state, is_user| {
        print_u64_padded(slot as u64, 4);
        print_str("  ");
        print_u64_padded(pid, 5);
        print_str("  ");
        let state_str = match state {
            crate::process::TaskState::Ready   => "ready  ",
            crate::process::TaskState::Running => "running",
            crate::process::TaskState::Blocked => "blocked",
            crate::process::TaskState::Zombie  => "zombie ",
        };
        print_str(state_str);
        print_str("  ");
        if slot == 0 {
            print_str("kernel (idle)");
        } else if slot == self_slot {
            print_str("kernel (self)");
        } else if is_user {
            print_str("user");
        } else {
            print_str("kernel");
        }
        print_str("\n");
    });
}

// ── process management ────────────────────────────────────────────────────────

fn cmd_spawn(arg: Option<&str>) {
    let name = match arg {
        Some(n) => n,
        None => { print_str("usage: spawn <name>  (tasks: hello, counter)\n"); return; }
    };
    match resolve_task(name) {
        None => { print_str("spawn: unknown task '"); print_str(name); print_str("'\n"); }
        Some(entry) => {
            match crate::scheduler::spawn_fn(entry) {
                Some(slot) => {
                    print_str("spawned '"); print_str(name);
                    print_str("' → slot "); print_u64(slot as u64); print_str("\n");
                }
                None => { print_str("spawn: run queue full\n"); }
            }
        }
    }
}

fn cmd_kill(arg: Option<&str>) {
    let s = match arg { Some(s) => s, None => { print_str("usage: kill <slot>\n"); return; } };
    let slot = match parse_decimal(s) {
        Some(n) => n as usize,
        None => { print_str("kill: invalid slot number\n"); return; }
    };
    if crate::scheduler::kill_task(slot) {
        print_str("killed slot "); print_u64(slot as u64); print_str("\n");
    } else {
        print_str("kill: no such task, cannot kill (slot 0 or self not allowed)\n");
    }
}

fn cmd_run(arg: Option<&str>) {
    let name = match arg {
        Some(n) => n,
        None => { print_str("usage: run <path>  or  run user\n"); return; }
    };
    match name {
        "user" => run_user_hello(),
        path   => run_elf_from_vfs(path),
    }
}

#[cfg(target_arch = "aarch64")]
fn run_user_hello() {
    print_str("run: EL0 userspace not implemented yet on aarch64 (Fase 2)\n");
}

#[cfg(not(target_arch = "aarch64"))]
fn run_user_hello() {
    #[cfg(target_arch = "x86_64")]
    let prog: &[u8] = &crate::userspace::HELLO_USER;
    #[cfg(target_arch = "riscv64")]
    let prog: &[u8] = &crate::userspace::HELLO_USER_RV64;
    match crate::scheduler::spawn_user(prog) {
        Some(slot) => { print_str("started user → slot "); print_u64(slot as u64); print_str("\n"); }
        None       => { print_str("run: failed (OOM or run queue full)\n"); }
    }
}

#[cfg(target_arch = "aarch64")]
fn run_elf_from_vfs(path: &str) {
    let _ = path;
    print_str("run: EL0 userspace not implemented yet on aarch64 (Fase 2)\n");
}

#[cfg(not(target_arch = "aarch64"))]
fn run_elf_from_vfs(path: &str) {
    let data = match crate::fs::read_as(path, 0, 0, 0) {
        Some(d) => d,
        None => { print_str("run: not found: "); print_str(path); print_str("\n"); return; }
    };
    if !crate::elf::is_elf(data) {
        print_str("run: not an ELF64 binary: "); print_str(path); print_str("\n");
        return;
    }
    match crate::scheduler::spawn_elf(data, path) {
        Some(slot) => {
            print_str("started '"); print_str(path);
            print_str("' → slot "); print_u64(slot as u64);
            print_str(" pid "); print_u64(crate::scheduler::task_pid(slot)); print_str("\n");
        }
        None => { print_str("run: failed (bad ELF, OOM, or run queue full)\n"); }
    }
}

// ── storage (USB mass storage) ────────────────────────────────────────────
//
// Two boards implement USB mass storage today (x86_64/UHCI and Raspberry
// Pi 3/DWC2, see `drivers::usb`/`boards::raspberrypi3::usb`), each behind
// its own module path; `disk_backend` picks the right one at compile time
// so `cmd_diskinfo`/`cmd_diskread`/`cmd_diskwrite` below are written once
// instead of once per board.

#[cfg(target_arch = "x86_64")]
mod disk_backend {
    pub const SUPPORTED: bool = true;
    pub fn count() -> usize { crate::drivers::usb::disk_count() }
    pub fn sector_size() -> usize { crate::drivers::usb::disk_sector_size() }
    pub fn block_count(i: usize) -> Option<u32> { crate::drivers::usb::disk_block_count(i) }
    pub fn read_block(i: usize, lba: u32, buf: &mut [u8]) -> Result<(), ()> { crate::drivers::usb::disk_read_block(i, lba, buf) }
    pub fn write_block(i: usize, lba: u32, data: &[u8]) -> Result<(), ()> { crate::drivers::usb::disk_write_block(i, lba, data) }
}

#[cfg(all(target_arch = "aarch64", feature = "board-raspberrypi3"))]
mod disk_backend {
    pub const SUPPORTED: bool = true;
    pub fn count() -> usize { crate::boards::raspberrypi3::usb::disk_count() }
    pub fn sector_size() -> usize { crate::boards::raspberrypi3::usb::disk_sector_size() }
    pub fn block_count(i: usize) -> Option<u32> { crate::boards::raspberrypi3::usb::disk_block_count(i) }
    pub fn read_block(i: usize, lba: u32, buf: &mut [u8]) -> Result<(), ()> { crate::boards::raspberrypi3::usb::disk_read_block(i, lba, buf) }
    pub fn write_block(i: usize, lba: u32, data: &[u8]) -> Result<(), ()> { crate::boards::raspberrypi3::usb::disk_write_block(i, lba, data) }
}

#[cfg(not(any(target_arch = "x86_64", all(target_arch = "aarch64", feature = "board-raspberrypi3"))))]
mod disk_backend {
    pub const SUPPORTED: bool = false;
    pub fn count() -> usize { 0 }
    pub fn sector_size() -> usize { 512 }
    pub fn block_count(_i: usize) -> Option<u32> { None }
    pub fn read_block(_i: usize, _lba: u32, _buf: &mut [u8]) -> Result<(), ()> { Err(()) }
    pub fn write_block(_i: usize, _lba: u32, _data: &[u8]) -> Result<(), ()> { Err(()) }
}

fn cmd_diskinfo() {
    if !disk_backend::SUPPORTED {
        print_str("diskinfo: USB mass storage is only supported on x86_64 and Raspberry Pi 3\n");
        return;
    }
    let n = disk_backend::count();
    if n == 0 {
        print_str("diskinfo: no USB mass storage device attached\n");
        return;
    }
    for i in 0..n {
        let Some(blocks) = disk_backend::block_count(i) else { continue };
        print_str("disk "); print_u64(i as u64);
        print_str(": "); print_u64(blocks as u64);
        print_str(" blocks x "); print_u64(disk_backend::sector_size() as u64);
        print_str(" bytes\n");
    }
}

fn cmd_diskread(arg: Option<&str>) {
    if !disk_backend::SUPPORTED {
        print_str("diskread: USB mass storage is only supported on x86_64 and Raspberry Pi 3\n");
        return;
    }
    let lba = match arg.and_then(parse_decimal) {
        Some(n) => n as u32,
        None => { print_str("usage: diskread <lba>\n"); return; }
    };
    if disk_backend::count() == 0 {
        print_str("diskread: no USB mass storage device attached\n");
        return;
    }
    let mut buf = [0u8; 512];
    match disk_backend::read_block(0, lba, &mut buf) {
        Ok(()) => print_hex_dump(&buf[..64]),
        Err(()) => print_str("diskread: read failed\n"),
    }
}

fn cmd_diskwrite(lba_arg: Option<&str>, line: &str) {
    if !disk_backend::SUPPORTED {
        print_str("diskwrite: USB mass storage is only supported on x86_64 and Raspberry Pi 3\n");
        return;
    }
    let lba = match lba_arg.and_then(parse_decimal) {
        Some(n) => n as u32,
        None => { print_str("usage: diskwrite <lba> <text>\n"); return; }
    };
    if disk_backend::count() == 0 {
        print_str("diskwrite: no USB mass storage device attached\n");
        return;
    }
    let text = skip_words(line, 2);
    let mut buf = [0u8; 512];
    let tbytes = text.as_bytes();
    let len = tbytes.len().min(buf.len());
    buf[..len].copy_from_slice(&tbytes[..len]);
    match disk_backend::write_block(0, lba, &buf) {
        Ok(()) => { print_str("wrote "); print_u64(len as u64); print_str(" bytes to block "); print_u64(lba as u64); print_str("\n"); }
        Err(()) => print_str("diskwrite: write failed\n"),
    }
}

fn cmd_halt() {
    print_str("System halting...\n");
    crate::arch::halt();
}

// ── built-in demo tasks ───────────────────────────────────────────────────────

fn resolve_task(name: &str) -> Option<fn() -> !> {
    match name {
        "hello"   => Some(task_hello),
        "counter" => Some(task_counter),
        _         => None,
    }
}

fn task_hello() -> ! {
    crate::arch::console_print_str("[hello task] Hello from a background kernel task!\n");
    loop { crate::scheduler::schedule(); }
}

fn task_counter() -> ! {
    for i in 1u64..=5 {
        crate::arch::console_print_str("[counter] tick ");
        let mut buf = [b'0'; 20];
        let mut pos = 20usize;
        let mut v = i;
        if v == 0 { pos -= 1; } else { while v > 0 { pos -= 1; buf[pos] = b'0' + (v % 10) as u8; v /= 10; } }
        if let Ok(s) = core::str::from_utf8(&buf[pos..]) { crate::arch::console_print_str(s); }
        crate::arch::console_print_str("\n");
        crate::scheduler::schedule();
    }
    crate::scheduler::exit_current(0);
    loop {}
}

fn cmd_uptime() {
    let ticks = crate::scheduler::tick_count();
    print_str("ticks: "); print_u64(ticks); print_str("\n");
}

fn cmd_mem() {
    let s = crate::memory::heap_stats();
    print_str("heap total:  "); print_u64((s.total / 1024) as u64); print_str(" KiB\n");
    print_str("     used:   "); print_u64(s.used as u64); print_str(" bytes");
    print_str(" ("); print_u64(s.allocs as u64); print_str(" allocations)\n");
    print_str("     free:   "); print_u64(s.free as u64); print_str(" bytes\n");
    // Show fragmentation indicator when free space is split across many blocks
    let overhead = s.total.saturating_sub(s.used).saturating_sub(s.free);
    if overhead > 0 {
        print_str("  overhead:  "); print_u64(overhead as u64); print_str(" bytes (headers)\n");
    }
}

// ── input key decoding ──────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Key { Char(u8), Enter, Backspace, Up, Down, Left, Right, Esc, Interrupt }

fn read_key() -> Key {
    let c = read_char();
    match c {
        b'\n' | b'\r'        => Key::Enter,
        b'\x08' | 0x7F       => Key::Backspace,
        b'\x03'              => Key::Interrupt, // Ctrl+C
        b'\x1b' => {
            match read_char_timeout(200_000) {
                Some(b'[') => {}
                _          => return Key::Esc,
            }
            match read_char_timeout(200_000) {
                Some(b'A') => Key::Up,
                Some(b'B') => Key::Down,
                Some(b'C') => Key::Right,
                Some(b'D') => Key::Left,
                _          => Key::Esc,
            }
        }
        b => Key::Char(b),
    }
}

fn read_char() -> u8 {
    loop {
        if let Some(b) = crate::drivers::serial::read_byte() { return b; }
        if let Some(c) = crate::drivers::keyboard::read_byte() { return c; }
        crate::scheduler::block_on_tty();
    }
}

fn read_char_nonblocking() -> Option<u8> {
    if let Some(b) = crate::drivers::serial::read_byte() { return Some(b); }
    crate::drivers::keyboard::read_byte()
}

fn read_char_timeout(spins: u64) -> Option<u8> {
    let mut i = 0u64;
    loop {
        if let Some(b) = read_char_nonblocking() { return Some(b); }
        i += 1;
        if i >= spins { return None; }
    }
}

// ── line editor with history navigation ──────────────────────────────────────

fn read_line(hist: &History) -> LineBuffer {
    let mut buf  = LineBuffer::new();
    let mut saved = LineBuffer::new(); // in-progress input saved during history nav
    let mut nav: i32 = -1;            // -1 = not navigating; ≥0 = history offset

    loop {
        match read_key() {
            Key::Enter => { print_str("\n"); break; }

            Key::Interrupt => {
                // Ctrl+C: cancel current line, print ^C, restart.
                print_str("^C\n");
                return LineBuffer::new();
            }

            Key::Backspace => {
                if buf.backspace() { print_str("\x08 \x08"); }
                nav = -1;
            }

            Key::Char(c) if c >= 32 && c <= 126 => {
                if buf.push(c) {
                    let s = [c];
                    if let Ok(ch) = core::str::from_utf8(&s) { print_str(ch); }
                }
                nav = -1;
            }

            Key::Up => {
                let next = if nav < 0 { 0 } else { nav + 1 };
                if let Some((data, len)) = hist.get(next as usize) {
                    if nav < 0 { saved = buf.clone_copy(); }
                    erase_chars(buf.len);
                    buf.load(data, len);
                    print_str(buf.as_str());
                    nav = next;
                }
            }

            Key::Down => {
                if nav > 0 {
                    let next = nav - 1;
                    if let Some((data, len)) = hist.get(next as usize) {
                        erase_chars(buf.len);
                        buf.load(data, len);
                        print_str(buf.as_str());
                        nav = next;
                    }
                } else if nav == 0 {
                    erase_chars(buf.len);
                    buf = saved;
                    saved = LineBuffer::new();
                    print_str(buf.as_str());
                    nav = -1;
                }
            }

            _ => {}
        }
    }
    buf
}

/// Erase `n` visible characters by printing backspace-space-backspace sequences.
fn erase_chars(n: usize) {
    for _ in 0..n { print_str("\x08 \x08"); }
}

// ── line buffer ───────────────────────────────────────────────────────────────

struct LineBuffer {
    data: [u8; MAX_CMD],
    len:  usize,
}

impl LineBuffer {
    fn new() -> Self { Self { data: [0u8; MAX_CMD], len: 0 } }

    fn push(&mut self, b: u8) -> bool {
        if self.len < MAX_CMD - 1 { self.data[self.len] = b; self.len += 1; true } else { false }
    }

    fn backspace(&mut self) -> bool {
        if self.len > 0 { self.len -= 1; true } else { false }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }

    fn as_bytes(&self) -> &[u8] { &self.data[..self.len] }

    fn is_empty(&self) -> bool { self.len == 0 }

    fn load(&mut self, data: &[u8], len: usize) {
        let l = len.min(MAX_CMD - 1);
        self.data[..l].copy_from_slice(&data[..l]);
        self.len = l;
    }

    fn clone_copy(&self) -> Self {
        let mut b = Self::new();
        b.data[..self.len].copy_from_slice(&self.data[..self.len]);
        b.len = self.len;
        b
    }
}

// ── formatting helpers ────────────────────────────────────────────────────────

fn format_mode(is_dir: bool, mode: u16) -> [u8; 10] {
    let mut s = [b'-'; 10];
    s[0] = if is_dir { b'd' } else { b'-' };
    if mode & 0o400 != 0 { s[1] = b'r'; }
    if mode & 0o200 != 0 { s[2] = b'w'; }
    if mode & 0o100 != 0 { s[3] = b'x'; }
    if mode & 0o040 != 0 { s[4] = b'r'; }
    if mode & 0o020 != 0 { s[5] = b'w'; }
    if mode & 0o010 != 0 { s[6] = b'x'; }
    if mode & 0o004 != 0 { s[7] = b'r'; }
    if mode & 0o002 != 0 { s[8] = b'w'; }
    if mode & 0o001 != 0 { s[9] = b'x'; }
    s
}

fn parse_octal(s: &str) -> Option<u16> {
    let mut val = 0u16;
    for b in s.bytes() {
        if b < b'0' || b > b'7' { return None; }
        val = val.saturating_mul(8).saturating_add((b - b'0') as u16);
    }
    Some(val)
}

fn parse_decimal(s: &str) -> Option<u64> {
    let mut val = 0u64;
    let mut any = false;
    for b in s.bytes() {
        if b < b'0' || b > b'9' { return None; }
        val = val.saturating_mul(10).saturating_add((b - b'0') as u64);
        any = true;
    }
    if any { Some(val) } else { None }
}

/// Skip the first `n` whitespace-separated words in `s`, return the remainder.
fn skip_words(s: &str, n: usize) -> &str {
    let mut s = s;
    for _ in 0..n {
        s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
        s = s.trim_start_matches(|c: char| !c.is_ascii_whitespace());
    }
    s.trim_start_matches(|c: char| c.is_ascii_whitespace())
}

fn print_u64(n: u64) {
    if n == 0 { print_str("0"); return; }
    let mut buf = [b'0'; 20];
    let mut i = 20usize;
    let mut v = n;
    while v > 0 { i -= 1; buf[i] = b'0' + (v % 10) as u8; v /= 10; }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) { print_str(s); }
}

fn print_hex_dump(data: &[u8]) {
    for (i, b) in data.iter().enumerate() {
        if i > 0 && i % 16 == 0 { print_str("\n"); }
        print_hex_byte(*b);
        print_str(" ");
    }
    print_str("\n");
}

fn print_hex_byte(b: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let s = [HEX[(b >> 4) as usize], HEX[(b & 0xF) as usize]];
    if let Ok(s) = core::str::from_utf8(&s) { print_str(s); }
}

fn print_u64_padded(n: u64, width: usize) {
    let mut buf = [b' '; 20];
    if n == 0 {
        buf[width - 1] = b'0';
    } else {
        let mut i = width;
        let mut v = n;
        while v > 0 && i > 0 { i -= 1; buf[i] = b'0' + (v % 10) as u8; v /= 10; }
    }
    if let Ok(s) = core::str::from_utf8(&buf[..width]) { print_str(s); }
}

// ── arch I/O ──────────────────────────────────────────────────────────────────

fn print_str(s: &str) {
    crate::arch::console_print_str(s);
}

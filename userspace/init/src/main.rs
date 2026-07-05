use std::io::{self, BufRead, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, exit};

fn main() {
    print!("\x1b[2J\x1b[H");
    println!("{}", banner());
    println!("Type 'help' for available commands.\n");

    let stdin = io::stdin();
    loop {
        print_prompt();
        let _ = io::stdout().flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Err(_) | Ok(0) => {
                println!("\n[init] stdin closed, halting");
                break;
            }
            Ok(_) => {}
        }
        let line = line.trim();
        if line.is_empty() { continue; }
        if matches!(line, "exit" | "halt" | "poweroff" | "shutdown") { break; }

        run_line(line);

        // Reap any remaining zombies
        loop {
            let r = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
            if r <= 0 { break; }
        }
    }

    println!("[init] done");
    exit(0);
}

// ── banner / prompt ─────────────────────────────────────────────────────────
//
// Mirrors the kernel's built-in shell (used on RISC-V, which has no userspace
// init yet) so both architectures present the same look and feel: same
// "Ferrugem v<release>" banner, same "user@ferrugem:<cwd># " prompt.

fn banner() -> String {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } == 0 {
        let release = cstr_to_string(u.release.as_ptr());
        format!("Ferrugem v{release}")
    } else {
        "Ferrugem".to_string()
    }
}

fn cstr_to_string(ptr: *const libc::c_char) -> String {
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn print_prompt() {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "/".to_string());
    print!("root@ferrugem:{cwd}# ");
}

// ── dispatch ─────────────────────────────────────────────────────────────────

fn run_line(line: &str) {
    let mut parts = line.splitn(2, ' ');
    let cmd  = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    let args: Vec<&str> = rest.split_whitespace().collect();

    match cmd {
        "help"   => cmd_help(),
        "pwd"    => cmd_pwd(),
        "cd"     => cmd_cd(args.first().copied()),
        "ls"     => cmd_ls(args.first().copied()),
        "mkdir"  => cmd_mkdir(args.first().copied()),
        "rmdir"  => cmd_rmdir(args.first().copied()),
        "rm"     => cmd_rm(args.first().copied()),
        "touch"  => cmd_touch(args.first().copied()),
        "cat"    => cmd_cat(args.first().copied()),
        "echo"   => cmd_echo(rest),
        "whoami" => println!("root"),
        "uname"  => println!("{}", banner()),
        "clear"  => print!("\x1b[2J\x1b[H"),
        _        => cmd_exec(cmd, &args),
    }
}

fn cmd_help() {
    println!("Ferrugem shell (userspace init)\n");
    println!("Navigation:");
    println!("  cd [path]              change directory (default: /)");
    println!("  ls [path]              list directory");
    println!("  pwd                    print working directory");
    println!("\nFiles:");
    println!("  touch <file>           create empty file");
    println!("  cat <file>             print file contents");
    println!("  rm <file>              delete file");
    println!("  mkdir <dir>            create directory");
    println!("  rmdir <dir>            remove empty directory");
    println!("\nRedirects:");
    println!("  echo <text>            print text");
    println!("  echo <text> > <file>   overwrite file");
    println!("  echo <text> >> <file>  append to file");
    println!("\nOther:");
    println!("  whoami                 print current user");
    println!("  uname                  kernel name and version");
    println!("  clear                  clear the screen");
    println!("  exit                   halt the system");
    println!("\nAnything else is looked up as a program in PATH.");
}

fn cmd_pwd() {
    match std::env::current_dir() {
        Ok(p)  => println!("{}", p.display()),
        Err(e) => eprintln!("pwd: {e}"),
    }
}

fn cmd_cd(path: Option<&str>) {
    let path = path.unwrap_or("/");
    if let Err(e) = std::env::set_current_dir(path) {
        eprintln!("cd: {path}: {e}");
    }
}

fn cmd_ls(path: Option<&str>) {
    let path = path.unwrap_or(".");
    let entries = match std::fs::read_dir(path) {
        Ok(e)  => e,
        Err(e) => { eprintln!("ls: {path}: {e}"); return; }
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names { println!("{name}"); }
}

fn cmd_mkdir(path: Option<&str>) {
    let Some(path) = path else { eprintln!("mkdir: missing operand"); return; };
    if let Err(e) = std::fs::create_dir(path) {
        eprintln!("mkdir: {path}: {e}");
    }
}

fn cmd_rmdir(path: Option<&str>) {
    let Some(path) = path else { eprintln!("rmdir: missing operand"); return; };
    if let Err(e) = std::fs::remove_dir(path) {
        eprintln!("rmdir: {path}: {e}");
    }
}

fn cmd_rm(path: Option<&str>) {
    let Some(path) = path else { eprintln!("rm: missing operand"); return; };
    if let Err(e) = std::fs::remove_file(path) {
        eprintln!("rm: {path}: {e}");
    }
}

fn cmd_touch(path: Option<&str>) {
    let Some(path) = path else { eprintln!("touch: missing operand"); return; };
    if !Path::new(path).exists() {
        if let Err(e) = std::fs::File::create(path) {
            eprintln!("touch: {path}: {e}");
        }
    }
}

fn cmd_cat(path: Option<&str>) {
    let Some(path) = path else { eprintln!("cat: missing operand"); return; };
    let mut file = match std::fs::File::open(path) {
        Ok(f)  => f,
        Err(e) => { eprintln!("cat: {path}: {e}"); return; }
    };
    let mut buf = Vec::new();
    if let Err(e) = file.read_to_end(&mut buf) {
        eprintln!("cat: {path}: {e}");
        return;
    }
    let _ = io::stdout().write_all(&buf);
}

fn cmd_echo(rest: &str) {
    if let Some((text, path)) = rest.rsplit_once(">>") {
        return echo_to_file(text.trim(), path.trim(), true);
    }
    if let Some((text, path)) = rest.rsplit_once('>') {
        return echo_to_file(text.trim(), path.trim(), false);
    }
    println!("{rest}");
}

fn echo_to_file(text: &str, path: &str, append: bool) {
    if path.is_empty() { eprintln!("echo: missing filename for redirect"); return; }
    let result = std::fs::OpenOptions::new()
        .write(true).create(true).append(append).truncate(!append)
        .open(path)
        .and_then(|mut f| writeln!(f, "{text}"));
    if let Err(e) = result {
        eprintln!("echo: cannot write: {path}: {e}");
    }
}

fn cmd_exec(cmd: &str, args: &[&str]) {
    if cmd.is_empty() { return; }
    let mut command = Command::new(cmd);
    command.args(args);
    // Force the plain fork()+execve() path instead of musl's posix_spawn/__clone
    // fast path: any pre_exec closure disables that fast path in Rust's std,
    // and our kernel's fork() (unlike its vfork/CLONE_VM emulation) is solid.
    unsafe { command.pre_exec(|| Ok(())); }
    match command.spawn() {
        Err(e) => eprintln!("[init] {cmd}: {e}"),
        Ok(mut child) => { let _ = child.wait(); }
    }
}

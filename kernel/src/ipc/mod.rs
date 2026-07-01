/// IPC: pipes and message queues.
///
/// The pipe pool is intentionally standalone — it has no dependency on the
/// scheduler.  Wake-up of blocked readers is handled by the scheduler itself
/// (see `scheduler::wake_pipe_waiter` / `scheduler::block_on_pipe`).

// ── ring-buffer pipe ──────────────────────────────────────────────────────────

const PIPE_BUF: usize = 4096;

pub struct Pipe {
    buf:  [u8; PIPE_BUF],
    head: usize, // write index
    tail: usize, // read  index
}

impl Pipe {
    const fn new() -> Self {
        Self { buf: [0u8; PIPE_BUF], head: 0, tail: 0 }
    }

    pub fn is_empty(&self) -> bool { self.head == self.tail }

    pub fn is_full(&self) -> bool { (self.head + 1) % PIPE_BUF == self.tail }

    pub fn write(&mut self, data: &[u8]) -> usize {
        let mut written = 0;
        for &b in data {
            let next = (self.head + 1) % PIPE_BUF;
            if next == self.tail { break; }
            self.buf[self.head] = b;
            self.head = next;
            written += 1;
        }
        written
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        for slot in buf.iter_mut() {
            if self.head == self.tail { break; }
            *slot = self.buf[self.tail];
            self.tail = (self.tail + 1) % PIPE_BUF;
            n += 1;
        }
        n
    }
}

// ── pipe pool ─────────────────────────────────────────────────────────────────

pub const MAX_PIPES: usize = 4;

struct PipeSlot {
    pipe:       Pipe,
    in_use:     bool,
    read_open:  bool,
    write_open: bool,
}

impl PipeSlot {
    const fn new() -> Self {
        Self { pipe: Pipe::new(), in_use: false, read_open: false, write_open: false }
    }
}

static mut POOL: [PipeSlot; MAX_PIPES] = [const { PipeSlot::new() }; MAX_PIPES];

/// Allocate a new pipe; returns its index or None if all slots are taken.
pub fn alloc_pipe() -> Option<u8> {
    unsafe {
        for (i, slot) in POOL.iter_mut().enumerate() {
            if !slot.in_use {
                slot.pipe = Pipe::new();
                slot.in_use     = true;
                slot.read_open  = true;
                slot.write_open = true;
                return Some(i as u8);
            }
        }
        None
    }
}

/// Close the read end of pipe `idx`.
pub fn close_pipe_read(idx: u8) {
    unsafe {
        let i = idx as usize;
        if i < MAX_PIPES && POOL[i].in_use {
            POOL[i].read_open = false;
            if !POOL[i].write_open { POOL[i].in_use = false; }
        }
    }
}

/// Close the write end of pipe `idx`.
pub fn close_pipe_write(idx: u8) {
    unsafe {
        let i = idx as usize;
        if i < MAX_PIPES && POOL[i].in_use {
            POOL[i].write_open = false;
            if !POOL[i].read_open { POOL[i].in_use = false; }
        }
    }
}

/// Returns true if the write end of the pipe is still open (reader can block).
pub fn pipe_write_open(idx: u8) -> bool {
    unsafe { idx < MAX_PIPES as u8 && POOL[idx as usize].write_open }
}

/// Returns true if the read end is still open (writer should not get SIGPIPE yet).
pub fn pipe_read_open(idx: u8) -> bool {
    unsafe { idx < MAX_PIPES as u8 && POOL[idx as usize].read_open }
}

/// Write `data` into pipe `idx`; returns bytes written (may be partial if full).
pub fn pipe_write(idx: u8, data: &[u8]) -> usize {
    unsafe {
        let i = idx as usize;
        if i >= MAX_PIPES || !POOL[i].in_use { return 0; }
        POOL[i].pipe.write(data)
    }
}

/// Read up to `buf.len()` bytes from pipe `idx`; returns bytes read.
pub fn pipe_read(idx: u8, buf: &mut [u8]) -> usize {
    unsafe {
        let i = idx as usize;
        if i >= MAX_PIPES || !POOL[i].in_use { return 0; }
        POOL[i].pipe.read(buf)
    }
}

/// True if the pipe has data available to read.
pub fn pipe_has_data(idx: u8) -> bool {
    unsafe { idx < MAX_PIPES as u8 && !POOL[idx as usize].pipe.is_empty() }
}

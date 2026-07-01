/// Generic fixed-size circular byte buffer shared by keyboard and serial drivers.
/// N can be any size; indices wrap via modulo.
///
/// # Safety
/// Callers that hold a `static mut RingBuf` must ensure exclusive access
/// (single-core kernel interrupts must be disabled during push/pop pairs
/// if the buffer is shared between an IRQ handler and a reader).

pub struct RingBuf<const N: usize> {
    buf:  [u8; N],
    head: usize, // write index
    tail: usize, // read index
}

impl<const N: usize> RingBuf<N> {
    pub const fn new() -> Self {
        Self { buf: [0u8; N], head: 0, tail: 0 }
    }

    /// Push one byte. Returns false if the buffer is full.
    pub fn push(&mut self, b: u8) -> bool {
        let next = (self.head + 1) % N;
        if next == self.tail {
            return false; // full
        }
        self.buf[self.head] = b;
        self.head = next;
        true
    }

    /// Pop one byte. Returns None if the buffer is empty.
    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let b = self.buf[self.tail];
        self.tail = (self.tail + 1) % N;
        Some(b)
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

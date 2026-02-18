use core::sync::atomic::{AtomicUsize, Ordering};

const CQ_N: usize = 8192;

pub type CqeT = u8;

pub struct CircularQueue {
    c_buf: [CqeT; CQ_N],
    fence: u32,
    head: AtomicUsize,
    tail: AtomicUsize,
    overflow: usize,
    errors_fatal_p: bool,
}

impl CircularQueue {
    pub fn new(errors_fatal_p: bool) -> Self {
        CircularQueue {
            c_buf: [0; CQ_N],
            fence: 0x12345678,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            overflow: 0,
            errors_fatal_p,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::SeqCst) == self.tail.load(Ordering::SeqCst)
    }

    pub fn is_full(&self) -> bool {
        (self.head.load(Ordering::SeqCst) + 1) % CQ_N == self.tail.load(Ordering::SeqCst)
    }

    pub fn nelem(&self) -> usize {
        (self.head.load(Ordering::SeqCst).wrapping_sub(self.tail.load(Ordering::SeqCst))) % CQ_N
    }

    pub fn nspace(&self) -> usize {
        (CQ_N - 1) - self.nelem()
    }

    pub fn pop_nonblock(&mut self) -> Option<CqeT> {
        if self.is_empty() {
            return None;
        }
        let tail = self.tail.load(Ordering::SeqCst);
        let e = self.c_buf[tail];
        self.tail.store((tail + 1) % CQ_N, Ordering::SeqCst);
        Some(e)
    }

    pub fn push(&mut self, x: CqeT) -> bool {
        if self.is_full() {
            return false;
        }
        let head = self.head.load(Ordering::SeqCst);
        self.c_buf[head] = x;
        self.head.store((head + 1) % CQ_N, Ordering::SeqCst);
        true
    }

    pub fn peek(&self) -> Option<CqeT> {
        if self.is_empty() {
            return None;
        }
        Some(self.c_buf[self.tail.load(Ordering::SeqCst)])
    }

    pub fn ok(&self) {
        assert_eq!(self.fence, 0x12345678, "fence is corrupted");
    }
}
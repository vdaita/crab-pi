use core::sync::atomic::{AtomicUsize, Ordering};

pub struct CircularQueue<T, const N: usize> {
    c_buf: [T; N],
    fence: u32,
    head: AtomicUsize,
    tail: AtomicUsize,
    overflow: usize,
    errors_fatal_p: bool,
}

impl<T: Copy + Default, const N: usize> CircularQueue<T, N> {
    pub fn new(errors_fatal_p: bool) -> Self {
        CircularQueue {
            c_buf: [T::default(); N],
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
        (self.head.load(Ordering::SeqCst) + 1) % N == self.tail.load(Ordering::SeqCst)
    }

    pub fn nelem(&self) -> usize {
        (self.head.load(Ordering::SeqCst).wrapping_sub(self.tail.load(Ordering::SeqCst))) % N
    }

    pub fn nspace(&self) -> usize {
        (N - 1) - self.nelem()
    }

    pub fn pop_nonblock(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let tail = self.tail.load(Ordering::SeqCst);
        let e = self.c_buf[tail];
        self.tail.store((tail + 1) % N, Ordering::SeqCst);
        Some(e)
    }

    pub fn push(&mut self, x: T) -> bool {
        if self.is_full() {
            return false;
        }
        let head = self.head.load(Ordering::SeqCst);
        self.c_buf[head] = x;
        self.head.store((head + 1) % N, Ordering::SeqCst);
        true
    }

    pub fn peek(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        Some(self.c_buf[self.tail.load(Ordering::SeqCst)])
    }

    pub fn ok(&self) {
        assert_eq!(self.fence, 0x12345678, "fence is corrupted");
    }
}
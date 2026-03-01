#[derive(Copy, Clone)]
pub struct Timer;

impl Timer {
    /// Get raw microseconds without barriers
    pub fn get_usec_raw() -> u32 {
        unsafe { core::ptr::read_volatile(0x2000_3004 as *const u32) }
    }

    /// Get microseconds with memory barriers
    pub fn get_usec() -> u32 {
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let u = Self::get_usec_raw();
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        u
    }

    /// Delay in microseconds
    pub fn delay_us(us: u32) {
        let start = Self::get_usec();
        while Self::get_usec().wrapping_sub(start) < us {}
    }

    /// Delay in milliseconds
    pub fn delay_ms(ms: u32) {
        Self::delay_us(ms.wrapping_mul(1000));
    }

    /// Delay in seconds
    pub fn delay_sec(sec: u32) {
        Self::delay_ms(sec.wrapping_mul(1000));
    }
}
/// Timer-seeded xorshift. Not cryptographic: it only has to look random
/// enough for AT_RANDOM (stack canaries) and getrandom.
pub fn fill_random(buf: &mut [u8]) {
    static mut STATE: u32 = 0;
    unsafe {
        if STATE == 0 {
            STATE = crate::timer::Timer::get_usec() | 1;
        }
        for byte in buf {
            STATE ^= STATE << 13;
            STATE ^= STATE >> 17;
            STATE ^= STATE << 5;
            *byte = (STATE >> 24) as u8;
        }
    }
}

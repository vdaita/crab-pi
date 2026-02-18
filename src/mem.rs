#[inline(always)]
pub fn put32(addr: u32, val: u32) {
    unsafe { ::core::arch::asm!("str r1, [r0]", in("r0") addr, in("r1") val) }
}

#[inline(always)]
pub fn get32(addr: u32) -> u32 {
    let result: u32;
    unsafe { ::core::arch::asm!("ldr r0, [r0]", in("r0") addr, lateout("r0") result) }
    result
}

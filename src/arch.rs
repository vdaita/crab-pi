//! Support for the CPU that we're using.

#[inline(always)]
pub fn dsb() {
    unsafe { ::core::arch::asm!("mcr p15, 0, {t}, c7, c10, 4", t = in(reg) 0) }
}

#[inline(always)]
pub fn dev_barrier() {
    unsafe {::core::arch::asm!("mcr p15, 0, r0, c7, c10, 4")}
}

#[inline(always)]
pub fn cpsr_get() -> u32 {
    let cpsr: u32;
    unsafe { ::core::arch::asm!("mrs {0}, cpsr", out(reg) cpsr) }
    cpsr
}

#[inline(always)]
pub fn cpsr_set(cpsr: u32) {
    unsafe { ::core::arch::asm!("msr cpsr, {0}", in(reg) cpsr) }
}

#[inline(always)]
pub fn cpsr_int_enabled() -> bool {
    ((cpsr_get() >> 7) & 1) == 0
}

#[inline(always)]
pub fn cpsr_int_enable() -> u32 {
    let cpsr = cpsr_get();
    cpsr_set(cpsr & !(1 << 7));
    cpsr
}

#[inline(always)]
pub fn cpsr_int_disable() -> u32 {
    let cpsr = cpsr_get();
    cpsr_set(cpsr | (1 << 7));
    cpsr
}

#[inline(always)]
pub fn cpsr_int_reset(cpsr: u32) -> u32 {
    if (cpsr & (1 << 7)) != 0 {
        cpsr_int_disable()
    } else {
        cpsr_int_enable()
    }
}

pub fn wait() {}
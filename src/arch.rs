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

#[inline(always)]
pub unsafe fn prefetch_flush() {
    core::arch::asm!("mcr p15, 0, {}, c7, c5, 4", in(reg) 0u32);
}

#[inline(always)]
pub fn prefetch_fetch() {
    unsafe { prefetch_flush() }
}

#[inline(always)]
pub fn flush_btb() {
    unsafe { core::arch::asm!("mcr p15, 0, {t}, c7, c5, 6", t = in(reg) 0u32) }
}

#[inline(always)]
pub fn control_reg1_rd() -> u32 {
    let value: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {value}, c1, c0, 0",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[inline(always)]
pub fn control_reg1_wr(value: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {value}, c1, c0, 0",
            value = in(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn gcc_mb() {
    // unsafe { ::core::arch::asm!("", options(nostack, preserves_flags)) }
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

#[inline(always)]
pub fn clean_inv_dcache() {
    unsafe { core::arch::asm!("mcr p15, 0, {t}, c7, c14, 0", t = in(reg) 0u32) }
}

#[inline(always)] 
pub fn inv_tlb() {
    unsafe { core::arch::asm!("mcr p15, 0, {t}, c8, c7, 0", t = in(reg) 0u32) }
}

#[inline(always)]
pub fn inv_icache() {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {t}, c7, c5, 0",
            "mcr p15, 0, {t}, c7, c5, 0",
            "mcr p15, 0, {t}, c7, c5, 0",
            "mcr p15, 0, {t}, c7, c5, 0",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            "nop",
            t = in(reg) 0u32,
            options(nostack)
        );
    }
}

#[inline(always)]
pub fn inv_all_caches() {
    clean_inv_dcache();
    inv_icache();
}

#[inline(always)]
pub fn asid_set(val: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {val}, c13, c0, 1",
            val = in(reg) val,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn ttrb0_set(val: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {val}, c2, c0, 0",
            val = in(reg) val,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn ttrb1_set(val: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {val}, c2, c0, 1",
            val = in(reg) val,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline(always)]
pub fn ttbr_base_ctrl_wr(val: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {val}, c2, c0, 2",
            val = in(reg) val,
            options(nomem, nostack, preserves_flags)
        );
    }
}


pub fn wait() {}
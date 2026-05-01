use core::arch::global_asm;
use crate::arch::prefetch_flush;
use crate::bit_utils::{bit_get, bits_get, bits_set, bit_clr, bit_set};
use crate::ckalloc;

global_asm!(r#"
.globl _interrupt_table_prof
.globl _interrupt_table_end_prof
_interrupt_table_prof:
    ldr pc, _reset_asm_prof
    ldr pc, _undefined_insturction 
...


"#);

fn breakpoint_mismatch_set(addr: u32) {
    unsafe {
        let bcr0_state = 0x4001e7;
        core::arch::asm!( // setting bcr0
            "mcr p14, 0, {0}, c0, c0, 5",
            in(reg) bcr0_state,
            options(nomem, nostack)
        );
        prefetch_flush();

        let bvr0_state = bits_set(0, 2, 31, addr >> 2);
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c0, 4",
            in(reg) bvr0_state,
            options(nomem, nostack)
        );
        prefetch_flush(); 
    }  
}

fn breakpoint_mismatch_start() {
    unsafe {
        let dscr_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) dscr_state,
            options(nomem, nostack)
        );
        let new_dscr_state = bit_clr(bit_set(dscr_state, 15), 14);
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c1, 0",
            in(reg) new_dscr_state,
            options(nomem, nostack)
        );
        prefetch_flush();

        breakpoint_mismatch_set(0);
        prefetch_flush();
    }
}

fn breakpoint_mismatch_stop() {
    unsafe {
        let zero = 0;
        core::arch::asm!( // setting bcr0
            "mcr p14, 0, {0}, c0, c0, 5",
            in(reg) zero,
            options(nomem, nostack)
        );

        prefetch_flush();
    }
}

fn was_breakpoint_fault() -> bool {
    unsafe {
        let ifsr: u32;
        core::arch::asm!(
            "mrc p15, 0, {0}, c5, c0, 1",
            out(reg) ifsr,
            options(nomem, nostack)
        );

        let dscr: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) dscr, 
            options(nomem, nostack)
        );
        return (bit_get(ifsr, 10) == 0) && (bits_get(ifsr, 0, 3) == 0b0010) && (bits_get(dscr, 2, 5) == 0b0001);
    }
}

pub fn test_profiler() {

}
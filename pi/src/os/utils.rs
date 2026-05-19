use crate::arch::{prefetch_flush};

pub unsafe fn enable_fiq_interrupts() {
    let mut cpsr: u32;

    core::arch::asm!(
        "mrs {0}, cpsr",
        "bic {0}, {0}, #(1 << 8)",
        "msr cpsr_c, {0}",
        lateout(reg) cpsr,
        options(nomem, nostack)
    );
}

pub unsafe fn disable_fiq_interrupts() {
    let mut cpsr: u32;
    core::arch::asm!(
        "mrs {0}, cpsr",
        "orr {0}, {0}, #(1 << 8)",
        "msr cpsr_c, {0}",
        lateout(reg) cpsr,
        options(nomem, nostack)
    );
}

unsafe fn get_sys_control() -> u32 {
    let mut r: u32;
    core::arch::asm!(
        "mrc p15, 0, {reg}, c1, c0, 0",
        reg = out(reg) r,
        options(nostack, nomem)
    );
    return r;
}

unsafe fn set_sys_control(val: u32) {
    core::arch::asm!(
        "mcr p15, 0, {reg}, c1, c0, 0",
        reg = in(reg) val,
        options(nostack, nomem)
    );
    prefetch_flush();
}

pub unsafe fn get_sys_aux_control() -> u32 {
    let mut r: u32;
    core::arch::asm!(
        "mrc p15, 0, {reg}, c1, c0, 1",
        reg = out(reg) r,
        options(nostack, nomem)
    );
    return r;
}

pub unsafe fn set_sys_aux_control(val: u32) {
    core::arch::asm!(
        "mcr p15, 0, {reg}, c1, c0, 1",
        reg = in(reg) val,
        options(nostack, nomem)
    );
    prefetch_flush();
}

pub fn enable_dcache() {
    unsafe {
        set_sys_control(
            get_sys_control()
                | (1 << 2)
        );
    }
}

pub fn disable_dcache() {
    unsafe { 
        set_sys_control(
            get_sys_control() 
            & !(1 << 12)
        );
    }
}

pub fn enable_l1_instruction_cache(){
    unsafe {
        set_sys_control(
            get_sys_control()
            | (1 << 12)
        );
    }
}

pub fn enable_branch_prediction() {
    unsafe {
        set_sys_control(
            get_sys_control()
            | (1 << 11)
        );
        
    }
}

pub fn disable_l1_instruction_cache(){
    unsafe {
        set_sys_control(
            get_sys_control()
            & !(1 << 12)
        );
    }
}

pub fn disable_branch_prediction() {
    unsafe {
        set_sys_control(
            get_sys_control()
            & !(1 << 11)
        );
    }
}

pub fn is_branch_prediction_enabled() -> bool {
    unsafe {
        return (get_sys_control() & (1 << 11)) != 0;
    }
}
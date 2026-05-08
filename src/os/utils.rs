use crate::arch::{prefetch_flush};

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
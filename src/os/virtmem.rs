use crate::println;
use crate::arch::{cpsr_int_enable, cpsr_int_disable, prefetch_flush};
use core::arch::asm;

#[allow(non_camel_case_types)]
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemPerm {
    perm_rw_user = 0b011, // read-write user 
    perm_ro_user = 0b010, // read-only user
    perm_na_user = 0b001, // no access user

    // kernel only, user no access
    perm_ro_priv = 0b101,

    // perm_rw_priv = perm_na_user,
    // perm_rw_priv = 0b001,
    // both of these are going to be written by MemPerm

    perm_na_priv = 0b000,
}
impl MemPerm {
    #[allow(non_upper_case_globals)]
    pub const perm_rw_priv: Self = Self::perm_na_user;
}

macro_rules! TEX_C_B {
    ($tex:expr, $c:expr, $b:expr) => {
        (($tex) << 2 | ($c) << 1 | ($b))
    };
}

#[allow(non_camel_case_types)]
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemAttr {
    //                              TEX   C  B 
    // strongly ordered
    // not shared.
    MEM_device     =  TEX_C_B!(    0b000,  0, 0),  
    // normal, non-cached
    MEM_uncached   =  TEX_C_B!(    0b001,  0, 0),  

    // write back no alloc
    MEM_wb_noalloc =  TEX_C_B!(    0b000,  1, 1),  
    // write through no alloc
    MEM_wt_noalloc =  TEX_C_B!(    0b000,  1, 0),  
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    G: u32, // is this a global entry?
    asid: u32,
    dom: u32, // domain id
    pagesize: u32, // 1MB or 16 MB

    AP_perm: MemPerm,
    mem_attr: MemAttr
}


pub fn make_global_pin(dom: u32, ap_perm: MemPerm, mem_attr: MemAttr) -> Pin {
    Pin {
        G: 1,
        asid: 0,
        dom,
        pagesize: 0,
        AP_perm: ap_perm,
        mem_attr,
    }
}

pub fn make_user_pin(dom: u32, asid: u32, ap_perm: MemPerm, mem_attr: MemAttr) -> Pin {
    Pin {
        G: 0,
        asid,
        dom,
        pagesize: 0,
        AP_perm: ap_perm,
        mem_attr,
    }
}

pub fn mmu_reset() {
    unsafe {
        asm!(
            // disable interrupts
            "mrs {saved_cpsr_disable}, cpsr",
            "orr {masked_cpsr_disable}, {saved_cpsr_disable}, #(1 << 7)",
            "msr cpsr_c, {masked_cpsr_disable}",
            // DSB a zero register
            "mcr p15, 0, {zero}, c7, c10, 4",
            // invalidate TLB
            "mcr p15, 0, {zero}, c8, c7, 0",
            // invalidate icache caches
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "nop", "nop", "nop", "nop", "nop", "nop",
             "nop", "nop", "nop", "nop", "nop", "nop",
            // invalidate dcache
            "mcr p15, 0, {zero}, c7, c6, 0",
            // flush btb
            "mcr p15, 0, {zero}, c7, c5, 6",
            // DSB
            "mcr p15, 0, {zero}, c7, c10, 4",
            // prefetch flush
            "mcr p15, 0, {zero}, c7, c5, 4",
            // re-enable interrupts
            "mrs {saved_cpsr_enable}, cpsr",
            "bic {masked_cpsr_enable}, {saved_cpsr_enable}, #(1<<7)",
            "msr cpsr_c, {masked_cpsr_enable}",
            saved_cpsr_disable = lateout(reg) _,
            masked_cpsr_disable = lateout(reg) _,
            saved_cpsr_enable = lateout(reg) _,
            masked_cpsr_enable = lateout(reg) _,
            zero = in(reg) 0u32,
            options(nomem, nostack)
        );
    }
}

pub fn mmu_enable() {
    unsafe {
        asm!(
            // disable and invalidate the instruction cache for the corresponding worlds
            // read control register
            "mrc p15, 0, {ctrl_reg1_disable}, c1, c0, 0",
            // disable the instruction cache for the world
            "bic {masked_ctrl_reg1_disable}, {ctrl_reg1_disable}, #(1 << 12)",
            // write this back
            "mcr p15, 0, {masked_ctrl_reg1_disable}, c1, c0, 0", 
            
            // flush btb
            "mcr p15, 0, {zero}, c7, c5, 6",

            // prefetch flush
            "mcr p15, 0, {zero}, c7, c5, 4",

            // clean invalidate the dcache
            "mcr p15, 0, {zero}, c7, c14, 0",

            // dsb
            "mcr p15, 0, {zero}, c7, c10, 4",

            // invalidate icache
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "nop", "nop", "nop", "nop", "nop", "nop",
             "nop", "nop", "nop", "nop", "nop", "nop",

            // invalidate dcache
            "mcr p15, 0, {zero}, c7, c6, 0",

            // invalidate tlb
            "mcr p15, 0, {zero}, c8, c7, 0",

            // set bit 0 to enable the mmu
            "mrc p15, 0, {ctrl_reg1_enable_mmu}, c1, c0, 0",
            "orr {masked_ctrl_reg1_enable_mmu}, {ctrl_reg1_enable_mmu}, #1",
            "mcr p15, 0, {masked_ctrl_reg1_enable_mmu}, c1, c0, 0",

            // flush btb
            "mcr p15, 0, {zero}, c7, c5, 6",

            // dsb
            "mcr p15, 0, {zero}, c7, c10, 4",
             
            // prefetch flush,
            "mcr p15, 0, {zero}, c7, c5, 4",

            zero = in(reg) 0u32,
            ctrl_reg1_disable = lateout(reg) _,
            masked_ctrl_reg1_disable = lateout(reg) _, 
            ctrl_reg1_enable_mmu = lateout(reg) _,
            masked_ctrl_reg1_enable_mmu = lateout(reg) _,
            options(nomem, nostack)
        )
    }
}

pub fn mmu_disable() {
    unsafe {
        asm!(
            // clear-invalidate the dcache
            "mcr p15, 0, {zero}, c7, c14, 0",
            // DSB
            "mcr p15, 0, {zero}, c7, c10, 4",
            // invalidate icache
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "nop", "nop", "nop", "nop", "nop", "nop",
            "nop", "nop", "nop", "nop", "nop", "nop",
            // invalidate dcache
            "mcr p15, 0, {zero}, c7, c6, 0",
            // prefetch flush
            "mcr p15, 0, {zero}, c7, c5, 4",

            // clear bit 0 to disable the mmu
            "mrc p15, 0, {ctrl_reg1_disable_mmu}, c1, c0, 0",
            "bic {masked_ctrl_reg1_disable_mmu}, {ctrl_reg1_disable_mmu}, #1",
            "mcr p15, 0, {masked_ctrl_reg1_disable_mmu}, c1, c0, 0",

            // invalidate the tlb
            "mcr p15, 0, {zero}, c8, c7, 0",
            // flush btb
            "mcr p15, 0, {zero}, c7, c5, 6",
            // prefetch flush
            "mcr p15, 0, {zero}, c7, c5, 4",

            zero = in(reg) 0u32,
            ctrl_reg1_disable_mmu = lateout(reg) _,
            masked_ctrl_reg1_disable_mmu = lateout(reg) _,
            options(nomem, nostack)

        )
    }
}

pub fn pin_mmu_sec(idx: u32, va: u32, pa: u32, e: Pin) {
    println!("about to map {} -> {}", va, pa);
    cpsr_int_disable();

    let mut va_ent: u32 = (va & 0xFFFFF000) | ((e.G & 1) << 9);
    if (e.G == 0) {
        va_ent |= e.asid & 0xFF;
    }

    let mut pa_ent: u32 = pa | ((e.AP_perm as u32) << 1) | (e.pagesize << 6) | 1;
    let mut attr: u32 = (e.dom << 7) | ((e.mem_attr as u32) << 1);

    unsafe {
        asm!(
            "mcr p15, 5, {}, c15, c4, 2",
            in(reg) idx
        ); // lockdown_index_set 

        asm!(
            "mcr p15, 5, {}, c15, c5, 2",
            in(reg) va_ent
        ); // lockdown_va_ent

        asm!(
            "mcr p15, 5, {}, c15, c7, 2",
            in(reg) attr
        ); // lockdown attr_set

        asm!(
            "mcr p15, 5, {}, c15, c6, 2",
            in(reg) pa_ent
        ); // lockdown_pa_set

        prefetch_flush();
    }
    cpsr_int_enable();
}

pub fn mmu_is_enabled() -> bool {
    let ctrl: u32;
    unsafe {
        asm!(
            "mrc p15, 0, {ctrl}, c1, c0, 0",
            ctrl = out(reg) ctrl,
            options(nomem, nostack)
        );
    }
    (ctrl & 1) != 0
}

pub fn set_domain_access(mask: u32) {
    unsafe {
        asm!(
            "mcr p15, 0, {mask}, c3, c0, 0",
            mask = in(reg) mask,
            options(nomem, nostack)
        );
    }
}

pub fn pin_mmu_init(domain_mask: u32) {
    mmu_reset();
    set_domain_access(domain_mask);
}

pub fn pin_mmu_switch(pid: u32, asid: u32) {
    let ctx = (pid << 8) | (asid & 0xff);
    unsafe {
        asm!(
            "mcr p15, 0, {ctx}, c13, c0, 1",
            ctx = in(reg) ctx,
            options(nomem, nostack)
        );
    }
}
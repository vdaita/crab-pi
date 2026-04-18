use crate::{kmalloc::{kmalloc, kmalloc_aligned}, bit_utils::{bit_get, bits_get}};
use crate::println;
use crate::print;
use crate::arch::{
    prefetch_flush, cpsr_int_disable, cpsr_int_enable, clean_inv_dcache, dsb, inv_tlb, inv_icache,
    inv_all_caches, prefetch_fetch, asid_set, ttrb0_set, ttrb1_set, ttbr_base_ctrl_wr,
    flush_btb, control_reg1_rd, control_reg1_wr,
};



#[derive(Debug, Clone, Copy)]
pub struct Cp15CtrlReg1 {
    pub value: u32,
}

macro_rules! bit_field {
    ($name:ident, $bit:expr) => {
        pub fn $name(&self) -> bool { (self.value & (1 << $bit)) != 0 }
        paste::paste! {
            pub fn [<set_ $name>](&mut self, enabled: bool) { self.set_bit($bit, enabled); }
        }
    };
    ($name:ident, $lo:expr, $width:expr) => {
        pub fn $name(&self) -> u32 { self.bits($lo, $width) }
        paste::paste! {
            pub fn [<set_ $name>](&mut self, value: u32) { self.set_bits($lo, $width, value); }
        }
    };
}

impl Cp15CtrlReg1 {
    bit_field!(mmu_enabled, 0);
    bit_field!(alignment_check, 1);
    bit_field!(cache_unified, 2);
    bit_field!(write_buffer, 3);
    bit_field!(unused1, 4, 3);
    bit_field!(endian, 7);
    bit_field!(s_prot, 8);
    bit_field!(r_rom_prot, 9);
    bit_field!(f, 10);
    bit_field!(branch_pred, 11);
    bit_field!(icache_enabled, 12);
    bit_field!(high_exception_vector, 13);
    bit_field!(rr_cache_rep, 14);
    bit_field!(l4, 15);
    bit_field!(dt, 16);
    bit_field!(sbz0, 17);
    bit_field!(it, 18);
    bit_field!(sbz1, 19);
    bit_field!(st, 20);
    bit_field!(f1, 21);
    bit_field!(unaligned, 22);
    bit_field!(xp_pt, 23);
    bit_field!(vect_int, 24);
    bit_field!(ee, 25);
    bit_field!(l2_enabled, 26);
    bit_field!(reserved0, 27);
    bit_field!(tex_remap, 28);
    bit_field!(force_ap, 29);
    bit_field!(reserved1, 30, 2);
    
    fn set_bit(&mut self, bit: u32, enabled: bool) {
        if enabled {
            self.value |= 1 << bit;
        } else {
            self.value &= !(1 << bit);
        }
    }

    fn bits(&self, lo: u32, width: u32) -> u32 {
        (self.value >> lo) & ((1u32 << width) - 1)
    }

    fn set_bits(&mut self, lo: u32, width: u32, v: u32) {
        let mask = ((1u32 << width) - 1) << lo;
        self.value = (self.value & !mask) | ((v << lo) & mask);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FirstLevelDescriptor {
    pub value: u32,
}

impl FirstLevelDescriptor {
    pub fn new() -> Self {
        Self { value: 0 }
    }

    bit_field!(tag, 0, 2);
    bit_field!(b, 2);
    bit_field!(c, 3);
    bit_field!(xn, 4);
    bit_field!(domain, 5, 4);
    bit_field!(imp, 9);
    bit_field!(ap, 10, 2);
    bit_field!(tex, 12, 3);
    bit_field!(apx, 15);
    bit_field!(s, 16);
    bit_field!(ng, 17);
    bit_field!(supersection, 18);
    bit_field!(sec_base_addr, 20, 12);

    fn set_bit(&mut self, bit: u32, enabled: bool) {
        if enabled {
            self.value |= 1 << bit;
        } else {
            self.value &= !(1 << bit);
        }
    }

    fn bits(&self, lo: u32, width: u32) -> u32 {
        (self.value >> lo) & ((1u32 << width) - 1)
    }

    fn set_bits(&mut self, lo: u32, width: u32, v: u32) {
        let mask = ((1u32 << width) - 1) << lo;
        self.value = (self.value & !mask) | ((v << lo) & mask);
    }

    pub fn section_base_pa(&self) -> u32 {
        self.sec_base_addr() << 20
    }

    pub fn set_section_base_pa(&mut self, pa: u32) {
        self.set_sec_base_addr(pa >> 20);
    }
}

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

pub const DOM_CLIENT: u32 = 0b01;

pub fn pin_mk_global(dom: u32, ap_perm: MemPerm, mem_attr: MemAttr) -> Pin {
    Pin {
        G: 1,
        asid: 0,
        dom,
        pagesize: 0,
        AP_perm: ap_perm,
        mem_attr,
    }
}

pub fn pin_mk_user(dom: u32, asid: u32, ap_perm: MemPerm, mem_attr: MemAttr) -> Pin {
    Pin {
        G: 0,
        asid,
        dom,
        pagesize: 0,
        AP_perm: ap_perm,
        mem_attr,
    }
}

pub fn pin_16mb(mut p: Pin) -> Pin {
    p.pagesize = 1;
    p
}

pub fn pin_mk_device(dom: u32) -> Pin {
    pin_mk_global(dom, MemPerm::perm_rw_priv, MemAttr::MEM_device)
}

pub fn mmu_reset() {
    let mut saved_cpsr: u32;
    unsafe {
        core::arch::asm!(
            "mrs {saved_cpsr}, cpsr",
            "orr {irq_masked_cpsr}, {saved_cpsr}, #(1 << 7)",
            "msr cpsr_c, {irq_masked_cpsr}",

            // DSB + full TLB invalidate.
            "mcr p15, 0, {zero}, c7, c10, 4",
            "mcr p15, 0, {zero}, c8, c7, 0",

            // Invalidate D-cache, then apply ARM1176 I-cache workaround.
            "mcr p15, 0, {zero}, c7, c6, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
            "mcr p15, 0, {zero}, c7, c5, 0",
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

            "mcr p15, 0, {zero}, c7, c5, 6", // flush BTB
            "mcr p15, 0, {zero}, c7, c10, 4", // DSB
            "mcr p15, 0, {zero}, c7, c5, 4", // prefetch flush
            "msr cpsr_c, {saved_cpsr}",
            saved_cpsr = lateout(reg) saved_cpsr,
            irq_masked_cpsr = lateout(reg) _,
            zero = in(reg) 0u32,
            options(nostack)
        );
    }
}

static mut null_pt: *mut u8 = core::ptr::null_mut();

pub fn cp15_ctrl_reg1_rd() -> Cp15CtrlReg1{
    let value: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {value}, c1, c0, 0",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    Cp15CtrlReg1 { value }
}

pub fn cp15_ctrl_reg1_wr(reg: Cp15CtrlReg1) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {value}, c1, c0, 0",
            "mcr p15, 0, {zero}, c7, c5, 4",
            value = in(reg) reg.value,
            zero = in(reg) 0u32,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn mmu_init() {
    mmu_reset();

    let mut my_c1: Cp15CtrlReg1 = cp15_ctrl_reg1_rd();
    my_c1.set_xp_pt(true);
    cp15_ctrl_reg1_wr(my_c1);

    let check_c1: Cp15CtrlReg1 = cp15_ctrl_reg1_rd();
    assert!(check_c1.xp_pt());
    assert!(!check_c1.mmu_enabled());
}

pub fn cp15_set_procid_ttbr0(proc_and_asid: u32, ptr: *mut u8) {
    let ttbr0: u32 = ptr as u32;
    clean_inv_dcache();
    dsb();
    inv_tlb();
    inv_icache();
    prefetch_fetch();

    asid_set(0);
    prefetch_fetch();

    ttrb0_set(ttbr0);
    ttrb1_set(0);
    ttbr_base_ctrl_wr(0);

    flush_btb();
    prefetch_fetch();

    asid_set(proc_and_asid);
}

pub fn mmu_set_ctx(pid: u32, asid: u32, ptr: *mut u8) {
    assert!(asid != 0);
    assert!(asid < 64);
    cp15_set_procid_ttbr0(pid << 8 | asid, ptr);
}

pub fn domain_access_ctrl_get() -> u32 {
    let value: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {value}, c3, c0, 0",
            value = out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

pub fn domain_access_ctrl_set(domain_reg: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {domain_reg}, c3, c0, 0",
            "mcr p15, 0, {zero}, c7, c5, 4",
            domain_reg = in(reg) domain_reg,
            zero = in(reg) 0u32,
            options(nomem, nostack, preserves_flags)
        );
    }
}

pub fn pin_mmu_init(domain_reg: u32) {
    mmu_init();
    unsafe { null_pt = kmalloc_aligned(4096 * 4, 1 << 14); }
    domain_access_ctrl_set(domain_reg);
}

pub fn mmu_enable() {
    let mut c: Cp15CtrlReg1 = cp15_ctrl_reg1_rd();
    assert!(!c.mmu_enabled());
    c.set_mmu_enabled(true);
    mmu_enable_set_asm(c);
    assert!(cp15_ctrl_reg1_rd().mmu_enabled());
}

pub fn mmu_disable() {
    let mut c: Cp15CtrlReg1 = cp15_ctrl_reg1_rd();
    assert!(c.mmu_enabled());
    c.set_mmu_enabled(false);
    mmu_disable_set_asm(c);
    assert!(!cp15_ctrl_reg1_rd().mmu_enabled());
}

pub fn mmu_disable_set_asm(c: Cp15CtrlReg1) {
    let saved_cpsr: u32;
    unsafe {
        core::arch::asm!(
            "mrs {saved_cpsr}, cpsr",
            "orr {irq_masked}, {saved_cpsr}, #(1 << 7)",
            "msr cpsr_c, {irq_masked}",

            "mcr p15, 0, {zero}, c7, c14, 0", // CLEAN_INV_DCACHE
            "mcr p15, 0, {zero}, c7, c10, 4", // DSB

            "mcr p15, 0, {zero}, c7, c6, 0", // INV_ALL_CACHES (D)
            "mcr p15, 0, {zero}, c7, c5, 0", // INV_ALL_CACHES (I)
            "mcr p15, 0, {zero}, c7, c5, 4", // PREFETCH_FLUSH

            "mcr p15, 0, {new_c1}, c1, c0, 0", // CONTROL_REG1_WR
            "mcr p15, 0, {zero}, c8, c7, 0",   // INV_TLB
            "mcr p15, 0, {zero}, c7, c5, 6",   // FLUSH_BTB
            "mcr p15, 0, {zero}, c7, c10, 4",  // DSB
            "mcr p15, 0, {zero}, c7, c5, 4",   // PREFETCH_FLUSH

            "msr cpsr_c, {saved_cpsr}",
            saved_cpsr = lateout(reg) saved_cpsr,
            irq_masked = lateout(reg) _,
            new_c1 = in(reg) c.value,
            zero = in(reg) 0u32,
            options(nostack, preserves_flags)
        );
    }
}

pub fn mmu_enable_set_asm(c: Cp15CtrlReg1) {
    println!("In mmu_enable_set_asm.");
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    unsafe {
        core::arch::asm!(
            "mov r2, #0",
            "mrc p15, 0, r3, c1, c0, 0",
            "bic r3, r3, #(1 << 12)",
            "mcr p15, 0, r3, c1, c0, 0",
            "mcr p15, 0, r2, c7, c5, 6",
            "mcr p15, 0, r2, c7, c5, 4",
            "mcr p15, 0, r2, c7, c14, 0",
            "mcr p15, 0, r2, c7, c10, 4",
            "mcr p15, 0, r2, c7, c6, 0",
            "mcr p15, 0, r2, c7, c5, 0",
            "mcr p15, 0, r2, c8, c7, 0",
            "mcr p15, 0, r0, c1, c0, 0",
            "mcr p15, 0, r2, c7, c5, 6",
            "mcr p15, 0, r2, c7, c10, 4",
            "mcr p15, 0, r2, c7, c5, 4",
            in("r0") c.value,
            lateout("r2") _,
            lateout("r3") _,
            options(nostack)
        );
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    println!("just returned from enable_set_asm!");
}

pub fn pin_mmu_enable() {
    assert!(!mmu_is_enabled());
    println!("MMU is not enabled already!");
    mmu_enable();
    assert!(mmu_is_enabled());
}

pub fn pin_mmu_disable() {
    assert!(mmu_is_enabled());
    println!("MMU is not disabled already!");
    mmu_disable();
    assert!(!mmu_is_enabled());
}

pub fn va_to_pa_set(val: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {val}, c7, c8, 0",
            val = in(reg) val,
            options(nomem, nostack, preserves_flags)
        );
    }
} 

pub fn va_to_pa_result_get() -> u32 {
    let result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {}, c7, c4, 0",
            out(reg) result,
            options(nomem, nostack, preserves_flags)
        );
    }
    result
}

pub fn lockdown_index_get() -> u32 {
    let result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 5, {}, c15, c4, 2",
            out(reg) result
        );
    }
    result
}

pub fn lockdown_index_set(value: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 5, {}, c15, c4, 2",
            in(reg) value
        );
    }
}

pub fn lockdown_pa_get() -> u32 {
    let result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 5, {}, c15, c6, 2",
            out(reg) result
        );
    }
    result
}

pub fn lockdown_pa_set(value: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 5, {}, c15, c6, 2",
            in(reg) value
        );
    }
}

pub fn lockdown_va_get() -> u32 {
    let result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 5, {}, c15, c5, 2",
            out(reg) result
        );
    }
    result
}

pub fn lockdown_va_set(value: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 5, {}, c15, c5, 2",
            in(reg) value
        );
    }
}

pub fn lockdown_attr_get() -> u32 {
    let result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 5, {}, c15, c7, 2",
            out(reg) result
        );
    }
    result
}

pub fn lockdown_attr_set(value: u32) {
    unsafe {
        core::arch::asm!(
            "mcr p15, 5, {}, c15, c7, 2",
            in(reg) value
        );
    }
}

pub fn mmu_is_enabled() -> bool {
    return cp15_ctrl_reg1_rd().mmu_enabled();
}

pub fn tlb_contains_va(va: u32) -> (u32, bool) {
    assert!(mmu_is_enabled());
    assert!(bits_get(va, 0, 2) == 0);
    va_to_pa_set(va);

    let translation = va_to_pa_result_get();
    let translation_aborted: bool = (translation & 1) == 1;
    if translation_aborted {
        return (translation, !translation_aborted);
    } else {
        return ((translation & !0x3FF) | (va & 0x3FF), !translation_aborted);
    }
}

pub fn pin_mmu_sec(idx: u32, va: u32, pa: u32, e: Pin) {
    println!("about to map {} -> {}", va, pa);
    cpsr_int_disable();

    let x: u32 = idx;
    let mut va_ent: u32 = (va & 0xFFFFF000) | ((e.G & 1) << 9);
    if (e.G == 0) {
        va_ent |= e.asid & 0xFF;
    }

    let mut pa_ent: u32 = pa | ((e.AP_perm as u32) << 1) | (e.pagesize << 6) | 1;
    let mut attr: u32 = (e.dom << 7) | ((e.mem_attr as u32) << 1);

    lockdown_index_set(x);
    lockdown_va_set(va_ent);
    lockdown_attr_set(attr);
    lockdown_pa_set(pa_ent);
    unsafe { prefetch_flush(); }
    cpsr_int_enable();
}

pub fn pin_exists(va: u32, verbose_p: bool) -> bool {
    if(!mmu_is_enabled()) {
        panic!("We can only check if it's pinned if the MMU is enabled.");
    }

    let (r, translated) = tlb_contains_va(va);
    if translated {
        assert!(va == r);
        return true;
    } else {
        if verbose_p {
            println!("TLB should have {:0x}, returned {:0x} [reason={:0b}]", va, r, bits_get(r, 1, 6));
        }
        return false;
    }
}

pub fn pin_set_context(asid: u32) {
    
    if (!(asid > 0 && asid < 64)) {
        panic!("invalid asid value");
    }
    
    unsafe {
        if(null_pt == core::ptr::null_mut()) {
            panic!("must set up null ptr");
        }
        mmu_set_ctx(0, asid, null_pt);
    }
}

pub fn pin_clear(idx: u32) {
    cpsr_int_disable();

    unsafe {
        core::arch::asm!("mcr p15, 5, {0}, c15, c4, 2", in(reg) idx);
        core::arch::asm!("mcr p15, 5, {0}, c15, c5, 2", in(reg) 0u32);
        core::arch::asm!("mcr p15, 5, {0}, c15, c7, 2", in(reg) 0u32);
        core::arch::asm!("mcr p15, 5, {0}, c15, c6, 2", in(reg) 0u32);
    }

    let va = lockdown_va_get();
    let pa = lockdown_pa_get();
    let attr = lockdown_attr_get();

    if (va != 0) {
        panic!("lockdown va on clear: expected {:0x}, have {:0x}", 0, va);
    }
    if (pa != 0) {
        panic!("lockdown pa on clear: expected {:0x}, have {:0x}", 0, pa);
    }
    if (attr != 0) {
        panic!("lockdown attr on clear: expected {:0x}, have {:0x}", 0, attr);
    }

    unsafe { prefetch_flush(); }
    cpsr_int_enable();
}


pub fn lockdown_print_entry(idx: u32) {
    println!("   idx={}", idx);
    lockdown_index_set(idx);

    let va_ent = lockdown_va_get();
    let pa_ent = lockdown_pa_get();
    let attr = lockdown_attr_get();
    let v = bit_get(pa_ent, 0);

    if v == 0 {
        println!("     [invalid entry {}]", idx);
        return;
    }

    let asid = bits_get(va_ent, 0, 7);
    let g = bit_get(va_ent, 9);
    let va = bits_get(va_ent, 12, 31);
    println!("     va_ent={:x}: va={:x}|G={}|ASID={}", va_ent, va, g, asid);

    let apx = bits_get(pa_ent, 1, 3);
    let size = bits_get(pa_ent, 6, 7);
    let nstid = bit_get(pa_ent, 8);
    let nsa = bit_get(pa_ent, 9);
    let pa = bits_get(pa_ent, 12, 31);
    println!(
        "     pa_ent={:x}: pa={:x}|nsa={}|nstid={}|size={:b}|apx={:b}|v={}",
        pa_ent, pa, nsa, nstid, size, apx, v
    );

    let b = bit_get(attr, 1);
    let c = bit_get(attr, 2);
    let tex = bits_get(attr, 3, 5);
    let xn = bit_get(attr, 6);
    let dom = bits_get(attr, 7, 10);
    println!(
        "     attr={:x}: dom={}|xn={}|tex={:b}|C={}|B={}",
        attr, dom, xn, tex, c, b
    );
}

pub fn lockdown_print_entries(msg: &str) {
    println!("-----  <{}> ----- ", msg);
    println!("  pinned TLB lockdown entries:");

    for i in 0..8 {
        lockdown_print_entry(i);
    }

    println!("----- ---------------------------------- ");
}


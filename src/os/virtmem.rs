
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
}

impl Cp15CtrlReg1 {
    bit_field!(mmu_enabled, 0);
    bit_field!(alignment_check, 1);
    bit_field!(cache_unified, 2);
    bit_field!(write_buffer, 3);
    bit_field!(endian, 7);
    bit_field!(branch_pred, 11);
    bit_field!(icache_enabled, 12);
    bit_field!(high_exception_vector, 13);
    bit_field!(l2_enabled, 26);
    bit_field!(tex_remap, 28);
    
    fn set_bit(&mut self, bit: u32, enabled: bool) {
        if enabled {
            self.value |= 1 << bit;
        } else {
            self.value &= !(1 << bit);
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemPerm {
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
    pub const perm_rw_priv: Self = Self::perm_na_user;
}

macro_rules! TEX_C_B {
    ($tex:expr, $c:expr, $b:expr) => {
        (($tex) << 2 | ($c) << 1 | ($b))
    };
}

enum MemAttr {
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

struct Pin {
    G: u32, // is this a global entry?
    asid: u32,
    dom: u32, // domain id
    pagesize: u32, // 1MB or 16 MB

    AP_perm: MemPerm,
    mem_attr: MemAttr
}

pub fn mmu_reset() {
    // asm!(
    //     "mrs r0, cpsr",
    //     "orr r0, r0, #(1 << 7)", // set 7th bit
    //     "msr cpsr_c, r0", 
    //     "mov r2, #0",
    //     "DSB(r2)", // need to define this function
    //     "INV_TLB(r2)",
    //     "INV_ALL_CACHES(r2)",
    //     "FLUSH_BTB(r2)",
    //     "DSB(r2)",
    //     "PREFETCH_FLUSH(r2)",
    //     "mrs r0, cpsr",
    //     "bic r0, r0 #(1 << 7)",
    //     "msr cpsr_c, r0",
    //     "bx lr"
    // );
}

pub fn mmu_init() {
    mmu_reset();


}

pub fn domain_access_ctrl_get() -> u32 {
    return 0;
}

pub fn pin_mmu_init(domain_reg: u32) {

}

pub fn tlb_contains_va(result: *const u32, va: u32) {

}

pub fn pin_mmu_sec(idx: u32, va: u32, pa: u32, e: Pin) {

}

pub fn pin_exists(va: u32, verbose_p: bool) {

}

pub fn pin_set_context(asid: u32) {

}

pub fn pin_clear(idx: u32) {

}

pub fn lockdown_print_entry(idx: u32) {
    
}
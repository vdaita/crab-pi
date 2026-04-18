use crate::mem::{get32, put32};
use crate::os::virtmem::{
    lockdown_print_entries, mmu_is_enabled, pin_16mb, pin_exists, pin_mk_device, pin_mk_global,
    pin_mk_user, pin_mmu_disable, pin_mmu_enable, pin_mmu_init, pin_mmu_sec, pin_set_context,
    MemPerm, MemAttr, DOM_CLIENT,
};
use crate::println;

const MB: u32 = 1024 * 1024;
const DOM_KERN: u32 = 1;

const SEG_CODE: u32 = 0x0000_0000;
const SEG_HEAP: u32 = 0x0010_0000;
const SEG_STACK: u32 = 0x07F0_0000;
const SEG_INT_STACK: u32 = 0x0780_0000;
const SEG_BCM_0: u32 = 0x2000_0000;

pub fn vm_test() {
    assert!(!mmu_is_enabled());

    let dom_bits = DOM_CLIENT << (DOM_KERN * 2);
    pin_mmu_init(dom_bits);

    let mut idx: u32 = 0;
    let kern = pin_mk_global(DOM_KERN, MemPerm::perm_rw_priv, MemAttr::MEM_uncached);
    pin_mmu_sec(idx, SEG_CODE, SEG_CODE, kern);
    idx += 1;
    pin_mmu_sec(idx, SEG_HEAP, SEG_HEAP, kern);
    idx += 1;
    pin_mmu_sec(idx, SEG_STACK, SEG_STACK, kern);
    idx += 1;
    pin_mmu_sec(idx, SEG_INT_STACK, SEG_INT_STACK, kern);
    idx += 1;

    let dev = pin_16mb(pin_mk_device(DOM_KERN));
    pin_mmu_sec(idx, SEG_BCM_0, SEG_BCM_0, dev);
    idx += 1;

    const ASID1: u32 = 1;
    const ASID2: u32 = 2;

    const USER_ADDR: u32 = 16 * MB;
    const PHYS_ADDR1: u32 = USER_ADDR + MB;
    const PHYS_ADDR2: u32 = USER_ADDR + 2 * MB;

    let user1 = pin_mk_user(DOM_KERN, ASID1, MemPerm::perm_na_user, MemAttr::MEM_uncached);
    let user2 = pin_mk_user(DOM_KERN, ASID2, MemPerm::perm_na_user, MemAttr::MEM_uncached);
    pin_mmu_sec(idx, USER_ADDR, PHYS_ADDR1, user1);
    idx += 1;
    pin_mmu_sec(idx, USER_ADDR, PHYS_ADDR2, user2);
    idx += 1;

    put32(PHYS_ADDR1, 0x1111_1111);
    put32(PHYS_ADDR2, 0x2222_2222);

    assert!(idx < 8);

    println!("about to enable");
    lockdown_print_entries("about to turn on first time");

    pin_set_context(ASID1);
    println!("Set context to ASID1");
    pin_mmu_enable();
    println!("MMU is on and working!");

    let mut x = get32(USER_ADDR);
    println!("ASID {} = got: {:x}", ASID1, x);
    assert!(x == 0x1111_1111);
    put32(USER_ADDR, ASID1);

    pin_mmu_disable();
    println!("phys addr1={:x}", get32(PHYS_ADDR1));
    assert!(get32(PHYS_ADDR1) == ASID1);

    pin_set_context(ASID2);
    pin_mmu_enable();
    x = get32(USER_ADDR);
    println!("ASID {} = got: {:x}", ASID2, x);
    assert!(x == 0x2222_2222);
    put32(USER_ADDR, ASID2);
    pin_mmu_disable();

    println!("phys addr2={:x}", get32(PHYS_ADDR2));
    assert!(get32(PHYS_ADDR2) == ASID2);

    println!("about to check that can switch ASID w/ MMU on");
    put32(PHYS_ADDR1, 0x1111_1111);
    put32(PHYS_ADDR2, 0x2222_2222);

    pin_set_context(ASID1);
    pin_mmu_enable();
    assert!(get32(USER_ADDR) == 0x1111_1111);
    put32(USER_ADDR, ASID1);

    pin_set_context(ASID2);
    assert!(get32(USER_ADDR) == 0x2222_2222);
    put32(USER_ADDR, ASID2);

    pin_mmu_disable();

    assert!(get32(PHYS_ADDR1) == ASID1);
    assert!(get32(PHYS_ADDR2) == ASID2);
    assert!(pin_exists(USER_ADDR, true));
    println!("SUCCESS!");
}

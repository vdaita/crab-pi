use crate::kmalloc;
use crate::mem::{get32, put32};
use crate::os::virtmem::{
    make_global_pin, make_user_pin, mmu_disable, mmu_enable, mmu_reset, pin_mmu_sec, MemAttr,
    MemPerm, pin_mmu_switch, pin_mmu_init, set_domain_access, mmu_is_enabled
};
use crate::println;

const ONE_MB: u32 = 1024 * 1024;
const STACK_ADDR: u32 = 0x0800_0000;
const DOM_KERN: u32 = 1;
const ASID1: u32 = 1;

pub fn vm_test() {
    assert!(!mmu_is_enabled());

    // Keep allocator init behavior from the original test harness.
    unsafe { kmalloc::kmalloc_init_mb(1) };

    pin_mmu_init(!0);

    let no_user = MemPerm::perm_rw_priv;

    // Device memory: kernel domain only, strongly ordered.
    let dev = make_global_pin(DOM_KERN, no_user, MemAttr::MEM_device);
    // Kernel memory: kernel domain only, uncached normal memory.
    let kern = make_global_pin(DOM_KERN, no_user, MemAttr::MEM_uncached);

    // Index into the 8 pinned TLB entries.
    let mut idx = 0;

    // Identity-map key device ranges.
    pin_mmu_sec(idx, 0x2000_0000, 0x2000_0000, dev);
    idx += 1;
    pin_mmu_sec(idx, 0x2010_0000, 0x2010_0000, dev);
    idx += 1;
    pin_mmu_sec(idx, 0x2020_0000, 0x2020_0000, dev);
    idx += 1;

    // Map first two MB for kernel code/data.
    pin_mmu_sec(idx, 0, 0, kern);
    idx += 1;
    pin_mmu_sec(idx, ONE_MB, ONE_MB, kern);
    idx += 1;

    // Map kernel stack region.
    pin_mmu_sec(idx, STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);
    idx += 1;

    // Create a single user mapping entry (non-global, ASID-tagged).
    let user1 = make_user_pin(DOM_KERN, ASID1, no_user, MemAttr::MEM_uncached);

    let user_addr = ONE_MB * 16;
    assert_eq!((user_addr >> 12) % 16, 0);
    let phys_addr1 = user_addr;
    put32(phys_addr1, 0xdead_beef);

    pin_mmu_sec(idx, user_addr, phys_addr1, user1);
    idx += 1;
    assert!(idx < 8);

    println!("about to enable");
    pin_mmu_switch(0, ASID1);
    mmu_enable();

    assert!(mmu_is_enabled());
    println!("MMU is on and working!");

    let x = get32(user_addr);
    println!("asid 1 = got: {:#x}", x);
    assert_eq!(x, 0xdead_beef);
    put32(user_addr, 1);

    mmu_disable();
    assert!(!mmu_is_enabled());
    println!("MMU is off!");
    println!("phys addr1={:#x}", get32(phys_addr1));
    assert_eq!(get32(phys_addr1), 1);

    println!("SUCCESS!");
}

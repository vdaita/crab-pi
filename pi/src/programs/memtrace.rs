use crate::os::{interrupts, virtmem};
use crate::kmalloc;
use crate::mem::{get32, put32};
use crate::println;
use crate::os::interrupts::{move_table, INTERRUPT_TABLE_START, INTERRUPT_TABLE_END};
use core::arch::asm;
use crate::gpio;
use crate::ckalloc;
use crate::os;

core::arch::global_asm!(r#"
.globl _memtrace_interrupt_table
.globl _memtrace_interrupt_table_end
_memtrace_interrupt_table:
  ldr pc, _reset_memtrace_asm
  ldr pc, _undefined_instruction_memtrace_asm
  ldr pc, _software_interrupt_memtrace_asm
  ldr pc, _prefetch_abort_memtrace_asm
  ldr pc, _data_abort_memtrace_asm
  ldr pc, _reset_memtrace_asm
  ldr pc, _interrupt_memtrace_asm
  ldr pc, _fast_interrupt_memtrace_asm

_reset_memtrace_asm:                 .word reset_memtrace_asm
_undefined_instruction_memtrace_asm: .word undefined_instruction_memtrace_asm
_software_interrupt_memtrace_asm:    .word software_interrupt_memtrace_asm
_prefetch_abort_memtrace_asm:        .word prefetch_abort_memtrace_asm
_data_abort_memtrace_asm:            .word data_abort_memtrace_asm
_interrupt_memtrace_asm:             .word interrupt_memtrace_asm
_fast_interrupt_memtrace_asm:        .word fast_interrupt_memtrace_asm
_memtrace_interrupt_table_end:

reset_memtrace_asm:
    bx lr
undefined_instruction_memtrace_asm:
    bx lr
software_interrupt_memtrace_asm:
    movs pc, lr
prefetch_abort_memtrace_asm:
    bx lr
interrupt_memtrace_asm:
    bx lr
fast_interrupt_memtrace_asm:
    bx lr

data_abort_memtrace_asm:
    sub   lr, lr, #8
    mov sp, #0x9000000           
    push  {{r0-r12, lr}}

    mov   r0, sp                  
    mov   r1, lr

    bl    data_abort_handler_memtrace

    pop   {{r0-r12, lr}}
    movs  pc, lr
"#);

unsafe extern "C" {
    #[link_name = "_memtrace_interrupt_table"]
    static MEMTRACE_TABLE_START: u8;
    #[link_name = "_memtrace_interrupt_table_end"]
    static MEMTRACE_TABLE_END: u8;
}
static mut TRACE_HANDLER_FN: Option<fn(u32, u32)> = None;
pub fn trace_handler_ckalloc(lr: u32, address: u32) {
    println!("Trace handler ckalloc: lr={:#x}, address={:#x}", lr, address);
    let header: *const ckalloc::CheckHeader = ckalloc::ck_ptr_is_alloced(address as *const u32);
    if (header.is_null()) {
        println!("Error: address {:#x} is not alloced.", address);
    } else {
        println!("Success: address {:#x} is alloced,", address);
    }
}

pub fn trace_handler_0(lr: u32, address: u32) {
    println!("Trace Handler 0: lr={:#x}, address={:#x}", lr, address);
}

static mut counter: u32 = 0;
pub fn trace_handler_1(lr: u32, address: u32) {
    unsafe { counter = counter + 1; }
    println!("Trace Handler 1: lr={:#x}, address={:#x}", lr, address);
}

#[unsafe(no_mangle)]
pub extern "C" fn data_abort_handler_memtrace(regs: *mut u32, lr: u32) {
    // println!("Data abort handler called: lr={:#x}", lr);

    unsafe {
        core::arch::asm!(
            "mov r0, #(1 << 27)",
            "ldr r1, =0x2020001C",
            "str r0, [r1]",
            out("r0") _,
            out("r1") _,
            options(nostack)
        );
        crate::arch::dsb();

        if (virtmem::mmu_is_enabled()) {
            let mut dscr: u32;
            core::arch::asm!("mrc p14, 0, {}, c0, c1, 0", out(reg) dscr);
            core::arch::asm!("mcr p14, 0, {}, c0, c1, 0", in(reg) dscr | (1 << 15));


            // this was from the invalid memory access
            let caught_memory_address: u32;
            core::arch::asm!("mrc p15, 0, {}, c6, c0, 0", out(reg) caught_memory_address);

            // enable watchpoint on that address
            core::arch::asm!("mcr p14, 0, {}, c0, c0, 6", in(reg) caught_memory_address);

            const watchpoint_control: u32 =
                (1 << 0) |      // enable
                (0b11 << 1) |   // privileged or user
                (0b11 << 3) |   // load or store
                (0b1111 << 5);  // any byte in the word
            core::arch::asm!(
                "mcr p14, 0, {}, c0, c0, 7",
                in(reg) watchpoint_control
            );

            let dfsr: u32;
            core::arch::asm!("mrc p15, 0, {}, c5, c0, 0", out(reg) dfsr);
            let is_store = (dfsr >> 11) & 1;

            println!(
                "Data abort handler called: address: {:#x}, LR: {:#x}, wcr: {:#x}, is store: {}",
                caught_memory_address,
                lr,
                watchpoint_control,
                is_store
            );
            virtmem::mmu_disable();

            // unsafe {
            //     *regs.add(13) = lr.wrapping_sub(4);
            //     println!("register 13 now: {:p}", *regs.add(13) as *const u32);
            // }
        } else { 
            // this was the address getting getting called 
            let watchpoint_access_instr: u32;
            core::arch::asm!("mrc p15, 0, {}, c6, c0, 1", out(reg) watchpoint_access_instr);

            let watchpoint_value_register: u32;
            core::arch::asm!("mrc p14, 0, {}, c0, c0, 6", out(reg) watchpoint_value_register);
            // println!("watchpoint access instruction: {:#x}, watchpoint address: {:#x}, lr: {:#x}", watchpoint_access_instr, watchpoint_value_register, lr);
            
            let watchpoint_control_register: u32;
            core::arch::asm!("mrc p14, 0, {}, c0, c0, 7", out(reg) watchpoint_control_register);
            core::arch::asm!(
                "mcr p14, 0, {}, c0, c0, 7",
                in(reg) (watchpoint_control_register & !(1))
            ); // disable watchpoints

            match TRACE_HANDLER_FN {
                Some(function) => {
                    function(watchpoint_access_instr, watchpoint_value_register);
                },
                None => {

                }
            };

            virtmem::mmu_enable();            
        }
    }
}

#[inline(never)]
pub fn memtrace_init(data: *const u32, trace_handler: fn(u32, u32)) {
    assert!(!virtmem::mmu_is_enabled());
    virtmem::mmu_reset();

    unsafe { kmalloc::kmalloc_init_mb_with_offset(1, 16 * 1024 * 1024); }

    const DOM_KERN: u32 = 1;
    const STACK_ADDR: u32 = 0x0800_0000;
    const SECONDARY_STACK_ADDR: u32 = 0x0900_0000;
    const ONE_MB: u32 = 1024 * 1024;
    const ASID: u32 = 1;


    let mut domain_mask = 0;
    for i in 0..16 {
        domain_mask |= (0b01) << (i * 2);
    }
    virtmem::pin_mmu_init(domain_mask);
    

    let unaccessible = virtmem::MemPerm::perm_na_priv;
    let no_user      = virtmem::MemPerm::perm_rw_priv;
    let dev  = virtmem::make_global_pin(DOM_KERN, no_user,      virtmem::MemAttr::MEM_device, virtmem::PageSizes::mb16);
    let kern = virtmem::make_global_pin(DOM_KERN, no_user,      virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb1);
    let heap = virtmem::make_global_pin(DOM_KERN, unaccessible, virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);

    virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);
    virtmem::pin_mmu_sec(1, 0, 0, kern);
    virtmem::pin_mmu_sec(2, STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);
    virtmem::pin_mmu_sec(3, SECONDARY_STACK_ADDR - ONE_MB, SECONDARY_STACK_ADDR - ONE_MB, kern);
    unsafe {
        virtmem::pin_mmu_sec(4, (kmalloc::HEAP_CURR as u32) & 0xFF000000, (kmalloc::HEAP_CURR as u32) & 0xFF000000, heap);
    }
    virtmem::pin_mmu_switch(0, 0);
    os::utils::disable_dcache();
    virtmem::tlb_invalidate();

    unsafe {
        move_table(
            core::ptr::addr_of!(MEMTRACE_TABLE_START) as usize,
            core::ptr::addr_of!(MEMTRACE_TABLE_END)   as usize,
        );
    }

    unsafe { TRACE_HANDLER_FN = Some(trace_handler); }

    println!("finished memtrace init");
}

#[inline(never)]
pub fn memtrace_trap_enable() {
    println!("virtual memory enabled");
    unsafe {
        interrupts::enable_interrupts_asm();
    }

    println!("ran trap enable");

    let cpsr: u32;
    unsafe {
        core::arch::asm!("mrs {}, cpsr", out(reg) cpsr);
    }
    println!("CPSR before mmu enable: {:#032b}", cpsr);
    println!("A bit (abort mask) = {}", (cpsr >> 8) & 1);
    println!("mode = {:#07b}", cpsr & 0x1f);

    virtmem::mmu_enable();

    unsafe {
        os::utils::enable_fiq_interrupts();
    }
}

#[inline(never)]
pub fn memtrace_trap_disable() {
    virtmem::mmu_disable();
    unsafe { 
        interrupts::disable_interrupts_asm(); 
        os::utils::disable_fiq_interrupts();
    }
    println!("ran trap disable");
}

pub fn test_memtrace_with_ckalloc() {
    unsafe {
        gpio::set_output(27);

        memtrace_init(0 as *const u32, trace_handler_ckalloc);
        println!("Heap address: {:p}", kmalloc::HEAP_CURR as *const u32);

        let space = ckalloc::ckalloc(8) as *mut u32;
        println!("heap ptr = {:p}", space);

        memtrace_trap_enable();
        space.write_volatile(0xdeadbeef);
        let val = space.read_volatile();
        space.write_volatile(space.read_volatile() + 1);
        space.write_volatile(space.read_volatile() + 2);
        space.write_volatile(space.read_volatile() + 3);
        memtrace_trap_disable();

        println!("read back: 0x{:08x}", val);
        println!("first test completed");

        // memtrace_init(0 as *const u32, trace_handler_ckalloc);
        let space2 = ckalloc::ckalloc(4 * 32) as *mut u32;
        memtrace_trap_enable();
        for i in 0..32 {
            *space2.add(i) = i as u32;
        }
        for i in 0..32 {
            println!("verifying: value @ i={} is {}", i, *space2.add(i));
        }
        memtrace_trap_disable();
        println!("second test completed");

        // memtrace_init(0 as *const u32, trace_handler_ckalloc);
        let space3 = ckalloc::ckalloc(4 * 32) as *mut u32;
        memtrace_trap_enable();
        for i in 0..32 {
            *space3.add(i) = i as u32;
        }
        let x = *(space3.sub(4));
        println!("The above should show an invalid access: {}", x);
        memtrace_trap_disable();
        println!("failing test completed");
    }
}

pub fn test_memtrace() {
    unsafe {
        gpio::set_output(27);

        memtrace_init(0 as *const u32, trace_handler_0);
        println!("Heap address: {:p}", kmalloc::HEAP_CURR as *const u32);

        let space = kmalloc::kmalloc(8) as *mut u32;
        println!("heap ptr = {:p}", space);

        memtrace_trap_enable();
        space.write_volatile(0xdeadbeef);
        let val = space.read_volatile();

        space.write_volatile(space.read_volatile() + 1);
        space.write_volatile(space.read_volatile() + 2);
        space.write_volatile(space.read_volatile() + 3);

        memtrace_trap_disable();
        println!("read back: 0x{:08x}", val);
        println!("first test completed");

        memtrace_init(0 as *const u32, trace_handler_1);
        memtrace_trap_enable();
        let space2 = kmalloc::kmalloc(4 * 32) as *mut u32;
        for i in 0..32 {
            *space2.add(i) = i as u32;
        }
        for i in 0..32 {
            println!("verifying: value @ i={} is {}", i, *space2.add(i));
        }
        memtrace_trap_disable();
        println!("second test completed");
    }
}

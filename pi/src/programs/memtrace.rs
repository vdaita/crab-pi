use crate::os::{interrupts, virtmem};
use crate::kmalloc;
use crate::mem::{get32, put32};
use crate::{println, print};
use crate::os::interrupts::{move_table, INTERRUPT_TABLE_START, INTERRUPT_TABLE_END};
use core::arch::asm;
use crate::gpio;
use crate::ckalloc;
use crate::os;
use crate::bit_utils;

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

#[derive(Copy, Clone, PartialEq, Debug)]
enum AddrState {
    Virgin = 1,
    Freed = 2,
    Shared = 3,
    Exclusive = 4,
    ModShared = 5,
    Invalid = 6,
}

#[derive(Copy, Clone, Debug)]
struct Location {
    state: AddrState,
    tid: usize,
    address: u32,
    has_store: bool,
    lockset: [bool; 32]
}

const NUM_LOCKS: usize = 32;

#[derive(Copy, Clone)]
struct ThreadLockset {
    lockset: [bool; NUM_LOCKS]
}

static mut TRACE_HANDLER_FN: Option<fn(u32, u32, bool)> = None;
const NUM_ADDRESSES: usize = 1024;
const NUM_THREADS: usize = 4;
static mut ERASER_CURR_THREAD: usize = 0;

static mut ERASER_ADDR_STATES: [Location; NUM_ADDRESSES] = [Location {
    state: AddrState::Virgin, // hasn't been oughted yet
    tid: -1i32 as usize,
    address: -1i32 as u32, // expectation is that we are not reading / writing -1
    has_store: false,
    lockset: [true; NUM_LOCKS]
}; NUM_ADDRESSES];

static mut ERASER_THREAD_STATES: [ThreadLockset; NUM_THREADS] = [
    ThreadLockset {
        lockset: [false; NUM_LOCKS]
    }
; NUM_THREADS];

static mut ERASER_LOCKS: [usize; NUM_LOCKS] = [0; NUM_LOCKS];

pub fn eraser_get_lock_index(addr: usize) -> usize { // give m
    unsafe {
        for i in 0..NUM_LOCKS {
            if ERASER_LOCKS[i] == addr {
                return i;
            }
            if ERASER_LOCKS[i] == 0 {
                ERASER_LOCKS[i] = addr;
                return i;
            }
        }
        panic!("out of locks!");
    }
}

pub fn eraser_intersect_lockset(a: &[bool], b: &[bool]) -> [bool; NUM_LOCKS] {
    assert!(a.len() == b.len());
    assert!(a.len() == NUM_LOCKS);
    let lockset: [bool; NUM_LOCKS] = core::array::from_fn(|i| a[i] && b[i]);
    return lockset;
}

pub fn eraser_lockset_is_empty(a: &[bool]) -> bool {
    for i in 0..a.len() {
        if a[i] {
            return false;
        }
    }
    return true;
}

pub fn eraser_reset() {
    unsafe {
        ERASER_CURR_THREAD = 0;
        ERASER_ADDR_STATES = [Location {
            state: AddrState::Virgin, // hasn't been oughted yet
            tid: -1i32 as usize,
            address: -1i32 as u32, // expectation is that we are not reading / writing -1
            has_store: false,
            lockset: [true; NUM_LOCKS]
        }; NUM_ADDRESSES];
        ERASER_THREAD_STATES = [
            ThreadLockset {
                lockset: [false; NUM_LOCKS]
            }
        ; NUM_THREADS];
        ERASER_LOCKS = [0; NUM_LOCKS];
    }
}

pub fn eraser_get_address(address: u32) -> &'static mut Location {
    unsafe {
        for i in 0..NUM_ADDRESSES {
            if ERASER_ADDR_STATES[i].address == address {
                return &mut ERASER_ADDR_STATES[i];
            }
            if ERASER_ADDR_STATES[i].address == (-1i32 as u32) { 
                return &mut ERASER_ADDR_STATES[i];
            }
        }
        panic!("ran out of slots sorry");
    }
}


pub fn print_lockset(lockset: &[bool]) {
    for lock in lockset {
        print!("{}", if *lock { '1' } else { '0' });
    }
    println!();
}

pub fn trace_handler_eraser(pc: u32, address: u32, is_write: bool) {
    unsafe {
        let eraser_curr_thread = ERASER_CURR_THREAD;

        let current_state = eraser_get_address(address);
        println!("processing instruction {:x} on mem addr {:x}, is_write?={}, current_thread={}", pc, address, is_write, eraser_curr_thread);
        print!("             -> current address lockset: "); print_lockset(&current_state.lockset);
        print!("             -> current thread lockset: "); print_lockset(&ERASER_THREAD_STATES[ERASER_CURR_THREAD].lockset);

        if current_state.state == AddrState::Virgin {
            current_state.state = AddrState::Exclusive;
            current_state.tid = ERASER_CURR_THREAD;
        }

        if !(current_state.state == AddrState::Virgin) {
            // intersect
            println!("           -> current state is not a virgin, intersecting with lockset");
            current_state.lockset = eraser_intersect_lockset(&current_state.lockset, &ERASER_THREAD_STATES[ERASER_CURR_THREAD].lockset);
            print!("             -> updated thread lockset: "); print_lockset(&current_state.lockset);
        }
        
        if current_state.state == AddrState::Exclusive {            
            if current_state.tid != ERASER_CURR_THREAD {
                // current_state.lockset = ERASER_THREAD_STATES[ERASER_CURR_THREAD].lockset;
                println!("           -> current state was touched by another thread while exclusive");
                if is_write {
                    current_state.state = AddrState::ModShared;
                } else {
                    current_state.state = AddrState::Shared;
                }
                println!("              -> moved to {:?}", current_state.state);
            }
        }

        if current_state.state == AddrState::Shared {
            if is_write {
                current_state.state = AddrState::ModShared;
            }
        }

        if current_state.state == AddrState::ModShared  {
            print!("                     -> checking because ModShared - "); print_lockset(&current_state.lockset);
            if eraser_lockset_is_empty(&current_state.lockset) {
                println!(">>>> ERROR! addr={} has empty lockset in state ModShared", current_state.address);
            }
        }

        //
    }
}

pub fn eraser_lock(lock_ptr: usize) {
    unsafe {
        let lock_index = eraser_get_lock_index(lock_ptr as usize);
        ERASER_THREAD_STATES[ERASER_CURR_THREAD].lockset[lock_index] = true;
    }
}

pub fn eraser_unlock(lock_ptr: usize) {
    unsafe {
        let lock_index = eraser_get_lock_index(lock_ptr as usize);
        ERASER_THREAD_STATES[ERASER_CURR_THREAD].lockset[lock_index] = false;
    }
}

pub fn eraser_set_thread_id(id: usize) {
    unsafe { ERASER_CURR_THREAD = id; }
}

pub fn trace_handler_ckalloc(lr: u32, address: u32, is_store: bool) {
    println!("Trace handler ckalloc: lr={:#x}, address={:#x}", lr, address);
    let header: *const ckalloc::CheckHeader = ckalloc::ck_ptr_is_alloced(address as *const u32);
    if (header.is_null()) {
        println!("Error: address {:#x} is not alloced.", address);
    } else {
        println!("Success: address {:#x} is alloced,", address);
    }
}

pub fn trace_handler_0(lr: u32, address: u32, is_store: bool) {
    println!("Trace Handler 0: lr={:#x}, address={:#x}", lr, address);
}

static mut counter: u32 = 0;
pub fn trace_handler_1(lr: u32, address: u32, is_store: bool) {
    unsafe { counter = counter + 1; }
    println!("Trace Handler 1: lr={:#x}, address={:#x}", lr, address);
}


static mut MEMTRACE_WAS_STORE: bool = false;
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
            MEMTRACE_WAS_STORE = is_store == 1;

            // println!(
            //     "Data abort handler called: address: {:#x}, LR: {:#x}, wcr: {:#x}, is store: {}",
            //     caught_memory_address,
            //     lr,
            //     watchpoint_control,
            //     is_store
            // );
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
                    function(watchpoint_access_instr, watchpoint_value_register, MEMTRACE_WAS_STORE);
                },
                None => {

                }
            };

            virtmem::mmu_enable();            
        }
    }
}

#[inline(never)]
pub fn memtrace_init(data: *const u32, trace_handler: fn(u32, u32, bool)) {
    assert!(!virtmem::mmu_is_enabled());
    virtmem::mmu_reset();

    // unsafe { kmalloc::kmalloc_init_mb_with_offset(16, 16 * 1024 * 1024); }

    const DOM_KERN: u32 = 1;
    const STACK_ADDR: u32 = 0x1800_0000;
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
    let kern = virtmem::make_global_pin(DOM_KERN, no_user,      virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);
    let heap = virtmem::make_global_pin(DOM_KERN, unaccessible, virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);

    virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);
    virtmem::pin_mmu_sec(1, 0, 0, kern);
    virtmem::pin_mmu_sec(2, STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);
    virtmem::pin_mmu_sec(3, SECONDARY_STACK_ADDR - ONE_MB, SECONDARY_STACK_ADDR - ONE_MB, kern);    
    unsafe {
        kmalloc::HEAP_CURR = 0x1500_0000;
        kmalloc::HEAP_END = 0x1600_0000;
        let heap_curr = kmalloc::HEAP_CURR;
        let heap_end = kmalloc::HEAP_END;
        println!("current heap_curr={:x}, heap_end={:x}", heap_curr, heap_end);
        virtmem::pin_mmu_sec(4, (kmalloc::HEAP_CURR as u32) & 0xFF000000, (kmalloc::HEAP_CURR as u32) & 0xFF000000, heap);
    }
    virtmem::pin_mmu_sec(5, 0x1000_0000, 0x1000_0000, kern);
    virtmem::pin_mmu_sec(6, 0x1000_0000 + 16 * ONE_MB as u32, 0x1000_0000 + 16 * ONE_MB as u32, kern);

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

pub fn test_eraser_0_basic() {
    println!("test: eraser_0_basic");
    println!("expected: no errors with lock/unlock on single shared state");
    
    unsafe {
        eraser_reset();
        eraser_set_thread_id(1);
        
        memtrace_init(0 as *const u32, trace_handler_eraser);
        let l = 0x8000_0000 as usize;
        let x = kmalloc::kmalloc(4) as *mut u32;
        
        memtrace_trap_enable();
        
        eraser_lock(l);
        x.write_volatile(0x12345678);
        let _ret = x.read_volatile();
        eraser_unlock(l);
        
        for _i in 0..1 {
            println!("going to increment x={:p} again with lock", x);
            eraser_lock(l);
            *x = x.read_volatile() + 1;
            eraser_unlock(l);
        }
        
        memtrace_trap_disable();
        println!("SUCCESS: test_eraser_0_basic passed\n");
    }
}

pub fn test_eraser_exclusive() {
    println!("test: eraser_exclusive");
    println!("expected: no error, address in exclusive state (single thread access)");
    
    unsafe {
        eraser_reset();
        eraser_set_thread_id(1);
        
        memtrace_init(0 as *const u32, trace_handler_eraser);
        let l = 0x8000_0000 as usize;
        let x = kmalloc::kmalloc(4) as *mut u32;
        
        memtrace_trap_enable();
        
        eraser_lock(l);
        x.write_volatile(0x12345678);
        eraser_unlock(l);
        
        println!("should be exclusive: addr={:p}, state={:?}", x, eraser_get_address(x as u32).state);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Exclusive);
        
        // These reads should not error
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        
        memtrace_trap_disable();
        println!("SUCCESS: test_eraser_exclusive passed\n");
    }
}

pub fn test_eraser_race_no_lock() {
    println!("test: eraser_race_no_lock");
    println!("expected: error - second thread touches memory without lock");
    
    unsafe {
        eraser_reset();
        eraser_set_thread_id(1);
        
        memtrace_init(0 as *const u32, trace_handler_eraser);
        let l = 0x8000_0000 as usize;
        let x = kmalloc::kmalloc(4) as *mut u32;
        
        memtrace_trap_enable();
        
        eraser_lock(l);
        x.write_volatile(0x12345678);
        eraser_unlock(l);
        
        println!("should be EXCLUSIVE: addr={:p}, state={:?}", x, eraser_get_address(x as u32).state);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Exclusive);
        
        eraser_set_thread_id(2);
        
        println!("should have an error because second thread touches w/o a lock");
        x.write_volatile(0x12345678);  // BUG: no lock held
        
        memtrace_trap_disable();
        println!("COMPLETED: test_eraser_race_no_lock\n");
    }
}

pub fn test_eraser_inconsistent_lock() {
    println!("test: eraser_race_inconsistent_lock");
    println!("expected: error - second thread touches memory without the same lock");
    
    unsafe {
        eraser_reset();
        eraser_set_thread_id(1);
        
        memtrace_init(0 as *const u32, trace_handler_eraser);
        let l = 0x8000_0000 as usize;
        let l2 = 0x8000_0004 as usize;

        let x = kmalloc::kmalloc(4) as *mut u32;
        
        memtrace_trap_enable();
        
        eraser_lock(l);
        x.write_volatile(0x12345678);
        eraser_unlock(l);
        
        println!("should be EXCLUSIVE: addr={:p}, state={:?}", x, eraser_get_address(x as u32).state);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Exclusive);
        
        eraser_set_thread_id(2);
        
        println!("should have an error because second thread touches w/o a lock");
        eraser_lock(l2);
        x.write_volatile(0x12345678);  // BUG: no lock held
        eraser_lock(l2);

        memtrace_trap_disable();
        println!("COMPLETED: test_eraser_inconsistent_lock\n");
    }
}

pub fn test_eraser_shared() {
    println!("\n========== Test: eraser_shared ==========");
    println!("Expected: no error, address in SHARED state (read-only by multiple threads)");
    
    unsafe {
        eraser_reset();
        eraser_set_thread_id(1);
        
        memtrace_init(0 as *const u32, trace_handler_eraser);
        let l = 0x8000_0000 as usize;
        let x = kmalloc::kmalloc(4) as *mut u32;
        
        memtrace_trap_enable();
        
        // Thread 1: lock, write, unlock
        eraser_lock(l);
        x.write_volatile(0x12345678);
        eraser_unlock(l);
        
        // Thread 1: read-only access
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        
        println!("should be exclusive: addr={:p}, state={:?}", x, eraser_get_address(x as u32).state);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Exclusive);
        
        // Thread 2: read-only access (no lock)
        eraser_set_thread_id(2);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Exclusive);
        
        let _ = x.read_volatile();  // Transitions to SHARED
        
        println!("should be SHARED: addr={:p}, state={:?}", x, eraser_get_address(x as u32).state);
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Shared);
        
        let _ = x.read_volatile();
        let _ = x.read_volatile();
        
        // Should still be SHARED (no writes)
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Shared);
        
        // Back to Thread 1: read-only access
        eraser_set_thread_id(1);
        let _ = x.read_volatile();
        
        // Should still be SHARED
        assert_eq!(eraser_get_address(x as u32).state, AddrState::Shared);
        
        memtrace_trap_disable();
        println!("SUCCESS: test_eraser_shared passed\n");
    }
}

pub fn run_all_eraser_tests() {
    println!("\n\n========== RUNNING ALL ERASER TESTS ==========\n");
    
    test_eraser_0_basic();
    println!("----------- RESET -----------\n");
    
    test_eraser_exclusive();
    println!("----------- RESET -----------\n");

    test_eraser_inconsistent_lock();
    println!("----------- RESET -----------\n");
    
    test_eraser_race_no_lock();
    println!("----------- RESET -----------\n");
    
    test_eraser_shared();
    println!("----------- RESET -----------\n");
    
    println!("\n========== ALL TESTS COMPLETED ==========\n");
}
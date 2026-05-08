use core::arch::asm;
use crate::println;
use core::mem::transmute;
use core::ptr;
use crate::arch::{prefetch_flush, dsb};
use crate::os::virtmem::{
    make_global_pin_16mb, make_user_pin, mmu_disable, mmu_enable, mmu_reset, pin_mmu_sec, MemAttr,
    MemPerm, pin_mmu_switch, pin_mmu_init, mmu_is_enabled,
};
use crate::mem::{get32, put32};
use crate::os::utils::{self, disable_branch_prediction, disable_dcache, disable_l1_instruction_cache, enable_branch_prediction, enable_dcache, enable_l1_instruction_cache};
use crate::kmalloc::{self, HEAP_CURR};

macro_rules! adds_8 {
    () => {
        concat!(
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
            "add {r0}, {r0}, #1\n",
        )
    };
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventBus {
    /// 0xFF: Increment each cycle
    Cycle = 0xFF,

    /// 0x26: Procedure return mispredicted
    ReturnMispredicted = 0x26,

    /// 0x25: Procedure return predicted (popped from return stack, branch taken)
    ReturnPredicted = 0x25,

    /// 0x24: Procedure return executed (popped from return stack)
    ReturnExecuted = 0x24,

    /// 0x23: Procedure call executed (pushed to return stack)
    CallExecuted = 0x23,

    /// 0x22: ETMEXTOUT[0]/[1] combined event
    EtmExtOutCombined = 0x22,

    /// 0x21: ETMEXTOUT[1] asserted
    EtmExtOut1 = 0x21,

    /// 0x20: ETMEXTOUT[0] asserted
    EtmExtOut0 = 0x20,

    /// 0x12: Write buffer drained (DSB or strongly ordered op)
    WriteBufferDrained = 0x12,

    /// 0x11: Stall due to full LSU request queue
    LsuQueueFullStall = 0x11,

    /// 0x10: External data access / linefill / noncacheable / write-through
    ExternalDataAccess = 0x10,

    /// 0x0F: Main TLB miss
    MainTlbMiss = 0x0F,

    /// 0x0D: Software changed PC (no mode change)
    SoftwarePcChange = 0x0D,

    /// 0x0C: Data cache write-back (per half-line)
    DataCacheWriteBack = 0x0C,

    /// 0x0B: Data cache miss
    DataCacheMiss = 0x0B,

    /// 0x0A: Data cache access (all nonsequential)
    DataCacheAccess = 0x0A,

    /// 0x09: Data cache access (cacheable only)
    DataCacheAccessCacheable = 0x09,

    /// 0x07: Instruction executed
    InstructionExecuted = 0x07,

    /// 0x06: Branch mispredicted
    BranchMispredicted = 0x06,

    /// 0x05: Branch instruction executed
    BranchExecuted = 0x05,

    /// 0x04: Data MicroTLB miss
    DataMicroTlbMiss = 0x04,

    /// 0x03: Instruction MicroTLB miss
    InstructionMicroTlbMiss = 0x03,

    /// 0x02: Stall due to data dependency
    DataDependencyStall = 0x02,

    /// 0x01: Stall due to instruction buffer starvation
    InstructionBufferStall = 0x01,

    /// 0x00: Instruction cache miss
    InstructionCacheMiss = 0x00,
}

static mut jit_buffer: [u32; 16] = [0; 16];
unsafe fn write_simple_jit(chk: u32) {
    let imm = (chk & 0xff) as u32;
    jit_buffer[0] = 0xE3A00000u32 | imm; // MOV r0, #imm  -> 0xE3A00000 | imm
    jit_buffer[1] = 0xE12FFF1Eu32; //BX lr -> 0xE12FFF1E
    dsb();
}
unsafe fn get_jit_function() -> extern "C" fn() -> u32 {
    let p = ptr::addr_of!(jit_buffer) as *const u8;

    let f: extern "C" fn() -> u32 =
        transmute(p);

    return f
}


fn set_event(event_idx: u8, event: EventBus) {
    unsafe {
        let pmcr: u32;
        asm!(
            "mrc p15, 0, {reg}, c15, c12, 0",
            reg = out(reg) pmcr,
            options(nostack, nomem)
        );

        let shift = 12 + (8 * event_idx as u32);
        let new_pmcr = (pmcr & (!(0xFF << shift))) | 1 | ((event as u32) << shift);
        asm!(
            "mcr p15, 0, {reg}, c15, c12, 0",
            reg = in(reg) new_pmcr,
            options(nostack, nomem)
        );

        prefetch_flush();
        dsb();
    }
}

fn get_cycle_count() -> u32 {
    unsafe {
        let num_cycles: u32;
        asm!(
            "mrc p15, 0, {reg}, c15, c12, 1",
            reg = out(reg) num_cycles,
            options(nostack, nomem)
        );
        num_cycles
    }
}

fn get_perf_0() -> u32 {
    unsafe {
        let perf0: u32;
        asm!(
            "mrc p15, 0, {reg}, c15, c12, 2",
            reg = out(reg) perf0,
            options(nostack, nomem)
        );
        perf0
    }
}

fn get_perf_1() -> u32 {
    unsafe {
        let perf1: u32;
        asm!(
            "mrc p15, 0, {reg}, c15, c12, 3",
            reg = out(reg) perf1,
            options(nostack, nomem)
        );
        perf1
    }
}

fn reset_all_counters() {
    unsafe {
        let zero: u32 = 0;
        check_pmu();
        asm!(
            "mcr p15, 0, {reg}, c15, c12, 1",
            "mcr p15, 0, {reg}, c15, c12, 2",
            "mcr p15, 0, {reg}, c15, c12, 3",
            reg = in(reg) zero,
            options(nostack, nomem)
        );
        prefetch_flush();
        dsb();
        // check_pmu();
    }
}

fn check_pmu() {
    unsafe {
        let pmcr: u32;
        let cycle: u32;
        let cnt0: u32;
        let cnt1: u32;
        asm!(
            "mrc p15, 0, {a}, c15, c12, 0",
            "mrc p15, 0, {d}, c15, c12, 1",
            "mrc p15, 0, {b}, c15, c12, 2",
            "mrc p15, 0, {c}, c15, c12, 3",
            a = out(reg) pmcr,
            b = out(reg) cnt0,
            c = out(reg) cnt1,
            d = out(reg) cycle,
        );
        println!("PMCR={:#010x} cycle={:#010x} cnt0={:#010x} cnt1={:#010x}", pmcr, cycle, cnt0, cnt1);
    }
}

fn prefetch_alignment_test() {
    println!("prefetch buffer alignment");
    set_event(0, EventBus::InstructionBufferStall);
    enable_l1_instruction_cache();
    reset_all_counters();
    unsafe {
        asm!(
           ".align 6",
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            r0 = out(reg) _,
            options(nostack, nomem)
        );
    }
    let aligned_stalls = get_perf_0();
    let aligned_cycles = get_cycle_count();

    reset_all_counters();
    unsafe {
        asm!(
            ".align 6",
            ".space 24",
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            adds_8!(),
            r0 = out(reg) _,
            options(nostack, nomem)
        );
    }
    let unaligned_stalls = get_perf_0();
    let unaligned_cycles = get_cycle_count();
    disable_l1_instruction_cache();
    println!("aligned stalls count={}, aligned cycles count={}, unaligned stalls count={}, unaligned cycles count={}", aligned_stalls, aligned_cycles, unaligned_stalls, unaligned_cycles);
}

fn icache_invalidate_test() {
    println!("icache invalidate test");
    unsafe {
        set_event(0, EventBus::InstructionCacheMiss);

        utils::enable_l1_instruction_cache();
        
        write_simple_jit(1);
        let jit_func = get_jit_function();
        reset_all_counters();
        let r1 = jit_func();
        let ic_miss_1 = get_perf_0();
        let cycles_1 = get_cycle_count();
        println!("first call returned {}, icache_miss={}, cycles={}", r1, ic_miss_1, cycles_1);

        write_simple_jit(2);
        reset_all_counters();
        let r_stale = jit_func();
        let ic_miss_stale = get_perf_0();
        let cycles_stale = get_cycle_count();
        println!("after overwrite (no invalidate) returned {}, icache_miss={}, cycles={}", r_stale, ic_miss_stale, cycles_stale);
        // Disable the instruction cache and see what happens (original test)
        utils::disable_l1_instruction_cache();

        reset_all_counters();
        prefetch_flush();
        dsb();
        let r_new = jit_func();
        let ic_miss_new = get_perf_0();
        let cycles_new = get_perf_1();
        println!("after icache invalidate returned {}, icache_miss={}, cycles={}", r_new, ic_miss_new, cycles_new);
    }
}

fn mva_invalidate(addr: *const u8) {
    unsafe {
        let a = addr as u32;
        asm!(
            "mcr p15, 0, {reg}, c7, c5, 1",
            reg = in(reg) a,
            options(nostack, nomem)
        );
        dsb();
    }
}

fn mva_test() {
    println!("mva icache invalidate test");
    unsafe {
        set_event(0, EventBus::InstructionCacheMiss);

        utils::enable_l1_instruction_cache();
        
        write_simple_jit(1);
        let jit_func = get_jit_function();
        reset_all_counters();
        let r1 = jit_func();
        let ic_miss_1 = get_perf_0();
        let cycles_1 = get_cycle_count();
        println!("first call returned {}, icache_miss={}, cycles={}", r1, ic_miss_1, cycles_1);

        write_simple_jit(2);
        reset_all_counters();
        let r_stale = jit_func();
        let ic_miss_stale = get_perf_0();
        let cycles_stale = get_cycle_count();
        println!("after overwrite (no invalidate) returned {}, icache_miss={}, cycles={}", r_stale, ic_miss_stale, cycles_stale);

        // Invalidate using MVA for the JIT buffer
        let p = ptr::addr_of!(jit_buffer) as *const u8;
        mva_invalidate(p);

        reset_all_counters();
        prefetch_flush();
        dsb();
        let r_new = jit_func();
        let ic_miss_new = get_perf_0();
        let cycles_new = get_perf_1();
        println!("after mva icache invalidate returned {}, icache_miss={}, cycles={}", r_new, ic_miss_new, cycles_new);
    }
}

fn branch_prediction_test() {
    println!("branch prediction test");
    unsafe {
        set_event(0, EventBus::BranchMispredicted);
        set_event(1, EventBus::BranchExecuted);
        reset_all_counters();

        enable_branch_prediction();

        asm!(
            "mov {r0}, #128",
            "1:",
            "subs {r0}, {r0}, #1",
            "bne 1b",
            r0 = out(reg) _,
            options(nostack, nomem)
        );

        let (mispreds_normal, executed_normal) = (get_perf_0(), get_perf_1()); 
        println!("normal: branch mispredictions={}, branch executed={}", mispreds_normal, executed_normal);

        asm!(
            "mov {r1}, #0",
            "mov {r2}, #128",
            "2:",
            "eor {r1}, {r1}, #1",
            "cmp {r1}, #0",
            "beq 3f",
            "subs {r2}, {r2}, #1",
            "bne 2b",
            "3:",
            r1 = out(reg) _,
            r2 = out(reg) _,
            options(nostack, nomem)
        );

        let (mispreds_hard, executed_hard) = (get_perf_0(), get_perf_1());
        println!("alternating: branch mispredictions={}, branch executed={}", mispreds_hard, executed_hard);
        
        disable_branch_prediction();
    }
}

fn dcache_test() {
    set_event(0, EventBus::DataCacheMiss);
    set_event(1, EventBus::DataMicroTlbMiss);
    
    const ONE_MB: u32 = 1024 * 1024;
    const STACK_ADDR: u32 = 0x0800_0000;
    const DOM_KERN: u32 = 1;
    const ASID1: u32 = 1;

    println!("dcache test");

    assert!(!mmu_is_enabled());
    mmu_disable();
    mmu_reset();

    // Keep allocator init behavior from the original test harness.
    unsafe { kmalloc::kmalloc_init_mb(1) };

    pin_mmu_init(!0);

    let no_user = MemPerm::perm_rw_priv;

    let dev = make_global_pin_16mb(DOM_KERN, no_user, MemAttr::MEM_device);
    let kern = make_global_pin_16mb(DOM_KERN, no_user, MemAttr::MEM_uncached);

    // Index into the 8 pinned TLB entries.
    pin_mmu_sec(0, 0x2000_0000, 0x2000_0000,dev);
    pin_mmu_sec(1, 0, 0, kern);
    pin_mmu_sec(2, STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);

    let user1 = make_user_pin(DOM_KERN, ASID1, no_user, MemAttr::MEM_cached);
    let user_addr = ONE_MB * 16;
    assert_eq!((user_addr >> 12) % 16, 0);
    for i in 0..16 {
        put32(user_addr + i * 4, 0xdead_beef);
    }
    pin_mmu_sec(3, user_addr, user_addr, user1);

    println!("about to enable");
    pin_mmu_switch(0, ASID1);
    mmu_enable();
    enable_dcache();

    assert!(mmu_is_enabled());
    println!("MMU is on and working!");

    reset_all_counters();
    let mut read_data: [u32; 16] = [0; 16];
    for i in 0..16 {
        read_data[i] = get32(user_addr + ((i * 4) as u32));
    }
    let (cold_read_cache_miss, cold_read_micro_tlb_miss) = (get_perf_0(), get_perf_1());

    reset_all_counters();
    for i in 0..16 {
        read_data[i] = get32(user_addr + ((i * 4) as u32));
    }
    let (warm_read_cache_miss, warm_read_micro_tlb_miss) = (get_perf_0(), get_perf_1());

    disable_dcache();
    mmu_disable();
    println!("Cold read cache miss: {}, cold read micro tlb miss: {}", cold_read_cache_miss, cold_read_micro_tlb_miss);
    println!("Warm read cache miss: {}, warm read micro tlb miss: {}", warm_read_cache_miss, warm_read_micro_tlb_miss);

    assert!(!mmu_is_enabled());
    println!("MMU is off!");
}

#[unsafe(no_mangle)]
#[inline(never)]
fn c() -> u32 {
    return 0;
}

#[unsafe(no_mangle)]
#[inline(never)]
fn b() -> u32 {
    return c() + 1;
}

#[unsafe(no_mangle)]
#[inline(never)]
fn a() -> u32{
    return b() + 1;
}


fn my_return_test() {
    enable_branch_prediction();
    set_event(0, EventBus::CallExecuted);
    set_event(1, EventBus::ReturnExecuted);
    reset_all_counters();
    a();
    let (calls, returns) = (get_perf_0(), get_perf_1());
    println!("calls: {}, returns: {}", calls, returns);
    disable_branch_prediction();
}

pub fn test_pmu_profiler() {
    println!("Testing PMU profiler!");

    prefetch_alignment_test();
    icache_invalidate_test();
    mva_test();
    branch_prediction_test();
    dcache_test();
    my_return_test();
}
use core::arch::asm;

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

fn set_event(event_idx: u8, event: u8) {
    unsafe {
        let pmcr: u32;
        asm!(
            "mrc p15, 0, {reg}, c15, c12, 0",
            reg = out(reg) pmcr,
            options(nostack, nomem)
        );

        let new_pmcr = pmcr | 1 | ((event as u32) << (12 + 8 * event_idx));
        asm!(
            "mcr p15, 0, {reg}, c15, c12, 0",
            reg = in(reg) new_pmcr,
            options(nostack, nomem)
        );
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

pub fn test_pmu_profiler() {

}
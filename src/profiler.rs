use core::arch::global_asm;
use core::time;
use crate::arch::{dsb, prefetch_flush};
use crate::bit_utils::{bit_get, bits_get, bits_set, bit_clr, bit_set};
use crate::print::{print_binary_compare, print_binary_table};
use crate::{println, print, ckalloc};
use crate::os::interrupts::{self, CPSR_USER_MODE, CPSR_SUPER_MODE, get_cpsr, mode_get};

global_asm!(r#"
.globl _interrupt_table_prof
.globl _interrupt_table_end_prof
_interrupt_table_prof:
  @ Q: why can we copy these ldr jumps and have
  @ them work the same?
  ldr pc, _reset_asm_profiler                    @ 0x0: Q: why this order?[A2-16]
  ldr pc, _reset_asm_profiler @ _undefined_instruction_asm    @ 0x4
  ldr pc, _software_interrupt_asm_profiler       @ 0x8
  ldr pc, _prefetch_abort_asm_profiler
  ldr pc, _reset_asm_profiler @ _data_abort_asm
  ldr pc, _reset_asm_profiler
  ldr pc, _reset_asm_profiler @ _interrupt_asm
_reset_asm_profiler:                   .word generic_interrupt_asm_profiler
_software_interrupt_asm_profiler:      .word software_interrupt_asm_profiler
_prefetch_abort_asm_profiler:          .word prefetch_abort_asm_profiler
_interrupt_table_end_prof:   @ end of the table.

generic_interrupt_asm_profiler:                      @ A2-19
    bx lr  
software_interrupt_asm_profiler:                         @ A2-20
    push {{r0-r12, lr}}
    mov r0, lr
    sub r0, r0, #4     
    mov r1, sp
    bl pixie_syscall
    pop {{r0-r12, lr}}
    movs pc, lr
prefetch_abort_asm_profiler:
    @ subs pc, r14, #4

    @ mov r0, #(1 << 27)
    @ ldr r1, =0x2020001C
    @ str r0, [r1]

    mrc p15, 0, sp, c15, c12, 1 @ cycle counter in sp
    mcr p15, 0, sp, c13, c0, 2 @ save this in my thread local storeage
    
    sub lr, lr, #4
    mov sp, 0x9000000
    push {{r0-r12, lr}}
    mov r0, lr
    bl prefetch_abort_vector
    pop {{r0-r12, lr}}

    mov sp, #0 @ save this instead!
    mcr p15, 0, sp, c15, c12, 1 @ reset the cycle counter in sp

    movs pc, lr

    @ sub   lr, lr, #4    
    @ mov sp, #INT_STACK_ADDR @ don't really need to, right?
    @ push  {{r0-r12,lr}}
    @ sub lr, lr, #4
    @ mov r0, #(1 << 27)
    @ ldr r1, =0x2020001C
    @ str r0, [r1]
    @ bx lr
    @ 1:  b 1b
    @ mov sp, 0x900000 @ don't really need to, right?
    @ push  {{r0-r12,lr}}
    @ mov   r0, lr
    @ bl    prefetch_abort_vector
    @ pop   {{r0-r12,lr}}
    @ movs    pc, lr 
"#, 
    // SUPER_MODE=const CPSR_SUPER_MODE
);

static mut num_instructions: u32 = 0;
static mut instruction_count_table: [u32; 50000] = [0; 50000];
static mut cycles_elapsed: [u64; 50000] = [0; 50000];

static mut handler_cycle_count: u32 = 0;
static mut regular_cycle_count: u32 = 0;

const PIXIE_SYS_DIE: u32 = 2;
const PIXIE_SYS_STOP: u32 = 1;

unsafe extern "C" {
    #[link_name = "_interrupt_table_prof"]
    static INTERRUPT_TABLE_PROF_START: u8;

    #[link_name = "_interrupt_table_end_prof"]
    static INTERRUPT_TABLE_PROF_END: u8;
}

fn get_bcr_state() -> u32 {
    unsafe {
        let bcr_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c0, 5",
            out(reg) bcr_state,
            options(nomem, nostack)
        );
        bcr_state
    }
}

fn set_bcr_state(state: u32) {
    unsafe {
        dsb();
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c0, 5",
            in(reg) state,
            options(nomem, nostack)
        );
        prefetch_flush();
        dsb();
    }
}

fn get_bvr_state() -> u32 {
    unsafe {
        let bvr_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c0, 4",
            out(reg) bvr_state,
            options(nomem, nostack)
        );
        bvr_state
    }
}

fn set_bvr_state(state: u32) {
    unsafe {
        dsb();
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c0, 4",
            in(reg) state,
            options(nomem, nostack)
        );
        prefetch_flush();
        dsb();
    }
}

fn breakpoint_mismatch_set(addr: u32) {
    unsafe {
        // println!("starting to set mismatch variables");

        // println!("starting to set mismatch variables");

        // let old_bcr_state: u32;
        // core::arch::asm!(
        //     "mrc p14, 0, {0}, c0, c0, 5",
        //     out(reg) old_bcr_state,
        //     options(nomem, nostack)
        // );
        // println!("old bcr0 state=0b{:b}", old_bcr_state);
        
        // let bcr_state = 0x4001e7;
        // core::arch::asm!( // setting bcr0
        //     "mcr p14, 0, {0}, c0, c0, 5",
        //     in(reg) bcr_state,
        //     options(nomem, nostack)
        // );
        // prefetch_flush();
        // println!("updated bcr0");

        // let bvr_state = bits_set(0, 2, 31, addr >> 2);
        // core::arch::asm!(
        //     "mcr p14, 0, {0}, c0, c0, 4",
        //     in(reg) bvr_state,
        //     options(nomem, nostack)
        // );
        // prefetch_flush(); 
        // println!("updated bvr0");

        // println!("bcr_state=0x{:0x}, bvr_state=0x{:0x}", bcr_state, bvr_state);
        
        let mut bcr_state = get_bcr_state();
        // print_binary_table("bcr_state", bcr_state);
        bcr_state = 0;
        bcr_state = bit_set(bcr_state, 0); // page 1112 of armv6, breakpoint enable
        bcr_state = bits_set(bcr_state, 1, 2, 0b11); // set supervisor access: user
        // the breakpoint always hits if I don't set bits 5-8
        bcr_state = bits_set(bcr_state, 5, 8, 0b1111); // why
        bcr_state = bits_set(bcr_state, 21, 22, 0b10); // enable mismatch
        // print_binary_table("bcr_state new", bcr_state);
        set_bcr_state(bcr_state);

        // print_binary_table("140e value", 0x4001e7);
        // print_binary_compare("compare my bcr_state and 140e vale", bcr_state, 0x4001e7);

        // print_binary_table("bvr_state",get_bvr_state());
        set_bvr_state(addr);
        // print_binary_table("bvr_state", addr);
    }
}

// fn breakpoint_mismatch_set(addr: u32) {
//     unsafe {
//         println!("starting to set mismatch variables");

//         let old_bcr_state: u32;
//         core::arch::asm!(
//             "mrc p14, 0, {0}, c0, c0, 5",
//             out(reg) old_bcr_state,
//             options(nomem, nostack)
//         );
//         println!("old bcr0 state=0b{:b}", old_bcr_state);
        
//         let bcr_state = 0x4001e7;
//         core::arch::asm!( // setting bcr0
//             "mcr p14, 0, {0}, c0, c0, 5",
//             in(reg) bcr_state,
//             options(nomem, nostack)
//         );
//         prefetch_flush();
//         println!("updated bcr0");

//         let bvr_state = bits_set(0, 2, 31, addr >> 2);
//         core::arch::asm!(
//             "mcr p14, 0, {0}, c0, c0, 4",
//             in(reg) bvr_state,
//             options(nomem, nostack)
//         );
//         prefetch_flush(); 
//         println!("updated bvr0");

//         println!("bcr_state=0x{:0x}, bvr_state=0x{:0x}", bcr_state, bvr_state);
//     }  
// }

fn breakpoint_mismatch_start() {
    unsafe {
        let dscr_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) dscr_state,
            options(nomem, nostack)
        );
        // println!("got old dscr state = 0b{:0b}", dscr_state);
        // print_binary_table("old dscr", dscr_state);

        let new_dscr_state = bit_clr(bit_set(0, 15), 14);
        println!("want to write dscr state = 0b{:0b}", new_dscr_state);
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c1, 0",
            in(reg) new_dscr_state,
            options(nomem, nostack)
        );
        // print_binary_table("dscr", new_dscr_state);
        prefetch_flush();

        // let verify_dscr: u32;
        // core::arch::asm!(
        //     "mrc p14, 0, {0}, c0, c1, 0",
        //     out(reg) verify_dscr,
        //     options(nomem, nostack)
        // );
        // println!("verify dscr = 0b{:0b}", verify_dscr);

        breakpoint_mismatch_set(0);
        prefetch_flush();
    }
}

fn breakpoint_mismatch_stop() {
    unsafe {
        let zero = 0;
        core::arch::asm!( // setting bcr0
            "mcr p14, 0, {0}, c0, c0, 5",
            in(reg) zero,
            options(nomem, nostack)
        );

        prefetch_flush();
        println!("stopped breakpoints");
    }
}

fn was_breakpoint_fault() -> bool {
    unsafe {
        let ifsr: u32;
        core::arch::asm!(
            "mrc p15, 0, {0}, c5, c0, 1",
            out(reg) ifsr,
            options(nomem, nostack)
        );

        let dscr: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) dscr, 
            options(nomem, nostack)
        );
        return (bit_get(ifsr, 10) == 0) && (bits_get(ifsr, 0, 3) == 0b0010) && (bits_get(dscr, 2, 5) == 0b0001);
    }
}

fn pixie_die_handler(regs: *const u32) {
    println!("done: dying");
}

fn init_cycle_counter() {
    unsafe {
        let i = 1;
        core::arch::asm!(
            "mcr p15, 0, {0}, c15, c12, 0",
            in(reg) i
        );
    }
}

fn get_current_cycle() -> u32 {
    let cycle_result: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {0}, c15, c12, 1",
            out(reg) cycle_result
        );
        cycle_result
    }
}

fn reset_cycle_counter() {
    unsafe {
        let val = 0;
        core::arch::asm!(
            "mcr p15, 0, {0}, c15, c12, 1",
            in(reg) val
        );
    }
}

fn get_thread_local_value() -> u32 {
    unsafe {
        let val;
        core::arch::asm!(
            "mrc p15, 0, {0}, c13, c0, 2",
            out(reg) val
        );
        val
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn prefetch_abort_vector(pc: u32) {
    unsafe {
        let time_since_last_handler = get_current_cycle();
        regular_cycle_count += time_since_last_handler;
        reset_cycle_counter();

        if(!was_breakpoint_fault()) {
            panic!("Have a non-breakpoint fault");
        }
        
        // unsafe { ::core::arch::asm!(
        //     "mov r0, #(1 << 27)",
        //     "ldr r1, =0x2020001C",
        //     "str r0, [r1]"
        // ); }
        // dsb();

        // println!("current mode: {:#b}, pc={:#x}, super_mode={:#b}, user_mode={:#b}", mode_get(get_cpsr()), pc, CPSR_SUPER_MODE, CPSR_USER_MODE);
        // println!("lr: {:#x}", pc);

        // unsafe {
        //     num_instructions += 1;
        //     // instruction_count_table[pc as usize] += 1;
        // }
        num_instructions += 1;
        instruction_count_table[pc as usize] += 1;
        // breakpoint_mismatch_stop();
        breakpoint_mismatch_set(pc); // so we can run this
        let time_handler_elapsed = get_current_cycle();
        handler_cycle_count += time_handler_elapsed;
        // cycles_elapsed[pc as usize] += time_since_last_handler as u64;
        cycles_elapsed[pc as usize] += get_thread_local_value() as u64;
        reset_cycle_counter();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn pixie_syscall(pc: u32, regs: *const u32) {
    let instr = unsafe { core::ptr::read_volatile((pc) as *const u32) };
    let sysno = instr & 0x0ff_ffff;
    println!("SWI called: pc={:p}, instr={:0x}, sysno={}", pc as *const u32, instr, sysno);

    match sysno {
        PIXIE_SYS_DIE => {
            pixie_die_handler(regs);
            println!("DONE!!!");
            crate::watchdog::restart();
        }
        PIXIE_SYS_STOP => {
            unsafe { 
                println!("done: pc=0x{:0x}", pc); 
                interrupts::switch_to_super_mode(regs);
            }
        }
        _ => {
            panic!("invalid syscall");
        }
    }
}

fn pixie_dump(num: usize) {
    unsafe {
        let mut pairs: [(u32, usize); 50000] =
            core::array::from_fn(|i| (instruction_count_table[i], i));
        pairs.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        unsafe {
            let v = num_instructions;
            for &(count, pc) in pairs.iter().take(num) {
                println!("pc: 0x{:0x}, count: {} / {}, cycles={}", pc, count, v, cycles_elapsed[pc]); // don't want to get the FPU involved
            }
            println!("handler_cycles={} vs regular_cycles={}", &mut *&raw mut handler_cycle_count, &mut *&raw mut regular_cycle_count);
        }
    }
}

fn pixie_start() {
    interrupts::start_interrupts(
        core::ptr::addr_of!(INTERRUPT_TABLE_PROF_START) as usize, 
        core::ptr::addr_of!(INTERRUPT_TABLE_PROF_END) as usize
    );
    println!("moved interrupt table");

    if !(mode_get(get_cpsr()) == CPSR_SUPER_MODE) {
        panic!("should be in super mode before starting pixie");
    }

    unsafe { prefetch_flush(); }

    breakpoint_mismatch_start();
    println!("started breakpoint");
    unsafe {prefetch_flush();}
    
    println!("about to switch to user mode");
    unsafe { interrupts::switch_to_user_mode(); }
    println!("switched to user mode");
    if !(mode_get(get_cpsr()) == CPSR_USER_MODE) {
        panic!("must be in user mode after making the switch");
    }
}

fn pixie_stop() {
    unsafe {
        core::arch::asm!(
            "swi {imm}",
            imm = const PIXIE_SYS_STOP,
        );
    }
}

fn pixie_reset() {
    unsafe {
        num_instructions = 0;
        for i in 0..32000 {
            instruction_count_table[i] = 0;
        }
    }
}

pub fn test_profiler() {
    init_cycle_counter();
    crate::gpio::set_output(27); // for debug

    println!("From reading");
    pixie_start();
    crate::gpio::read(24);
    pixie_stop();
    pixie_dump(10);


    println!("From writing");
    pixie_start();
    crate::gpio::write(24, 1);
    pixie_stop();
    pixie_dump(10);

    pixie_start();
    for i in 0..10 {
        println!("{}: hello world\n", i);
    }
    pixie_stop();
    pixie_dump(10);
    pixie_reset();
}
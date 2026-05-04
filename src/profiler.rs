use core::arch::global_asm;
use crate::arch::prefetch_flush;
use crate::bit_utils::{bit_get, bits_get, bits_set, bit_clr, bit_set};
use crate::{println, print, ckalloc};
use crate::os::interrupts::{self, CPSR_USER_MODE, CPSR_SUPER_MODE, get_cpsr, mode_get};

global_asm!(r#"
.globl _interrupt_table_prof
.globl _interrupt_table_end_prof
_interrupt_table_prof:
  @ Q: why can we copy these ldr jumps and have
  @ them work the same?
  ldr pc, _reset_asm                    @ 0x0: Q: why this order?[A2-16]
  ldr pc, _undefined_instruction_asm    @ 0x4
  ldr pc, _software_interrupt_asm       @ 0x8
  ldr pc, _prefetch_abort_asm
  ldr pc, _data_abort_asm
  ldr pc, _reset_asm
  ldr pc, _interrupt_asm
_reset_asm:                   .word reset_asm
_undefined_instruction_asm:   .word undefined_instruction_asm
_software_interrupt_asm:      .word software_interrupt_asm_profiler
_prefetch_abort_asm:          .word prefetch_abort_asm_profiler
_data_abort_asm:              .word data_abort_asm
_interrupt_asm:               .word interrupt_asm
_interrupt_table_end_prof:   @ end of the table.

undefined_instruction_asm:                      @ A2-19
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
    @ sub   lr, lr, #4    
    @ mov sp, #INT_STACK_ADDR @ don't really need to, right?
    @ push  {{r0-r12,lr}}
    mov r0, #(1 << 27)
    ldr r1, =0x2020001C
    str r0, [r1]
    1:  b 1b
    @ mov   r0, lr
    @ bl    prefetch_abort_vector
    @ pop   {{r0-r12,lr}}
    @ movs    pc, lr 
data_abort_asm:
    bx lr
reset_asm:
    bx lr
interrupt_asm:
    bx lr
"#);

static mut num_instructions: u32 = 0;
static mut instruction_count_table: [u32; 32000] = [0; 32000];

const PIXIE_SYS_DIE: u32 = 2;
const PIXIE_SYS_STOP: u32 = 1;

unsafe extern "C" {
    #[link_name = "_interrupt_table_prof"]
    static INTERRUPT_TABLE_PROF_START: u8;

    #[link_name = "_interrupt_table_end_prof"]
    static INTERRUPT_TABLE_PROF_END: u8;
}

fn breakpoint_mismatch_set(addr: u32) {
    unsafe {
        println!("starting to set mismatch variables");

        let old_bcr0_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c0, 5",
            out(reg) old_bcr0_state,
            options(nomem, nostack)
        );
        println!("old bcr0 state=0b{:b}", old_bcr0_state);
        
        let bcr0_state = 0x4001e7;
        core::arch::asm!( // setting bcr0
            "mcr p14, 0, {0}, c0, c0, 5",
            in(reg) bcr0_state,
            options(nomem, nostack)
        );
        prefetch_flush();
        println!("updated bcr0");

        let bvr0_state = bits_set(0, 2, 31, addr >> 2);
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c0, 4",
            in(reg) bvr0_state,
            options(nomem, nostack)
        );
        prefetch_flush(); 
        println!("updated bvr0");

        println!("bcr0_state=0x{:0x}, bvr0_state=0x{:0x}", bcr0_state, bvr0_state);
    }  
}

fn breakpoint_mismatch_start() {
    unsafe {
        let dscr_state: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) dscr_state,
            options(nomem, nostack)
        );
        println!("got old dscr state = 0b{:0b}", dscr_state);

        let new_dscr_state = bit_clr(bit_set(dscr_state, 15), 14);
        println!("want to write dscr state = 0b{:0b}", new_dscr_state);
        core::arch::asm!(
            "mcr p14, 0, {0}, c0, c1, 0",
            in(reg) new_dscr_state,
            options(nomem, nostack)
        );
        println!("updated dscr state = 0b{:0b}", new_dscr_state);
        prefetch_flush();

        let verify_dscr: u32;
        core::arch::asm!(
            "mrc p14, 0, {0}, c0, c1, 0",
            out(reg) verify_dscr,
            options(nomem, nostack)
        );
        println!("verify dscr = 0b{:0b}", verify_dscr);

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

#[unsafe(no_mangle)]
pub extern "C" fn prefetch_abort_vector(pc: u32) {
    if(!was_breakpoint_fault()) {
        panic!("Have a non-breakpoint fault");
    }

    unsafe {
        num_instructions += 1;
        instruction_count_table[pc as usize] += 1;
    }

    breakpoint_mismatch_set(pc); // so we can run this
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
        let mut pairs: [(u32, usize); 4096] =
            core::array::from_fn(|i| (instruction_count_table[i], i));
        pairs.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        unsafe {
            for &(count, pc) in pairs.iter().take(num) {
                let v = num_instructions;
                println!("pc: 0x{:0x}, count: {} / {}", pc, count, v); // don't want to get the FPU involved
            }
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
    pixie_start();
    for i in 0..10 {
        println!("{}: hello world\n", i);
    }
    pixie_stop();
    pixie_dump(10);
    pixie_reset();
}
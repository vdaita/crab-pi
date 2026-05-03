use crate::arch::{dev_barrier, gcc_mb};
use crate::gpio::{read, set_input, set_output, set_on, set_off};
use crate::{bit_utils, print, start};
use crate::println;
use crate::timer::{Timer};
use core::arch::{asm, global_asm};
use crate::mem::{get32, put32};
use crate::gpio;

// defined in rpi_constants.h. put the stack for this above the regular stack. 
global_asm!(r#"
.globl _interrupt_table
.globl _interrupt_table_end
_interrupt_table:
  @ Q: why can we copy these ldr jumps and have
  @ them work the same?
  ldr pc, _reset_asm                    @ 0x0: Q: why this order?[A2-16]
  ldr pc, _undefined_instruction_asm    @ 0x4
  ldr pc, _software_interrupt_asm       @ 0x8
  ldr pc, _prefetch_abort_asm
  ldr pc, _data_abort_asm
  ldr pc, _reset_asm
  ldr pc, _interrupt_asm
fast_interrupt_asm:
  sub   lr, lr, #4 @First instr of FIQ handler
  push  {{lr}}
  push  {{r0-r12}}
  mov   r0, lr              @ Pass old pc
  bl    fast_interrupt_vector    @ C function
  pop   {{r0-r12}}
  ldm   sp!, {{pc}}^
_reset_asm:                   .word reset_asm
_undefined_instruction_asm:   .word undefined_instruction_asm
_software_interrupt_asm:      .word software_interrupt_asm
_prefetch_abort_asm:          .word prefetch_abort_asm
_data_abort_asm:              .word data_abort_asm
_interrupt_asm:               .word interrupt_asm
_interrupt_table_end:   @ end of the table.

undefined_instruction_asm_user:
    movs pc, lr

undefined_instruction_asm:                      @ A2-19
    bx lr  
@ software_interrupt_asm:
@    push {{r0-r12, lr}}
@    mov r0, #(1 << 27)
@    ldr r1, =0x2020001C
@    str r0, [r1]
@    pop {{r0-r12, lr}}
@    movs pc, lr
software_interrupt_asm:                         @ A2-20
    push {{r0-r12, lr}}
    mov r0, lr
    sub r0, r0, #4     
    mov r1, sp
    bl software_interrupt_vector
    pop {{r0-r12, lr}}
    movs pc, lr
prefetch_abort_asm:
    bx lr
data_abort_asm:
    bx lr
reset_asm:
    bx lr


interrupt_asm:
  @ NOTE:
  @  - each mode has its own <sp> that persists when
  @    we switch out of the mode (i.e., will be the same
  @    when switch back).
  @  - <INT_STACK_ADDR> is a physical address we reserve 
  @   for exception stacks today.  we don't do recursive
  @   exception/interupts so one stack is enough.
  mov sp, 0x9000000   @ Q: what if you delete?
  sub   lr, lr, #4

  @ push regs: beter match a pop
  push  {{r0-r12,lr}}         @ XXX: pushing too many 
                            @ registers: only need caller
                            @ saved.

  mov   r0, lr              @ Pass old pc as arg 0
  bl    interrupt_vector    @ C function: expects C 
                            @ calling conventions.

  @ pop regs: better match push (what happens if not?)
  pop   {{r0-r12,lr}} 	    @ pop integer registers
                            @ this MUST MATCH the push.
                            @ very common mistake.

  @ return from interrupt handler: will re-enable general ints.
  @ Q: what happens if you do "mov" instead?
  @ Q: what other instructions could we use?
  movs    pc, lr        @ 1: moves <spsr> into <cpsr> 
                        @ 2. moves <lr> into the <pc> of that
                        @    mode.

.globl enable_interrupts
enable_interrupts:
    mrs r0, cpsr @ move cpsr to r0
    bic r0,r0,#(1<<7)	@ clear 7th bit.
    msr cpsr_c,r0		@ move r0 back to PSR
    bx lr		        @ return

.globl disable_interrupts
disable_interrupts:
    mrs r0,cpsr		       
    orr r0,r0,#(1<<7)	@ set 7th bit
    msr cpsr_c,r0
    bx lr

.globl switch_to_user_mode
switch_to_user_mode:
    mrs r0, cpsr
    bic r0, r0, #0b11111  @ clear mode bits (bits 0-4)
    orr r0, r0, #0b10000  @ set user mode
    @ bic r0, r0, #0b10000000  @ enable IRQs (clear I bit)

    push {{sp}}
    ldm sp, {{sp}}^
    add sp, sp, #4 @ moves the stack pointer up so that we get rid of the stack pointer we just wrote

    push {{r0}}
    push {{lr}}
    rfe sp

.globl switch_to_super_mode
switch_to_super_mode:
    cps {SUPER_MODE}
    bx lr
"#,
    SUPER_MODE = const CPSR_SUPER_MODE
);


const IRQ_BASE: usize = 0x2000_b200;
const IRQ_BASIC_PENDING: usize = IRQ_BASE + 0x00; // 0x200
const IRQ_PENDING_1: usize = IRQ_BASE + 0x04; // 0x204
const IRQ_PENDING_2: usize = IRQ_BASE + 0x08; // 0x208
const IRQ_FIQ_CONTROL: usize = IRQ_BASE + 0x0c; // 0x20c
const IRQ_ENABLE_1: usize = IRQ_BASE + 0x10; // 0x210
const IRQ_ENABLE_2: usize = IRQ_BASE + 0x14; // 0x214
const IRQ_ENABLE_BASIC: usize = IRQ_BASE + 0x18; // 0x218
const IRQ_DISABLE_1: usize = IRQ_BASE + 0x1c; // 0x21c
const IRQ_DISABLE_2: usize = IRQ_BASE + 0x20; // 0x220
const IRQ_DISABLE_BASIC: usize = IRQ_BASE + 0x24; // 0x224

const ARM_TIMER_BASE: usize = 0x2000_b400;
const ARM_TIMER_LOAD: usize = ARM_TIMER_BASE + 0x00; // p196
const ARM_TIMER_VALUE: usize = ARM_TIMER_BASE + 0x04; // read-only
const ARM_TIMER_CONTROL: usize = ARM_TIMER_BASE + 0x08;

const ARM_TIMER_IRQ_CLEAR: usize = ARM_TIMER_BASE + 0x0c;

// Errata for p198:
// neither are register 0x40c raw is 0x410, masked is 0x414
const ARM_TIMER_IRQ_RAW: usize = ARM_TIMER_BASE + 0x10;
const ARM_TIMER_IRQ_MASKED: usize = ARM_TIMER_BASE + 0x14;

const ARM_TIMER_RELOAD: usize = ARM_TIMER_BASE + 0x18;
const ARM_TIMER_PREDIV: usize = ARM_TIMER_BASE + 0x1c;
const ARM_TIMER_COUNTER: usize = ARM_TIMER_BASE + 0x20;

const ARM_TIMER_IRQ: u32 = (1 << 0); // timer interrupt number

const PARTHIV_PIN: u32 = 27;
pub const CPSR_USER_MODE: u32 = 0b10000;
pub const CPSR_SUPER_MODE: u32 = 0b10011;

unsafe extern "C" {
    #[link_name = "enable_interrupts"]
    fn enable_interrupts_asm();

    #[link_name = "disable_interrupts"]
    fn disable_interrupts_asm();

    #[link_name = "interrupt_asm"]
    unsafe fn interrupt_asm();

    #[link_name = "switch_to_user_mode"]
    pub unsafe fn switch_to_user_mode();

    #[link_name = "switch_to_super_mode"]
    pub unsafe fn switch_to_super_mode(regs: *const u32);

    #[link_name = "_interrupt_table"]
    pub static INTERRUPT_TABLE_START: u8;

    #[link_name = "_interrupt_table_end"]
    pub static INTERRUPT_TABLE_END: u8;
}

pub fn move_table(interrupt_table_start_addr: usize, interrupt_table_end_addr: usize) {
    let start: *const u32 = interrupt_table_start_addr as *const u32;
    let end: *const u32 = interrupt_table_end_addr as *const u32;
    let len = ((end as usize) - (start as usize)) / 4;
    let dst = core::ptr::without_provenance_mut::<u32>(0);
    unsafe {
        for i in 0..len {
            core::arch::asm!(
                "ldr {t}, [{i}]",
                "str {t}, [{o}]",
                t = out(reg) _,
                i = in(reg) start.add(i),
                o = in(reg) dst.add(i),
            )
        }
        // core::ptr::copy_nonoverlapping(start, dst, len);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn print_asm(val: u32) {
    println!("ASM print val: {}", val);
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector(pc: u32) {
    dev_barrier();
    let pending: u32 = get32(IRQ_PENDING_1 as u32);
    if((pending & ARM_TIMER_IRQ) == 0) {
        println!("This doesn't seem to be a timer interrupt.");
        return;
    }
    put32(ARM_TIMER_IRQ_CLEAR as u32, 1);
    println!("This appears to be a timer interrupt.");
    dev_barrier();
}

#[unsafe(no_mangle)]
pub extern "C" fn fast_interrupt_vector(pc: u32) {
    println!("Fast interrupt vector!");
}

// #[unsafe(no_mangle)]
// pub extern "C" fn software_interrupt_vector(pc: u32, sp: u32) {
//     dev_barrier();
//     gpio::set_on(PARTHIV_PIN);
//     let instr = unsafe { core::ptr::read_volatile(pc as *const u32) };
//     println!("SWI called: pc={:p}, instr={:0x}, sp={:p},", pc as *const u32, instr, sp as *const u32);
//     let nr = unsafe { core::ptr::read_volatile((sp + 7 * 4) as *const u32) };
//     gpio::set_off(PARTHIV_PIN);

//     let mut lr: u32;
//     unsafe {
//         asm!(
//             "mov {0}, lr",
//             out(reg) lr,
//             options(nomem,  nostack)
//         );
//     }
//     println!("lr: {:p}", lr as *const u32);
//     println!("sp: {:p}", sp as *const u32);

//     print_cpsr();

//     dev_barrier();
// } 

#[unsafe(no_mangle)]
pub extern "C" fn software_interrupt_vector(pc: u32, sp: u32) {
    dev_barrier();
    // For SVC, lr points to the next instruction, so SVC is at pc - 4.
    let instr = unsafe { core::ptr::read_volatile((pc) as *const u32) };
    println!("SWI called: pc={:p}, instr={:0x}, sp={:p}", pc as *const u32, instr, sp as *const u32);

    if (instr != 0xef00_0000) {
        println!("not a SVC instruction");
        return;
    }

    // Linux ARM EABI: r7 holds syscall number, 
    let nr = unsafe { core::ptr::read_volatile((sp + 7 * 4) as *const u32) };
    
    // r0-r2 carry write(fd, buf, len).
    let arg0 = unsafe { core::ptr::read_volatile((sp + 0 * 4) as *const u32) };
    let arg1 = unsafe { core::ptr::read_volatile((sp + 1 * 4) as *const u32) };
    let arg2 = unsafe { core::ptr::read_volatile((sp + 2 * 4) as *const u32) };

    let ret = 1;
    let ret: i32 = match nr {
        4 => {
            // sys_write(fd, buf, len): support stdout/stderr only.
            let fd = arg0;
            let buf_ptr = (arg1 + 0x1000) as *const u8;
            let len = arg2 as usize;
            if (fd == 1 || fd == 2) && !buf_ptr.is_null() {
                println!("writing out with fd={}, buf_ptr={:p}, len={}", fd, buf_ptr, len);
                let bytes = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
                crate::uart::write_bytes(bytes);
                crate::uart::flush();
                len as i32
            } else {
                -1
            }
        }
        _ => {
            println!("unknown SVC: {}", nr);
            -38 // -ENOSYS
        }
    };

    // Return value in r0 by updating saved frame before pop {r0-r12, lr}.
    unsafe { core::ptr::write_volatile(sp as *mut u32, ret as u32) };
    dev_barrier();
}

pub fn start_interrupts(itable_start: usize, itable_end: usize) {
    println!("about to install interrupts");
    unsafe {
        disable_interrupts_asm();
        core::ptr::write_volatile(IRQ_DISABLE_1 as *mut u32, 0xffffffff);
        core::ptr::write_volatile(IRQ_DISABLE_2 as *mut u32, 0xffffffff);
        println!("just disabled interrupts");
    }
    dev_barrier();
    gcc_mb();
    move_table(itable_start, itable_end);
    gcc_mb();
     
    unsafe {
        enable_interrupts_asm();
    }
    println!("just enabled interrupts");
}

pub fn mode_get(cpsr: u32) -> u32 {
    bit_utils::bits_get(cpsr, 0, 4)
}

pub fn get_cpsr() -> u32 {
    let mut cpsr: u32;
    unsafe {
        asm!(
            "mrs {0}, cpsr",
            out(reg) cpsr,
            options(nomem, nostack),
        );
    };
    return cpsr;
}

pub fn print_cpsr() {
    println!("cpsr: {:0b}", get_cpsr());
}

#[inline(always)]
pub fn get_stack_pointer() -> u32 {
    let sp: u32;
    unsafe {
        asm!(
            "mov {0}, sp",
            out(reg) sp
        );
    }
    return sp;
}

#[inline(always)]
pub fn report() {
    unsafe {
        let sp: u32 = get_stack_pointer();

        for i in 0..8 {
            print!("sp + {}={:0x}, ", i * 4, *((sp + 4 * i) as *const u32));
        }
        
        // print out the link register and program counter as well
        let lr: u32;
        let pc: u32;
        unsafe {
            asm!(
                "mov {0}, lr",
                "mov {1}, pc",
                out(reg) lr,
                out(reg) pc
            );
        }

        print!("lr = {}, pc = {}", lr, pc);
        print!("\n");
    }
}

pub fn test_interrupts() {
    start_interrupts(
        core::ptr::addr_of!(INTERRUPT_TABLE_START) as usize,
        core::ptr::addr_of!(INTERRUPT_TABLE_END) as usize
    );
    gpio::set_output(PARTHIV_PIN);

    // println!("Address of this function: {:p}", test_interrupts as *const u32);
    let here: u32;
    unsafe {
        asm!(
            "adr {0}, .",  // "." means current instruction address
            out(reg) here,
        );
    }
    println!("Expected link register: {:0x}", (here + 8)); // next instruction is the switch to user mode function, and then the instruction after that.

    // report();

    println!("Stack pointer: {:0x}", get_stack_pointer());

    unsafe { switch_to_user_mode(); }
    println!("Switched to user mode!");
    print_cpsr();

    println!("Switched to user mode.");
    println!("Address of this function: {:p}", test_interrupts as *const u32);

    // here print out the stack
    
    let mut r0: u32 = 1; // for standard out
    let test_str = "testing interrupt\n";
    unsafe {
        asm!(
            "svc 0",
            inout("r0") r0 => r0,
            in("r1") test_str.as_ptr(),
            in("r2") test_str.len(),
            in("r7") 4u32,
            options(nostack)
        )
    }

    println!("Finished running SWI handler.");
    let sp:u32;
    unsafe{::core::arch::asm!("mov {t},sp",t=out(reg)sp)}
    println!("Stack pointer: {sp:08x}");
    // println!("Stack pointer: {:0x}", get_stack_pointer());

    // report();

    // here print out the stack
    
    // switch_to_super_mode();
    
    // unsafe { disable_interrupts_asm(); }
    // println!("returned from SWI instruction {}", r0);

    // println!("passing value test: {}", ret);
    // println!("disabled interrupts, svc write returned: {}", r0 as i32);
}
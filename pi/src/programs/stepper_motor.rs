use crate::gpio;
use crate::os::interrupts::{ARM_TIMER_CONTROL, IRQ_ENABLE_BASIC};
use crate::timer::{self, Timer};
use core::arch::{asm, global_asm};
use crate::println;
use crate::os::interrupts::{ARM_TIMER_BASE, ARM_TIMER_LOAD, IRQ_PENDING_1, ARM_TIMER_IRQ, ARM_TIMER_IRQ_CLEAR, IRQ_ENABLE_1, IRQ_BASIC_PENDING, move_table, start_interrupts, switch_to_super_mode, switch_to_user_mode};
use crate::arch::{dev_barrier, gcc_mb};
use crate::mem::{get32, put32};

const STEP_PIN: u32 = 20;
const DIR_PIN: u32 = 21;

const ARM_TIMER_CTRL_32BIT: u32        = ( 1 << 1 );
const ARM_TIMER_CTRL_PRESCALE_1: u32  = ( 0 << 2 );
const ARM_TIMER_CTRL_PRESCALE_16: u32  = ( 1 << 2 );
const ARM_TIMER_CTRL_PRESCALE_256: u32 = ( 2 << 2 );
const ARM_TIMER_CTRL_INT_ENABLE: u32   = ( 1 << 5 );
const ARM_TIMER_CTRL_ENABLE: u32       = ( 1 << 7 );


global_asm!(r#"
.globl _stepper_interrupt_table
.globl _stepper_interrupt_table_end
_stepper_interrupt_table:
  @ Q: why can we copy these ldr jumps and have
  @ them work the same?
  ldr pc, _reset_asm_stepper                    @ 0x0: Q: why this order?[A2-16]
  ldr pc, _undefined_instruction_asm_stepper    @ 0x4
  ldr pc, _software_interrupt_asm_stepper       @ 0x8
  ldr pc, _prefetch_abort_asm_stepper
  ldr pc, _data_abort_asm_stepper
  ldr pc, _reset_asm_stepper
  ldr pc, _interrupt_asm_stepper
_reset_asm_stepper:                   .word reset_asm_stepper
_undefined_instruction_asm_stepper:   .word undefined_instruction_asm_stepper
_software_interrupt_asm_stepper:      .word software_interrupt_asm_stepper
_prefetch_abort_asm_stepper:          .word prefetch_abort_asm_stepper
_data_abort_asm_stepper:              .word data_abort_asm_stepper
_interrupt_asm_stepper:               .word interrupt_asm_stepper
_stepper_interrupt_table_end:   @ end of the table.

undefined_instruction_asm_user:
    movs pc, lr
undefined_instruction_asm_stepper:                      @ A2-19
    bx lr
software_interrupt_asm_stepper:                         @ A2-20
    bx lr
prefetch_abort_asm_stepper:
    bx lr
data_abort_asm_stepper:
    bx lr
reset_asm_stepper:
    bx lr

interrupt_asm_stepper:
  @ NOTE:
  @  - each mode has its own <sp> that persists when
  @    we switch out of the mode (i.e., will be the same
  @    when switch back).
  @  - <INT_STACK_ADDR> is a physical address we reserve 
  @   for exception stacks today.  we don't do recursive
  @   exception/interupts so one stack is enough.
  mov sp, 0x8800000   @ Q: what if you delete?
  sub   lr, lr, #4

  @ push regs: beter match a pop
  push  {{r0-r12,lr}}         @ XXX: pushing too many 
                            @ registers: only need caller
                            @ saved.

  mov   r0, lr              @ Pass old pc as arg 0
  bl    interrupt_vector_stepper    @ C function: expects C 
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
"#
);

unsafe extern "C" {
    #[link_name = "_stepper_interrupt_table"]
    pub static STEPPER_INTERRUPT_TABLE_START: u8;

    #[link_name = "_stepper_interrupt_table_end"]
    pub static STEPPER_INTERRUPT_TABLE_END: u8;
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector_stepper(pc: u32) {
    dev_barrier();
    let pending: u32 = get32(IRQ_BASIC_PENDING as u32);
    if((pending & ARM_TIMER_IRQ) == 0) {
        step();
        // println!("This aint a timer interrupt: {:0b}", pending);
        return;
    }
    put32(ARM_TIMER_IRQ_CLEAR as u32, 1);
    // println!("This appears to be a timer interrupt.");
    step();
    dev_barrier();
}


fn step() {
    gpio::set_on(STEP_PIN);
    timer::Timer::delay_us(2);
    gpio::set_off(STEP_PIN);
}

#[inline(never)]
fn run_with_delay(count: u32, delay: u32) {
    for _ in 0..count {
        unsafe {
            gpio::set_on(STEP_PIN);
        }
        timer::Timer::delay_us(2);
        unsafe {
            gpio::set_off(STEP_PIN);
        }
        timer::Timer::delay_us(2);
        timer::Timer::delay_us(delay);
    }
}

pub fn run_stepper_motor() {
    gpio::set_output(STEP_PIN);
    gpio::set_output(DIR_PIN);
    
    // run_with_delay(10000, 2000);

    start_interrupts(core::ptr::addr_of!(STEPPER_INTERRUPT_TABLE_START) as usize, core::ptr::addr_of!(STEPPER_INTERRUPT_TABLE_END) as usize);

    dev_barrier();
    put32(IRQ_ENABLE_BASIC as u32, ARM_TIMER_IRQ);
    dev_barrier();
    put32(ARM_TIMER_LOAD as u32, 0x1000);
    dev_barrier();
    put32(ARM_TIMER_CONTROL as u32, ARM_TIMER_CTRL_32BIT |
            ARM_TIMER_CTRL_ENABLE |
            ARM_TIMER_CTRL_INT_ENABLE | ARM_TIMER_CTRL_PRESCALE_1);

    unsafe { switch_to_user_mode(); }
    println!("Switched to user mode!");

    Timer::delay_sec(5);
}
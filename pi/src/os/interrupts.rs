use crate::arch::dev_barrier;
use crate::println;
use core::arch::{asm, global_asm};
use crate::mem::{get32, put32};
use crate::os::syscalls;
use crate::os::holder::OSHolder;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct InterruptFrame {
	pub r0: u32,
	pub r1: u32,
	pub r2: u32,
	pub r3: u32,
	pub r4: u32,
	pub r5: u32,
	pub r6: u32,
	pub r7: u32,
	pub r8: u32,
	pub r9: u32,
	pub r10: u32,
	pub r11: u32,
	pub r12: u32,
	pub lr: u32,
}

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

undefined_instruction_asm:                      @ A2-19
    ldr sp, =0x17800000
    sub lr, lr, #4                              @ adjust lr to point to faulting instruction
    push {{r0-r12, lr}}

    mov r0, sp                                  @ frame pointer
    mov r1, lr                                  @ faulting pc

    bl os_undefined_instruction_vector

    pop {{r0-r12, lr}}
    movs pc, lr

software_interrupt_asm:                         @ A2-20
    cpsid i
    ldr sp, =0x17800000
    push {{r0-r12, lr}}

    mov r0, sp
    mov r1, lr

    bl software_interrupt_vector

    str r0, [sp]
    pop {{r0-r12, lr}}
    movs pc, lr

prefetch_abort_asm:
    ldr sp, =0x17b00000 @ needs to be different...
    sub lr, lr, #4
    push {{r0-r12, lr}}

    mov r0, sp                                  @ frame pointer
    mov r1, lr                                  @ faulting pc

    bl os_prefetch_abort_vector

    pop {{r0-r12, lr}}
    movs pc, lr
data_abort_asm:
    ldr sp, =0x17800000
    sub lr, lr, #4
    push {{r0-r12, lr}}

    mov r0, sp                                  @ frame pointer
    mov r1, lr                                  @ faulting pc

    bl os_data_abort_vector

    pop {{r0-r12, lr}}
    movs pc, lr
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
  ldr sp, =0x17c00000   @ Q: what if you delete?
  sub   lr, lr, #4

  @ push regs: beter match a pop
  push  {{r0-r12,lr}}         @ XXX: pushing too many 
                            @ registers: only need caller
                            @ saved.

  mov   r0, lr              @ Pass old pc as arg 0
  mov   r1, sp @ pass in the stack pointer as arg 1
  bl    interrupt_vector    @ C function: expects C 
                            @ calling conventions.

  @ this only runs as the default option if there's nothing better to run
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
    bic r0, r0, #0b10000000  @ enable IRQs (clear I bit)

    push {{sp}}
    ldm sp, {{sp}}^
    add sp, sp, #4 @ moves the stack pointer up so that we get rid of the stack pointer we just wrote

    push {{r0}}
    push {{lr}}
    rfe sp
"#);


pub const IRQ_BASE: usize = 0x2000_b200;
pub const IRQ_BASIC_PENDING: usize = IRQ_BASE + 0x00; // 0x200
pub const IRQ_ENABLE_BASIC: usize = IRQ_BASE + 0x18; // 0x218

pub const ARM_TIMER_BASE: usize = 0x2000_b400;
pub const ARM_TIMER_LOAD: usize = ARM_TIMER_BASE + 0x00; // p196
pub const ARM_TIMER_CONTROL: usize = ARM_TIMER_BASE + 0x08;

pub const ARM_TIMER_IRQ_CLEAR: usize = ARM_TIMER_BASE + 0x0c;

pub const ARM_TIMER_IRQ: u32 = 1 << 0; // timer interrupt number

pub const CPSR_USER_MODE: u32 = 0b10000;

pub const ARM_TIMER_CTRL_32BIT: u32 = 1 << 1;
pub const ARM_TIMER_CTRL_PRESCALE_256: u32 = 2 << 2;
pub const ARM_TIMER_CTRL_INT_ENABLE: u32 = 1 << 5;
pub const ARM_TIMER_CTRL_ENABLE: u32 = 1 << 7;

pub const LOAD_PERIOD: u32 = 10_000; // was 1000; longer slice so ticks rarely land back-to-back

pub const VBAR: usize = 0x0900_0000;

unsafe extern "C" {
    #[link_name = "disable_interrupts"]
    pub fn disable_interrupts_asm();

    #[link_name = "switch_to_user_mode"]
    pub unsafe fn switch_to_user_mode();

    #[link_name = "_interrupt_table"]
    pub static INTERRUPT_TABLE_START: u8;

    #[link_name = "_interrupt_table_end"]
    pub static INTERRUPT_TABLE_END: u8;
}


pub fn move_table_vbar(interrupt_table_start_addr: usize, interrupt_table_end_addr: usize, vbar: usize) {
    let start: *const u32 = interrupt_table_start_addr as *const u32;
    let end: *const u32 = interrupt_table_end_addr as *const u32;
    let len = ((end as usize) - (start as usize)) / 4;
    let dst = core::ptr::without_provenance_mut::<u32>(vbar);

    println!("copying interrupt table from {:p} start -> {:p} end to dst {:p}", start, end, dst);

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
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector(_pc: u32, _frame: *mut InterruptFrame) { // don't do anything other than mark the value
    unsafe {
        let pending: u32 = get32(IRQ_BASIC_PENDING as u32);
        if (pending & ARM_TIMER_IRQ) == 0 {
            println!("not a timer interrupt: {:0b}", pending);
            dev_barrier();
            return;
        }

        put32(ARM_TIMER_IRQ_CLEAR as u32, 1);
        println!("timer interrupt");
        
        let holder = OSHolder::os_holder_mut();
        holder.should_cswitch = true;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fast_interrupt_vector(_pc: u32) {
    println!("Fast interrupt vector!");
}

#[unsafe(no_mangle)]
pub extern "C" fn os_undefined_instruction_vector(_frame: *mut InterruptFrame, pc: u32) {
    unsafe {
        println!("Undefined instruction at pc={:#x}, inst={:#x}", pc, *(pc as *const u32));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn os_data_abort_vector(_frame: *mut InterruptFrame, pc: u32) {
    unsafe { 
        let far: u32;
        core::arch::asm!("mrc p15, 0, {}, c6, c0, 0", out(reg) far);
        let instr = *(pc as *const u32);
        println!("data abort at pc={:#x}, fault address: {:#x}, instr: {:#x}", pc, far, instr);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn os_prefetch_abort_vector(frame: *mut InterruptFrame, pc: u32) {
    unsafe {
        let frame = &*frame;
        let instr = core::ptr::read_volatile(pc as *const u32);
        println!(
            "Prefetch abort at pc={:#x}, instr={:#x}, r0={:#x}, r1={:#x}, r2={:#x}, r3={:#x}, r4={:#x}, r5={:#x}, r6={:#x}, r7={:#x}, r8={:#x}, r9={:#x}, r10={:#x}, r11={:#x}, r12={:#x}, lr={:#x}",
            pc,
            instr,
            frame.r0,
            frame.r1,
            frame.r2,
            frame.r3,
            frame.r4,
            frame.r5,
            frame.r6,
            frame.r7,
            frame.r8,
            frame.r9,
            frame.r10,
            frame.r11,
            frame.r12,
            frame.lr,
        );
    }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn software_interrupt_vector(frame: *mut InterruptFrame, svc_lr: u32) -> u32 {
    syscalls::handle_software_interrupt(frame, svc_lr)
}

pub fn install_interrupts_vbar() {
    unsafe {
        // copy over the interrupt table
        move_table_vbar(
            core::ptr::addr_of!(INTERRUPT_TABLE_START) as usize,
            core::ptr::addr_of!(INTERRUPT_TABLE_END) as usize,
            VBAR
        );

        dev_barrier();
        asm!("mcr p15, 0, {0}, c12, c0, 0", in(reg) VBAR, options(nostack, preserves_flags));
        dev_barrier();
    }
}

pub fn enable_timer_interrupts() {
    dev_barrier();
    put32(IRQ_ENABLE_BASIC as u32, ARM_TIMER_IRQ);
    dev_barrier();
    put32(ARM_TIMER_LOAD as u32, LOAD_PERIOD);
    dev_barrier();
    let control_value = ARM_TIMER_CTRL_32BIT |
                       ARM_TIMER_CTRL_ENABLE |
                       ARM_TIMER_CTRL_INT_ENABLE |
                       ARM_TIMER_CTRL_PRESCALE_256;

    put32(ARM_TIMER_CONTROL as u32, control_value);
    dev_barrier();
    put32(ARM_TIMER_IRQ_CLEAR as u32, 1);
    dev_barrier();
}

use crate::arch::{dev_barrier, gcc_mb};
use crate::{bit_utils, print};
use crate::println;
use core::arch::{asm, global_asm};
use crate::mem::{get32, put32};
use crate::os::virtmem;
use crate::gpio;
use crate::profiler;
use crate::os::syscalls::{self};
use crate::os::holder::{self, OSHolder};

pub fn update_current_program_frame(frame: *mut InterruptFrame, sp: usize) { // note: this must only be called from a SWI handler or something that disables the MMU beforehand
    unsafe {
        let holder = OSHolder::os_holder_mut();
        let idx = holder.current_program;
        let prog = holder.get_program_mut(idx);
        if holder.active[idx] {
            prog.frame = *frame;
            prog.sp = sp;

            println!("Saving program frame for {}, lr={:x}, sp={:x}", idx, prog.frame.lr, prog.sp);
        }
    }
}

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
    bic r0, r0, #0b10000000  @ enable IRQs (clear I bit)

    push {{sp}}
    ldm sp, {{sp}}^
    add sp, sp, #4 @ moves the stack pointer up so that we get rid of the stack pointer we just wrote

    push {{r0}}
    push {{lr}}
    rfe sp

.globl switch_to_super_mode
switch_to_super_mode:
    cps {SUPER_MODE}
    mov r0, 0
    mcr p15, 0, r0, c7, c5, 4
    mov pc, lr

.globl fork_trampoline_back
fork_trampoline_back:
    @ r0 = return_pc
    @ r1 = return_sp
    @ r2 = pointer to frame

    @ Set up SPSR for user mode  
    mrs r12, cpsr
    bic r12, r12, #0x1F
    orr r12, r12, #0x10
    bic r12, r12, #0x80
    msr spsr_cxsf, r12
    
    @ Set user SP
    cps #0x1F
    mov sp, r1
    cps #0x13
    
    @ Load return PC into our temp register
    ldr lr, [r2, #52]       @ Load the saved lr (14th field, offset 13*4 = 52)
    
    @ Restore all general purpose registers
    mov r1, r2              @ Save frame pointer in r1
    ldmia r1, {{r0-r12}}      @ Load r0-r12
    
    @ Now jump with the correct lr
    movs pc, lr
"#,
    SUPER_MODE = const CPSR_SUPER_MODE
);


pub const IRQ_BASE: usize = 0x2000_b200;
pub const IRQ_BASIC_PENDING: usize = IRQ_BASE + 0x00; // 0x200
pub const IRQ_PENDING_1: usize = IRQ_BASE + 0x04; // 0x204
pub const IRQ_PENDING_2: usize = IRQ_BASE + 0x08; // 0x208
pub const IRQ_FIQ_CONTROL: usize = IRQ_BASE + 0x0c; // 0x20c
pub const IRQ_ENABLE_1: usize = IRQ_BASE + 0x10; // 0x210
pub const IRQ_ENABLE_2: usize = IRQ_BASE + 0x14; // 0x214
pub const IRQ_ENABLE_BASIC: usize = IRQ_BASE + 0x18; // 0x218
pub const IRQ_DISABLE_1: usize = IRQ_BASE + 0x1c; // 0x21c
pub const IRQ_DISABLE_2: usize = IRQ_BASE + 0x20; // 0x220
pub const IRQ_DISABLE_BASIC: usize = IRQ_BASE + 0x24; // 0x224

pub const ARM_TIMER_BASE: usize = 0x2000_b400;
pub const ARM_TIMER_LOAD: usize = ARM_TIMER_BASE + 0x00; // p196
pub const ARM_TIMER_VALUE: usize = ARM_TIMER_BASE + 0x04; // read-only
pub const ARM_TIMER_CONTROL: usize = ARM_TIMER_BASE + 0x08;

pub const ARM_TIMER_IRQ_CLEAR: usize = ARM_TIMER_BASE + 0x0c;

// Errata for p198:
// neither are register 0x40c raw is 0x410, masked is 0x414
pub const ARM_TIMER_IRQ_RAW: usize = ARM_TIMER_BASE + 0x10;
pub const ARM_TIMER_IRQ_MASKED: usize = ARM_TIMER_BASE + 0x14;

pub const ARM_TIMER_RELOAD: usize = ARM_TIMER_BASE + 0x18;
pub const ARM_TIMER_PREDIV: usize = ARM_TIMER_BASE + 0x1c;
pub const ARM_TIMER_COUNTER: usize = ARM_TIMER_BASE + 0x20;

pub const ARM_TIMER_IRQ: u32 = (1 << 0); // timer interrupt number

const PARTHIV_PIN: u32 = 27;
pub const CPSR_USER_MODE: u32 = 0b10000;
pub const CPSR_SUPER_MODE: u32 = 0b10011;

pub const ARM_TIMER_CTRL_32BIT: u32 = ( 1 << 1 );
pub const ARM_TIMER_CTRL_PRESCALE_1: u32 = ( 0 << 2 );
pub const ARM_TIMER_CTRL_PRESCALE_16: u32 = ( 1 << 2 );
pub const ARM_TIMER_CTRL_PRESCALE_256: u32 = ( 2 << 2 );
pub const ARM_TIMER_CTRL_INT_ENABLE: u32 = ( 1 << 5 );
pub const ARM_TIMER_CTRL_ENABLE: u32 = ( 1 << 7 );

pub const LOAD_PERIOD: u32 = 1000;


// pub const VBAR: usize = 0x0900_0000;
pub const VBAR: usize = 0x0900_0000;

unsafe extern "C" {
    #[link_name = "enable_interrupts"]
    pub fn enable_interrupts_asm();

    #[link_name = "disable_interrupts"]
    pub fn disable_interrupts_asm();

    #[link_name = "interrupt_asm"]
    unsafe fn interrupt_asm();

    #[link_name = "switch_to_user_mode"]
    pub unsafe fn switch_to_user_mode();

    #[link_name = "switch_to_super_mode"]
    pub unsafe fn switch_to_super_mode(regs: *const u32);

    #[link_name = "fork_trampoline_back"]
    pub fn fork_trampoline_back(return_pc: u32, return_sp: u32, return_frame: *const InterruptFrame);

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
        // core::ptr::copy_nonoverlapping(start, dst, len);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn print_asm(val: u32) {
    println!("ASM print val: {}", val);
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector(pc: u32, frame: *mut InterruptFrame) { // don't do anything other than mark the value
    unsafe {
        let pending: u32 = get32(IRQ_BASIC_PENDING as u32);
        if((pending & ARM_TIMER_IRQ) == 0) {
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

// #[unsafe(no_mangle)]
// pub extern "C" fn interrupt_vector(pc: u32, frame: *mut InterruptFrame) {
//     unsafe {
//         let pending: u32 = get32(IRQ_BASIC_PENDING as u32);
//         if((pending & ARM_TIMER_IRQ) == 0) {
//             println!("This aint a timer interrupt: {:0b}", pending);
//             dev_barrier();
//             return;
//         }
//         put32(ARM_TIMER_IRQ_CLEAR as u32, 1);
//         println!("This appears to be a timer interrupt.");

//         if virtmem::mmu_is_enabled() { // means we have a program
//             virtmem::mmu_disable();


//             let holder = OSHolder::os_holder_mut();

//             if !holder.active[holder.current_program] {
//                 println!("Current program not active");
//                 virtmem::mmu_enable();
//                 return;
//             }
            
//             update_current_program_frame(frame);
//             let next_program_index = holder.get_next_active_program_index(holder.current_program);
//             println!("Timer interrupt, moving from program {} -> {}", holder.current_program, next_program_index);
//             holder.current_program = next_program_index;
//             holder.map_program_mmu(holder.current_program);

//             virtmem::mmu_enable();

//             let mapped_program_ptr = 0x0000_0000 as *mut holder::Program;
//             let mapped_program = unsafe { &mut *mapped_program_ptr };
//             let mapped_next_frame: InterruptFrame = mapped_program.frame;

//             let return_pc = mapped_next_frame.lr;
//             let return_sp = mapped_program.sp as u32;
//             let return_frame_ptr = &mapped_program.frame as *const InterruptFrame;

//             println!("Returning to program: pc={:x}, sp={:x}", return_pc, return_sp);
//             unsafe {
//                 print!("instr bytes:");
//                 for i in 0..8 {
//                     let b = *((return_pc as *const u8).add(i));
//                     print!(" {:02x}", b);
//                 }
//                 println!();

//                 print!("stack words at sp:");
//                 for i in 0..8 {
//                     let w = *((return_sp as *const u32).add(i));
//                     print!(" {:08x}", w);
//                 }
//                 println!();

//                 println!("timer back regs: r0={:x} r1={:x} r2={:x} r3={:x} r4={:x} r5={:x} r6={:x} r7={:x}",
//                     mapped_program.frame.r0, mapped_program.frame.r1, mapped_program.frame.r2, mapped_program.frame.r3,
//                     mapped_program.frame.r4, mapped_program.frame.r5, mapped_program.frame.r6, mapped_program.frame.r7);
//                 println!("timer back regs cont: r8={:x} r9={:x} r10={:x} r11={:x} r12={:x} lr={:x}",
//                     mapped_program.frame.r8, mapped_program.frame.r9, mapped_program.frame.r10, mapped_program.frame.r11,
//                     mapped_program.frame.r12, mapped_program.frame.lr);
//             }

//             fork_trampoline_back(return_pc, return_sp, return_frame_ptr);
//         } else {
//             println!("MMU disabled, skipping");
//         }
//     }
// }

#[unsafe(no_mangle)]
pub extern "C" fn fast_interrupt_vector(pc: u32) {
    println!("Fast interrupt vector!");
}

#[unsafe(no_mangle)]
pub extern "C" fn os_undefined_instruction_vector(frame: *mut InterruptFrame, pc: u32) {
    unsafe {
        let frame = unsafe { &mut *frame };
        println!("Undefined instruction at pc={:#x}, inst={:#x}", pc, *(pc as *const u32));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn os_data_abort_vector(frame: *mut InterruptFrame, pc: u32) {
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
        profiler::breakpoint_mismatch_set(pc);
    }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn software_interrupt_vector(frame: *mut InterruptFrame, svc_lr: u32) -> u32 {
    syscalls::handle_software_interrupt(frame, svc_lr)
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



pub fn run_test_interrupt() {
    let mut r0: u32 = 1; // for standard out
    let test_str = "testing interrupt\n";

    println!("before executing the actual asm instruction, string address: {:p}", core::ptr::addr_of!(test_str));

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
}

pub fn test_interrupts_vbar() {
    unsafe {
        install_interrupts_vbar();
        switch_to_user_mode();
        run_test_interrupt();
    }
}

pub fn test_interrupts_vbar_vmem() {
    unsafe {
        virtmem::mmu_reset();
        
        let user = virtmem::MemPerm::perm_rw_user;
        let dev = virtmem::make_global_pin(holder::DOM_KERN, user, virtmem::MemAttr::MEM_device, virtmem::PageSizes::mb16);
        let kern = virtmem::make_global_pin(holder::DOM_KERN, user,virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);

        let ONE_MB = 1024 * 1024;

        install_interrupts_vbar();

        // Peripherals
        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);

        // virtmem::pin_mmu_sec(1, 0x0, 0x0, kern);
        // Kernel memory mappings (identity)
        virtmem::pin_mmu_sec(3, 0x1000_0000, 0x1000_0000, kern);
        virtmem::pin_mmu_sec(4, 0x1000_0000 + 16 * ONE_MB as u32, 0x1000_0000 + 16 * ONE_MB as u32, kern);

        // VBAR helpers
        virtmem::pin_mmu_sec(5, VBAR as u32, VBAR as u32, kern);

        // Stack region
        virtmem::pin_mmu_sec(6, 0x1800_0000 - 16 * ONE_MB as u32, 0x1800_0000 - 16 * ONE_MB as u32, kern);

        virtmem::pin_mmu_init(!0);

        // profiler::breakpoint_mismatch_start();

        virtmem::mmu_enable();

        println!("enabled!");

        let vbar: u32;
        unsafe {
            core::arch::asm!("mrc p15, 0, {}, c12, c0, 0", out(reg) vbar);
        }
        println!("VBAR = 0x{:08x}", vbar);


        switch_to_user_mode();

        println!("switched to user mode");

        run_test_interrupt();

        println!("Finished the interrupt test!");
        // virtmem::mmu_disable(); -> this doesn't work because you are not in special mode 
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
    run_test_interrupt();
    // println!("Stack pointer: {:0x}", get_stack_pointer());

    // report();

    // here print out the stack
    
    // switch_to_super_mode();
    
    // unsafe { disable_interrupts_asm(); }
    // println!("returned from SWI instruction {}", r0);

    // println!("passing value test: {}", ret);
    // println!("disabled interrupts, svc write returned: {}", r0 as i32);
}

pub fn enable_timer_interrupts() {
    unsafe {
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
}

pub fn verify_timer_setup() {
    unsafe {
        let control_value = ARM_TIMER_CTRL_32BIT |
                           ARM_TIMER_CTRL_ENABLE |
                           ARM_TIMER_CTRL_INT_ENABLE |
                           ARM_TIMER_CTRL_PRESCALE_256;
        
        let ctrl_readback = get32(ARM_TIMER_CONTROL as u32);
        let load_readback = get32(ARM_TIMER_LOAD as u32);
        let irq_enabled = get32(IRQ_ENABLE_BASIC as u32);
        
        println!("Timer Setup Verification:");
        println!("  Control register: {:#010x} (expected: {:#010x})", 
                 ctrl_readback, control_value);
        println!("  Load value: {} (expected: 1000)", load_readback);
        println!("  IRQ enabled bits: {:#010x} (expected: {:#010x})", 
                 irq_enabled, ARM_TIMER_IRQ);
        
        if ctrl_readback != control_value {
            panic!("  Control register mismatch!");
        }
        if load_readback != LOAD_PERIOD {
            panic!("  Load value mismatch!");
        }
        if (irq_enabled & ARM_TIMER_IRQ) == 0 {
            panic!("  Timer IRQ not enabled in controller!");
        }
        
        let timer_val1 = get32(ARM_TIMER_VALUE as u32);
        for _ in 0..10000 { asm!("nop"); }
        let timer_val2 = get32(ARM_TIMER_VALUE as u32);
        
        println!("  Timer value 1: {}", timer_val1);
        println!("  Timer value 2: {}", timer_val2);
        
        let pending = get32(IRQ_BASIC_PENDING as u32);
        println!("  Pending IRQs: {:#010x}", pending);
        if (pending & ARM_TIMER_IRQ) != 0 {
            println!("  Timer interrupt is PENDING!");
        }
    }
}
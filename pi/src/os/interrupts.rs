use crate::arch::{dev_barrier, gcc_mb};
use crate::gpio::{read, set_input, set_output, set_on, set_off};
use crate::{bit_utils, print, start};
use crate::println;
use crate::timer::{Timer};
use core::arch::{asm, global_asm};
use crate::mem::{get32, put32};
use crate::gpio;
use crate::os::elf_loader;
use crate::profiler;
use crate::fat32::{self, fs_manager};

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
    mov sp, 0x8800000
    sub lr, lr, #4                              @ adjust lr to point to faulting instruction
    push {{r0-r12, lr}}

    mov r0, sp                                  @ frame pointer
    mov r1, lr                                  @ faulting pc

    bl os_undefined_instruction_vector

    pop {{r0-r12, lr}}
    movs pc, lr

software_interrupt_asm:                         @ A2-20
    cpsid i
    mov sp, 0x8800000
    push {{r0-r12, lr}}

    mov r0, sp
    mov r1, lr

    bl software_interrupt_vector

    str r0, [sp]
    pop {{r0-r12, lr}}
    movs pc, lr
prefetch_abort_asm:
    mov sp, 0x8b00000 @ needs to be different...
    sub lr, lr, #4
    push {{r0-r12, lr}}

    mov r0, sp                                  @ frame pointer
    mov r1, lr                                  @ faulting pc

    bl os_prefetch_abort_vector

    pop {{r0-r12, lr}}
    movs pc, lr
data_abort_asm:
    mov sp, 0x8800000
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
  mov sp, 0x8800000   @ Q: what if you delete?
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
    mov r0, 0
    mcr p15, 0, r0, c7, c5, 4
    mov pc, lr
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

#[unsafe(no_mangle)]
pub extern "C" fn os_undefined_instruction_vector(frame: *mut SoftwareInterruptFrame, pc: u32) {
    unsafe {
        let frame = unsafe { &mut *frame };
        println!("Undefined instruction at pc={:#x}, inst={:#x}", pc, *(pc as *const u32));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn os_data_abort_vector(frame: *mut SoftwareInterruptFrame, pc: u32) {
    unsafe { 
        let far: u32;
        core::arch::asm!("mrc p15, 0, {}, c6, c0, 0", out(reg) far);
        let instr = *(pc as *const u32);
        println!("data abort at pc={:#x}, fault address: {:#x}, instr: {:#x}", pc, far, instr);
    }
}

// #[unsafe(no_mangle)]
// pub extern "C" fn os_prefetch_abort_vector(frame: *mut SoftwareInterruptFrame, pc: u32) {
//     println!("Prefetch abort vector at pc:{:#x}", pc);
// }


#[unsafe(no_mangle)]
pub extern "C" fn os_prefetch_abort_vector(frame: *mut SoftwareInterruptFrame, pc: u32) {
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

#[repr(C)]
pub struct SoftwareInterruptFrame {
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
    r4: u32,
    r5: u32,
    r6: u32,
    r7: u32,
    r8: u32,
    r9: u32,
    r10: u32,
    r11: u32,
    r12: u32,
    lr: u32,
}

const ENOSYS: u32 = (-38i32) as u32;
const EINVAL: u32 = (-22i32) as u32;
const ENOENT: u32 = (-2i32) as u32;
const EACCES: u32 = (-13i32) as u32;
const CURRENT_TID: u32 = 1;

static mut PROGRAM_BREAK: u32 = 0;
static mut THREAD_POINTER: u32 = 0;
static mut CLEAR_CHILD_TID: u32 = 0;

unsafe fn set_tls(tls: u32) {
    unsafe { THREAD_POINTER = tls };
    unsafe {
        asm!(
            "mcr p15, 0, {tls}, c13, c0, 3",
            tls = in(reg) tls,
        );
    }
}

fn set_tid_address(tidptr: u32) -> u32 {
    unsafe { CLEAR_CHILD_TID = tidptr };
    CURRENT_TID
}

unsafe fn exit_current_process() {
    let tidptr = unsafe { CLEAR_CHILD_TID };
    if tidptr != 0 {
        unsafe { core::ptr::write_volatile(tidptr as *mut u32, 0) };
    }
    unsafe { elf_loader::elf_loader_return(); }
}

static mut count: u32 = 0;

pub unsafe fn c_str_to_str(ptr: *const u8) -> &'static str {
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let bytes = core::slice::from_raw_parts(ptr, len);
    core::str::from_utf8_unchecked(bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn software_interrupt_vector(frame: *mut SoftwareInterruptFrame, svc_lr: u32) -> u32 {
    dev_barrier();

    // For SVC, lr points to the next instruction, so SVC is at lr - 4.
    let svc_pc = svc_lr.wrapping_sub(4);
    let instr = unsafe { core::ptr::read_volatile(svc_pc as *const u32) };
    let imm = instr & 0x00ff_ffff;
    let frame = unsafe { &mut *frame };

    let nr = if imm == 0 {
        frame.r7
    } else if (imm & 0x00ff_0000) == 0x0090_0000 {
        imm - 0x0090_0000
    } else {
        imm
    };

    println!(
        "SWI called: pc={:#x}, instr={:#x}, arg0={:#x}, arg1={:#x}, arg2={:#x}, arg3={:#x}, arg4={:#x}, arg5={:#x}, nr={:#x}",
        svc_pc, instr, frame.r0, frame.r1, frame.r2, frame.r3, frame.r4, frame.r5, nr
    );

    let ret = match nr {
        0x1 => {
            unsafe { exit_current_process(); }
            0
        }
        0x3 => {
            let fd = frame.r0;
            let buf_ptr = frame.r1 as *mut u8;
            let len = frame.r2 as usize;
            if fd != 0 {
                EINVAL
            } else if len == 0 {
                0
            } else if buf_ptr.is_null() {
                EINVAL
            } else {
                let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
                crate::uart::read_bytes(buf) as u32
            }
        }
        0x4 => {
            let fd = frame.r0;
            let buf_ptr = frame.r1 as *const u8;
            let len = frame.r2 as usize;
            if (fd == 1 || fd == 2) && !buf_ptr.is_null() {
                println!("writing out with fd={}, buf_ptr={:p}, len={}", fd, buf_ptr, len);
                let bytes = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
                crate::uart::write_bytes("[prog]".as_bytes());
                crate::uart::write_bytes(bytes);
                crate::uart::write_bytes("[/prog]".as_bytes());
                crate::uart::flush();
                len as u32
            } else {
                EINVAL
            }
        }
        0x2d => unsafe {
            if PROGRAM_BREAK == 0 {
                PROGRAM_BREAK = crate::kmalloc::kmalloc_aligned(4096, 4096) as u32;
            }
            if frame.r0 != 0 {
                PROGRAM_BREAK = frame.r0;
            }
            PROGRAM_BREAK
        },
        0x92 => unsafe {
            let fd = frame.r0;
            let iov = frame.r1 as *const u32;
            let iovcnt = frame.r2 as usize;
            if fd != 1 && fd != 2 {
                EINVAL
            } else {
                let mut total: u32 = 0;
                for i in 0..iovcnt {
                    let base = unsafe { core::ptr::read_volatile(iov.add(i * 2)) } as *const u8;
                    let len  = unsafe { core::ptr::read_volatile(iov.add(i * 2 + 1)) } as usize;
                    if !base.is_null() && len > 0 {
                        let bytes = unsafe { core::slice::from_raw_parts(base, len) };
                        if (fd == 1 || fd == 2){
                            crate::uart::write_bytes("[prog]".as_bytes());
                        }
                        crate::uart::write_bytes(bytes);
                        if (fd == 1 || fd == 2){
                            crate::uart::write_bytes("[/prog]".as_bytes());
                        }
                        total = total.wrapping_add(len as u32);
                    }
                }
                crate::uart::flush();
                total as u32
            }
        },
        0xc0 => unsafe {
            let len = frame.r1 as usize;
            let ptr = crate::kmalloc::kmalloc_aligned(len, 4096);
            core::ptr::write_bytes(ptr, 0, len);
            // println!("mmap2 returning {:#x}", ptr);
            ptr as u32
        },
        0x14 => 0,
        0xf0005 => {
            unsafe { set_tls(frame.r0); }
            0
        }
        0x100 => set_tid_address(frame.r0),
        0xf8 => {
            unsafe { exit_current_process(); }
            0
        },
        0xae => CURRENT_TID,
        0xc9 => {
            println!("exit_group called with code {}", frame.r0);
            0
            // loop {}
        },
        0xb7 => {
            let buf = frame.r0 as *mut u8;
            unsafe {
                *buf = b'/';
                *buf.add(1) = 0;
            }
            frame.r0
        },
        0x36 => 0,
        0x40 => 0,
        0x18d => unsafe {
            let _dirfd = frame.r0;
            let pathname_bytes = frame.r1 as *mut u8;
            let _flags = frame.r2;
            let _mask = frame.r3;
            let statx_out = frame.r4 as *mut fs_manager::Statx;

            let mut filename_len = 0;
            while *(pathname_bytes.add(filename_len)) != 0 && filename_len < 256 {
                filename_len += 1;
            }
            let filename_slice = core::slice::from_raw_parts(pathname_bytes, filename_len);
            let filename = core::str::from_utf8(filename_slice).unwrap_or("");

            let fs_manager = fs_manager::get_fat32_manager();
            let stat_ptr = fat32::fat32_stat(&(*fs_manager).fs, &(*fs_manager).root, filename);
            if stat_ptr.is_null() {
                ENOENT
            } else {
                (*statx_out) = (*fs_manager).get_file_stat(filename);
                0
            }
        },
        _ => {
            println!("unknown SVC: {:#x}", nr);
            ENOSYS
        }
    };

    dev_barrier();
    ret
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

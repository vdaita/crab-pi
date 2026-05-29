#![feature(sync_unsafe_cell)]

use crate::os::interrupts::move_table;
use crate::os::{interrupts, virtmem};
use crate::os::virtmem::{MemAttr, PageSizes};
use crate::{println, print};
use crate::circular::{CircularQueue};
use crate::os::elf_loader;
use crate::profiler;
use crate::fat32::{self, Fat32Manager, fs_manager};
use core::ffi::{CStr, c_char};
use crate::kmalloc;
use core::arch::asm;
use std::cell::SyncUnsafeCell;

const DOM_KERN: u32 = 1;
const DOM_USER: u32 = 2;
const TINY_PAGE: usize = 4 * 1024;


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
"#,
    SUPER_MODE = const CPSR_SUPER_MODE
);

unsafe extern "C" {
    #[link_name = "_interrupt_table"]
    pub static INTERRUPT_TABLE_START: u8;

    #[link_name = "_interrupt_table_end"]
    pub static INTERRUPT_TABLE_END: u8;
}

#[repr(C)]
struct ElfHeader {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32, // version
    e_entry: usize, // this is the memory address of where the process starts executing
    e_phoff: usize, // points to the start of the program header table
    e_shoff: usize, // point to start of section header table
    e_flags: u32, // depends on target arch
    e_ehsize: u16, // size of this header
    e_phentsize: u16, // size of program header table entry
    e_phnum: u16, // number of entries in the program header table
    e_shentsize: u16, // size of a section header table entry
    e_shnum: u16, // contains number of entries in the section header table
    e_shstrndx: u16 // index of the section header table entry that contains the section names
}

#[repr(C)]
struct ProgramHeader {
    p_type: u32, // identifies the type of this segment
    p_offset: usize, // offset of this segment in the file image
    p_vaddr: usize, // virtual address of this segment in memory
    p_paddr: usize, // physical address of this segment (if relevant)
    p_filesz: usize, // size of this segment in the file image
    p_memsz: usize, // size of this segment in memory
    p_flags: u32, // segment dependent flags
    p_align: u32 // required alignment of this segment
}

#[repr(C)]
struct SectionHeader {
    sh_name: u32, // offset to section name in the section name string table
    sh_type: u32, // identifies the type of this section header
    sh_flags: u32, // identifies attributes of this section
    sh_addr: u32, // virtual address of this section in memory
    sh_offset: u32, // offset of this section in the file image
    sh_size: u32, // size of this section in bytes
    sh_link: u32, // section index link value (type-dependent meaning)
    sh_info: u32, // extra section information (type-dependent meaning)
    sh_addralign: u32, // required alignment of this section
    sh_entsize: u32 // size of entries if section has fixed-size entries
}

#[repr(C)]
struct ProgramContext {
    user_stack: u32,
    entry: u32,
    arg0: u32,
    arg1: u32,
    arg2: u32,
}

const NUM_PROGRAMS: usize = 8;
const MAX_ELF_SIZE: usize = 1024 * 1024; // binary can take up at most 1MB
const MAX_STACK_SIZE: usize = 1024 * 64; // stack can take up at most 64KB
const MAX_HEAP_SIZE: usize = 16 * 1024 * 1024; // heap can take up at most 2MB only 

#[derive(Copy, Clone)]
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


#[derive(Copy, Clone, Default)]
struct Program {
    #[repr(align(MAX_ELF_SIZE))]
    elf: [u8; MAX_ELF_SIZE],
    #[repr(align(MAX_STACK_SIZE))]
    stack: [u8; MAX_STACK_SIZE],
    #[repr(align(MAX_HEAP_SIZE))]
    heap: [u8; MAX_HEAP_SIZE],

    sp: usize,
    heap_ptr: usize,
    tid: u32,
    frame: SoftwareInterruptFrame
}

impl Program {
    const fn new() -> Self {
        Self { 
            elf: [0; MAX_ELF_SIZE],
            stack: [0; MAX_STACK_SIZE],
            heap: [0; MAX_HEAP_SIZE],
            sp: 0,
            heap_ptr: 0,
            tid: 0
        }
    }
}

#[derive(Copy, Clone, Default)]
struct OSHolder {
    programs: [Program; NUM_PROGRAMS],
    #[repr(align(TINY_PAGE))]
    interrupt_vector_base: [u8; TINY_PAGE],
    #[repr(align(TINY_PAGE))]
    kuser_helpers: [u8; TINY_PAGE],
    num_programs: usize
}

unsafe fn kuser_get_tls() -> u32 {
    let tls: u32;
    core::arch::asm!(
        "mrc p15, 0, {tls}, c13, c0, 3",
        tls = out(reg) tls,
        options(nostack)
    );
    tls
}

unsafe fn kuser_cmpxchg(newval: u32, ptr: *mut u32) -> u32 {
    // Simple swap: load old value, store new value, return old value
    let old: u32;
    core::arch::asm!(
        "ldr {old}, [{ptr}]",
        "str {newval}, [{ptr}]",
        ptr = in(reg) ptr,
        newval = in(reg) newval,
        old = out(reg) old,
        options(nostack)
    );
    old
}

unsafe fn kuser_memory_barrier() {
    core::arch::asm!(
        "mcr p15, 0, {r0}, c7, c10, 5",
        r0 = in(reg) 0u32,
        options(nostack)
    );
}

unsafe fn kuser_version() -> u32 {
    return 5;
}

// for each of the waitpid syscalls, you can just assume that fork runs through the entire program and that you have no real threading that demands switching your timer or handing things off via a scheduler

impl OSHolder {
    fn new() -> Self {
        let os_holder = Self {
            programs: [Program::default(); NUM_PROGRAMS], 
            num_programs: 0,
            kuser_helpers: [u8; TINY_PAGE],
            interrupt_vector_base: [u8; TINY_PAGE]
        };

        // copy over the kuser helpers
        let pa = os_holder.kuser_helpers.as_mut_ptr() as u32;
        
        // __kernel_get_tls at VA 0xFFFF0FA0
        core::ptr::copy_nonoverlapping(
            kuser_get_tls as *const u32,
            (pa + 0x00FF0FA0) as *mut u32, 4);

        // __kernel_cmpxchg at VA 0xFFFF0FC0
        core::ptr::copy_nonoverlapping(
            kuser_cmpxchg as *const u32,
            (pa + 0x00FF0FC0) as *mut u32, 8);

        // __kernel_memory_barrier at VA 0xFFFF0FE0
        core::ptr::copy_nonoverlapping(
            kuser_memory_barrier as *const u32,
            (pa + 0x00FF0FE0) as *mut u32, 2);

        // __kernel_version at VA 0xFFFF0FFC
        core::ptr::copy_nonoverlapping(
            kuser_version as *const u32,
            (pa + 0x00FF0FFC) as *mut u32, 4);

        // copy over the interrupt table
        move_table(INTERRUPT_TABLE_START, INTERRUPT_TABLE_END);

        os_holder
    }

    fn load_elf(self, prog_name: &str, program_index: usize) {
        unsafe {
            let file_manager: *mut Fat32Manager = get_fat32_manager();
            let file: *mut pi_file_t = (*file_manager).read_file(prog_name);
            let elf_header_ptr: *mut ElfHeader = (*file).data as *mut ElfHeader;
            let elf_header = *elf_header_ptr;
            let first_prog_header_ptr: *mut ProgramHeader = (*file).data.byte_add(elf_header.e_phoff) as *mut ProgramHeader;
            
            for prog_header_idx in 0..elf_header.e_phnum {
                let prog_header_ptr: *mut ProgramHeader = first_prog_header_ptr.add(prog_header_idx as usize);
                let prog_header = *prog_header_ptr;
                
                if prog_header.p_type != 1 {
                    continue;
                }

                core::ptr::copy_nonoverlapping(
                    ((*file).data as *mut u8).add(prog_header.p_offset),
                    self.programs[program_index].elf.as_mut_ptr().byte_add(prog_header.p_offset),
                    prog_header.p_filesz
                );

                // shouldn't need to set bss because it was already set when initializing everything
            }
        }
    }

    fn map_program_mmu(self, program_index: usize) {
        virtmem::mmu_disable();
        virtmem::mmu_reset();
        let user = MemPerm::perm_rw_user;
        
        let dev_pin_mb16 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_device, PageSizes::mb16);
        let kern_pin_mb16 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_cached, PageSizes::mb16);
        let kern_pin_kb4 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::kb4);

        let user_pin_mb16 = virtmem::make_user_pin(DOM_USER, program_index as u32 + 1, user, MemAttr::MEM_cached,PageSizes::mb16);
        let user_pin_mb1 = virtmem::make_user_pin(DOM_USER, program_index as u32 + 1, user, MemAttr::MEM_cached, PageSizes::mb1);
        let user_pin_kb64 = virtmem::make_user_pin(DOM_USER, program_index as u32 + 1, user, MemAttr::MEM_cached, PageSizes::kb64);
        

        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev_pin_mb16); // pin the device memory

        let vbar = 0x1800_0000;
        unsafe {
            asm!("mcr p15, 0, {0}, c12, c0, 0", in(reg) vbar, options(nostack, preserves_flags));
        }
        virtmem::pin_mmu_sec(1, vbar, self.interrupt_vector_base.as_mut_ptr() as u32, kern_pin_kb4); // map the interrupt table
        

        virtmem::pin_mmu_sec(2, 0x1000_0000, self.programs[program_index].heap.as_mut_ptr() as u32, user_pin_mb16); // pin the heap
        virtmem::pin_mmu_sec(3, 0x0900_0000 - 1024 * 1024, self.programs[program_index].stack.as_mut_ptr() as u32, user_pin_mb1); // pin the stack
        
        // pin kuser
        virtmem::pin_mmu_sec(4, 0xff00_0000, self.kuser_helpers.as_mut_ptr() as u32, kern_pin_kb4); // map the kuser helpers;
    }

    fn run_elf(self, program_index: usize, args) { // TODO: implement the arguments in this
        // update the stack variable for this

        switch_to_user_mode();
        elf_loader_tramp(...);
        // note: the program will finish here after it has finished running
    }

    fn switch_to_program(self, program_index: usize) {
        // map the program w/ mmu
        // launch the trampoline to context switch back into it
    } 

}

static os_holder: SyncUnsafeCell<OSHolder> = SyncUnsafeCell::new(OSHolder::new());

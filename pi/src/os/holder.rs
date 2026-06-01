use core::arch::global_asm;
use crate::arch::dev_barrier;
use crate::mem::{get32, put32};
use crate::os::{interrupts, kuser};
use crate::os::virtmem::{self, MemPerm, MemAttr, PageSizes, mmu_disable, mmu_enable};
use crate::{println, print};
use crate::circular::CircularQueue;
use crate::profiler;
use crate::fat32::{self, Fat32Manager, fs_manager, get_fat32_manager, pi_file_t};
use crate::kmalloc;
use core::cell::SyncUnsafeCell;
use core::arch::asm;
use core::mem::MaybeUninit;
use crate::os::elf_file::{ElfHeader, ProgramHeader, SectionHeader};
use crate::os::interrupts::{InterruptFrame};

unsafe impl Sync for OSHolder {}

pub static OS_HOLDER: SyncUnsafeCell<MaybeUninit<OSHolder>> = 
    SyncUnsafeCell::new(MaybeUninit::zeroed());

pub const DOM_KERN: u32 = 1;
const DOM_USER: u32 = 2;
const TINY_PAGE: usize = 4 * 1024;
const LARGE_PAGE: usize = 16 * 1024 * 1024;
const ONE_MB: usize = 1024 * 1024;
const NUM_PROGRAMS: usize = 3;
const MAX_ELF_SIZE: usize = 1024 * 1024 * 4;
const MAX_STACK_SIZE: usize = 1024 * 1024 * 4;
const MAX_HEAP_SIZE: usize = 1024 * 1024;


#[derive(Copy, Clone)]
pub struct ELF {
    pub data: [u8; MAX_ELF_SIZE],
}

#[derive(Copy, Clone)]
pub struct Stack {
    pub data: [u8; MAX_STACK_SIZE],
}

#[derive(Copy, Clone)]
pub struct Heap {
    pub data: [u8; MAX_HEAP_SIZE],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Program {
    pub elf: ELF,
    pub stack: Stack,
    pub heap: Heap,
    pub frame: InterruptFrame,
    pub sp: usize,
    pub heap_ptr: usize,
    pub tid: u32,
    pub elf_header: ElfHeader,
    pub elf_base: usize,

    // for when the program returns
    pub return_sp: usize,
    pub return_lr: usize
}

#[repr(C)]
pub struct ProgramContext {
    pub user_stack: u32,
    pub entry: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
}

global_asm!(r#"
.globl elf_loader_tramp
.type elf_loader_tramp, %function
elf_loader_tramp:
    str sp, [r1]
    str lr, [r2]

    ldr r1, [r0]        @ stack pointer
    mov sp, r1

    ldr r1, [r0, #4]    @ entry point
    mov lr, r1

    ldr r2, [r0, #16]   @ third argument
    ldr r1, [r0, #12]   @ second argument
    ldr r0, [r0, #8]    @ first argument

    bx lr

.globl elf_loader_return
.type elf_loader_return, %function
elf_loader_return:
    mov sp, r0
    mov lr, r1
    bx lr
"#,
);

unsafe extern "C" {
    pub fn elf_loader_tramp(data: *mut ProgramContext, return_sp: *mut usize, return_lr: *mut usize);
    pub fn elf_loader_return(return_sp: usize, return_lr: usize);
}

#[repr(C)]
pub struct OSHolder {
    pub programs: [*mut Program; NUM_PROGRAMS],
    pub current_program: usize,
    pub active: [bool; NUM_PROGRAMS],
    
    pub should_cswitch: bool

}

unsafe fn hexdump(ptr: *const u8, lines: u32) {
    for i in 0..lines {
        for j in 0..8 {
            print!("{:0x} ", *(ptr.byte_add(8*i as usize + j)));
        }
        println!();
    }
}

pub fn get_user_sp() -> u32 {
    let mut user_sp: u32 = 0;
    unsafe {
        asm!(
            "str sp, [{tmp}]",
            "stm {tmp}, {{sp}}^",
            "ldr {sp}, [{tmp}]",
            tmp = in(reg) &user_sp as *const u32,
            sp = out(reg) user_sp,
        );
    }
    user_sp
}

impl OSHolder {
    pub unsafe fn os_holder_mut() -> &'static mut OSHolder {
        &mut *OS_HOLDER.get().cast::<OSHolder>()
    }

    pub fn init() {
        unsafe {
            core::ptr::write(OS_HOLDER.get().cast::<OSHolder>(), core::mem::zeroed());
            kuser::install_kuser_helpers();
            interrupts::install_interrupts_vbar();
            // interrupts::enable_timer_interrupts();         // -> this shit is causing me hell    

            let holder = OSHolder::os_holder_mut();

            // initialize program pointers
            for i in 0..NUM_PROGRAMS {
                holder.programs[i] = (0x0200_0000 * i + 0x0100_0000) as *mut Program; // this is actually the address that they are supposed to be mapped to 
                // note: these memory addresses are properly aligned - 
                core::ptr::write_bytes(
                    (holder.programs[i] as *mut u8),
                    0,
                    core::mem::size_of::<Program>()
                );
                holder.active[i] = false;
            }            

            for i in 0..NUM_PROGRAMS {
                println!("Program {} has memory location {:p}, active={}",
                    i, holder.programs[i], holder.active[i]);
            }
        }
    }

    pub unsafe fn get_next_active_program_index(&mut self, index: usize) -> usize {
        for offset in 1..(NUM_PROGRAMS + 1) {
            let next_index = (index + offset) % NUM_PROGRAMS;
            if self.active[next_index] {
                return next_index;
            }
        }
        panic!("Somehow, nothing is active.");
    }

    pub unsafe fn get_program_mut(&mut self, index: usize) -> &'static mut Program {
        &mut *self.programs[index]
    }

    pub unsafe fn get_program(&self, index: usize) -> &'static Program {
        &*self.programs[index]
    }

    pub unsafe fn get_next_empty_index(&self) -> usize {
        for i in 0..NUM_PROGRAMS {
            if !self.active[i] {
                return i
            }
        }
        panic!("out of program slots!");
    }

    pub fn map_program_mmu(&mut self, program_index: usize) {
        virtmem::mmu_reset();

        let user = MemPerm::perm_rw_user;
        let dev = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_device, PageSizes::mb16);
        let kern = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::mb16);


        // Peripherals
        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);

        let program_addr = self.programs[program_index] as u32;

        // Program index memory mapping
        // virtmem::pin_mmu_sec(1, 00, 0x0000, kern);
        virtmem::pin_mmu_sec(1, 0x0000, program_addr, kern);
        virtmem::pin_mmu_sec(2, 0x0000 + 16 * ONE_MB as u32, program_addr + 16 * ONE_MB as u32, kern);
        
        // Kernel memory mappings (identity)
        virtmem::pin_mmu_sec(3, 0x1000_0000, 0x1000_0000, kern);
        virtmem::pin_mmu_sec(4, 0x1000_0000 + 16 * ONE_MB as u32, 0x1000_0000 + 16 * ONE_MB as u32, kern);

        // VBAR helpers
        virtmem::pin_mmu_sec(5, interrupts::VBAR as u32, interrupts::VBAR as u32, kern);

        // Stack region
        virtmem::pin_mmu_sec(6, 0x1800_0000 - 16 * ONE_MB as u32, 0x1800_0000 - 16 * ONE_MB as u32, kern);

        // KUSER helpers
        virtmem::pin_mmu_sec(7, 0xff000000, kuser::KUSER_ADDR as u32, kern);
        virtmem::pin_mmu_init(!0);
    }

    pub fn run_elf(&mut self, program_index: usize, prog_name: &str) {
        unsafe {
            interrupts::disable_interrupts_asm();

            kmalloc::ensure_init();

            println!("Setting up MMU for program {}", program_index);
            self.map_program_mmu(program_index);
            
            dev_barrier();
            println!("About to enable MMU");
            virtmem::mmu_enable();
            println!("MMU enabled");
            
            dev_barrier();
            
            let manager = get_fat32_manager();
            let file = (*manager).read_file(prog_name);

            println!("File size from FAT32: {}", (*file).n_data);
            hexdump((*file).data, 8);

            let program_ptr = 0x0000_0000 as *mut Program;
            let program: &mut Program = &mut *program_ptr;

            crate::os::elf_file::load_elf_into_program((*file).data as *const u8, program);

            println!("number of program headers: {}", program.elf_header.e_phnum);
            println!("Program entry point (physical): {:#x}", program.elf_header.e_entry);

            let program_addr = program_ptr as usize;
            let user_stack_base = (program.stack.data.as_ptr() as usize) - (program_addr as usize);
            println!("Program stack end: {:p}", program.stack.data.as_ptr());
            println!("Program base address: {:x}", program_addr);
            println!("User stack base: {:x}", user_stack_base);

            let argv0_bytes = b"sh\0";
            let argv0_heap = kmalloc::kmalloc(argv0_bytes.len()) as *mut u8;

            println!("Allocated heap for argv0_bytes: {:p}", argv0_heap);
            core::ptr::copy_nonoverlapping(argv0_bytes.as_ptr(), argv0_heap, argv0_bytes.len());

            let argv0_ptr = argv0_heap as u32;

            let stack_top = user_stack_base;
            println!("About to write to address: {:#x}", stack_top);
            core::ptr::write_bytes((stack_top - 1024) as *mut u8, 0, 1024);
            println!("User stack base just written to: {:#x}", stack_top);

            let mut sp = stack_top as *mut u32;
            let phdr_addr = (program.elf_base as u32).wrapping_add(program.elf_header.e_phoff as u32);
            
            sp = sp.sub(1); *sp = 0;                             // AT_NULL val
            sp = sp.sub(1); *sp = 0;                             // AT_NULL type
            sp = sp.sub(1); *sp = 4096;                          // AT_PAGESZ val
            sp = sp.sub(1); *sp = 6;                             // AT_PAGESZ type
            sp = sp.sub(1); *sp = program.elf_header.e_phnum as u32; // AT_PHNUM val
            sp = sp.sub(1); *sp = 5;                             // AT_PHNUM type
            sp = sp.sub(1); *sp = program.elf_header.e_phentsize as u32; // AT_PHENT val
            sp = sp.sub(1); *sp = 4;                             // AT_PHENT type
            sp = sp.sub(1); *sp = phdr_addr;                     // AT_PHDR val
            sp = sp.sub(1); *sp = 3;                             // AT_PHDR type
            sp = sp.sub(1); *sp = 0;

            // argv pointers: argv[0], NULL
            sp = sp.sub(1); *sp = 0;          // argv[1] == NULL
            sp = sp.sub(1); *sp = argv0_ptr;  // argv[0]

            // argc = 1
            sp = sp.sub(1); *sp = 1;

            if (sp as usize) & 7 != 0 {
                sp = sp.sub(1);
                *sp = 0;
            }

            println!("Finished constructing stack");

            let mut context = ProgramContext {
                user_stack: sp as u32,
                entry: (program.elf_header.e_entry) as u32,
                arg0: 1,                                 // r0 = argc
                arg1: (sp.add(1) as *const u32) as u32, // r1 = &argv[0]
                arg2: 0,                                 // r2 = envp (NULL)
            };

            println!("want to run the following instructions: ");
            hexdump(context.entry as *const u8, 8);

            println!("Jumping to entry point: {:#x}", context.entry);

            dev_barrier();

            let ret_sp_addr = core::ptr::addr_of_mut!((*program_ptr).return_sp);
            let ret_lr_addr = core::ptr::addr_of_mut!((*program_ptr).return_lr);
            println!("Location of where to return sp={:p}, return lr={:p}, program_location: {:p}", ret_sp_addr, ret_lr_addr, program_ptr);

            println!("About to switch to user mode from PC: {:p}, SP: {:p}", 
                interrupts::switch_to_user_mode as *const (),
                &stack_top as *const _
            );


            // interrupts::verify_timer_setup();

            let holder = OSHolder::os_holder_mut();
            holder.current_program = program_index;
            holder.active[program_index] = true;

            println!("Set variables in holder.");

            // profiler::breakpoint_mismatch_start();

            interrupts::switch_to_user_mode();
            println!("Switched to user mode");
            
            // interrupts::run_test_interrupt(); // expect the text to be off because buffer mismatch
            // interrupts::switch_to_user_mode();
            elf_loader_tramp(core::ptr::addr_of_mut!(context), core::ptr::addr_of_mut!(program.return_sp), core::ptr::addr_of_mut!(program.return_lr));
        }
    }
}

pub fn test_elf_holder() {
    unsafe {
        OSHolder::init();
        let holder = OSHolder::os_holder_mut();
        println!("About to run user program!");
        holder.run_elf(1, "BUSYBOX");
    }
}
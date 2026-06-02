use core::arch::global_asm;
use crate::arch::dev_barrier;
use crate::mem::{get32, put32};
use crate::os::{interrupts, kuser};
use crate::os::virtmem::{self, MemPerm, MemAttr, PageSizes, mmu_disable, mmu_enable};
use crate::{println, print};
use crate::circular::CircularQueue;
use crate::profiler;
use crate::fat32::{self, pi_file_t};
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
const ONE_MB: usize = 1024 * 1024;
const NUM_PROGRAMS: usize = 3;
const MAX_ELF_SIZE: usize = 1024 * 1024 * 4;
const MAX_STACK_SIZE: usize = 1024 * 1024 * 4;
const MAX_HEAP_SIZE: usize = 1024 * 1024 * 2;
pub const NUM_FILE_DESCRIPTORS: usize = 8;
// at least 12MB in reserved memory should be enough out of a 16MB page


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

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SpecialFileMarker {
    NotSpecial = 0, 
    Stdin = 1,  
    Stdout = 2,  
    Stderr = 3,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct KernelFile {
    pub active: bool,
    pub pos: usize,

    pub dirent: fat32::pi_dirent_t,
    pub data: *mut u8,
    pub nbytes: usize,
    pub nbytes_alloc: usize,
    pub is_directory: bool,
    pub dirents: *mut u8, // load the dirents at the same time as the regular listings

    pub parent: fat32::pi_dirent_t,
    pub special_file: SpecialFileMarker
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

    // elf loader return - should i rename this to not be confused with frame.lr, sp?
    pub return_sp: usize,
    pub return_lr: usize,

    pub file_descriptors: [KernelFile; NUM_FILE_DESCRIPTORS],
    pub cwd: fat32::pi_dirent_t
}

impl Program {
    pub fn allocate_file_descriptor(&mut self) -> usize {
        for i in 3..NUM_FILE_DESCRIPTORS {
            if !self.file_descriptors[i].active {
                self.file_descriptors[i] = unsafe { core::mem::zeroed() };
                self.file_descriptors[i].active = true;
                return i;
            }
        }
        panic!("no file descriptors available!"); // very dumb but will help with error hcecking
    }

    pub fn get_file(&mut self, fd: usize) -> &mut KernelFile {
        if fd >= NUM_FILE_DESCRIPTORS { 
            panic!("FD out of bounds");
        }
        &mut self.file_descriptors[fd]
    }
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
    
    pub should_cswitch: bool,

    pub files: [KernelFile; NUM_FILE_DESCRIPTORS],

    pub fs: fat32::fat32_fs_t,
    pub root: fat32::pi_dirent_t
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
            interrupts::enable_timer_interrupts();         // -> this shit is causing me hell 

            let holder = OSHolder::os_holder_mut();

            fat32::pi_sd_init();
            let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");
            holder.fs = fat32::fat32_mk(&partition);
            holder.root = fat32::fat32_get_root(&holder.fs);
            println!("Root is directory = {}", holder.root.is_dir_p);

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

    pub fn run_elf(&mut self, program_index: usize, prog_name: &str, args: &[&str], argc: usize) {
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
            
            let file = fat32::fat32_read(&self.fs, &self.root, prog_name);

            println!("File size from FAT32: {}", (*file).n_data);
            hexdump((*file).data, 8);

            let program_ptr = 0x0000_0000 as *mut Program;
            let program: &mut Program = &mut *program_ptr;
            program.cwd = self.root;
            println!("Program CWD is dir = {}, at address: {:p}", program.cwd.is_dir_p, core::ptr::addr_of!(program.cwd.is_dir_p));

            unsafe {
                for file in program.file_descriptors.iter_mut() {
                    *file = unsafe { core::mem::zeroed() }; 
                }
            }

            program.file_descriptors[0].special_file = SpecialFileMarker::Stdin;
            program.file_descriptors[1].special_file = SpecialFileMarker::Stdout;
            program.file_descriptors[2].special_file = SpecialFileMarker::Stderr;
            for i in 0..3 {
                program.file_descriptors[i].active = true;
            }

            crate::os::elf_file::load_elf_into_program((*file).data as *const u8, program);

            println!("number of program headers: {}", program.elf_header.e_phnum);
            println!("Program entry point (physical): {:#x}", program.elf_header.e_entry);

            let program_ptr = 0x0000_0000 as *mut Program;
            let program: &mut Program = &mut *program_ptr;
            let user_stack_base = (program.stack.data.as_ptr() as usize) - (program_ptr as usize);
            
            let mut arg_ptrs = [0u32; 16]; 

            for (i, arg_str) in args.iter().take(argc).enumerate() {
                let bytes = arg_str.as_bytes();
                let len = bytes.len() + 1;

                let heap_ptr = unsafe { crate::kmalloc::kmalloc(len) as *mut u8 };
                
                if heap_ptr.is_null() {
                    panic!("Out of memory while allocating argv[{}]", i);
                }

                unsafe {
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), heap_ptr, bytes.len());
                    *heap_ptr.add(bytes.len()) = 0;
                }

                arg_ptrs[i] = heap_ptr as u32;
            }

            let mut sp_words = user_stack_base as *mut u32;

            unsafe {
                let phdr_addr = (program.elf_base as u32).wrapping_add(program.elf_header.e_phoff as u32);
                let auxv = [
                    3, phdr_addr,
                    4, program.elf_header.e_phentsize as u32,
                    5, program.elf_header.e_phnum as u32,
                    6, 4096,
                    0, 0, // AT_NULL
                ];
                sp_words = sp_words.sub(auxv.len());
                core::ptr::copy_nonoverlapping(auxv.as_ptr(), sp_words, auxv.len());

                sp_words = sp_words.sub(1);
                *sp_words = 0; // envp[0] = NULL

                sp_words = sp_words.sub(1);
                *sp_words = 0; // argv[argc] = NULL

                for i in (0..argc).rev() {
                    sp_words = sp_words.sub(1);
                    *sp_words = arg_ptrs[i];
                }

                sp_words = sp_words.sub(1);
                *sp_words = argc as u32;
            }

            let mut context = ProgramContext {
                user_stack: sp_words as u32,
                entry: (program.elf_header.e_entry) as u32,
                arg0: argc as u32,                       // r0 = argc
                arg1: (sp_words.add(1) as *const u32) as u32, // r1 = argv
                arg2: 0,                                 // r2 = envp
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
                &sp_words as *const _
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

pub fn run_busybox() {
    unsafe {
        OSHolder::init();
        let holder = OSHolder::os_holder_mut();
        println!("About to run user program!");
        holder.run_elf(1, "BUSYBOX", &["sh"], 1);
    }
}
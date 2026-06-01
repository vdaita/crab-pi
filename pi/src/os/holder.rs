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

unsafe impl Sync for OSHolder {}

pub static OS_HOLDER: SyncUnsafeCell<MaybeUninit<OSHolder>> = 
    SyncUnsafeCell::new(MaybeUninit::zeroed());

const DOM_KERN: u32 = 1;
const DOM_USER: u32 = 2;
const TINY_PAGE: usize = 4 * 1024;
const LARGE_PAGE: usize = 16 * 1024 * 1024;
const ONE_MB: usize = 1024 * 1024;
const NUM_PROGRAMS: usize = 3;
const MAX_ELF_SIZE: usize = 1024 * 1024 * 12;
const MAX_STACK_SIZE: usize = 1024 * 1024 * 12;
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

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct SoftwareInterruptFrame {
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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Program {
    pub elf: ELF,
    pub stack: Stack,
    pub heap: Heap,
    pub sp: usize,
    pub heap_ptr: usize,
    pub tid: u32,
    pub active: bool,
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
    pub current_program: usize
}

fn print_elf_header(elf_header: ElfHeader) {
    println!("ELF header:");
    println!("  e_ident     = {:02x?}", elf_header.e_ident);
    println!("  e_type      = {:#06x}", elf_header.e_type);
    println!("  e_machine   = {:#06x}", elf_header.e_machine);
    println!("  e_version   = {:#010x}", elf_header.e_version);
    println!("  e_entry     = {:#010x}", elf_header.e_entry);
    println!("  e_phoff     = {:#010x}", elf_header.e_phoff);
    println!("  e_shoff     = {:#010x}", elf_header.e_shoff);
    println!("  e_flags     = {:#010x}", elf_header.e_flags);
    println!("  e_ehsize    = {:#06x}", elf_header.e_ehsize);
    println!("  e_phentsize = {:#06x}", elf_header.e_phentsize);
    println!("  e_phnum     = {:#06x}", elf_header.e_phnum);
    println!("  e_shentsize = {:#06x}", elf_header.e_shentsize);
    println!("  e_shnum     = {:#06x}", elf_header.e_shnum);
    println!("  e_shstrndx  = {:#06x}", elf_header.e_shstrndx);
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

pub fn mmu_identity_map_test() {
    virtmem::mmu_reset();
    let user = MemPerm::perm_rw_user;
    let dev = virtmem::make_global_pin(DOM_KERN, user, virtmem::MemAttr::MEM_device, virtmem::PageSizes::mb16);
    let kern = virtmem::make_global_pin(DOM_KERN, user, virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);
    let kern_1mb = virtmem::make_global_pin(DOM_KERN, user, virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb1);

    virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);
    virtmem::pin_mmu_sec(2, 0x1000_0000, 0x1000_0000, kern);
    virtmem::pin_mmu_sec(3, (0x1000_0000 + 16 * ONE_MB) as u32, (0x1000_0000 + 16 * ONE_MB) as u32, kern);
    virtmem::pin_mmu_sec(4, (0x1800_0000 - 16 * ONE_MB) as u32, (0x1800_0000 - 16 * ONE_MB) as u32, kern);

    unsafe { *(0x0650_0000 as *mut u8) = 10; }

    virtmem::pin_mmu_sec(5, 0x0500_0000, 0x0600_0000, kern);

    virtmem::pin_mmu_init(!0);
    println!("About to pin the identity test!");
    virtmem::mmu_enable();
    println!("MMU successfully enabled");

    unsafe { println!("testing out a memory access to: {}", *(0x0550_0000 as *mut u8)); }

    virtmem::mmu_disable();
    println!("Ok done");
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
            let holder = OSHolder::os_holder_mut();

            // initialize program pointers
            for i in 0..NUM_PROGRAMS {
                holder.programs[i] = (0x0200_0000 * i) as *mut Program; // this is actually the address that they are supposed to be mapped to 
                core::ptr::write_bytes(
                    (holder.programs[i] as *mut u8),
                    0,
                    core::mem::size_of::<Program>()
                );
            }            

            for i in 0..NUM_PROGRAMS {
                println!("Program {} has memory location {:p}, active={}",
                    i, holder.programs[i], (*holder.programs[i]).active);
            }

            interrupts::enable_interrupts_asm();
        }
    }

    pub unsafe fn get_program_mut(&mut self, index: usize) -> &'static mut Program {
        &mut *self.programs[index]
    }

    pub unsafe fn get_program(&self, index: usize) -> &'static Program {
        &*self.programs[index]
    }

    fn map_program_mmu(&mut self, program_index: usize) {
        virtmem::mmu_reset();

        let user = MemPerm::perm_rw_user;
        let dev = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_device, PageSizes::mb16);
        let kern = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::mb16);


        // Peripherals
        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);

        let program_addr = self.programs[program_index] as u32;

        // Program index memory mapping
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
            kmalloc::ensure_init();

            println!("Setting up MMU for program {}", program_index);
            self.map_program_mmu(program_index);
            
            dev_barrier();
            virtmem::mmu_enable();
            println!("MMU enabled");
            
            dev_barrier();
            
            let manager = get_fat32_manager();
            let file = (*manager).read_file(prog_name);

            println!("File size from FAT32: {}", (*file).n_data);
            hexdump((*file).data, 8);

            let elf_header_ptr: *mut ElfHeader = (*file).data as *mut ElfHeader;
            let first_program_header_ptr: *mut ProgramHeader =
                ((*file).data as *mut u8).add((*elf_header_ptr).e_phoff as usize) as *mut ProgramHeader;

            println!("number of program headers: {}", (*elf_header_ptr).e_phnum);
            println!("Program entry point (physical): {:#x}", (*elf_header_ptr).e_entry);

            let program_ptr = 0x0000_0000 as *mut Program;
            let program: &mut Program = &mut *program_ptr;
            program.active = true;
            program.elf_header = *elf_header_ptr;

            for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
                let program_header_ptr: *mut ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

                if (*program_header_ptr).p_type != 1 {  // PT_LOAD
                    continue;
                }

                let paddr = (program.elf.data.as_mut_ptr()).byte_add((*program_header_ptr).p_paddr as usize) as u32;
                println!("Loading segment: p_paddr={:#x} -> paddr={:#x}, filesz={}",
                    (*program_header_ptr).p_paddr, paddr, (*program_header_ptr).p_filesz);

                // Copy segment data
                core::ptr::copy_nonoverlapping(
                    ((*file).data as *mut u8).add((*program_header_ptr).p_offset as usize),
                    paddr as *mut u8,
                    (*program_header_ptr).p_filesz as usize,
                );

                println!("Finished copying.");

                // Zero BSS (uninitialized data)
                let bss_start = (paddr as *mut u8).add((*program_header_ptr).p_filesz as usize);
                let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
                if bss_size > 0 {
                    core::ptr::write_bytes(bss_start, 0, bss_size as usize);
                    println!("Zeroed BSS: size={}", bss_size);
                }
            }

            // Copy ELF header and program headers into memory.
            let mut lowest_paddr = u32::MAX;
            let mut lowest_offset = u32::MAX;
            for i in 0..(*elf_header_ptr).e_phnum {
                let ph = first_program_header_ptr.add(i as usize);
                if (*ph).p_type == 1 {
                    if (*ph).p_paddr < lowest_paddr {
                        lowest_paddr = (*ph).p_paddr;
                        lowest_offset = (*ph).p_offset;
                    }
                }
            }
            let elf_base = lowest_paddr - lowest_offset;
            program.elf_base = elf_base as usize;
            
            println!("ELF base address: {:#x} (p_paddr={:#x}, p_offset={:#x})",
                elf_base, lowest_paddr, lowest_offset);
            
            let ehdr_total = (*elf_header_ptr).e_phoff as usize
                + (*elf_header_ptr).e_phnum as usize * (*elf_header_ptr).e_phentsize as usize;

            println!("Diagnostics: lowest_offset={:#x}, ehdr_total={}", lowest_offset, ehdr_total);

            let ehdr_end = elf_base.wrapping_add(ehdr_total as u32);
            let mut program_end = ehdr_end;
            for i in 0..(*elf_header_ptr).e_phnum {
                let ph = first_program_header_ptr.add(i as usize);
                if (*ph).p_type != 1 { continue; }
                let pstart = (*ph).p_paddr;
                let pend = pstart.wrapping_add((*ph).p_memsz);
                if pend > program_end {
                    program_end = pend;
                }
                let overlap = !(pend <= elf_base || pstart >= ehdr_end);
                println!(
                    "PT_LOAD[{}]: p_paddr={:#x}, p_filesz={}, p_memsz={}, overlaps_headers={}",
                    i, pstart, (*ph).p_filesz, (*ph).p_memsz, overlap
                );
                if overlap {
                    println!("  --> Overlap with headers region {:#x}-{:#x}", elf_base, ehdr_end);
                }
            }

            let phys_elf_base = (program.elf.data.as_mut_ptr()).byte_add(elf_base as usize) as u32;
            println!("elf base {:x} -> phys elf base: {:x}", elf_base, phys_elf_base);

            core::ptr::write_bytes(phys_elf_base as *mut u8, 0, lowest_offset as usize);
            core::ptr::copy_nonoverlapping(
                (*file).data,
                phys_elf_base as *mut u8,
                ehdr_total,
            );

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
            let phdr_addr = elf_base + (*elf_header_ptr).e_phoff as u32;
            
            sp = sp.sub(1); *sp = 0;                             // AT_NULL val
            sp = sp.sub(1); *sp = 0;                             // AT_NULL type
            sp = sp.sub(1); *sp = 4096;                          // AT_PAGESZ val
            sp = sp.sub(1); *sp = 6;                             // AT_PAGESZ type
            sp = sp.sub(1); *sp = (*elf_header_ptr).e_phnum as u32; // AT_PHNUM val
            sp = sp.sub(1); *sp = 5;                             // AT_PHNUM type
            sp = sp.sub(1); *sp = (*elf_header_ptr).e_phentsize as u32; // AT_PHENT val
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

            println!("About to switch to user mode from PC: {:p}, SP: {:p}", 
                interrupts::switch_to_user_mode as *const (),
                &stack_top as *const _
            );
            interrupts::switch_to_user_mode();
            println!("Switched to user mode");

            dev_barrier();

            let ret_sp_addr = core::ptr::addr_of_mut!((*program_ptr).return_sp);
            let ret_lr_addr = core::ptr::addr_of_mut!((*program_ptr).return_lr);
            println!("Location of where to return sp={:p}, return lr={:p}, program_location: {:p}", ret_sp_addr, ret_lr_addr, program_ptr);

            elf_loader_tramp(core::ptr::addr_of_mut!(context), core::ptr::addr_of_mut!(program.return_sp), core::ptr::addr_of_mut!(program.return_lr));
        }
    }
}

pub fn test_elf_holder() {
    unsafe {
        OSHolder::init();
        let holder = OSHolder::os_holder_mut();
        println!("About to run user program!");
        holder.run_elf(0, "BUSYBOX");
    }
}
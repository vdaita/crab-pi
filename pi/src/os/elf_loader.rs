use crate::fat32::{self, pi_file_t};
use crate::os::threads::Thread;
use crate::os::virtmem::{MemPerm, mmu_is_enabled};
use core::arch::global_asm;
use crate::os::{self, virtmem};
use crate::os::interrupts;
use crate::{println, print};
use crate::kmalloc;
use crate::profiler;
use crate::fat32::{get_fat32_manager};
use crate::os::threads;
use crate::os::elf_file::{ElfHeader, ProgramHeader, SectionHeader};
use crate::os::holder::{Program};

const ONE_MB: u32 = 1024 * 1024;
const DOM_KERN: u32 = 1;
const MAX_PINNED_ENTRIES: u32 = 8;

#[repr(C)]
pub struct ProgramContext {
    pub user_stack: u32,
    pub entry: u32,
    pub arg0: u32,
    pub arg1: u32,
    pub arg2: u32,
}

pub fn print_program_context(program_context: &ProgramContext) {
    println!("Printing program context:");
    println!("      -> User stack: {:x}", program_context.user_stack);
    println!("      -> Entry: {:x}", program_context.entry);
    println!("      -> Arg0: {}", program_context.arg0);
    println!("      -> Arg1: {}", program_context.arg1);
    println!("      -> Arg2: {}", program_context.arg2);
}

pub static mut KERNEL_STACK: u32 = 0;
pub static mut KERNEL_RETURN: u32 = 0;

global_asm!(r#"
.globl elf_loader_tramp
.type elf_loader_tramp, %function
elf_loader_tramp:
    ldr r3, ={kernel_stack}
    str sp, [r3]
    ldr r3, ={kernel_return}
    str lr, [r3]

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
    ldr r3, ={kernel_stack}
    ldr sp, [r3]
    ldr r3, ={kernel_return}
    ldr lr, [r3]
    bx lr
"#,
    kernel_stack = sym KERNEL_STACK,
    kernel_return = sym KERNEL_RETURN,
);

unsafe extern "C" {
    pub fn elf_loader_tramp(data: *mut ProgramContext);
    pub fn elf_loader_return();
}

unsafe fn hexdump(ptr: *const u8, lines: u32) {
    for i in 0..8 {
        for j in 0..8 {
            print!("{:0x} ", *(ptr.byte_add(8*i + j)));
        }
        println!();
    }
}

pub unsafe fn kuser_get_tls() -> u32 {
    let tls: u32;
    core::arch::asm!(
        "mrc p15, 0, {tls}, c13, c0, 3",
        tls = out(reg) tls,
        options(nostack)
    );
    tls
}

pub unsafe fn kuser_cmpxchg(newval: u32, ptr: *mut u32) -> u32 {
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

pub unsafe fn kuser_memory_barrier() {
    core::arch::asm!(
        "mcr p15, 0, {r0}, c7, c10, 5",
        r0 = in(reg) 0u32,
        options(nostack)
    );
}

pub unsafe fn kuser_version() -> u32 {
    return 5;
}

unsafe fn install_kuser_helpers(pa: u32) {
    // VA 0xFF000000 -> PA pa, so offset = target_va - 0xFF000000
    
    // __kernel_helper_version at VA 0xFFFF0FFC
    core::ptr::write_volatile(
        (pa + 0x00FF0FFC) as *mut u32, kuser_version());

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
}

pub static mut elf_loader_program_start: usize = 0;
pub static mut elf_loader_program_end: usize = 0;
pub static mut elf_loader_heap_start: usize = 0;

struct ElfLoader {
    next_pin_index: u32,
}
impl ElfLoader {
    const fn new() -> Self {
        unsafe {
            Self {
                next_pin_index: 0,
            }
        }
    }

    fn pin_next(&mut self, va: u32, pa: u32, pin: virtmem::Pin) {
        assert!(self.next_pin_index < MAX_PINNED_ENTRIES);
        virtmem::pin_mmu_sec(self.next_pin_index, va, pa, pin);
        self.next_pin_index += 1;
    }

    unsafe fn run(&mut self, prog_name: &str, arg1: u32, arg2: u32, arg3: u32, asid: u32) {
        kmalloc::ensure_init();

        let manager = get_fat32_manager();
        let file = (*manager).read_file(prog_name);

        println!("File size from FAT32: {}", (*file).n_data);
        hexdump((*file).data, 8);

        let elf_header_ptr: *mut ElfHeader = (*file).data as *mut ElfHeader;
        let first_program_header_ptr: *mut ProgramHeader =
            ((*file).data as *mut u8).add((*elf_header_ptr).e_phoff as usize) as *mut ProgramHeader;

        println!("number of program headers: {}", (*elf_header_ptr).e_phnum);
        println!("Program entry point (physical): {:#x}", (*elf_header_ptr).e_entry);

        let program: &mut Program = &mut *(0x0000_0000 as *mut Program);
        // let stack_pointer_end_addr = program.stack.data.as_ptr().byte_add(program.stack.data.len()) as usize;
        // let program_base: usize = program as *mut Program as usize;
        // println!("Stack pointer end address: {:x}, base address: {:x}", stack_pointer_end_addr, program_base);
        // program.sp = 1024 * 1024 * 4 + 1024 * 8;
        // program.sp =  (stack_pointer_end_addr - program_base) - (1024 * 1024);
        program.sp = 0x1700_0000;
        // program.sp = 0x0700_0000 - 1024;
        println!("Stack pointer assignment: {:x}", program.sp);

        for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
            let program_header_ptr: *mut ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

            if (*program_header_ptr).p_type != 1 {  // PT_LOAD
                continue;
            }

            let paddr = (*program_header_ptr).p_paddr as usize;
            let p_offset = (*program_header_ptr).p_offset as usize;
            
            println!("Loading segment: p_paddr={:#x} -> paddr={:#x}, p_offset={:#x}, filesz={}",
                (*program_header_ptr).p_paddr, paddr, p_offset, (*program_header_ptr).p_filesz);

            // Check if this segment contains the ELF header and program headers
            let ehdr_and_phdr_size = (*elf_header_ptr).e_phoff as usize
                + (*elf_header_ptr).e_phnum as usize * (*elf_header_ptr).e_phentsize as usize;
            
            if p_offset == 0 {
                // This is the first segment - it should contain the ELF header and program headers
                println!("  -> This segment contains ELF header + program headers (size={})", ehdr_and_phdr_size);
                
                // Copy ELF header and program headers first
                core::ptr::copy_nonoverlapping(
                    (*file).data,
                    program.elf.data.as_mut_ptr().add(paddr),
                    ehdr_and_phdr_size,
                );
                
                // Then copy the rest of the segment data (if any beyond headers)
                if (*program_header_ptr).p_filesz > (ehdr_and_phdr_size as u32) {
                    core::ptr::copy_nonoverlapping(
                        ((*file).data as *mut u8).add(ehdr_and_phdr_size),
                        program.elf.data.as_mut_ptr().add(paddr + ehdr_and_phdr_size),
                        ((*program_header_ptr).p_filesz - ehdr_and_phdr_size as u32) as usize,
                    );
                }
            } else {
                // Other segments - copy normally
                core::ptr::copy_nonoverlapping(
                    ((*file).data as *mut u8).add(p_offset),
                    program.elf.data.as_mut_ptr().add(paddr),
                    (*program_header_ptr).p_filesz as usize,
                );
            }

            println!("Finished copying.");

            // Zero BSS (uninitialized data)
            let bss_start = (program.elf.data.as_mut_ptr().add(paddr)).add((*program_header_ptr).p_filesz as usize);
            let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
            if bss_size > 0 {
                core::ptr::write_bytes(bss_start, 0, bss_size as usize);
                println!("Zeroed BSS: size={}", bss_size);
            }
        }

        //  for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
        //     let program_header_ptr: *mut ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

        //     if (*program_header_ptr).p_type != 1 {  // PT_LOAD
        //         continue;
        //     }

        //     let paddr = (*program_header_ptr).p_paddr as usize;
        //     println!("Loading segment: p_paddr={:#x} -> paddr={:#x}, filesz={}",
        //         (*program_header_ptr).p_paddr, paddr, (*program_header_ptr).p_filesz);

        //     // Copy segment data
        //     core::ptr::copy_nonoverlapping(
        //         ((*file).data as *mut u8).add((*program_header_ptr).p_offset as usize),
        //         // paddr as *mut u8,
        //         program.elf.data.as_mut_ptr().add(paddr),
        //         (*program_header_ptr).p_filesz as usize,
        //     );

        //     println!("Finished copying.");

        //     // Zero BSS (uninitialized data)
        //     // let bss_start = (paddr as *mut u8).add((*program_header_ptr).p_filesz as usize);
        //     let bss_start = (program.elf.data.as_mut_ptr().add(paddr)).add((*program_header_ptr).p_filesz as usize);
        //     let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
        //     if bss_size > 0 {
        //         core::ptr::write_bytes(bss_start, 0, bss_size as usize);
        //         println!("Zeroed BSS: size={}", bss_size);
        //     }
        // }

        // Copy ELF header and program headers into memory.
        // The first PT_LOAD has p_paddr = p_offset + elf_base, so
        // elf_base = p_paddr - p_offset.  glibc's __ehdr_start points
        // here, and it also needs the program headers accessible.
        // let mut lowest_paddr = u32::MAX;
        // let mut lowest_offset = u32::MAX;
        // for i in 0..(*elf_header_ptr).e_phnum {
        //     let ph = first_program_header_ptr.add(i as usize);
        //     if (*ph).p_type == 1 {
        //         if (*ph).p_paddr < lowest_paddr {
        //             lowest_paddr = (*ph).p_paddr;
        //             lowest_offset = (*ph).p_offset;
        //         }
        //     }
        // }
        // let elf_base = lowest_paddr - lowest_offset;
        // elf_loader_program_start = elf_base as usize;
        // elf_loader_heap_start = kmalloc::HEAP_CURR;
        // println!("ELF base address: {:#x} (p_paddr={:#x}, p_offset={:#x})",
        //     elf_base, lowest_paddr, lowest_offset);
        // let ehdr_total = (*elf_header_ptr).e_phoff as usize
        //     + (*elf_header_ptr).e_phnum as usize * (*elf_header_ptr).e_phentsize as usize;

        // println!("Diagnostics: lowest_offset={:#x}, ehdr_total={}", lowest_offset, ehdr_total);

        // let ehdr_end = elf_base.wrapping_add(ehdr_total as u32);
        // let mut program_end = ehdr_end;
        // for i in 0..(*elf_header_ptr).e_phnum {
        //     let ph = first_program_header_ptr.add(i as usize);
        //     if (*ph).p_type != 1 { continue; }
        //     let pstart = (*ph).p_paddr;
        //     let pend = pstart.wrapping_add((*ph).p_memsz);
        //     if pend > program_end {
        //         program_end = pend;
        //     }
        //     let overlap = !(pend <= elf_base || pstart >= ehdr_end);
        //     println!(
        //         "PT_LOAD[{}]: p_paddr={:#x}, p_filesz={}, p_memsz={}, overlaps_headers={}",
        //         i, pstart, (*ph).p_filesz, (*ph).p_memsz, overlap
        //     );
        //     if overlap {
        //         println!("  --> Overlap with headers region {:#x}-{:#x}", elf_base, ehdr_end);
        //     }
        // }
        // elf_loader_program_end = program_end as usize;
        // core::ptr::write_bytes(elf_base as *mut u8, 0, lowest_offset as usize);
        // core::ptr::copy_nonoverlapping(
        //     (*file).data,
        //     elf_base as *mut u8,
        //     ehdr_total,
        // );


        // set up MMU
        virtmem::mmu_reset();
        let user = MemPerm::perm_rw_user;
        let dev = virtmem::make_global_pin(DOM_KERN, user, virtmem::MemAttr::MEM_device, virtmem::PageSizes::mb16);
        let kern = virtmem::make_global_pin(DOM_KERN, user, virtmem::MemAttr::MEM_uncached, virtmem::PageSizes::mb16);

        self.pin_next(0x2000_0000, 0x2000_0000, dev);
        
        self.pin_next(0x0, 0x0000_0000, kern);
        // self.pin_next(16 * ONE_MB, 16 * ONE_MB, kern);

        self.pin_next(0x1000_0000, 0x1000_0000, kern);
        self.pin_next(0x1000_0000 + 16 * ONE_MB, 0x1000_0000 + 16 * ONE_MB, kern);
        // self.pin_next(0x1000_0000 + 32 * ONE_MB, 0x1000_0000 + 32 * ONE_MB, kern);
        // this will go from 0x1000_0000 to 0x1300_0000 incl.
        // the main stack is 0x1700_0000 to 0x1800_0000 incl.

        // self.pin_next(64 * ONE_MB, 64 * ONE_MB, kern);

        let user_stack_base = program.sp;
        // map the stack pointers
        self.pin_next(0x1800_0000 - 16 * ONE_MB, 0x1800_0000 - 16 * ONE_MB, kern);
        self.pin_next(0x1700_0000 - 16 * ONE_MB, 0x1700_0000 - 16 * ONE_MB, kern);
        self.pin_next(os::holder::VBAR as u32, os::holder::VBAR as u32, kern);
        os::holder::OSHolder::install_interrupts();

        // self.pin_next(0x0700_0000 - 16 * ONE_MB, 0x0700_0000 - 16 * ONE_MB, kern); // or that it will be covered by this?


        // let kuser_helpers_pa = kmalloc::kmalloc_aligned(16 * ONE_MB as usize, 16 * ONE_MB as usize); // allocate a page
        // install_kuser_helpers(os::holder::KUSER_ADDR as u32);
        // self.pin_next(0xff000000, os::holder::KUSER_ADDR  as u32, kern);

        os::holder::OSHolder::install_kuser_helpers();
        self.pin_next(0xff00_0000, os::holder::KUSER_ADDR as u32, kern);

        virtmem::pin_mmu_init(!0);
        virtmem::mmu_enable();
        println!("MMU enabled");

        interrupts::switch_to_user_mode();
        println!("Switched to user mode");

        let argv0_bytes = b"sh\0";
        let argv0_heap = kmalloc::kmalloc(argv0_bytes.len()) as *mut u8;

        // let argv1_bytes = b"HELLO.TXT\0";
        // let argv1_heap = kmalloc::kmalloc(argv1_bytes.len()) as *mut u8;

        // println!("Allocated heap for argv0_bytes: {:p}", argv0_heap);
        core::ptr::copy_nonoverlapping(argv0_bytes.as_ptr(), argv0_heap, argv0_bytes.len());
        // core::ptr::copy_nonoverlapping(argv1_bytes.as_ptr(), argv1_heap, argv1_bytes.len());

        let argv0_ptr = argv0_heap as u32;
        // let argv1_ptr = argv1_heap as u32;

        // println!("Hello?");

        let stack_top = user_stack_base;
        println!("About to write to address: {:#x}", stack_top);
        core::ptr::write_bytes((stack_top - 1024) as *mut u8, 0, 1024);
        println!("User stack base just written to: {:#x}", stack_top);

        let mut sp = stack_top as *mut u32;
        let phdr_addr = program.elf_base as u32 + program.elf_header.e_phoff as u32;

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
        sp = sp.sub(1); *sp = 0;          // argv[2] == NULL
        // sp = sp.sub(1); *sp = argv1_ptr; // argv[1]
        sp = sp.sub(1); *sp = argv0_ptr;  // argv[0]

        // // argc = 2
        // sp = sp.sub(1); *sp = 2;
        
        // argc = 1
        sp = sp.sub(1); *sp = 1;

        if (sp as usize) & 7 != 0 {
            sp = sp.sub(1);
            *sp = 0;
        }

        println!("Finished constructing stack");

        let mut context: ProgramContext = ProgramContext {
            user_stack: sp as u32,
            entry: (*elf_header_ptr).e_entry,
            arg0: 1,                                 // r0 = argc
            arg1: (sp.add(1) as *const u32) as u32, // r1 = &argv[0] (pointer to first argv pointer)
            arg2: 0,                                 // r2 = envp (NULL)
        };

        print_program_context(&context);

        println!("want to run the following instructions: ");
        hexdump(context.entry as *const u8, 8);

        println!("Jumping to entry point: {:#x}", context.entry);
        // profiler::breakpoint_mismatch_start();
        elf_loader_tramp(core::ptr::addr_of_mut!(context));
    }
}

pub fn test_elf_loader() {
    interrupts::start_interrupts(
        core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_START) as usize,
        core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_END) as usize
    );

    unsafe {
        let mut loader: ElfLoader = ElfLoader::new();
        println!("About to run user program!");
        loader.run("BUSYBOX", 0, 0, 0, 1);
    }
}
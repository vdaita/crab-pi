use crate::fat32::{self, pi_file_t};
use crate::os::virtmem::{MemPerm, mmu_is_enabled};
use core::arch::global_asm;
use crate::os::virtmem;
use crate::os::interrupts;
use crate::{println, print};
use crate::kmalloc;

const ONE_MB: u32 = 1024 * 1024;
const DOM_KERN: u32 = 1;
const MAX_PINNED_ENTRIES: u32 = 8;

#[repr(C)]
struct ElfHeader {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32, // version
    e_entry: u32, // this is the memory address of where the process starts executing
    e_phoff: u32, // points to the start of the program header table
    e_shoff: u32, // point to start of sectino header table
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
    p_offset: u32, // offset of this segment in the file image
    p_vaddr: u32, // virtual address of this segment in memory
    p_paddr: u32, // physical address of this segment (if relevant)
    p_filesz: u32, // size of this segment in the file image
    p_memsz: u32, // size of this segment in memory
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

unsafe fn copy_argv0(prog_name: &str) -> *const u8 {
    let argv0 = unsafe { kmalloc::kmalloc(prog_name.len() + 1) };
    unsafe {
        core::ptr::copy_nonoverlapping(prog_name.as_ptr(), argv0, prog_name.len());
        core::ptr::write(argv0.add(prog_name.len()), 0);
    }
    argv0
}

struct ElfLoader {
    next_pin_index: u32
}
impl ElfLoader {
    const fn new() -> Self {
        Self {
            next_pin_index: 0
        }
    }

    fn pin_next(&mut self, va: u32, pa: u32, pin: virtmem::Pin) {
        assert!(self.next_pin_index < MAX_PINNED_ENTRIES);
        virtmem::pin_mmu_sec(self.next_pin_index, va, pa, pin);
        self.next_pin_index += 1;
    }

    unsafe fn run(&mut self, prog_name: &str, arg1: u32, arg2: u32, arg3: u32, asid: u32) {
        fat32::pi_sd_init();
        let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");
        let fs = fat32::fat32_mk(&partition);
        let root = fat32::fat32_get_root(&fs);

        let file: *mut pi_file_t = fat32::fat32_read(&fs, &root, prog_name);
        println!("File size from FAT32: {}", (*file).n_data);
        hexdump((*file).data, 8);

        let elf_header_ptr: *mut ElfHeader = (*file).data as *mut ElfHeader;
        let first_program_header_ptr: *mut ProgramHeader =
            ((*file).data as *mut u8).add((*elf_header_ptr).e_phoff as usize) as *mut ProgramHeader;

        println!("number of program headers: {}", (*elf_header_ptr).e_phnum);
        println!("Program entry point (physical): {:#x}", (*elf_header_ptr).e_entry);

         for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
            let program_header_ptr: *mut ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

            if (*program_header_ptr).p_type != 1 {  // PT_LOAD
                continue;
            }

            let paddr = (*program_header_ptr).p_paddr;
            println!("Loading segment: p_paddr={:#x} -> paddr={:#x}, filesz={}",
                (*program_header_ptr).p_paddr, paddr, (*program_header_ptr).p_filesz);

            // Copy segment data
            core::ptr::copy_nonoverlapping(
                ((*file).data as *mut u8).add((*program_header_ptr).p_offset as usize),
                paddr as *mut u8,
                (*program_header_ptr).p_filesz as usize,
            );

            // Zero BSS (uninitialized data)
            let bss_start = (paddr as *mut u8).add((*program_header_ptr).p_filesz as usize);
            let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
            if bss_size > 0 {
                core::ptr::write_bytes(bss_start, 0, bss_size as usize);
                println!("Zeroed BSS: size={}", bss_size);
            }
        }

        // Copy ELF header and program headers into memory.
        // The first PT_LOAD has p_paddr = p_offset + elf_base, so
        // elf_base = p_paddr - p_offset.  glibc's __ehdr_start points
        // here, and it also needs the program headers accessible.
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
        println!("ELF base address: {:#x} (p_paddr={:#x}, p_offset={:#x})",
            elf_base, lowest_paddr, lowest_offset);
        let ehdr_total = (*elf_header_ptr).e_phoff as usize
            + (*elf_header_ptr).e_phnum as usize * (*elf_header_ptr).e_phentsize as usize;

        println!("Diagnostics: lowest_offset={:#x}, ehdr_total={}", lowest_offset, ehdr_total);

        let ehdr_end = elf_base.wrapping_add(ehdr_total as u32);
        for i in 0..(*elf_header_ptr).e_phnum {
            let ph = first_program_header_ptr.add(i as usize);
            if (*ph).p_type != 1 { continue; }
            let pstart = (*ph).p_paddr;
            let pend = pstart.wrapping_add((*ph).p_memsz);
            let overlap = !(pend <= elf_base || pstart >= ehdr_end);
            println!(
                "PT_LOAD[{}]: p_paddr={:#x}, p_filesz={}, p_memsz={}, overlaps_headers={}",
                i, pstart, (*ph).p_filesz, (*ph).p_memsz, overlap
            );
            if overlap {
                println!("  --> Overlap with headers region {:#x}-{:#x}", elf_base, ehdr_end);
            }
        }
        core::ptr::write_bytes(elf_base as *mut u8, 0, lowest_offset as usize);
        core::ptr::copy_nonoverlapping(
            (*file).data,
            elf_base as *mut u8,
            ehdr_total,
        );

        interrupts::switch_to_user_mode();
        println!("Switched to user mode");

        let user_stack_base = 0x10000000u32 + 128 * 1024;

        // Clear the stack area (a few pages for startup data)
        let stack_top = user_stack_base;
        core::ptr::write_bytes(stack_top as *mut u8, 0, 1024);

        // Build ARM Linux ABI startup stack (top-down):
        // sp -> argc | argv[] | NULL | envp[] | NULL | auxv...
        let mut sp = stack_top as *mut u32;

        // Auxiliary vector: AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ, AT_NULL
        let phdr_addr = elf_base + (*elf_header_ptr).e_phoff as u32;
        sp = sp.sub(1); *sp = 0;                             // AT_NULL val
        sp = sp.sub(1); *sp = 0;                             // AT_NULL type
        sp = sp.sub(1); *sp = 4096;                          // AT_PAGESZ val
        sp = sp.sub(1); *sp = 6;                              // AT_PAGESZ type
        sp = sp.sub(1); *sp = (*elf_header_ptr).e_phnum as u32; // AT_PHNUM val
        sp = sp.sub(1); *sp = 5;                              // AT_PHNUM type
        sp = sp.sub(1); *sp = (*elf_header_ptr).e_phentsize as u32; // AT_PHENT val
        sp = sp.sub(1); *sp = 4;                              // AT_PHENT type
        sp = sp.sub(1); *sp = phdr_addr;                     // AT_PHDR val
        sp = sp.sub(1); *sp = 3;                              // AT_PHDR type

        // NULL terminator for environment
        sp = sp.sub(1); *sp = 0;
        // NULL terminator for argv
        sp = sp.sub(1); *sp = 0;

        // argc = 0 (no command-line args)
        sp = sp.sub(1); *sp = 0;

        let mut context: ProgramContext = ProgramContext {
            user_stack: sp as u32,
            entry: (*elf_header_ptr).e_entry,
            arg0: arg1,
            arg1: arg2,
            arg2: arg3,
        };

        println!("want to run the following instructions: ");
        hexdump(context.entry as *const u8, 8);

        println!("Jumping to entry point: {:#x}", context.entry);

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


#[derive(Clone, Copy)]
#[repr(C)]
pub struct ElfHeader {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32, // version
    pub e_entry: u32, // this is the memory address of where the process starts executing
    pub e_phoff: u32, // points to the start of the program header table
    pub e_shoff: u32, // point to start of sectino header table
    pub e_flags: u32, // depends on target arch
    pub e_ehsize: u16, // size of this header
    pub e_phentsize: u16, // size of program header table entry
    pub e_phnum: u16, // number of entries in the program header table
    pub e_shentsize: u16, // size of a section header table entry
    pub e_shnum: u16, // contains number of entries in the section header table
    pub e_shstrndx: u16 // index of the section header table entry that contains the section names
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct ProgramHeader {
    pub p_type: u32, // identifies the type of this segment
    pub p_offset: u32, // offset of this segment in the file image
    pub p_vaddr: u32, // virtual address of this segment in memory
    pub p_paddr: u32, // physical address of this segment (if relevant)
    pub p_filesz: u32, // size of this segment in the file image
    pub p_memsz: u32, // size of this segment in memory
    pub p_flags: u32, // segment dependent flags
    pub p_align: u32 // required alignment of this segment
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SectionHeader {
    pub sh_name: u32, // offset to section name in the section name string table
    pub sh_type: u32, // identifies the type of this section header
    pub sh_flags: u32, // identifies attributes of this section
    pub sh_addr: u32, // virtual address of this section in memory
    pub sh_offset: u32, // offset of this section in the file image
    pub sh_size: u32, // size of this section in bytes
    pub sh_link: u32, // section index link value (type-dependent meaning)
    pub sh_info: u32, // extra section information (type-dependent meaning)
    pub sh_addralign: u32, // required alignment of this section
    pub sh_entsize: u32 // size of entries if section has fixed-size entries
}

use crate::{println, print};

/// Load an ELF image from `file_data` into `program`'s memory.
pub unsafe fn load_elf_into_program(file_data: *const u8, program: &mut crate::os::holder::Program) {
    let elf_header_ptr: *const ElfHeader = file_data as *const ElfHeader;
    let first_program_header_ptr: *const ProgramHeader = file_data.add((*elf_header_ptr).e_phoff as usize) as *const ProgramHeader;

    // copy ELF header into program struct
    program.elf_header = *elf_header_ptr;

    for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
        let program_header_ptr: *const ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

        if (*program_header_ptr).p_type != 1 {  // PT_LOAD
            continue;
        }

        let paddr = (program.elf.data.as_mut_ptr() as *mut u8).add((*program_header_ptr).p_paddr as usize) as *mut u8;
        println!("Loading segment: p_paddr={:#x} -> paddr={:#x}, filesz={}",
            (*program_header_ptr).p_paddr, paddr as usize, (*program_header_ptr).p_filesz);

        core::ptr::copy_nonoverlapping(
            file_data.add((*program_header_ptr).p_offset as usize),
            paddr,
            (*program_header_ptr).p_filesz as usize,
        );

        // Zero BSS
        let bss_start = paddr.add((*program_header_ptr).p_filesz as usize);
        let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
        if bss_size > 0 {
            core::ptr::write_bytes(bss_start, 0, bss_size as usize);
            println!("Zeroed BSS: size={}", bss_size);
        }
    }

    // Find elf_base and copy headers into program memory
    let mut lowest_paddr: u32 = u32::MAX;
    let mut lowest_offset: u32 = u32::MAX;
    for i in 0..(*elf_header_ptr).e_phnum {
        let ph = first_program_header_ptr.add(i as usize);
        if (*ph).p_type == 1 {
            if (*ph).p_paddr < lowest_paddr {
                lowest_paddr = (*ph).p_paddr;
                lowest_offset = (*ph).p_offset;
            }
        }
    }
    let elf_base = lowest_paddr.wrapping_sub(lowest_offset);
    program.elf_base = elf_base as usize;

    let ehdr_total = (*elf_header_ptr).e_phoff as usize + (*elf_header_ptr).e_phnum as usize * (*elf_header_ptr).e_phentsize as usize;
    let phys_elf_base = (program.elf.data.as_mut_ptr() as *mut u8).add(elf_base as usize) as *mut u8;

    core::ptr::write_bytes(phys_elf_base, 0, lowest_offset as usize);
    core::ptr::copy_nonoverlapping(
        file_data,
        phys_elf_base,
        ehdr_total,
    );

    println!("ELF loaded: elf_base={:#x}, phys_elf_base={:#x}", elf_base, phys_elf_base as usize);
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
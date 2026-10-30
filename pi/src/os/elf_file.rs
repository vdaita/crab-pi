
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

use crate::println;
use crate::os::errno::{Errno, ENOEXEC};

/// Load an ELF image from `file_data` into `program`'s memory.
/// Everything is checked before anything is copied, so a failed execve leaves the old image alone.
pub unsafe fn load_elf_into_program(file_data: *const u8, file_len: usize, program: &mut crate::os::holder::Program) -> Result<(), Errno> {
    unsafe {
        let elf_header_ptr: *const ElfHeader = file_data as *const ElfHeader;
        if file_len < core::mem::size_of::<ElfHeader>() {
            return Err(ENOEXEC);
        }
        let magic = (*elf_header_ptr).e_ident;
        if magic[0..4] != *b"\x7fELF" || (*elf_header_ptr).e_machine != 40 { // EM_ARM
            return Err(ENOEXEC);
        }
        if (*elf_header_ptr).e_type != 2 { // ET_EXEC
            println!("only static non-PIE executables are supported (e_type={})", (*elf_header_ptr).e_type);
            return Err(ENOEXEC);
        }
        let ph_end = (*elf_header_ptr).e_phoff as usize + (*elf_header_ptr).e_phnum as usize * core::mem::size_of::<ProgramHeader>();
        if ph_end > file_len {
            return Err(ENOEXEC);
        }
        let first_program_header_ptr: *const ProgramHeader = file_data.add((*elf_header_ptr).e_phoff as usize) as *const ProgramHeader;

        for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
            let ph = first_program_header_ptr.add(prog_header_idx as usize);
            if (*ph).p_type == 3 { // PT_INTERP
                println!("dynamically linked executables are not supported");
                return Err(ENOEXEC);
            }
            if (*ph).p_type != 1 {
                continue;
            }
            if (*ph).p_filesz > (*ph).p_memsz
                || ((*ph).p_offset + (*ph).p_filesz) as usize > file_len
                || ((*ph).p_paddr + (*ph).p_memsz) as usize > program.elf.data.len() {
                println!("segment {:#x}+{:#x} doesn't fit in the ELF region", (*ph).p_paddr, (*ph).p_memsz);
                return Err(ENOEXEC);
            }
        }

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
        Ok(())
    }
}

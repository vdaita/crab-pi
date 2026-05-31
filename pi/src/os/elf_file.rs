
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
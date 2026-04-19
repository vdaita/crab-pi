use crate::fat32::{self, pi_file_t};
use core::arch::global_asm;
use crate::os::virtmem;

const ONE_MB: u32 = 1024 * 1024;
const STACK_ADDR: u32 = 0x0800_0000;
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
    sp: u32,
    lr: u32,
    arg0: u32,
    arg1: u32,
    arg2: u32
}

global_asm!(r#"
.globl elf_loader_tramp
.type elf_loader_tramp, %function
elf_loader_tramp:
    ldr r1, [r0] // stack pointer
    mov sp, r1

    ldr r1, [r0, #4] // link register
    mov lr, r1

    ldr r2, [r0, #16] // third argument
    ldr r1, [r0, #12] // second argument
    ldr r0, [r0, #8] // first argument

    bx lr
"#);

unsafe extern "C" {
    fn elf_loader_tramp(data: *mut ProgramContext) -> u32;
}

struct ElfLoader {
    next_index: u32,
    first_user_index: u32,
    kernel_initialized: bool,
    kernel_mmu_enabled_on_init: bool,
}

impl ElfLoader {
    const fn new() -> Self {
        Self {
            next_index: 0,
            first_user_index: 0,
            kernel_initialized: false,
            kernel_mmu_enabled_on_init: false,
        }
    }

    fn pin_next(&mut self, va: u32, pa: u32, pin: virtmem::Pin) {
        assert!(self.next_index < MAX_PINNED_ENTRIES);
        virtmem::pin_mmu_sec(self.next_index, va, pa, pin);
        self.next_index += 1;
    }

    fn init_kernel_state(&mut self) {
        if self.kernel_initialized {
            return;
        }

        virtmem::pin_mmu_init(!0);

        let no_user = virtmem::MemPerm::perm_rw_priv;
        let dev = virtmem::make_global_pin(DOM_KERN, no_user, virtmem::MemAttr::MEM_device);
        let kern = virtmem::make_global_pin(DOM_KERN, no_user, virtmem::MemAttr::MEM_uncached);

        self.pin_next(0x2000_0000, 0x2000_0000, dev);
        self.pin_next(0x2010_0000, 0x2010_0000, dev);
        self.pin_next(0x2020_0000, 0x2020_0000, dev);

        self.pin_next(0, 0, kern);
        self.pin_next(ONE_MB, ONE_MB, kern);
        self.pin_next(STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);

        self.first_user_index = self.next_index;
        self.kernel_mmu_enabled_on_init = virtmem::mmu_is_enabled();
        self.kernel_initialized = true;
    }

    fn begin_run(&mut self) {
        if virtmem::mmu_is_enabled() {
            virtmem::mmu_disable();
        }
        self.next_index = self.first_user_index;
    }

    unsafe fn run_program_inner(&mut self, prog_name: &str, arg1: u32, arg2: u32, arg3: u32, asid: u32) {
        self.init_kernel_state();
        self.begin_run();

        fat32::pi_sd_init();

        let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");
        let fs = fat32::fat32_mk(&partition);
        let root = fat32::fat32_get_root(&fs);

        let file: *mut pi_file_t = fat32::fat32_read(&fs, &root, prog_name);

        let user = virtmem::make_user_pin(
            DOM_KERN,
            asid,
            virtmem::MemPerm::perm_rw_priv,
            virtmem::MemAttr::MEM_uncached,
        );
        let current_position = (asid + 16) * ONE_MB;

        let elf_header_ptr: *mut ElfHeader = unsafe { (*file).data as *mut ElfHeader };

        let first_program_header_ptr: *mut ProgramHeader = unsafe {
            elf_header_ptr.add(1) as *mut ProgramHeader
        };

        unsafe {
            for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
                let program_header_ptr: *mut ProgramHeader = unsafe {
                    first_program_header_ptr.add(prog_header_idx as usize)
                };

                self.pin_next((*program_header_ptr).p_vaddr, current_position, user);
                core::ptr::copy_nonoverlapping(
                    ((*file).data as *mut u8).add((*program_header_ptr).p_offset as usize),
                    (*program_header_ptr).p_paddr as *mut u8,
                    (*program_header_ptr).p_memsz as usize,
                );
            }
        }

        self.pin_next(STACK_ADDR, current_position, user);

        virtmem::pin_mmu_switch(0, asid);
        virtmem::mmu_enable();

        let mut context: ProgramContext = ProgramContext {
            sp: STACK_ADDR,
            lr: (*elf_header_ptr).e_entry,
            arg0: arg1,
            arg1: arg2,
            arg2: arg3,
        };

        unsafe {
            elf_loader_tramp(core::ptr::addr_of_mut!(context));
        }

        virtmem::mmu_disable();
        if self.kernel_mmu_enabled_on_init {
            virtmem::mmu_enable();
        }
    }
}
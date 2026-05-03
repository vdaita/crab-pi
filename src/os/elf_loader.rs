use crate::fat32::{self, pi_file_t};
use crate::os::virtmem::{MemPerm, mmu_is_enabled};
use core::arch::global_asm;
use crate::os::virtmem;
use crate::os::interrupts;
use crate::{println, print};
use crate::kmalloc;

const ONE_MB: u32 = 1024 * 1024;
const STACK_ADDR: u32 = 0x0800_0000;
const DOM_KERN: u32 = 1;
const MAX_PINNED_ENTRIES: u32 = 8;
const USER_IMAGE_VA_BASE: u32 = 0x1000_0000;
const USER_IMAGE_PA_BASE: u32 = 0x1000_0000;
const USER_IMAGE_SIZE: u32 = 16 * ONE_MB;
const USER_STACK_TOP: u32 = USER_IMAGE_VA_BASE + USER_IMAGE_SIZE;

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

unsafe fn hexdump(ptr: *const u8, lines: u32) {
    for i in 0..8 {
        for j in 0..8 {
            print!("{:0x} ", *(ptr.byte_add(8*i + j)));
        }
        println!();
    }
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

    fn user_phys_for_va(va: u32) -> u32 {
        assert!(va >= USER_IMAGE_VA_BASE);
        let offset = va - USER_IMAGE_VA_BASE;
        assert!(offset < USER_IMAGE_SIZE);
        USER_IMAGE_PA_BASE + offset
    }

    fn init_kernel_state(&mut self) {
        if self.kernel_initialized {
            return;
        }
        assert!(!virtmem::mmu_is_enabled());
        virtmem::mmu_disable();
        virtmem::mmu_reset();

        unsafe { kmalloc::kmalloc_init_mb(1) };
        virtmem::pin_mmu_init(!0);
        println!("initialized mmu");

        let no_user = virtmem::MemPerm::perm_rw_priv;
        let dev = virtmem::make_global_pin_16mb(DOM_KERN, no_user, virtmem::MemAttr::MEM_device);
        let kern = virtmem::make_global_pin_16mb(DOM_KERN, no_user, virtmem::MemAttr::MEM_uncached);
        let user = virtmem::make_user_pin_16mb(
            DOM_KERN,
            1,
            virtmem::MemPerm::perm_rw_user,
            virtmem::MemAttr::MEM_uncached,
        );

        self.pin_next(0x2000_0000, 0x2000_0000, dev);
        // self.pin_next(0x2010_0000, 0x2010_0000, dev);
        // self.pin_next(0x2020_0000, 0x2020_0000, dev);

        self.pin_next(0, 0, kern);
        // self.pin_next(ONE_MB, ONE_MB, kern);
        // self.pin_next(2 * ONE_MB, 2 * ONE_MB, kern);
        self.pin_next(STACK_ADDR - ONE_MB, STACK_ADDR - ONE_MB, kern);
        self.pin_next(USER_IMAGE_VA_BASE, USER_IMAGE_PA_BASE, user);

        self.first_user_index = self.next_index;
        
        // println!("about to enable virtual memory");
        self.kernel_mmu_enabled_on_init = virtmem::mmu_is_enabled();
        self.kernel_initialized = true;
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
        assert!(
            (*elf_header_ptr).e_entry >= USER_IMAGE_VA_BASE
                && (*elf_header_ptr).e_entry < USER_IMAGE_VA_BASE + USER_IMAGE_SIZE,
            "ELF entry point is outside the user image window"
        );

        // Load all loadable segments into the reserved user image window.
        for prog_header_idx in 0..(*elf_header_ptr).e_phnum {
            let program_header_ptr: *mut ProgramHeader = first_program_header_ptr.add(prog_header_idx as usize);

            if (*program_header_ptr).p_type != 1 {
                continue;
            }

            let vaddr = (*program_header_ptr).p_vaddr;
            assert!(
                vaddr >= USER_IMAGE_VA_BASE
                    && vaddr + (*program_header_ptr).p_memsz <= USER_IMAGE_VA_BASE + USER_IMAGE_SIZE,
                "loadable segment does not fit inside the user image window"
            );
            let paddr = Self::user_phys_for_va(vaddr);
            // Copy segment data
            println!(
                "About to copy segment {} to vaddr 0x{:0x} (phys 0x{:0x})",
                prog_header_idx, vaddr, paddr
            );
            core::ptr::copy_nonoverlapping(
                ((*file).data as *mut u8).add((*program_header_ptr).p_offset as usize),
                paddr as *mut u8,
                (*program_header_ptr).p_filesz as usize,
            );

            // Zero BSS
            let bss_start = (paddr as *mut u8).add((*program_header_ptr).p_filesz as usize);
            let bss_size = (*program_header_ptr).p_memsz - (*program_header_ptr).p_filesz;
            core::ptr::write_bytes(bss_start, 0, bss_size as usize);
        }
        let stack_top = USER_STACK_TOP;
        
        interrupts::switch_to_user_mode(); 
        println!("Switched to user mode");

        let mut context: ProgramContext = ProgramContext {
            sp: stack_top,
            lr: (*elf_header_ptr).e_entry,
            arg0: arg1,
            arg1: arg2,
            arg2: arg3,
        };

        println!("want to run the following instructions: ");
        hexdump(context.lr as *const u8, 8);

        println!("Jumping to entry point: {:#x}", context.lr);
        
        elf_loader_tramp(core::ptr::addr_of_mut!(context));

    }
}

pub fn test_elf_loader() {
    interrupts::start_interrupts(
        core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_START) as usize,
        core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_START) as usize
    );

    unsafe {
        let mut loader: ElfLoader = ElfLoader::new();
        println!("About to run user program!");
        loader.run("TEST.ELF", 0, 0, 0, 1);
    }
}

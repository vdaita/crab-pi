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

        interrupts::switch_to_user_mode();
        println!("Switched to user mode");

        let mut context: ProgramContext = ProgramContext {
            sp: !0,
            lr: (*elf_header_ptr).e_entry,
            arg0: arg1,
            arg1: arg2,
            arg2: arg3
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
        core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_END) as usize
    );

    unsafe {
        let mut loader: ElfLoader = ElfLoader::new();
        println!("About to run user program!");
        loader.run("TEST.ELF", 0, 0, 0, 1);
    }
}
use core::arch::global_asm;
use crate::arch::dev_barrier;
use crate::mem::{get32, put32};
use crate::os::interrupts;
use crate::os::virtmem::{self, MemPerm, MemAttr, PageSizes, mmu_disable, mmu_enable};
use crate::{println, print};
use crate::circular::CircularQueue;
use crate::profiler;
use crate::fat32::{self, Fat32Manager, fs_manager, get_fat32_manager, pi_file_t};
use crate::kmalloc;
use core::cell::SyncUnsafeCell;
use core::arch::asm;
use core::mem::MaybeUninit;
use crate::os::elf_loader;

unsafe impl Sync for OSHolder {}

pub static OS_HOLDER: SyncUnsafeCell<MaybeUninit<OSHolder>> = 
    SyncUnsafeCell::new(MaybeUninit::zeroed());

const DOM_KERN: u32 = 1;
const DOM_USER: u32 = 2;
const TINY_PAGE: usize = 4 * 1024;
const LARGE_PAGE: usize = 16 * 1024 * 1024;
const VBAR: usize = 0x1900_0000;
const ONE_MB: usize = 1024 * 1024;
const NUM_PROGRAMS: usize = 3;
const MAX_ELF_SIZE: usize = 1024 * 1024;
const MAX_STACK_SIZE: usize = 1024 * 64;
const MAX_HEAP_SIZE: usize = 1024 * 1024;

const KUSER_ADDR: usize = 0x1500_0000;

#[derive(Copy, Clone)]
struct ELF {
    data: [u8; MAX_ELF_SIZE],
}

#[derive(Copy, Clone)]
struct Stack {
    data: [u8; MAX_STACK_SIZE],
}

#[derive(Copy, Clone)]
struct Heap {
    data: [u8; MAX_HEAP_SIZE],
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct ElfHeader {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: usize,
    e_phoff: usize,
    e_shoff: usize,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16
}

#[repr(C)]
struct ProgramHeader {
    p_type: u32,
    p_offset: usize,
    p_vaddr: usize,
    p_paddr: usize,
    p_filesz: usize,
    p_memsz: usize,
    p_flags: u32,
    p_align: u32
}

#[repr(C)]
struct SectionHeader {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u32,
    sh_addr: u32,
    sh_offset: u32,
    sh_size: u32,
    sh_link: u32,
    sh_info: u32,
    sh_addralign: u32,
    sh_entsize: u32
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct SoftwareInterruptFrame {
    r0: u32,
    r1: u32,
    r2: u32,
    r3: u32,
    r4: u32,
    r5: u32,
    r6: u32,
    r7: u32,
    r8: u32,
    r9: u32,
    r10: u32,
    r11: u32,
    r12: u32,
    lr: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Program {
    elf: ELF,
    stack: Stack,
    heap: Heap,
    sp: usize,
    heap_ptr: usize,
    tid: u32,
    frame: SoftwareInterruptFrame,
    active: bool,
    elf_header: ElfHeader,
    elf_base: usize,
}

#[repr(C)]
pub struct OSHolder {
    programs: [*mut Program; NUM_PROGRAMS],
    current_program: usize
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

unsafe fn kuser_get_tls() -> u32 {
    let tls: u32;
    core::arch::asm!(
        "mrc p15, 0, {tls}, c13, c0, 3",
        tls = out(reg) tls,
        options(nostack)
    );
    tls
}

unsafe fn kuser_cmpxchg(newval: u32, ptr: *mut u32) -> u32 {
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

unsafe fn kuser_memory_barrier() {
    core::arch::asm!(
        "mcr p15, 0, {r0}, c7, c10, 5",
        r0 = in(reg) 0u32,
        options(nostack)
    );
}

unsafe fn kuser_version() -> u32 {
    return 5;
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
            interrupts::disable_interrupts_asm();
            dev_barrier();

            core::ptr::write(OS_HOLDER.get().cast::<OSHolder>(), core::mem::zeroed());

            println!("About to copy Kuser helpers");

            let holder = OSHolder::os_holder_mut();
            
            // __kernel_get_tls at VA 0xFFFF0FA0
            core::ptr::copy_nonoverlapping(
                kuser_get_tls as *const u32,
                (KUSER_ADDR + 0x00FF0FA0) as *mut u32, 4);

            // __kernel_cmpxchg at VA 0xFFFF0FC0
            core::ptr::copy_nonoverlapping(
                kuser_cmpxchg as *const u32,
                (KUSER_ADDR + 0x00FF0FC0) as *mut u32, 8);

            // __kernel_memory_barrier at VA 0xFFFF0FE0
            core::ptr::copy_nonoverlapping(
                kuser_memory_barrier as *const u32,
                (KUSER_ADDR + 0x00FF0FE0) as *mut u32, 2);

            // __kernel_version at VA 0xFFFF0FFC
            core::ptr::copy_nonoverlapping(
                kuser_version as *const u32,
                (KUSER_ADDR + 0x00FF0FFC) as *mut u32, 4);

            println!("Finished copying KUSER");

            // copy over the interrupt table
            interrupts::move_table_vbar(
                core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_START) as usize,
                core::ptr::addr_of!(interrupts::INTERRUPT_TABLE_END) as usize,
                VBAR
            );

            dev_barrier();
            asm!("mcr p15, 0, {0}, c12, c0, 0", in(reg) VBAR, options(nostack, preserves_flags));
            dev_barrier();

            // initialize program pointers
            for i in 0..NUM_PROGRAMS {
                let program_address = 0x0200_0000 + 0x0100_0000 * i;
                holder.programs[i] = program_address as *mut Program;
                core::ptr::write_bytes(
                    program_address as *mut u8,
                    0,
                    core::mem::size_of::<Program>()
                );
            }            
            (*holder.programs[0]).active = true;

            for i in 0..NUM_PROGRAMS {
                println!("Program {} has memory location {:p}, active={}",
                    i, holder.programs[i], (*holder.programs[i]).active);
            }
        }
    }

    unsafe fn get_program_mut(&mut self, index: usize) -> &'static mut Program {
        &mut *self.programs[index]
    }

    unsafe fn get_program(&self, index: usize) -> &'static Program {
        &*self.programs[index]
    }

    pub fn load_elf(&mut self, prog_name: &str) -> usize {
        unsafe {
            let file_manager = get_fat32_manager();
            let file = (*file_manager).read_file(prog_name);
            let elf_header_ptr = (*file).data as *mut ElfHeader;
            let elf_header = core::ptr::read_unaligned(elf_header_ptr);
            let first_prog_header_ptr = (*file).data.byte_add(elf_header.e_phoff) as *mut ProgramHeader;
            
            let mut program_index = 0;
            for i in 0..NUM_PROGRAMS {
                if !self.get_program(i).active {
                    program_index = i;
                    break;
                }
            }

            println!("Current program index: {}", program_index);
            let program = self.get_program_mut(program_index);
            
            // Find lowest address to determine ELF base
            let mut lowest_paddr = usize::MAX;
            let mut lowest_offset = usize::MAX;
            
            for prog_header_idx in 0..elf_header.e_phnum {
                let prog_header_ptr = first_prog_header_ptr.add(prog_header_idx as usize);
                let prog_header = core::ptr::read_unaligned(prog_header_ptr);
                
                if prog_header.p_type == 1 {  // PT_LOAD
                    if prog_header.p_paddr < lowest_paddr {
                        lowest_paddr = prog_header.p_paddr;
                        lowest_offset = prog_header.p_offset;
                    }
                }
            }
            
            let elf_base = lowest_paddr - lowest_offset;
            program.elf_base = elf_base;
            
            println!("ELF base address: {:#x} (p_paddr={:#x}, p_offset={:#x})",
                elf_base, lowest_paddr, lowest_offset);
            
            // Load segments
            for prog_header_idx in 0..elf_header.e_phnum {
                let prog_header_ptr = first_prog_header_ptr.add(prog_header_idx as usize);
                let prog_header = core::ptr::read_unaligned(prog_header_ptr);
                
                if prog_header.p_type != 1 {
                    continue;
                }

                println!("Loading segment {}: vaddr={:#x}, paddr={:#x}, offset={:#x}, filesz={}, memsz={}",
                    prog_header_idx, prog_header.p_vaddr, prog_header.p_paddr, 
                    prog_header.p_offset, prog_header.p_filesz, prog_header.p_memsz);

                core::ptr::copy_nonoverlapping(
                    ((*file).data as *mut u8).add(prog_header.p_offset),
                    program.elf.data.as_mut_ptr().add(prog_header.p_paddr),
                    prog_header.p_filesz
                );
                
                if prog_header.p_memsz > prog_header.p_filesz {
                    let bss_size = prog_header.p_memsz - prog_header.p_filesz;
                    core::ptr::write_bytes(
                        program.elf.data.as_mut_ptr().add(prog_header.p_paddr + prog_header.p_filesz),
                        0,
                        bss_size
                    );
                    println!("  Zeroed BSS: {} bytes", bss_size);
                }
            }
            
            // Copy ELF header and program headers into memory
            let ehdr_total = elf_header.e_phoff + 
                elf_header.e_phnum as usize * elf_header.e_phentsize as usize;
            
            println!("Copying ELF headers: base={:#x}, size={}", elf_base, ehdr_total);
            
            core::ptr::write_bytes(program.elf.data.as_mut_ptr().add(elf_base), 0, lowest_offset);
            core::ptr::copy_nonoverlapping(
                (*file).data,
                program.elf.data.as_mut_ptr().add(elf_base),
                ehdr_total,
            );

            program.elf_header = elf_header;

            println!("Loading ELF File with header: ");
            print_elf_header(elf_header);

            program.sp = 0x00ff_ffff - 1024;
            program.heap_ptr = 0x0088_8888;
            program.tid = program_index as u32;
            program.active = true;

            println!("Loaded ELF entry point: {:#x}", program.elf_header.e_entry);
            println!("Size of written program object: {} bytes", size_of::<Program>());

            program_index
        }
    }

    fn map_program_mmu(&mut self, program_index: usize) {
        virtmem::mmu_disable();
        virtmem::mmu_reset();

        let user = MemPerm::perm_rw_user;
        let dev_pin_mb16 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_device, PageSizes::mb16);
        let kern_pin_mb16 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::mb16);
        let kern_pin_kb4 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::kb4);

        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev_pin_mb16);
        virtmem::pin_mmu_sec(1, 0x1000_0000, 0x1000_0000, kern_pin_mb16);
        virtmem::pin_mmu_sec(2, (0x1000_0000 + 16 * ONE_MB) as u32, (0x1000_0000 + 16 * ONE_MB) as u32, kern_pin_mb16);
        virtmem::pin_mmu_sec(3, (0x1800_0000 - 16 * ONE_MB) as u32, (0x1800_0000 - 16 * ONE_MB) as u32, kern_pin_mb16);

        virtmem::pin_mmu_sec(4, VBAR as u32, VBAR as u32, kern_pin_kb4);
        virtmem::pin_mmu_sec(5, 0xff00_0000, KUSER_ADDR as u32, kern_pin_mb16);
        
        virtmem::pin_mmu_sec(6, 0x0000_0000, self.programs[program_index] as u32, kern_pin_mb16);
    }

    pub fn test_swi() {
        unsafe {
            interrupts::switch_to_user_mode();
            let mut r0: u32 = 1; // for standard out
            let test_str = "testing interrupt\n";
            unsafe {
                asm!(
                    "svc 0",
                    inout("r0") r0 => r0,
                    in("r1") test_str.as_ptr(),
                    in("r2") test_str.len(),
                    in("r7") 4u32,
                    options(nostack)
                )
            }
            println!("Finished running SWI handler.");
        }
    }

    pub fn run_elf(&mut self, program_index: usize, prog_name: &str) {
        println!("Setting up MMU for program {}", program_index);
        self.map_program_mmu(program_index);
        
        crate::arch::dev_barrier();
        virtmem::pin_mmu_init(!0);
        crate::arch::dev_barrier();
        virtmem::mmu_enable();
        crate::arch::dev_barrier();
        
        println!("MMU enabled!");

        unsafe {
            interrupts::switch_to_user_mode();
            interrupts::enable_interrupts_asm();
            println!("Switched to user mode");

            let program: &mut Program = &mut *(0x0000_0000 as *mut Program);
            crate::arch::dev_barrier();
            println!("Made reference to object at base memory");
            
            // Set up argv
            let argv0_bytes = b"sh\0";
            let argv0_ptr = program.heap.data.as_mut_ptr().add(program.heap_ptr);
            core::ptr::copy_nonoverlapping(argv0_bytes.as_ptr(), argv0_ptr, argv0_bytes.len());
            let argv0_addr = argv0_ptr as u32;
            
            // Set up stack
            let stack_top = program.sp;
            println!("User stack base: {:#x}", stack_top);
            
            core::ptr::write_bytes((stack_top - 1024) as *mut u8, 0, 1024);
            
            let mut sp = stack_top as *mut u32;
            let phdr_addr = program.elf_base as u32 + program.elf_header.e_phoff as u32;
            
            sp = sp.sub(1); *sp = 0;                                        // AT_NULL val
            sp = sp.sub(1); *sp = 0;                                        // AT_NULL type
            sp = sp.sub(1); *sp = 4096;                                     // AT_PAGESZ val
            sp = sp.sub(1); *sp = 6;                                        // AT_PAGESZ type
            sp = sp.sub(1); *sp = program.elf_header.e_phnum as u32;        // AT_PHNUM val
            sp = sp.sub(1); *sp = 5;                                        // AT_PHNUM type
            sp = sp.sub(1); *sp = program.elf_header.e_phentsize as u32;    // AT_PHENT val
            sp = sp.sub(1); *sp = 4;                                        // AT_PHENT type
            sp = sp.sub(1); *sp = phdr_addr;                                // AT_PHDR val
            sp = sp.sub(1); *sp = 3;                                        // AT_PHDR type
            sp = sp.sub(1); *sp = 0;                                        // envp terminator
            
            sp = sp.sub(1); *sp = 0;           // argv[1] == NULL
            sp = sp.sub(1); *sp = argv0_addr;  // argv[0]
            
            sp = sp.sub(1); *sp = 1;
            
            // Align stack to 8 bytes
            if (sp as usize) & 7 != 0 {
                sp = sp.sub(1);
                *sp = 0;
            }
            
            println!("Stack pointer: {:#x}", sp as u32);
            println!("Entry point: {:#x}", program.elf_header.e_entry);
            
            let mut context = elf_loader::ProgramContext {
                user_stack: sp as u32,
                entry: program.elf_header.e_entry as u32,
                arg0: 1,                                    // argc
                arg1: (sp.add(1) as *const u32) as u32,     // argv
                arg2: 0,                                    // envp (NULL)
            };

            println!("Jumping to entry point via trampoline");
            elf_loader::print_program_context(&context);
            
            elf_loader::elf_loader_tramp(core::ptr::addr_of_mut!(context));
        }
    }

    // pub fn switch_to_program(&mut self, program_index: usize) {
    //     println!("Switching to program {}", program_index);
        
    //     virtmem::mmu_disable();
    //     self.map_program_mmu(program_index);
    //     virtmem::mmu_enable();

    //     self.current_program = program_index;
        
    //     unsafe {
    //         let program = self.get_program(program_index);
            
    //         cswitch_tramp(
    //             &program.frame as *const SoftwareInterruptFrame,
    //             program.sp as *mut u8
    //         );
    //     }
    // }
}
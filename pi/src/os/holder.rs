use core::arch::global_asm;
use crate::arch::dev_barrier;
use crate::os::{interrupts, kuser};
use crate::os::virtmem::{self, MemPerm, MemAttr, PageSizes};
use crate::{println, print};
use crate::fat32;
use crate::kmalloc;
use core::cell::SyncUnsafeCell;
use core::arch::asm;
use core::mem::{offset_of, MaybeUninit};
use crate::os::elf_file::ElfHeader;
use crate::os::interrupts::{InterruptFrame};
use crate::os::errno::*;
use crate::os::file::FdTable;
use crate::os::utils;
use crate::os::vfs::Path;

unsafe impl Sync for OSHolder {}

pub static OS_HOLDER: SyncUnsafeCell<MaybeUninit<OSHolder>> = 
    SyncUnsafeCell::new(MaybeUninit::zeroed());

pub const DOM_KERN: u32 = 1;
const ONE_MB: usize = 1024 * 1024;
pub const NUM_PROGRAMS: usize = 3;
const MAX_ELF_SIZE: usize = 1024 * 1024 * 4;
const MAX_STACK_SIZE: usize = 1024 * 1024 * 4;
const MAX_HEAP_SIZE: usize = 1024 * 1024 * 2;
pub const PAGE_SIZE: usize = 4096;
pub const HEAP_PAGES: usize = MAX_HEAP_SIZE / PAGE_SIZE;
// at least 12MB in reserved memory should be enough out of a 16MB page

const MAX_ARGS: usize = 64;
const DEFAULT_ENV: &[&str] = &["PATH=/", "HOME=/", "PWD=/", "TERM=dumb", "USER=root"];


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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Program {
    pub elf: ELF,
    pub stack: Stack,
    pub heap: Heap,
    pub frame: InterruptFrame,
    pub sp: usize,
    pub heap_ptr: usize,
    pub tid: u32,
    pub elf_header: ElfHeader,
    pub elf_base: usize,

    // elf loader return - should i rename this to not be confused with frame.lr, sp?
    pub return_sp: usize,
    pub return_lr: usize,

    pub fds: FdTable,
    pub cwd: Path,
    pub exe: Path, // path this program was exec'd from, for /proc/self/exe

    pub thread_pointer: u32,
    pub clear_child_tid: u32,

    // the rest of the user state that the SVC entry stub doesn't push
    pub user_lr: u32,
    pub spsr: u32,
    pub fpscr: u32,
    pub vfp: [u64; 16],

    pub mmap_pages: [u32; HEAP_PAGES / 32], // heap pages handed out by mmap (from the top), one bit each
    pub sig_ignored: u64, // bit n set = signal n is SIG_IGN
    pub wake_at: u64, // deadline (us since boot) of an in-progress sleep, 0 if not sleeping
}

impl Program {
    // VA ranges of the heap and stack windows
    pub fn heap_range() -> (usize, usize) {
        (offset_of!(Program, heap), offset_of!(Program, heap) + MAX_HEAP_SIZE)
    }
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
    pub fn elf_loader_tramp(data: *const ProgramContext, return_sp: *mut usize, return_lr: *mut usize);
    pub fn elf_loader_return(return_sp: usize, return_lr: usize);
}

#[repr(C)]
pub struct OSHolder {
    pub programs: [*mut Program; NUM_PROGRAMS],
    pub current_program: usize,
    pub active: [bool; NUM_PROGRAMS],
    pub zombie: [bool; NUM_PROGRAMS], // exited, but the parent hasn't collected the status with wait4 yet
    pub exit_status: [u32; NUM_PROGRAMS], // wait4-style: exit code << 8, or the terminating signal
    pub parent: [Option<usize>; NUM_PROGRAMS],
    
    pub should_cswitch: bool,

    pub fs: fat32::fat32_fs_t,
    pub root: fat32::pi_dirent_t
}

unsafe fn hexdump(ptr: *const u8, lines: u32) {
    unsafe {
        for i in 0..lines {
            for j in 0..8 {
                print!("{:0x} ", *(ptr.byte_add(8*i as usize + j)));
            }
            println!();
        }
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

impl OSHolder {
    pub unsafe fn os_holder_mut() -> &'static mut OSHolder {
        unsafe {
            &mut *OS_HOLDER.get().cast::<OSHolder>()
        }
    }

    pub fn init() {
        unsafe {
            core::ptr::write(OS_HOLDER.get().cast::<OSHolder>(), core::mem::zeroed());
            kuser::install_kuser_helpers();
            interrupts::install_interrupts_vbar();
            interrupts::enable_timer_interrupts();         // -> this shit is causing me hell 
            kmalloc::ensure_init();
            
            let holder = OSHolder::os_holder_mut();

            fat32::pi_sd_init();
            let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");
            holder.fs = fat32::fat32_mk(&partition);
            holder.root = fat32::fat32_get_root(&holder.fs);
            println!("Root is directory = {}", holder.root.is_dir_p);

            // initialize program pointers
            for i in 0..NUM_PROGRAMS {
                holder.programs[i] = (0x0200_0000 * i + 0x0100_0000) as *mut Program; // this is actually the address that they are supposed to be mapped to 
                // note: these memory addresses are properly aligned - 
                core::ptr::write_bytes(
                    holder.programs[i] as *mut u8,
                    0,
                    core::mem::size_of::<Program>()
                );
                holder.active[i] = false;
            }            

            for i in 0..NUM_PROGRAMS {
                println!("Program {} has memory location {:p}, active={}",
                    i, holder.programs[i], holder.active[i]);
            }
        }
    }

    pub unsafe fn get_next_active_program_index(&mut self, index: usize) -> usize {
        for offset in 1..(NUM_PROGRAMS + 1) {
            let next_index = (index + offset) % NUM_PROGRAMS;
            if self.active[next_index] {
                return next_index;
            }
        }
        panic!("Somehow, nothing is active.");
    }

    pub unsafe fn get_program_mut(&mut self, index: usize) -> &'static mut Program {
        unsafe {
            &mut *self.programs[index]
        }
    }

    pub unsafe fn get_program(&self, index: usize) -> &'static Program {
        unsafe {
            &*self.programs[index]
        }
    }

    // first slot that is neither running nor a zombie waiting to be reaped
    pub fn free_slot(&self) -> Option<usize> {
        for i in 0..NUM_PROGRAMS {
            if !self.active[i] && !self.zombie[i] {
                return Some(i);
            }
        }
        None
    }

    // the running program, through its physical address (valid with the MMU off)
    pub fn current(&mut self) -> &'static mut Program {
        unsafe { self.get_program_mut(self.current_program) }
    }

    pub fn pid(&self) -> u32 {
        self.current_program as u32 + 1
    }

    pub fn map_program_mmu(&mut self, program_index: usize) {
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

    pub fn setup_elf(&mut self, program_index: usize, prog_name: &str, args: &[&str], argc: usize) -> ProgramContext { // need this to work with out mmu mapping beforehand2
        unsafe {
            let program_ptr = self.programs[program_index];
            let program: &mut Program = &mut *program_ptr;
            program.cwd = Path::ROOT;
            program.exe = Path::resolve(&Path::ROOT, prog_name).expect("bad program name");
            println!("Program CWD is {}", program.cwd.as_str());
            let file = fat32::fat32_read(&self.fs, &self.root, prog_name);

            program.fds.init_stdio();

            let context = self.exec_elf(program_index, (*file).data as *const u8, (*file).n_data, &args[..argc], DEFAULT_ENV)
                .expect("failed to load the program");
            self.active[program_index] = true;
            context
        }
    }

    // Load an ELF into a program slot and build its initial stack. execve uses this too,
    // so it leaves the file descriptors and cwd alone.
    pub fn exec_elf(&mut self, program_index: usize, file_data: *const u8, file_len: usize, args: &[&str], envs: &[&str]) -> Result<ProgramContext, Errno> {
        unsafe {
            let program_ptr = self.programs[program_index];
            let program: &mut Program = &mut *program_ptr;

            if args.len() > MAX_ARGS || envs.len() > MAX_ARGS {
                return Err(E2BIG);
            }
            let mut string_bytes = 0;
            for s in args.iter().chain(envs) {
                string_bytes += s.len() + 1;
            }
            if string_bytes > 64 * 1024 {
                return Err(E2BIG);
            }

            crate::os::elf_file::load_elf_into_program(file_data, file_len, program)?;

            println!("number of program headers: {}", program.elf_header.e_phnum);
            println!("Program entry point (physical): {:#x}", program.elf_header.e_entry);

            program.heap_ptr = (program.heap.data.as_ptr() as usize) - (program_ptr as usize); // need to make sure that you showing the relative heap.data
            // should write a helper for this
            program.mmap_pages = [0; HEAP_PAGES / 32];
            program.thread_pointer = 0;
            program.clear_child_tid = 0;
            program.wake_at = 0;

            // strings go at the top of the user stack: the program can't read kmalloc'd memory,
            // and every execve would leak it
            let mut sp_bytes = program.stack.data.as_mut_ptr().add(program.stack.data.len());

            let mut arg_ptrs = [0u32; MAX_ARGS];
            for i in 0..args.len() {
                sp_bytes = push_string(sp_bytes, args[i].as_bytes());
                arg_ptrs[i] = (sp_bytes as usize - program_ptr as usize) as u32;
            }
            let mut env_ptrs = [0u32; MAX_ARGS];
            for i in 0..envs.len() {
                sp_bytes = push_string(sp_bytes, envs[i].as_bytes());
                env_ptrs[i] = (sp_bytes as usize - program_ptr as usize) as u32;
            }
            sp_bytes = push_string(sp_bytes, program.exe.as_str().as_bytes());
            let execfn = (sp_bytes as usize - program_ptr as usize) as u32;
            sp_bytes = push_string(sp_bytes, b"v6l");
            let platform = (sp_bytes as usize - program_ptr as usize) as u32;
            sp_bytes = sp_bytes.sub(16); // AT_RANDOM: libc seeds its stack canary from these
            utils::fill_random(core::slice::from_raw_parts_mut(sp_bytes, 16));
            let random = (sp_bytes as usize - program_ptr as usize) as u32;

            let phdr_addr = (program.elf_base as u32).wrapping_add(program.elf_header.e_phoff as u32);
            let hwcap = 1 | 2 | 4 | 16 | 64 | 128 | (1 << 15); // swp half thumb fast_mult vfp edsp tls
            let auxv = [
                3, phdr_addr,
                4, program.elf_header.e_phentsize as u32,
                5, program.elf_header.e_phnum as u32,
                6, 4096,
                7, 0, // AT_BASE
                9, program.elf_header.e_entry, // AT_ENTRY
                11, 0, 12, 0, 13, 0, 14, 0, // uid, euid, gid, egid
                15, platform, // AT_PLATFORM
                16, hwcap, // AT_HWCAP
                17, 100, // AT_CLKTCK
                23, 0, // AT_SECURE
                25, random, // AT_RANDOM
                31, execfn, // AT_EXECFN
                0, 0, // AT_NULL
            ];

            // the ABI wants sp 8-byte aligned at argc
            let total_words = auxv.len() + envs.len() + 1 + args.len() + 1 + 1;
            let aligned_sp = (sp_bytes as usize - total_words * 4) & !7;
            let mut sp_words = (aligned_sp + total_words * 4) as *mut u32;

            sp_words = sp_words.sub(auxv.len());
            core::ptr::copy_nonoverlapping(auxv.as_ptr(), sp_words, auxv.len());

            sp_words = sp_words.sub(1);
            *sp_words = 0; // envp[envc] = NULL

            for i in (0..envs.len()).rev() {
                sp_words = sp_words.sub(1);
                *sp_words = env_ptrs[i];
            }

            sp_words = sp_words.sub(1);
            *sp_words = 0; // argv[argc] = NULL

            for i in (0..args.len()).rev() {
                sp_words = sp_words.sub(1);
                *sp_words = arg_ptrs[i];
            }

            sp_words = sp_words.sub(1);
            *sp_words = args.len() as u32;

            program.sp = sp_words as usize - (program_ptr as usize); // normalize the stack and make sure that it is relative to the base
            program.frame = core::mem::zeroed();
            program.frame.lr = program.elf_header.e_entry;
            program.frame.r0 = 0; // rtld_fini: static binaries have none (libc registers a nonzero r0 with atexit)
            program.frame.r1 = (program.sp as *const u32).add(1) as u32;
            program.frame.r2 = 0;
            program.user_lr = 0;
            program.spsr = interrupts::CPSR_USER_MODE; // ARM state, IRQs on
            program.fpscr = 0;
            program.vfp = [0; 16];

            Ok(ProgramContext {
                user_stack: program.sp as u32,
                entry: program.frame.lr as u32,
                arg0: program.frame.r0 as u32,
                arg1: program.frame.r1 as u32,
                arg2: program.frame.r2 as u32,
            })
        }
    }

    pub fn run_elf(&mut self, program_index: usize, context: ProgramContext) {
        unsafe {
            interrupts::disable_interrupts_asm();
            println!("Setting up MMU for program {}", program_index);
            self.map_program_mmu(program_index);

            dev_barrier();
            println!("About to enable MMU");
            virtmem::mmu_enable();
            println!("MMU enabled");
            dev_barrier();

            println!("want to run the following instructions: ");
            hexdump(context.entry as *const u8, 8);

            let holder = OSHolder::os_holder_mut();
            holder.current_program = program_index;

            let program_ptr = 0x0000_0000 as *mut Program;
            let program: &mut Program = &mut *program_ptr;

            println!("Set variables in holder.");


            interrupts::switch_to_user_mode();
            println!("Switched to user mode");
            
            elf_loader_tramp(core::ptr::addr_of!(context), core::ptr::addr_of_mut!(program.return_sp), core::ptr::addr_of_mut!(program.return_lr));
        }
    }
}

// copy a NUL-terminated string just below `sp` and return the new sp
unsafe fn push_string(sp: *mut u8, bytes: &[u8]) -> *mut u8 {
    unsafe {
        let sp = sp.sub(bytes.len() + 1);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), sp, bytes.len());
        *sp.add(bytes.len()) = 0;
        sp
    }
}

pub fn run_busybox() {
    unsafe {
        OSHolder::init();
        let holder = OSHolder::os_holder_mut();
        println!("About to run user program!");

        let busybox_prog_index = 1; 
        let context = holder.setup_elf(busybox_prog_index, "BUSYBOX", &["sh"], 1);
        println!("Launching into program context: user_stack={:x}, entry={:x}, arg0={}, arg1={:x}, arg2={}", context.user_stack, context.entry, context.arg0, context.arg1, context.arg2);
        holder.run_elf(busybox_prog_index, context);
    }
}
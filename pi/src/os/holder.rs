use core::arch::asm;
use core::mem::MaybeUninit;

use crate::os::interrupts::move_table;
use crate::os::virtmem::{MemAttr, MemPerm, PageSizes};
use crate::arch::{dev_barrier};
use crate::{println};
use crate::fat32::{self, fs_manager};
use crate::kmalloc;
use crate::os::virtmem;

// ─── Constants ──────────────────────────────────────────────
const DOM_KERN: u32 = 1;
const DOM_USER: u32 = 2;
const TINY_PAGE: usize = 4096;
const ENOSYS: u32 = (-38i32) as u32;
const EINVAL: u32 = (-22i32) as u32;
const ENOENT: u32 = (-2i32) as u32;
const ECHILD: u32 = (-10i32) as u32;
const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const DIRENT64_BASE: usize = 19;
const NUM_PROGRAMS: usize = 8;
const MAX_ELF_SIZE: usize = 1024 * 1024;
const MAX_STACK_SIZE: usize = 1024 * 64;
const MAX_HEAP_SIZE: usize = 16 * 1024 * 1024;

// ─── Assembly symbols (defined in interrupts.rs global_asm) ──
unsafe extern "C" {
    #[link_name = "switch_to_user_mode"]
    pub fn switch_to_user_mode_asm();
    #[link_name = "switch_to_super_mode"]
    pub fn switch_to_super_mode_asm();
    #[link_name = "fork_trampoline_back"]
    fn fork_trampoline_back(return_pc: u32, return_sp: u32, return_frame: *const SoftwareInterruptFrame);
    pub fn elf_loader_tramp(data: *mut ProgramContext);
    pub fn enable_interrupts_asm();
    pub fn disable_interrupts_asm();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn elf_loader_return() {
    let h = holder();
    let pid = h.current_pid;
    let prog = &h.programs[pid];
    fork_trampoline_back(
        prog.frame.lr,
        prog.sp as u32,
        core::ptr::addr_of!(prog.frame),
    );
    loop {}
}

// ─── Structs ────────────────────────────────────────────────
#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct SoftwareInterruptFrame {
    pub r0: u32, pub r1: u32, pub r2: u32, pub r3: u32,
    pub r4: u32, pub r5: u32, pub r6: u32, pub r7: u32,
    pub r8: u32, pub r9: u32, pub r10: u32, pub r11: u32,
    pub r12: u32, pub lr: u32,
}

#[repr(C)]
struct ElfHeader {
    e_ident: [u8; 16],
    e_type: u16, e_machine: u16, e_version: u32,
    e_entry: usize, e_phoff: usize, e_shoff: usize,
    e_flags: u32, e_ehsize: u16, e_phentsize: u16,
    e_phnum: u16, e_shentsize: u16, e_shnum: u16, e_shstrndx: u16,
}

#[repr(C)]
struct ProgramHeader {
    p_type: u32, p_offset: usize, p_vaddr: usize,
    p_paddr: usize, p_filesz: usize, p_memsz: usize,
    p_flags: u32, p_align: u32,
}

#[repr(C)]
struct ProgramContext {
    user_stack: u32, entry: u32, arg0: u32, arg1: u32, arg2: u32,
}

#[derive(Copy, Clone, PartialEq, Default)]
enum ProgState {
    #[default]
    Free,
    Running,
    Zombie,
}

#[derive(Copy, Clone)]
struct Program {
    elf: [u8; MAX_ELF_SIZE],
    stack: [u8; MAX_STACK_SIZE],
    heap: [u8; MAX_HEAP_SIZE],
    sp: usize,
    heap_ptr: usize,
    tid: u32,
    pid: u32,
    state: ProgState,
    exit_code: u32,
    frame: SoftwareInterruptFrame,
}

impl Default for Program {
    fn default() -> Self {
        Self {
            elf: [0; MAX_ELF_SIZE],
            stack: [0; MAX_STACK_SIZE],
            heap: [0; MAX_HEAP_SIZE],
            sp: 0, heap_ptr: 0, tid: 0, pid: 0,
            state: ProgState::Free, exit_code: 0,
            frame: SoftwareInterruptFrame::default(),
        }
    }
}

struct OSHolder {
    programs: [Program; NUM_PROGRAMS],
    current_pid: usize,
    next_tid: u32,
    interrupt_vector_base: [u8; TINY_PAGE],
    kuser_helpers: [u8; TINY_PAGE],
}

// ─── Mutable syscall state globals ──────────────────────────
static mut PROGRAM_BREAK: u32 = 0;
static mut DIR_FD: u32 = 3;
static mut DIR_BUF: *mut u8 = core::ptr::null_mut();
static mut DIR_BUF_LEN: usize = 0;
static mut DIR_BUF_OFF: usize = 0;
static mut DIR_IDX: usize = 0;

// ─── OSHolder implementation ───────────────────────────────
impl OSHolder {
    fn new() -> Self {
        let mut holder = Self {
            programs: [Program::default(); NUM_PROGRAMS],
            current_pid: 0,
            next_tid: 1,
            interrupt_vector_base: [0u8; TINY_PAGE],
            kuser_helpers: [0u8; TINY_PAGE],
        };

        unsafe {
            let pa = holder.kuser_helpers.as_mut_ptr() as u32;
            core::ptr::copy_nonoverlapping(
                kuser_get_tls as *const u32, (pa + 0x00FF0FA0) as *mut u32, 4);
            core::ptr::copy_nonoverlapping(
                kuser_cmpxchg as *const u32, (pa + 0x00FF0FC0) as *mut u32, 8);
            core::ptr::copy_nonoverlapping(
                kuser_memory_barrier as *const u32, (pa + 0x00FF0FE0) as *mut u32, 2);
            core::ptr::copy_nonoverlapping(
                kuser_version as *const u32, (pa + 0x00FF0FFC) as *mut u32, 4);
        }

        move_table(
            core::ptr::addr_of!(crate::os::interrupts::INTERRUPT_TABLE_START) as usize,
            core::ptr::addr_of!(crate::os::interrupts::INTERRUPT_TABLE_END) as usize,
        );

        holder
    }

    fn find_free(&self) -> Option<usize> {
        for i in 0..NUM_PROGRAMS {
            if self.programs[i].state == ProgState::Free { return Some(i); }
        }
        None
    }

    fn load_elf(&mut self, name: &str, pid: usize) {
        unsafe {
            let mgr = fs_manager::get_fat32_manager();
            let file = (*mgr).read_file(name);
            let elf = &*((*file).data.cast::<ElfHeader>());
            let phs = (*file).data.byte_add(elf.e_phoff).cast::<ProgramHeader>();
            for i in 0..elf.e_phnum {
                let ph = &*phs.add(i as usize);
                if ph.p_type != 1 { continue; }
                core::ptr::copy_nonoverlapping(
                    (*file).data.add(ph.p_offset),
                    self.programs[pid].elf.as_mut_ptr().byte_add(ph.p_offset),
                    ph.p_filesz,
                );
            }
        }
    }

    fn map_program_mmu(&mut self, pid: usize) {
        virtmem::mmu_disable();
        virtmem::mmu_reset();
        let user = MemPerm::perm_rw_user;

        let dev = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_device, PageSizes::mb16);
        let kern = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_cached, PageSizes::mb16);
        let kern4 = virtmem::make_global_pin(DOM_KERN, user, MemAttr::MEM_uncached, PageSizes::kb4);
        let usr16 = virtmem::make_user_pin(DOM_USER, pid as u32 + 1, user, MemAttr::MEM_cached, PageSizes::mb16);
        let usr1 = virtmem::make_user_pin(DOM_USER, pid as u32 + 1, user, MemAttr::MEM_cached, PageSizes::mb1);

        virtmem::pin_mmu_sec(0, 0x2000_0000, 0x2000_0000, dev);

        let vbar = 0x1800_0000;
        unsafe { asm!("mcr p15, 0, {0}, c12, c0, 0", in(reg) vbar, options(nostack, preserves_flags)); }
        virtmem::pin_mmu_sec(1, vbar, self.interrupt_vector_base.as_mut_ptr() as u32, kern4);

        virtmem::pin_mmu_sec(2, 0x1000_0000,
            self.programs[pid].heap.as_mut_ptr() as u32, usr16);
        virtmem::pin_mmu_sec(3, 0x0900_0000 - 1024 * 1024,
            self.programs[pid].stack.as_mut_ptr() as u32, usr1);
        virtmem::pin_mmu_sec(4, 0xff00_0000,
            self.kuser_helpers.as_mut_ptr() as u32, kern4);
    }

    fn launch(&mut self, name: &str, pid: usize) {
        unsafe {
            self.load_elf(name, pid);
            self.map_program_mmu(pid);

            virtmem::pin_mmu_init(!0);
            virtmem::mmu_enable();

            let user_stack_base = 0x0900_0000 - 128 * 4;
            let prog = &mut self.programs[pid];
            prog.heap_ptr = kmalloc::HEAP_CURR;
            prog.state = ProgState::Running;
            prog.pid = pid as u32;

            let argv0 = b"sh\0";
            let av = kmalloc::kmalloc(argv0.len()) as *mut u8;
            core::ptr::copy_nonoverlapping(argv0.as_ptr(), av, argv0.len());
            let av_ptr = av as u32;

            let stack_top = user_stack_base;
            core::ptr::write_bytes((stack_top - 1024) as *mut u8, 0, 1024);

            let mut sp = stack_top as *mut u32;
            let eh = &*(prog.elf.as_ptr().cast::<ElfHeader>());

            sp = sp.sub(1); *sp = 0;
            sp = sp.sub(1); *sp = 0;
            sp = sp.sub(1); *sp = 4096; sp = sp.sub(1); *sp = 6;
            sp = sp.sub(1); *sp = eh.e_phnum as u32; sp = sp.sub(1); *sp = 5;
            sp = sp.sub(1); *sp = eh.e_phentsize as u32; sp = sp.sub(1); *sp = 4;
            sp = sp.sub(1); *sp = eh.e_entry as u32; sp = sp.sub(1); *sp = 3;
            sp = sp.sub(1); *sp = 0;
            sp = sp.sub(1); *sp = av_ptr;
            sp = sp.sub(1); *sp = 1;
            if (sp as usize) & 7 != 0 { sp = sp.sub(1); *sp = 0; }

            let mut ctx = ProgramContext {
                user_stack: sp as u32,
                entry: eh.e_entry as u32,
                arg0: 1,
                arg1: (sp.byte_add(4)) as u32,
                arg2: 0,
            };

            prog.sp = sp as usize;
            prog.frame.lr = ctx.entry;
            self.current_pid = pid;

            switch_to_user_mode_asm();
            elf_loader_tramp(core::ptr::addr_of_mut!(ctx));
        }
    }

    fn zero_program(&mut self, pid: usize) {
        self.programs[pid] = Program::default();
    }

    fn sys_fork(&mut self) -> u32 {
        let child = match self.find_free() {
            Some(p) => p,
            None => return ENOSYS,
        };
        let parent = self.current_pid;
        self.programs[child] = self.programs[parent];
        self.programs[child].tid = self.next_tid;
        self.next_tid += 1;
        self.programs[child].pid = child as u32;
        self.programs[child].state = ProgState::Running;
        self.programs[child].frame.r0 = 0;
        self.programs[child].sp = self.programs[parent].sp;
        self.programs[child].tid
    }

    fn sys_vfork(&mut self) -> u32 { self.sys_fork() }

    fn sys_exit(&mut self, code: u32) {
        let pid = self.current_pid;
        self.programs[pid].exit_code = code;
        self.programs[pid].state = ProgState::Zombie;
        for i in 0..NUM_PROGRAMS {
            if self.programs[i].state == ProgState::Running {
                self.current_pid = i;
                unsafe { elf_loader_return(); }
                return;
            }
        }
        loop {}
    }

    fn sys_exit_group(&mut self, code: u32) { self.sys_exit(code); }

    fn sys_read(&self, fd: u32, buf: *mut u8, len: usize) -> u32 {
        unsafe {
            if fd != 0 {
                if fd == DIR_FD {
                    if buf.is_null() { return EINVAL; }
                    if DIR_BUF.is_null() || DIR_BUF_OFF >= DIR_BUF_LEN { return 0; }
                    let rem = DIR_BUF_LEN - DIR_BUF_OFF;
                    let n = if len < rem { len } else { rem };
                    core::ptr::copy_nonoverlapping(DIR_BUF.add(DIR_BUF_OFF), buf, n);
                    DIR_BUF_OFF += n;
                    n as u32
                } else { EINVAL }
            } else if len == 0 { 0 }
            else if buf.is_null() { EINVAL }
            else {
                let s = core::slice::from_raw_parts_mut(buf, len);
                crate::uart::read_bytes(s) as u32
            }
        }
    }

    fn sys_write(&self, fd: u32, buf: *const u8, len: usize) -> u32 {
        if (fd == 1 || fd == 2) && !buf.is_null() {
            let bytes = unsafe { core::slice::from_raw_parts(buf, len) };
            crate::uart::write_bytes("[prog]".as_bytes());
            crate::uart::write_bytes(bytes);
            crate::uart::write_bytes("[/prog]".as_bytes());
            crate::uart::flush();
            len as u32
        } else { EINVAL }
    }

    fn sys_writev(&self, fd: u32, iov: *const u32, iovcnt: usize) -> u32 {
        if fd != 1 && fd != 2 { return EINVAL; }
        let mut total: u32 = 0;
        for i in 0..iovcnt {
            let base = unsafe { core::ptr::read_volatile(iov.add(i * 2)) } as *const u8;
            let len = unsafe { core::ptr::read_volatile(iov.add(i * 2 + 1)) } as usize;
            if !base.is_null() && len > 0 {
                let bytes = unsafe { core::slice::from_raw_parts(base, len) };
                crate::uart::write_bytes("[prog]".as_bytes());
                crate::uart::write_bytes(bytes);
                crate::uart::write_bytes("[/prog]".as_bytes());
                total = total.wrapping_add(len as u32);
            }
        }
        crate::uart::flush();
        total
    }

    fn sys_open(&self, pathname: *const u8) -> u32 {
        if pathname.is_null() { return EINVAL; }
        let path = unsafe { c_str_to_str(pathname) };
        let path = normalize_path(path);
        if path == "." || path == "/" {
            unsafe { build_root_dir_listing(); }
            unsafe { DIR_IDX = 0; }
            unsafe { DIR_FD }
        } else { ENOENT }
    }

    fn sys_close(&self) -> u32 { 0 }

    fn sys_mmap2(&self, len: usize) -> u32 {
        let ptr = unsafe { kmalloc::kmalloc_aligned(len, 4096) };
        unsafe { core::ptr::write_bytes(ptr, 0, len); }
        ptr as u32
    }

    fn sys_brk(&self, addr: u32) -> u32 {
        unsafe {
            if PROGRAM_BREAK == 0 {
                PROGRAM_BREAK = kmalloc::kmalloc_aligned(4096, 4096) as u32;
            }
            if addr != 0 { PROGRAM_BREAK = addr; }
            PROGRAM_BREAK
        }
    }

    fn sys_getdents64(&self, fd: u32, dirp: *mut u8, count: usize) -> u32 {
        if fd != unsafe { DIR_FD } { return EINVAL; }
        unsafe { build_root_dirents64(dirp, count) }
    }

    fn sys_getcwd(&self, buf: *mut u8) -> u32 {
        unsafe { *buf = b'/'; *buf.add(1) = 0; }
        buf as u32
    }

    fn sys_waitpid(&mut self) -> u32 {
        for i in 0..NUM_PROGRAMS {
            if self.programs[i].state == ProgState::Zombie
                && self.programs[i].pid != self.current_pid as u32
            {
                let tid = self.programs[i].tid;
                self.zero_program(i);
                return tid;
            }
        }
        ECHILD
    }

    fn sys_execve(&self, pathname: *const u8) -> u32 {
        if pathname.is_null() { return EINVAL; }
        let s = unsafe { c_str_to_str(pathname) };
        let cmd = if let Some(pos) = s.rfind('/') { &s[pos + 1..] } else { s };
        match cmd {
            "cat" | "ls" | "mkdir" | "cp" | "env" | "crc32" | "printf" => ENOSYS,
            _ => ENOENT,
        }
    }
}

// ─── Directory helpers ──────────────────────────────────────
unsafe fn build_root_dir_listing() {
    let mgr = fs_manager::get_fat32_manager();
    let dir = fat32::fat32_readdir(&(*mgr).fs, &(*mgr).root);
    let mut total = 0usize;
    for i in 0..dir.ndirents {
        let e = &*dir.dirents.add(i);
        let mut l = 0;
        while l < e.name.len() && e.name[l] != 0 { l += 1; }
        total += l + 1;
    }
    let buf = if total == 0 { core::ptr::null_mut() } else { kmalloc::kmalloc(total) as *mut u8 };
    let mut off = 0usize;
    for i in 0..dir.ndirents {
        let e = &*dir.dirents.add(i);
        let mut l = 0;
        while l < e.name.len() && e.name[l] != 0 { l += 1; }
        if l > 0 { core::ptr::copy_nonoverlapping(e.name.as_ptr(), buf.add(off), l); off += l; }
        if !buf.is_null() { *buf.add(off) = b'\n'; off += 1; }
    }
    DIR_BUF = buf;
    DIR_BUF_LEN = total;
    DIR_BUF_OFF = 0;
}

unsafe fn build_root_dirents64(buf_ptr: *mut u8, buf_len: usize) -> u32 {
    if buf_ptr.is_null() || buf_len == 0 { return EINVAL; }
    let mgr = fs_manager::get_fat32_manager();
    let dir = fat32::fat32_readdir(&(*mgr).fs, &(*mgr).root);
    let mut off = 0usize;
    while DIR_IDX < dir.ndirents {
        let e = &*dir.dirents.add(DIR_IDX);
        let mut nl = 0;
        while nl < e.name.len() && e.name[nl] != 0 { nl += 1; }
        let base = DIRENT64_BASE;
        let mut reclen = base + nl + 1;
        reclen = (reclen + 7) & !7;
        if off + reclen > buf_len { break; }
        let ino = (e.cluster_id as u64).to_le_bytes();
        let ob = ((DIR_IDX + 1) as i64).to_le_bytes();
        let rb = (reclen as u16).to_le_bytes();
        core::ptr::copy_nonoverlapping(ino.as_ptr(), buf_ptr.add(off), ino.len());
        core::ptr::copy_nonoverlapping(ob.as_ptr(), buf_ptr.add(off + 8), ob.len());
        core::ptr::copy_nonoverlapping(rb.as_ptr(), buf_ptr.add(off + 16), rb.len());
        *buf_ptr.add(off + 18) = if e.is_dir_p != 0 { DT_DIR } else { DT_REG };
        if nl > 0 { core::ptr::copy_nonoverlapping(e.name.as_ptr(), buf_ptr.add(off + base), nl); }
        *buf_ptr.add(off + base + nl) = 0;
        let pad = off + base + nl + 1;
        let pl = reclen - (base + nl + 1);
        if pl > 0 { core::ptr::write_bytes(buf_ptr.add(pad), 0, pl); }
        off += reclen;
        DIR_IDX += 1;
    }
    off as u32
}

// ─── Global holder (lazy init) ──────────────────────────────
static mut HOLDER: MaybeUninit<OSHolder> = MaybeUninit::uninit();
static mut HOLDER_INIT: bool = false;

fn holder() -> &'static mut OSHolder {
    unsafe {
        if !HOLDER_INIT {
            let holder_ptr = core::ptr::addr_of_mut!(HOLDER).cast::<OSHolder>();
            core::ptr::write(holder_ptr, OSHolder::new());
            HOLDER_INIT = true;
        }
        &mut *core::ptr::addr_of_mut!(HOLDER).cast::<OSHolder>()
    }
}

// ─── SWI dispatcher ────────────────────────────────────────
unsafe fn kuser_get_tls() -> u32 {
    let tls: u32;
    asm!("mrc p15, 0, {tls}, c13, c0, 3", tls = out(reg) tls, options(nostack));
    tls
}
unsafe fn kuser_cmpxchg(newval: u32, ptr: *mut u32) -> u32 {
    let old: u32;
    asm!("ldr {old}, [{ptr}]", "str {newval}, [{ptr}]",
        ptr = in(reg) ptr, newval = in(reg) newval, old = out(reg) old, options(nostack));
    old
}
unsafe fn kuser_memory_barrier() {
    asm!("mcr p15, 0, {r0}, c7, c10, 5", r0 = in(reg) 0u32, options(nostack));
}
unsafe fn kuser_version() -> u32 { 5 }

pub fn install_kuser_helpers(pa: u32) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            kuser_get_tls as *const u32, (pa + 0x00FF0FA0) as *mut u32, 4);
        core::ptr::copy_nonoverlapping(
            kuser_cmpxchg as *const u32, (pa + 0x00FF0FC0) as *mut u32, 8);
        core::ptr::copy_nonoverlapping(
            kuser_memory_barrier as *const u32, (pa + 0x00FF0FE0) as *mut u32, 2);
        core::ptr::copy_nonoverlapping(
            kuser_version as *const u32, (pa + 0x00FF0FFC) as *mut u32, 4);
    }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn software_interrupt_vector(
    frame: *mut SoftwareInterruptFrame,
    svc_lr: u32,
) -> u32 {
    dev_barrier();
    virtmem::mmu_disable();

    let svc_pc = svc_lr.wrapping_sub(4);
    let instr = unsafe { core::ptr::read_volatile(svc_pc as *const u32) };
    let imm = instr & 0x00ff_ffff;
    let frame = unsafe { &mut *frame };

    let nr = if imm == 0 {
        frame.r7
    } else if (imm & 0x00ff_0000) == 0x0090_0000 {
        imm - 0x0090_0000
    } else {
        imm
    };

    let ret = dispatch_syscall(nr, frame);

    virtmem::mmu_enable();
    dev_barrier();
    ret
}

fn dispatch_syscall(nr: u32, frame: &mut SoftwareInterruptFrame) -> u32 {
    let h = holder();
    match nr {
        0x1 => { h.sys_exit(frame.r0); 0 }
        0x2 => h.sys_fork(),
        0x3 => h.sys_read(frame.r0, frame.r1 as *mut u8, frame.r2 as usize),
        0x4 => h.sys_write(frame.r0, frame.r1 as *const u8, frame.r2 as usize),
        0x5 => h.sys_open(frame.r0 as *const u8),
        0x6 => h.sys_close(),
        0x14 => 0,
        0x2d => h.sys_brk(frame.r0),
        0x36 => 0,
        0x40 => 0,
        0x5b => 0,
        0x72 => h.sys_waitpid(),
        0x92 => h.sys_writev(frame.r0, frame.r1 as *const u32, frame.r2 as usize),
        0xac => 0,
        0xae => 0,
        0xaf => 0,
        0xb => h.sys_execve(frame.r0 as *const u8),
        0xb7 => h.sys_getcwd(frame.r0 as *mut u8),
        0xc0 => h.sys_mmap2(frame.r1 as usize),
        0xc9 => { h.sys_exit_group(frame.r0); 0 }
        0xd9 => h.sys_getdents64(frame.r0, frame.r1 as *mut u8, frame.r2 as usize),
        0xdd => 0,
        0xf8 => {
            let mut found = None;
            for i in 0..NUM_PROGRAMS {
                if h.programs[i].state == ProgState::Zombie
                    && h.programs[i].pid != h.current_pid as u32
                {
                    found = Some(h.programs[i].tid);
                    h.programs[i].state = ProgState::Free;
                    break;
                }
            }
            match found {
                Some(tid) => tid,
                None => { h.sys_exit(frame.r0); 0 }
            }
        }
        0x100 => 1,
        0x18d => {
            unsafe {
                let pathname = frame.r1 as *mut u8;
                let st = frame.r4 as *mut fs_manager::Statx;
                let mut len = 0;
                while *(pathname.add(len)) != 0 && len < 256 { len += 1; }
                let s = core::slice::from_raw_parts(pathname, len);
                let fname = normalize_path(core::str::from_utf8(s).unwrap_or(""));
                let mgr = fs_manager::get_fat32_manager();
                let sp = fat32::fat32_stat(&(*mgr).fs, &(*mgr).root, fname);
                if sp.is_null() { ENOENT }
                else { *st = (*mgr).get_file_stat(fname); 0 }
            }
        }
        0xf0005 => {
            unsafe { asm!("mcr p15, 0, {tls}, c13, c0, 3", tls = in(reg) frame.r0); }
            0
        }
        _ => {
            println!("unknown SVC: {:#x}", nr);
            ENOSYS
        }
    }
}

// ─── Public helpers ────────────────────────────────────────
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

pub unsafe fn c_str_to_str(ptr: *const u8) -> &'static str {
    let mut len = 0;
    while *ptr.add(len) != 0 { len += 1; }
    let bytes = core::slice::from_raw_parts(ptr, len);
    core::str::from_utf8_unchecked(bytes)
}

fn normalize_path(path: &str) -> &str {
    let mut out = path;
    while out.starts_with("./") { out = &out[2..]; }
    if out.ends_with('/') && out.len() > 1 { out = out.trim_end_matches('/'); }
    if out.is_empty() { "." } else { out }
}

pub fn launch_program(name: &str, pid: usize) {
    holder().launch(name, pid);
}

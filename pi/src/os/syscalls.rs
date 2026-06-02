use crate::arch::dev_barrier;
use crate::fat32::{self};
use crate::kmalloc;
use crate::os::holder::{self, OSHolder};
use crate::os::virtmem::{mmu_disable, mmu_enable, mmu_is_enabled};
use crate::os::interrupts::{self, InterruptFrame};
use crate::println;
use crate::print;
use core::arch::asm;
use core::ptr::copy_nonoverlapping;
use core::mem::size_of;

const ENOSYS: u32 = (-38i32) as u32;
const EINVAL: u32 = (-22i32) as u32;
const ENOENT: u32 = (-2i32) as u32;
const CURRENT_TID: u32 = 1;

static mut PROGRAM_BREAK: u32 = 0;
static mut THREAD_POINTER: u32 = 0;
static mut CLEAR_CHILD_TID: u32 = 0;
static mut DID_PRINT_RETURN_LOCATION: bool = false;
static mut DIR_FD: u32 = 3;
static mut DIR_BUF: *mut u8 = core::ptr::null_mut();
static mut DIR_BUF_LEN: usize = 0;
static mut DIR_BUF_OFF: usize = 0;
static mut DIR_IDX: usize = 0;

const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const DIRENT64_BASE: usize = 19;

unsafe fn set_tls(tls: u32) {
	unsafe {
		THREAD_POINTER = tls;
		asm!(
			"mcr p15, 0, {tls}, c13, c0, 3",
			tls = in(reg) tls,
		);
	}
}

fn set_tid_address(tidptr: u32) -> u32 {
	unsafe {
		CLEAR_CHILD_TID = tidptr;
	}
	CURRENT_TID
}

fn decode_syscall_number(frame: &InterruptFrame, instr: u32) -> u32 {
	let imm = instr & 0x00ff_ffff;
	if imm == 0 {
		frame.r7
	} else if (imm & 0x00ff_0000) == 0x0090_0000 {
		imm - 0x0090_0000
	} else {
		imm
	}
}

fn normalize_path(path: &str) -> &str {
	let mut out = path;
	while out.starts_with("./") {
		out = &out[2..];
	}
	if out.ends_with('/') && out.len() > 1 {
		out = out.trim_end_matches('/');
	}
	if out.is_empty() { "." } else { out }
}

fn user_ptr_const(holder: &OSHolder, user_va: u32) -> *const u8 {
	unsafe { (holder.programs[holder.current_program] as *const u8).byte_add(user_va as usize) }
}

fn user_ptr_mut(holder: &OSHolder, user_va: u32) -> *mut u8 {
	unsafe { (holder.programs[holder.current_program] as *mut u8).byte_add(user_va as usize) }
}

unsafe fn c_str_to_str(ptr: *const u8) -> &'static str {
	let mut len = 0;
	while unsafe { *ptr.add(len) } != 0 {
		len += 1;
	}
	let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
	unsafe { core::str::from_utf8_unchecked(bytes) }
}

fn syscall_exit(holder: &mut OSHolder) -> u32 {
	unsafe {
		if CLEAR_CHILD_TID != 0 {
			let tidptr = user_ptr_mut(holder, CLEAR_CHILD_TID);
			core::ptr::write_volatile(tidptr as *mut u32, 0);
		}
		println!("Program finished, calling exit");

		let current_program = holder.get_program_mut(holder.current_program);
		holder.active[holder.current_program] = false;
		println!("Current program id: {}, return sp: {:x}, return lr: {:x}", holder.current_program, current_program.return_sp, current_program.return_lr);

		// if no other program is active, return to loader (exit)
		let any_other = holder.active.iter().enumerate().any(|(idx, &a)| idx != holder.current_program && a);
		if !any_other {
			println!("No other active programs — exiting to loader");
			holder::elf_loader_return(current_program.return_sp, current_program.return_lr);
		} else {
			holder.should_cswitch = true;
			println!("Done with active program");
		}
	}
	0
}

fn syscall_exit_group(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 { // apparently implementing exit_group to do nothing works
	println!("exit_group called with code {}", frame.r0);
    0
}

fn syscall_read(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let fd = frame.r0 as usize;
    let buf_ptr = user_ptr_mut(holder, frame.r1);
    let len = frame.r2 as usize;

    if buf_ptr.is_null() { return EINVAL; }
    if len == 0 { return 0; }

    let file = unsafe { &mut (*holder.programs[holder.current_program]).file_descriptors[fd] };

    if file.special_file == holder::SpecialFileMarker::Stdin {
        let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
        let mut num_bytes: usize = 0;
        let mut tmp = [0u8; 64];

        while num_bytes < len {
            let n = crate::uart::read_bytes(&mut tmp);
            if n == 0 { continue; }

            for i in 0..n {
                if num_bytes < len {
                    let b = tmp[i];
                    buf[num_bytes] = b;
                    num_bytes += 1;
                    if b == b'\n' { return num_bytes as u32; }
                }
            }
        }
        return num_bytes as u32;
    }


    if !file.active || file.data.is_null() {
        return EINVAL;
    }

    let remaining = file.nbytes.saturating_sub(file.pos);
    let to_copy = if len < remaining { len } else { remaining };

    if to_copy > 0 {
        unsafe {
            copy_nonoverlapping(file.data.add(file.pos), buf_ptr, to_copy);
        }
        file.pos += to_copy;
    }

    to_copy as u32
}

fn syscall_write(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let fd = frame.r0 as usize;
    let buf_ptr = user_ptr_const(holder, frame.r1);
    let len = frame.r2 as usize;

    if buf_ptr.is_null() { return EINVAL; }

    let proc = unsafe { &mut *holder.programs[holder.current_program] };
    let file = &mut proc.file_descriptors[fd];

    if file.special_file == holder::SpecialFileMarker::Stderr || file.special_file == holder::SpecialFileMarker::Stdout {
        let bytes = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
        crate::uart::write_bytes("[prog]".as_bytes());
		crate::uart::write_bytes(bytes);
		crate::uart::write_bytes("[/prog]".as_bytes());
        return len as u32;
    }

	if !file.active {
		panic!("trying to write to an inactive file");
	}
    if file.is_directory { 
		panic!("trying to write to a directory");
	 }

    unsafe {
        let mut remaining_space = file.nbytes_alloc.saturating_sub(file.pos);
        if len > remaining_space {
            println!("reallocating memory in write");
            let new_alloc_size = core::cmp::max(file.nbytes_alloc + 1024, file.pos + len);
            unsafe {
                let new_data = kmalloc::kmalloc(new_alloc_size) as *mut u8;
                if new_data.is_null() {
                    return 28; // ENOSPC (Out of space)
                }
                if !file.data.is_null() {
                    core::ptr::copy_nonoverlapping(file.data, new_data, file.nbytes);
                }
                file.data = new_data;
                file.nbytes_alloc = new_alloc_size;
                remaining_space = file.nbytes_alloc - file.pos;
            }
        }

        let to_write = core::cmp::min(len, remaining_space);

        if to_write > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr, file.data.add(file.pos), to_write);
            }
            file.pos += to_write;
            
            if file.pos > file.nbytes {
                file.nbytes = file.pos;
            }
            return to_write as u32;
        }
    }

    0
}


fn syscall_writev(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let fd = frame.r0 as usize;
    let iov_ptr = user_ptr_const(holder, frame.r1) as *const u32;
    let iovcnt = frame.r2 as usize;

    if iov_ptr.is_null() { return EINVAL; }

    let mut total_written: u32 = 0;

    for i in 0..iovcnt {
        unsafe {
            let base_va = core::ptr::read_volatile(iov_ptr.add(i * 2));
            let len = core::ptr::read_volatile(iov_ptr.add(i * 2 + 1)) as usize;

            if len == 0 { continue; }
            let mut temp_frame = *frame;
            temp_frame.r0 = fd as u32;
            temp_frame.r1 = base_va;
            temp_frame.r2 = len as u32;

            let result = syscall_write(holder, &temp_frame);
            if (result as i32) < 0 {
                return if total_written > 0 { total_written } else { result };
            }
            
            total_written += result;
        }
    }

    total_written
}

fn heap_alloc(holder: &mut OSHolder, alloc_len: usize) -> u32 {
    unsafe {
        let program = holder.get_program_mut(holder.current_program);
        let heap_size = core::mem::size_of_val(&program.heap.data);

        if program.heap_ptr == 0 {
            program.heap_ptr = 0;
        }

        let align = 4096usize;
        let mut heap_curr = program.heap_ptr;
        if heap_curr % align != 0 {
            heap_curr = (heap_curr + align - 1) & !(align - 1);
        }

        if heap_curr + alloc_len > heap_size {
            println!("syscall_brk: out of heap: heap_ptr={:#x}, alloc_len={}", heap_curr, alloc_len);
            return EINVAL;
        }

		let program_base = holder.programs[holder.current_program] as usize;
		let user_ptr = ((program.heap.data.as_mut_ptr() as usize - program_base) + heap_curr) as u32;
        program.heap_ptr = heap_curr + alloc_len;
        println!("syscall_brk: allocated {} bytes at {:#x}, new heap_ptr={:#x}", alloc_len, user_ptr, program.heap_ptr);
        user_ptr
    }
}

fn syscall_brk(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 {
	let len = frame.r0 as usize;
	let alloc_len = if len == 0 { 1 } else { len };
	heap_alloc(holder, alloc_len)
}

fn syscall_mmap2(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 {
	let len = frame.r1 as usize;
	let alloc_len = if len == 0 { 1 } else { len };
	heap_alloc(holder, alloc_len)
}

fn syscall_open(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 {
    let pathname = user_ptr_const(holder, frame.r0);
    if pathname.is_null() { return EINVAL; }

    let path = unsafe { normalize_path(c_str_to_str(pathname)) };
    
    let fs_ptr = &mut holder.fs as *mut fat32::fat32_fs_t;
    let root_ptr = &holder.root as *const fat32::pi_dirent_t;
    
    let entry_ptr = unsafe { fat32::fat32_stat(&mut *fs_ptr, &*root_ptr, path) };
    if entry_ptr.is_null() { return ENOENT; }
    let entry = unsafe { *entry_ptr };

    let proc = unsafe { holder.get_program_mut(holder.current_program) };    
    let cwd_copy = proc.cwd; 
    let fd = proc.allocate_file_descriptor();
    let file = proc.get_file(fd);

    file.dirent = entry;
    file.parent = cwd_copy;
    file.pos = 0;
    file.active = true;

    if entry.is_dir_p != 0 {
        file.is_directory = true;
        
        let raw_dir = unsafe { fat32::fat32_readdir(&mut *fs_ptr, &entry) };
        
        let friendly = fat32::get_dir_listing_as_file(&raw_dir);
        file.data = friendly.data;
        file.nbytes = friendly.n_data;

        let binary = fat32::get_dirents64_as_file(&raw_dir);
        file.dirents = binary.data;
        file.nbytes_alloc = binary.n_data; 
    } else {
        file.is_directory = false;
        
        let raw_file = unsafe { fat32::fat32_read(&mut *fs_ptr, &*root_ptr, path) };
        if raw_file.is_null() {
            file.active = false;
            return ENOENT;
        }

        unsafe {
            file.data = (*raw_file).data;
            file.nbytes = (*raw_file).n_data;
            file.nbytes_alloc = (*raw_file).n_alloc;
            file.dirents = core::ptr::null_mut();
        }
    }

    fd as u32
}
fn syscall_getdents64(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let fd = frame.r0 as usize;
    let dirp = user_ptr_mut(holder, frame.r1);
    let count = frame.r2 as usize;
    
    let proc = unsafe { &mut *holder.programs[holder.current_program] };
    let file = &mut proc.file_descriptors[fd];

    if !file.active || !file.is_directory || file.dirents.is_null() {
        return EINVAL;
    }

    let remaining = file.nbytes_alloc.saturating_sub(file.pos);
    let to_copy = core::cmp::min(count, remaining);

    if to_copy > 0 {
        unsafe {
            copy_nonoverlapping(file.dirents.add(file.pos), dirp, to_copy);
        }
        file.pos += to_copy;
    }

    to_copy as u32
}

fn syscall_getcwd(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let buf_ptr = user_ptr_mut(holder, frame.r0);
    let size = frame.r1 as usize;

    if buf_ptr.is_null() {
        return EINVAL;
    }

    let proc = unsafe { holder.get_program(holder.current_program) };
    let mut temp_buf = [0u8; 256];
    let mut total_len: usize = 0;
    if proc.cwd.cluster_id == holder.root.cluster_id {
        temp_buf[0] = b'/';
        temp_buf[1] = 0;
        total_len = 2;
    } else {
        let name = &proc.cwd.name;
        let name_len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        temp_buf[0] = b'/';
        unsafe {
            core::ptr::copy_nonoverlapping(name.as_ptr(), temp_buf.as_mut_ptr().add(1), name_len);
        }
        temp_buf[1 + name_len] = 0;
        total_len = name_len + 2;
    }

    if size < total_len {
        return 34;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(temp_buf.as_ptr(), buf_ptr, total_len);
    }

    frame.r0
}

fn syscall_statx(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
    let pathname_ptr = user_ptr_const(holder, frame.r1);
    let statx_out = user_ptr_mut(holder, frame.r4) as *mut fat32::Statx;

    if pathname_ptr.is_null() || statx_out.is_null() {
        return EINVAL;
    }

    unsafe {
        let path_str = c_str_to_str(pathname_ptr);
        let path = normalize_path(path_str);

        let entry_ptr = fat32::fat32_stat(&holder.fs, &holder.root, path);
        if entry_ptr.is_null() {
            return ENOENT;
        }

        let stats = fat32::get_file_stat(&*entry_ptr);
        core::ptr::write_volatile(statx_out, stats);
        0
    }
}

fn syscall_close(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 {
    let fd = frame.r0 as usize;
    if fd >= holder::NUM_FILE_DESCRIPTORS {
        // return 22; // EINVAL
		panic!("trying to close a file descriptor out of range");
    }

    let proc = unsafe { holder.get_program_mut(holder.current_program) };
    let file = &mut proc.file_descriptors[fd];
    
    if !file.active {
        // return 9; // EBADF
		panic!("trying to close an inactive file descriptor");
    }
    if !file.is_directory && !file.data.is_null() && file.special_file == holder::SpecialFileMarker::NotSpecial {
        let fs_ptr = &holder.fs as *const _ as *mut fat32::fat32_fs_t;
        let pi_file = fat32::pi_file_t {
            data: file.data,
            n_data: file.nbytes,
            n_alloc: file.nbytes_alloc,
        };

        let name_str = unsafe { c_str_to_str(file.dirent.name.as_ptr()) };
        unsafe {
            fat32::fat32_write(
                &*fs_ptr,
                &file.parent,
                name_str,
                &pi_file
            );
        }
    }

    unsafe {
        if !file.data.is_null() {
            file.data = core::ptr::null_mut();
        }

        if !file.dirents.is_null() {
            file.dirents = core::ptr::null_mut();
        }
    }

    file.active = false;
    file.pos = 0;
    file.nbytes = 0;
    file.nbytes_alloc = 0;
    file.is_directory = false;

    0
}

fn syscall_execve(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let pathname = user_ptr_const(holder, frame.r0);
	let argv = user_ptr_mut(holder, frame.r1) as *mut *const u8;

	if pathname.is_null() {
		println!("[execve] pathname is null");
		return EINVAL;
	}

	unsafe {
		let path_str = c_str_to_str(pathname);
		println!("[execve] pathname: {}", path_str);

		let cmd = if let Some(pos) = path_str.rfind('/') {
			&path_str[pos + 1..]
		} else {
			path_str
		};
		println!("[execve] command: {}", cmd);

		let mut argc = 0;
		if !argv.is_null() {
			loop {
				let arg_user = *argv.add(argc);
				if arg_user.is_null() {
					break;
				}
				let translated = user_ptr_const(holder, arg_user as u32);
				let arg_str = c_str_to_str(translated);
				println!("[execve] argv[{}]: {}", argc, arg_str);
				argc += 1;
			}
		}
		println!("[execve] argc: {}", argc);
		match cmd {
			"cat" | "ls" | "mkdir" | "cp" | "env" | "crc32" | "printf" => {
				println!("[execve] recognized busybox applet: {}", cmd);
				ENOSYS
			}
			_ => {
				println!("[execve] unknown command: {}", cmd);
				ENOENT
			}
		}
	}
}

fn syscall_set_tls(frame: &InterruptFrame) -> u32 {
	unsafe { set_tls(frame.r0) };
	0
}

fn syscall_set_tid_address(frame: &InterruptFrame) -> u32 {
	set_tid_address(frame.r0)
}

fn syscall_waitpid(holder: &mut OSHolder, frame: &mut InterruptFrame) -> u32 {
    let pid = frame.r0 as i32; 
    
    println!("Waiting for process with PID: {}", pid);
    
    let should_block = if pid == -1 {
        holder.active.iter()
            .enumerate()
            .any(|(idx, &active)| idx != holder.current_program && active)
    } else if pid > 0 && (pid as usize) <= holder.active.len() {
        holder.active[(pid as usize) - 1]
    } else {
        false
    };

    if should_block {
        println!("Child processes still active, yielding and retrying...");        
        frame.lr = frame.lr.wrapping_sub(4);
        holder.should_cswitch = true;
        frame.r0 
    } else {
        println!("No active target children, done waiting.");
        if pid > 0 {
            pid as u32 
        } else {
            (-10i32) as u32 
        }
    }
}

fn syscall_getpid(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	holder.current_program as u32 + 1 
}

fn syscall_noop(_frame: &InterruptFrame) -> u32 {
	0
}

fn syscall_fork(holder: &mut OSHolder) -> u32 {
    // copy the stuff from this into the next active slot
	unsafe {
		let next_prog_index = holder.get_next_empty_index();
		let new_prog = holder.get_program_mut(next_prog_index);
		core::ptr::copy(
			holder.programs[holder.current_program] as *mut u8,
			holder.programs[next_prog_index] as *mut u8,
			size_of::<holder::Program>()
		);
		holder.active[next_prog_index] = true;

		println!("Finished copying program {} -> {}", holder.current_program, next_prog_index);

		// ensure child appears to return 0
		new_prog.frame.r0 = 0; // this indicates that you are in the forked process

		// diagnostic dump: instructions at lr and stack words at sp
		let pc = new_prog.frame.lr;
		let sp = new_prog.sp as u32;
		println!("fork: new program {} lr={:x} sp={:x}", next_prog_index, pc, sp);
		// print saved registers from the frame
		println!("fork: saved regs: r0={:x} r1={:x} r2={:x} r3={:x} r4={:x} r5={:x} r6={:x} r7={:x}",
			new_prog.frame.r0, new_prog.frame.r1, new_prog.frame.r2, new_prog.frame.r3,
			new_prog.frame.r4, new_prog.frame.r5, new_prog.frame.r6, new_prog.frame.r7);
		println!("fork: saved regs cont: r8={:x} r9={:x} r10={:x} r11={:x} r12={:x} lr={:x}",
			new_prog.frame.r8, new_prog.frame.r9, new_prog.frame.r10, new_prog.frame.r11,
			new_prog.frame.r12, new_prog.frame.lr);
		print!("fork instr bytes:");
		for i in 0..8 {
			let b = *((pc as *const u8).add(i));
			print!(" {:02x}", b);
		}
		println!();
		print!("fork stack words at sp:");
		for i in 0..8 {
			let w = *((sp as *const u32).add(i));
			print!(" {:08x}", w);
		}
		println!();

		println!("returning next program index: {}", next_prog_index as u32 + 1);
		next_prog_index as u32 + 1 // to say which pid needs to be tracked
	}
}

fn syscall_dup2(holder: &mut OSHolder, frame: &InterruptFrame) -> u32 {
    let oldfd = frame.r0 as usize;
    let newfd = frame.r1 as usize;

    if oldfd >= holder::NUM_FILE_DESCRIPTORS || newfd >= holder::NUM_FILE_DESCRIPTORS {
        return 22;
    }

    let proc = unsafe { holder.get_program_mut(holder.current_program) };

    if !proc.file_descriptors[oldfd].active {
        return 9; // EBADF
    }

    if oldfd == newfd {
        return newfd as u32;
    }

    if proc.file_descriptors[newfd].active {
        let mut close_frame = *frame;
        close_frame.r0 = newfd as u32;
        syscall_close(holder, &close_frame);
    }

    let proc = unsafe { holder.get_program_mut(holder.current_program) };
    proc.file_descriptors[newfd] = proc.file_descriptors[oldfd];
    newfd as u32
}

fn dispatch_syscall(holder: &mut OSHolder, frame: &mut InterruptFrame, nr: u32) -> u32 {
	match nr {
		0x1 => syscall_exit(holder),
        0x2 => syscall_fork(holder),
		0x3 => syscall_read(holder, frame),
		0x4 => syscall_write(holder, frame),
		0x5 => syscall_open(holder, frame),
		0x6 => syscall_close(holder, frame),
		0xb => syscall_execve(holder, frame),
		0x14 => syscall_getpid(holder, frame),
		0x2d => syscall_brk(holder, frame),
		0x36 => syscall_noop(frame),
		0x3f => syscall_dup2(holder, frame),
		0x40 => syscall_noop(frame),
		0x5b => syscall_noop(frame),
		0x72 => syscall_waitpid(holder, frame),
		0x92 => syscall_writev(holder, frame),
		0x100 => syscall_set_tid_address(frame),
		0xac => syscall_noop(frame),
		0xae => syscall_noop(frame),
		0xaf => syscall_noop(frame),
		0x7d => syscall_noop(frame),
		0xb7 => syscall_getcwd(holder, frame),
		0xc0 => syscall_mmap2(holder, frame),
		0xc9 => syscall_exit_group(holder, frame),
		0xd9 => syscall_getdents64(holder, frame),
		0xdd => syscall_noop(frame),
		0xf0005 => syscall_set_tls(frame),
		0xf8 => syscall_exit_group(holder, frame),
		0x18d => syscall_statx(holder, frame),
		_ => {
			println!("unknown SVC: {:#x}", nr);
			ENOSYS
		}
	}
}

#[inline(never)]
pub fn handle_software_interrupt(frame: *mut InterruptFrame, svc_lr: u32) -> u32 {
	dev_barrier();
	let should_toggle_mmu = mmu_is_enabled();

	if should_toggle_mmu {
		mmu_disable();
	}

	let svc_pc = svc_lr.wrapping_sub(4);
	let instr = unsafe { core::ptr::read_volatile(svc_pc as *const u32) };
	let frame = unsafe { &mut *frame };
	let nr = decode_syscall_number(frame, instr);

	println!(
		"SWI called: pc={:#x}, instr={:#x}, arg0={:#x}, arg1={:#x}, arg2={:#x}, arg3={:#x}, arg4={:#x}, arg5={:#x}, nr={:#x}",
		svc_pc, instr, frame.r0, frame.r1, frame.r2, frame.r3, frame.r4, frame.r5, nr
	);

	dev_barrier();

	unsafe {
		// there are two options: this is being called from the program, or this is being called in some testing code inside the kernel
		let holder = OSHolder::os_holder_mut(); 
		if holder.active[holder.current_program] {
			mmu_disable(); // disable the MMU
			// let syscall_ret = dispatch_syscall(holder, frame, nr);
			// frame.r0 = syscall_ret; // updating the ret value with this
			// let user_sp = holder::get_user_sp();
			// interrupts::update_current_program_frame(frame, user_sp as usize); // update the current program frame

			let user_sp = holder::get_user_sp();
			interrupts::update_current_program_frame(frame, user_sp as usize);
			let syscall_ret = dispatch_syscall(holder, frame, nr);
			frame.r0 = syscall_ret; 
			interrupts::update_current_program_frame(frame, user_sp as usize);

			// move the program
			holder.current_program = match holder.should_cswitch {
				true => {
					holder.get_next_active_program_index(holder.current_program)
				}
				false => {
					holder.current_program
				}
			};
			holder.should_cswitch = false;

			// enable the mmu
			holder.map_program_mmu(holder.current_program);

			// re-enable the MMU before continuing on with the program
			mmu_enable();

			let mapped_program_ptr = 0x0000_0000 as *mut holder::Program;
            let mapped_program = unsafe { &mut *mapped_program_ptr };
            let mapped_next_frame: InterruptFrame = mapped_program.frame;
			let mapped_next_frame_ptr = &mapped_next_frame as *const InterruptFrame;
			interrupts::fork_trampoline_back(mapped_next_frame.lr, mapped_program.sp as u32, mapped_next_frame_ptr);

			// interrupts::fork_trampoline_back(frame.lr, user_sp, frame as *const InterruptFrame); // -> valid because you are not referencing the pointer memory address

			panic!("should not reach this point of the code");
			// execute the syscall... and save the output to the frame
			// check context switching
			// trampoline back instead of standard return back
		} else {
			dispatch_syscall(holder, frame, nr)
		}
	}
}

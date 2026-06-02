use crate::arch::dev_barrier;
use crate::fat32::{self, fs_manager};
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

unsafe fn build_root_dir_listing() {
	let fs_mgr = unsafe { fs_manager::get_fat32_manager() };
	let dir = unsafe { fat32::fat32_readdir(&(*fs_mgr).fs, &(*fs_mgr).root) };

	let mut total = 0usize;
	for i in 0..dir.ndirents {
		let e = unsafe { &*dir.dirents.add(i) };
		let mut len = 0usize;
		while len < e.name.len() && e.name[len] != 0 {
			len += 1;
		}
		total += len + 1;
	}

	let buf = if total == 0 {
		core::ptr::null_mut()
	} else {
		unsafe { kmalloc::kmalloc(total) as *mut u8 }
	};

	let mut off = 0usize;
	for i in 0..dir.ndirents {
		let e = unsafe { &*dir.dirents.add(i) };
		let mut len = 0usize;
		while len < e.name.len() && e.name[len] != 0 {
			len += 1;
		}
		if len > 0 {
			unsafe { copy_nonoverlapping(e.name.as_ptr(), buf.add(off), len) };
			off += len;
		}
		if !buf.is_null() {
			unsafe { *buf.add(off) = b'\n' };
			off += 1;
		}
	}

	unsafe {
		DIR_BUF = buf;
		DIR_BUF_LEN = total;
		DIR_BUF_OFF = 0;
	}
}

unsafe fn get_root_dirents64(buf_ptr: *mut u8, buf_len: usize) -> u32 {
	if buf_ptr.is_null() || buf_len == 0 {
		return EINVAL;
	}

	let fs_mgr = unsafe { fs_manager::get_fat32_manager() };
	let dir = unsafe { fat32::fat32_readdir(&(*fs_mgr).fs, &(*fs_mgr).root) };
	let mut off = 0usize;

	while unsafe { DIR_IDX } < dir.ndirents {
		let e = unsafe { &*dir.dirents.add(DIR_IDX) };
		let mut name_len = 0usize;
		while name_len < e.name.len() && e.name[name_len] != 0 {
			name_len += 1;
		}

		let base = DIRENT64_BASE;
		let mut reclen = base + name_len + 1;
		reclen = (reclen + 7) & !7;

		if off + reclen > buf_len {
			break;
		}

		unsafe {
			let ino = (e.cluster_id as u64).to_le_bytes();
			let off_bytes = ((DIR_IDX + 1) as i64).to_le_bytes();
			let reclen_bytes = (reclen as u16).to_le_bytes();
			copy_nonoverlapping(ino.as_ptr(), buf_ptr.add(off), ino.len());
			copy_nonoverlapping(off_bytes.as_ptr(), buf_ptr.add(off + 8), off_bytes.len());
			copy_nonoverlapping(reclen_bytes.as_ptr(), buf_ptr.add(off + 16), reclen_bytes.len());
			*buf_ptr.add(off + 18) = if e.is_dir_p != 0 { DT_DIR } else { DT_REG };
			if name_len > 0 {
				copy_nonoverlapping(e.name.as_ptr(), buf_ptr.add(off + base), name_len);
			}
			*buf_ptr.add(off + base + name_len) = 0;
			let pad_start = off + base + name_len + 1;
			let pad_len = reclen - (base + name_len + 1);
			if pad_len > 0 {
				core::ptr::write_bytes(buf_ptr.add(pad_start), 0, pad_len);
			}
		}

		off += reclen;
		unsafe {
			DIR_IDX += 1;
		}
	}

	off as u32
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
	let fd = frame.r0;
	let buf_ptr = user_ptr_mut(holder, frame.r1);
	let len = frame.r2 as usize;

	if fd != 0 {
		unsafe {
			if fd == DIR_FD {
				if buf_ptr.is_null() {
					EINVAL
				} else if DIR_BUF.is_null() || DIR_BUF_OFF >= DIR_BUF_LEN {
					0
				} else {
					let remaining = DIR_BUF_LEN - DIR_BUF_OFF;
					let to_copy = if len < remaining { len } else { remaining };
					copy_nonoverlapping(DIR_BUF.add(DIR_BUF_OFF), buf_ptr, to_copy);
					DIR_BUF_OFF += to_copy;
					to_copy as u32
				}
			} else {
				EINVAL
			}
		}
	} else if len == 0 {
		0
	} else if buf_ptr.is_null() {
		EINVAL
	} else {
		let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
		crate::uart::read_bytes(buf) as u32
	}
}

fn syscall_write(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let fd = frame.r0;
	let buf_ptr = user_ptr_const(holder, frame.r1);
	let len = frame.r2 as usize;

	if (fd == 1 || fd == 2) && !buf_ptr.is_null() {
		println!("writing out with fd={}, buf_ptr={:p}, len={}", fd, buf_ptr, len);
		let bytes = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
		crate::uart::write_bytes("[prog]".as_bytes());
		crate::uart::write_bytes(bytes);
		crate::uart::write_bytes("[/prog]".as_bytes());
		crate::uart::flush();
		len as u32
	} else {
		EINVAL
	}
}

fn syscall_writev(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let fd = frame.r0;
	let iov = user_ptr_const(holder, frame.r1) as *const u32;
	let iovcnt = frame.r2 as usize;

	if fd != 1 && fd != 2 {
		return EINVAL;
	}

	let mut total: u32 = 0;
	for i in 0..iovcnt {
		let base_user_va = unsafe { core::ptr::read_volatile(iov.add(i * 2)) };
		let len = unsafe { core::ptr::read_volatile(iov.add(i * 2 + 1)) } as usize;
		let base = user_ptr_const(holder, base_user_va);
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

fn syscall_open(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let pathname = user_ptr_const(holder, frame.r0);
	if pathname.is_null() {
		return EINVAL;
	}

	let path = unsafe { normalize_path(c_str_to_str(pathname)) };
	if path == "." || path == "/" {
		unsafe {
			build_root_dir_listing();
			DIR_IDX = 0;
			DIR_FD
		}
	} else {
		ENOENT
	}
}

fn syscall_getdents64(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let fd = frame.r0;
	let dirp = user_ptr_mut(holder, frame.r1);
	let count = frame.r2 as usize;
	unsafe {
		if fd != DIR_FD {
			EINVAL
		} else {
			get_root_dirents64(dirp, count)
		}
	}
}

fn syscall_getcwd(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let buf = user_ptr_mut(holder, frame.r0);
	if buf.is_null() {
		return EINVAL;
	}
	unsafe {
		*buf = b'/';
		*buf.add(1) = 0;
	}
	frame.r0
}

fn syscall_statx(holder: &OSHolder, frame: &InterruptFrame) -> u32 {
	let pathname_bytes = user_ptr_mut(holder, frame.r1);
	let statx_out = user_ptr_mut(holder, frame.r4) as *mut fs_manager::Statx;
	unsafe {
		let mut filename_len = 0;
		while *pathname_bytes.add(filename_len) != 0 && filename_len < 256 {
			filename_len += 1;
		}
		let filename_slice = core::slice::from_raw_parts(pathname_bytes, filename_len);
		let filename = core::str::from_utf8(filename_slice).unwrap_or("");
		let filename = normalize_path(filename);

		let fs_mgr = fs_manager::get_fat32_manager();
		let stat_ptr = fat32::fat32_stat(&(*fs_mgr).fs, &(*fs_mgr).root, filename);
		if stat_ptr.is_null() {
			ENOENT
		} else {
			*statx_out = (*fs_mgr).get_file_stat(filename);
			0
		}
	}
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

fn dispatch_syscall(holder: &mut OSHolder, frame: &mut InterruptFrame, nr: u32) -> u32 {
	match nr {
		0x1 => syscall_exit(holder),
        0x2 => syscall_fork(holder),
		0x3 => syscall_read(holder, frame),
		0x4 => syscall_write(holder, frame),
		0x5 => syscall_open(holder, frame),
		0xb => syscall_execve(holder, frame),
		0x14 => syscall_getpid(holder, frame),
		0x2d => syscall_brk(holder, frame),
		0x36 => syscall_noop(frame),
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

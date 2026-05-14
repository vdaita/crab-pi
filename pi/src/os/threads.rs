use crate::os::interrupts;
use crate::{println, print};
use crate::circular::{CircularQueue};
use core::arch::{asm, global_asm};
use crate::ckalloc::{ckalloc, ckfree, SourceLocation};

const MAX_STACK_SIZE: usize = 64 * 1024;
const MAX_THREADS: usize = 4096;
static mut GARBAGE_ROOM: [u32; 128] = [0; 128];

global_asm!(r#"
.globl context_switch
.type context_switch, %function
context_switch:
     push {{r4-r11, lr}}
     str sp, [r0]
     mov sp, r1
     
     pop {{r4-r11, lr}}
     bx lr

.globl init_trampoline
.type init_trampoline, %function
init_trampoline:
    mov lr, r4
    mov r0, r5
    blx lr
    mov r0, #0
    bl rpi_exit
"#);

unsafe extern "C" {    
    #[link_name = "context_switch"]
    fn context_switch(cur_sp_loc: *mut u32, next_sp: u32);
    
    #[link_name = "init_trampoline"]
    fn init_trampoline();
}

#[repr(C)]
struct Thread {
    sp: u32, // where is my stack pointer?
    tid: u32,
    function: *mut u32,
    args: *mut u32,
    stack: [u32; MAX_STACK_SIZE]
}

pub struct ThreadManager {
    running_queue: CircularQueue<*mut Thread, MAX_THREADS>,
    
    current_thread: *mut Thread,    
    scheduler_thread: *mut Thread,
    tid_counter: u32
}

impl ThreadManager {
    pub fn new() -> Self {
        let scheduler_thread: *mut Thread = ckalloc(
            core::mem::size_of::<Thread>(),
            SourceLocation {
                file: "threads.rs",
                func: "new",
                lineno: 0
            }
        ) as *mut Thread;
        unsafe { (*scheduler_thread).tid = 0; }
        
        ThreadManager {
            running_queue: CircularQueue::new(true),
            current_thread: scheduler_thread,
            scheduler_thread: scheduler_thread,
            tid_counter: 1
        }
    }
        
    pub fn thread_yield(&mut self) {
         match self.running_queue.pop_nonblock() {
             Some(thread_ptr) => {
                 let old_thread: *mut Thread = self.current_thread;
                 self.running_queue.push(old_thread);
                 unsafe {
                    println!("switching from tid={} to tid={} \n", (*old_thread).tid, (*thread_ptr).tid);
                    context_switch(
                        core::ptr::addr_of_mut!((*old_thread).sp),
                        (*thread_ptr).sp
                    );
                }
             }
             None => {
                 return;
             }
         }
    }
     
    pub fn thread_start(&mut self) {       
        match self.running_queue.pop_nonblock() {
            Some(thread_ptr) => {
                self.current_thread = thread_ptr;
                unsafe {
                    context_switch(
                        core::ptr::addr_of_mut!((*self.scheduler_thread).sp),
                        (*self.current_thread).sp
                    );
                }
            }
            None => {
                println!("No more threads.");
                return;
            }
        }
    }
    
    pub fn thread_fork(&mut self, function_ptr: fn(), arguments: *const u32) {
        let new_thread: *mut Thread = ckalloc(
            core::mem::size_of::<Thread>(),
            SourceLocation {
                file: "threads.rs",
                func: "thread_fork",
                lineno: 0
            }
        ) as *mut Thread;
        
        unsafe {
            (*new_thread).tid = self.tid_counter;
            self.tid_counter = self.tid_counter + 1;
            
            (*new_thread).stack[MAX_STACK_SIZE - 9 + 0] = function_ptr as u32;
            (*new_thread).stack[MAX_STACK_SIZE - 9 + 1] = arguments as u32;
            (*new_thread).stack[MAX_STACK_SIZE - 9 + 8] = init_trampoline as u32;
            (*new_thread).sp = core::ptr::addr_of_mut!(
                (*new_thread).stack[MAX_STACK_SIZE - 9]
            ) as u32;
            
            println!(
                "thread fork: tid={}, code={:x}, arg={:x}, current sp={:x}, init_trampoline_ptr={:x}", 
                self.tid_counter, function_ptr as u32, arguments as u32, (*new_thread).sp, (*new_thread).stack[MAX_STACK_SIZE - 9 + 8]
            );
        }
        
    }
}


static mut thread_manager: *mut ThreadManager = core::ptr::null_mut();

#[unsafe(no_mangle)]
pub extern "C" fn rpi_exit(exit_code: u32) {
    unsafe {
        if (thread_manager.is_null()) {
            panic!("Thread manager is null when running rpi_exit, this is unexpected");
        }

        let next_thread = match ((*thread_manager).running_queue.is_empty()) {
            true => {
                println!("Returning to scheduler");
                (*thread_manager).scheduler_thread
            }
            false => {
                (*thread_manager).running_queue.pop_nonblock().unwrap()
            }
        };
        
        let finished_thread = (*thread_manager).current_thread;
        (*thread_manager).current_thread = next_thread;
        ckfree(finished_thread as *mut u32);
        
        unsafe {
            context_switch(
                core::ptr::addr_of_mut!(GARBAGE_ROOM) as *mut u32, // you don't actually care where this is written to, as long as someone else ain't reading it
                (*next_thread).sp
            );
        }
    }
}

pub fn test_threads() {
    unsafe { 
        thread_manager = ckalloc(
            core::mem::size_of::<ThreadManager>(),
            SourceLocation {
                file: "threads.rs",
                func: "test_threads",
                lineno: 0
            }
        ) as *mut ThreadManager;
        core::ptr::write(thread_manager, ThreadManager::new());

        // Test function 1: Simple counter
        fn thread_func1() {
            for i in 0..5 {
                println!("Thread 1: iteration {}", i);
                unsafe { (*thread_manager).thread_yield(); }
            }
            println!("Thread 1: completed!");
        }
        
        // Test function 2: Another counter
        fn thread_func2() {
            for i in 0..3 {
                println!("  Thread 2: iteration {}", i);
                unsafe { (*thread_manager).thread_yield(); }
            }
            println!("  Thread 2: completed!");
        }
        
        // Test function 3: With argument usage
        fn thread_func3() {
            for i in 0..4 {
                println!("    Thread 3: iteration {}", i);
                unsafe { (*thread_manager).thread_yield(); }
            }
            println!("    Thread 3: completed!");
        }
        
        println!("=== Starting Thread Test ===\n");
        
        // Fork some threads
        (*thread_manager).thread_fork(thread_func1, core::ptr::null());
        (*thread_manager).thread_fork(thread_func2, core::ptr::null());
        (*thread_manager).thread_fork(thread_func3, core::ptr::null());
        
        println!("Forked 3 threads, starting scheduler...\n");
        
        // Start the scheduler - this will run until all threads complete
        (*thread_manager).thread_start();
        
        println!("\n=== All threads completed ===");
        ckfree(thread_manager as *mut u32);
        thread_manager = core::ptr::null_mut();
    }
}
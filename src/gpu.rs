use core::ptr::{read_volatile, write_volatile};
use crate::println;

const MAILBOX_BASE: usize = 0x2000_B880;
const MAILBOX_READ: *mut u32 = (MAILBOX_BASE + 0x00) as *mut u32;
const MAILBOX_STATUS: *mut u32 = (MAILBOX_BASE + 0x18) as *mut u32;
const MAILBOX_WRITE: *mut u32 = (MAILBOX_BASE + 0x20) as *mut u32;

const MAILBOX_FULL: u32 = 1 << 31;
const MAILBOX_EMPTY: u32 = 1 << 30;

const V3D_BASE: u32 = 0x20C0_0000;
const V3D_SRQSC: *mut u32 = (V3D_BASE + 0x418) as *mut u32;
const V3D_L2CACTL: *mut u32 = (V3D_BASE + 0x020) as *mut u32;
const V3D_SLCACTL: *mut u32 = (V3D_BASE + 0x024) as *mut u32;
const V3D_SRQPC: *mut u32 = (V3D_BASE + 0x0430) as *mut u32;
const V3D_SRQUA: *mut u32 = (V3D_BASE + 0x0434) as *mut u32;
const V3D_SRQCS: *mut u32 = (V3D_BASE + 0x043c) as *mut u32;
const V3D_DBCFG: *mut u32 = (V3D_BASE + 0x0e00) as *mut u32;
const V3D_DBQITE: *mut u32 = (V3D_BASE + 0x0e2c) as *mut u32;
const V3D_DBQITC: *mut u32 = (V3D_BASE + 0x0e30) as *mut u32;

pub const GPU_BASE: u32 = 0x40000000;
static ADD_VECTOR_GPU_CODE: &[u8] = include_bytes!("gpu_kernels/add_kernel.bin");
static DEADBEEF_GPU_CODE: &[u8] = include_bytes!("gpu_kernels/deadbeef.bin");
const GPU_MEM_FLAG: u32 = 0xC;
const MAX_VIDEOCORE_CORES: usize = 16;

unsafe fn mbox_write(channel: u8, data: u32) {
    while(read_volatile(MAILBOX_STATUS) & MAILBOX_FULL != 0) {
        core::hint::spin_loop();
    }
    
    let message = (data & !0xF) | (channel as u32 & 0xF);
    write_volatile(MAILBOX_WRITE, message);
}

unsafe fn mbox_read(channel: u8) -> u32 {
    loop {
        let data = read_volatile(MAILBOX_READ);
        if (data & 0xF) == (channel as u32) {
            return data & !0xF;
        }
    }
}

unsafe fn mbox_property(message: &mut [u32]) -> bool {
    let ptr = message.as_ptr() as u32;
    if ptr & 0xF != 0 {
        return false;
    }
    mbox_write(8, ptr);
    while mbox_read(8) != ptr {
        core::hint::spin_loop();
    }

    if (message[1] == 0x8000_0000) {
        true
    } else {
        println!("Message doesnt' have 8000_0000 in first index");
        for (i, &val) in message.iter().enumerate() {
            println!("  message[{}] = 0x{:08x}", i, val);
        }
        false
    }
}   

unsafe fn mem_alloc(size: u32, align: u32, flags: u32) -> u32 {
    #[repr(C)]
    #[repr(align(16))]
    struct Msg([u32; 9]);
    
    let mut msg = Msg([0; 9]);
    msg.0[0] = 9 * 4;          // Total size in bytes
    msg.0[1] = 0x0000_0000;    // Process request
    msg.0[2] = 0x0003_000c;    // Tag: Allocate Memory
    msg.0[3] = 12;             // Request size (3 * 4)
    msg.0[4] = 12;             // Response size
    msg.0[5] = size;
    msg.0[6] = align;
    msg.0[7] = flags;
    msg.0[8] = 0;              // End tag

    if mbox_property(&mut msg.0) { msg.0[5] } else { 0 }
}

unsafe fn mem_lock(handle: u32) -> u32 {
    #[repr(C)]
    #[repr(align(16))]
    struct Msg([u32; 7]);
    
    let mut msg = Msg([0; 7]);
    msg.0[0] = 7 * 4;
    msg.0[1] = 0x0000_0000;
    msg.0[2] = 0x0003_000d;    // Tag: Lock Memory
    msg.0[3] = 4;
    msg.0[4] = 4;
    msg.0[5] = handle;
    msg.0[6] = 0;

    if mbox_property(&mut msg.0) { msg.0[5] } else { 0 }
}

unsafe fn mem_unlock(handle: u32) -> u32 {
    #[repr(C)]
    #[repr(align(16))]
    struct Msg([u32; 7]);
    
    let mut msg = Msg([0; 7]);
    msg.0[0] = 7 * 4;
    msg.0[1] = 0x0000_0000;
    msg.0[2] = 0x0003_000e;    // Tag: Unlock Memory
    msg.0[3] = 4;
    msg.0[4] = 4;
    msg.0[5] = handle;
    msg.0[6] = 0;

    if mbox_property(&mut msg.0) { msg.0[5] } else { 0 }
}

unsafe fn mem_free(handle: u32) -> u32 {
    #[repr(C)]
    #[repr(align(16))]
    struct Msg([u32; 7]);
    
    let mut msg = Msg([0; 7]);
    msg.0[0] = 7 * 4;
    msg.0[1] = 0x0000_0000;
    msg.0[2] = 0x0003_000f;    // Tag: Free Memory
    msg.0[3] = 4;
    msg.0[4] = 4;
    msg.0[5] = handle;
    msg.0[6] = 0;

    if mbox_property(&mut msg.0) { msg.0[5] } else { 0 }
}

unsafe fn qpu_enable(enable: u32) -> bool {
    #[repr(C)]
    #[repr(align(16))]
    struct Msg([u32; 7]);
    
    let mut msg = Msg([0; 7]);
    msg.0[0] = 7 * 4;
    msg.0[1] = 0x0000_0000;
    msg.0[2] = 0x0003_0012;    // Tag: Enable QPU
    msg.0[3] = 4;
    msg.0[4] = 4;
    msg.0[5] = enable;
    msg.0[6] = 0;

    mbox_property(&mut msg.0)
}

unsafe fn gpu_fft_base_exec_direct(code: u32, unifs: &[u32], num_qpus: u32) {
    write_volatile(V3D_DBCFG,  0); // Disallow IRQ
    write_volatile(V3D_DBQITE, 0); // Disable IRQ
    write_volatile(V3D_DBQITC, !0); // Resets IRQ flags
    write_volatile(V3D_L2CACTL, 1 << 2); // Clear L2 Cache
    write_volatile(V3D_SLCACTL, !0); // Clear other caches

    write_volatile(V3D_SRQCS, (1 << 7) | (1 << 8) | (1 << 16)); // Reset err bit and counts
    for q in 0..num_qpus {
        write_volatile(V3D_SRQUA, unifs[q as usize]);
        write_volatile(V3D_SRQPC, code); 
    }

    while (((read_volatile(V3D_SRQCS) >> 16) & 0xff) != num_qpus) { 
        core::hint::spin_loop();
        // println!("QPUs finished: {}", (read_volatile(V3D_SRQCS) >> 16) & 0xff);
    }
}
/// Common GPU kernel definition and implementation.
macro_rules! define_gpu_kernel {
    ($name:ident, $code_static:ident) => {
        #[repr(C)]
        #[repr(align(16))]
        pub struct $name {
            pub output: [u32; 64],
            pub data: [[u32; 256]; 4],
            pub code: [u32; 128],
            pub unif: [[u32; MAX_VIDEOCORE_CORES]; 4],
            pub mail: [u32; 2],
            pub handle: u32,
        }

        impl $name {
            pub unsafe fn init() -> *mut $name {
                crate::arch::gcc_mb();
                if !qpu_enable(1) {
                    panic!("Failed to enable GPU");
                }
                crate::arch::gcc_mb();

                let handle = mem_alloc(core::mem::size_of::<$name>() as u32, 4096, GPU_MEM_FLAG);
                if handle == 0 {
                    qpu_enable(0);
                    panic!("Failed to allocate GPU memory");
                }
                let vc = mem_lock(handle);

                let ptr = (vc - GPU_BASE) as *mut $name;
                if ptr.is_null() {
                    mem_unlock(handle);
                    mem_free(handle);
                    qpu_enable(0);
                    panic!("Failed to convert handle to GPU bus address.");
                }

                (*ptr).handle = handle;
                let dst = (*ptr).code.as_mut_ptr() as *mut u8;
                let src = $code_static.as_ptr();
                let len = $code_static.len();
                core::ptr::copy_nonoverlapping(src, dst, len);

                let code_offset = (&(*ptr).code as *const _ as u32) - (ptr as u32);
                let unif_offset = (&(*ptr).unif as *const _ as u32) - (ptr as u32);
                (*ptr).mail[0] = vc + code_offset;
                (*ptr).mail[1] = vc + unif_offset;

                (*ptr).unif[0][0] = (ptr as u32 + ((&(*ptr).data[0] as *const _ as u32) - (ptr as u32)));
                (*ptr).unif[0][1] = (ptr as u32 + ((&(*ptr).data[1] as *const _ as u32) - (ptr as u32)));
                (*ptr).unif[0][2] = (ptr as u32 + ((&(*ptr).output as *const _ as u32) - (ptr as u32)));
                ptr
            }

            pub unsafe fn execute(&mut self, num_cores: u32) {
                crate::arch::gcc_mb();
                gpu_fft_base_exec_direct(self.mail[0], &[self.mail[1]], num_cores);
            }

            pub unsafe fn release(&mut self) {
                mem_unlock(self.handle);
                mem_free(self.handle);
                qpu_enable(0);
            }
        }
    };
}

define_gpu_kernel!(AddVectorGpu, ADD_VECTOR_GPU_CODE);
define_gpu_kernel!(DeadbeefGpu, DEADBEEF_GPU_CODE);
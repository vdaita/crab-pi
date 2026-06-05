use crate::arch::dev_barrier;
use crate::mem::{get32, put32};
use crate::println;
use crate::gpio;
use crate::kmalloc;
use crate::timer::Timer;
use core::ptr::addr_of;

// NOTE: this won't work unless you adjust the memory layout to support this abomination

const _2DMODE:          u32 = 1 << 1;
const TI_DST_INC:       u32 = 1 << 4;
const TI_DST_INC_WIDE:  u32 = 1 << 5;
const TI_SRC_INC:       u32 = 1 << 8;
const TI_SRC_INC_WIDE:  u32 = 1 << 9;

const DMA_BASE:         usize = 0x2000_7000;
const DMA_ENABLE:       usize = DMA_BASE + 0xff0;
const DMA_CS_ACTIVE:    u32   = 1 << 0;
const DMA_CS_RESET:     u32   = 1 << 31;

const GPIO_BASE:        usize = 0x2020_0000;
const PARTHIV_PIN:      u32   = 27;

static PARTHIV_BIT: u32 = 1 << PARTHIV_PIN;

static mut ADD8S:   [u8; 131072] = [0; 131072];
static mut CARRY8S: [u8; 131072] = [0; 131072];

static mut OUT_SUM:   u8 = 0;
static mut OUT_CARRY: u8 = 0;
static mut A_VAL:     u8 = 0;
static mut B_VAL:     u8 = 0;

static SCRATCH:       u32 = 0;

static mut ADD32_A:           u32 = 0;
static mut ADD32_B:           u32 = 0;
static mut ADD32_OUT:         u32 = 0;
static mut ADD32_OUT_CARRY:   u32 = 0;

static mut DISPATCH_BLOCKS: [ControlBlock; 8] = [ControlBlock::ZERO; 8];
static mut PATCH_A_BLOCKS:  [ControlBlock; 8] = [ControlBlock::ZERO; 8];

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct BusAddr(pub u32);

impl BusAddr {
    pub fn from_arm(addr: u32) -> Self {
        if addr < 0x2000_0000 {
            BusAddr(addr | 0x4000_0000)
        } else {
            BusAddr(addr - 0x2000_0000 + 0x7e00_0000)
        }
    }

    pub fn from_bus(addr: u32) -> Self {
        BusAddr(addr)
    }

    pub fn to_arm(self) -> u32 {
        if self.0 >= 0x7e00_0000 {
            self.0 - 0x7e00_0000 + 0x2000_0000
        } else {
            self.0 & !0x4000_0000
        }
    }

    pub fn add(self, offset: i32) -> Self {
        BusAddr(self.0.wrapping_add(offset as u32))
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, align(32))]
struct ControlBlock {
    transfer_info: u32,
    src_addr:      BusAddr,
    dest_addr:     BusAddr,
    transfer_len:  u32,
    stride:        u32,
    next_cb:       BusAddr,
}

impl ControlBlock {
    const ZERO: Self = Self {
        transfer_info: 0,
        src_addr:      BusAddr(0),
        dest_addr:     BusAddr(0),
        transfer_len:  0,
        stride:        0,
        next_cb:       BusAddr(0),
    };

    pub fn new(src: BusAddr, dst: BusAddr, len: u32) -> Self {
        let wide = if len % 32 == 0 { TI_DST_INC_WIDE | TI_SRC_INC_WIDE } else { 0 };
        let ti   = TI_DST_INC | TI_SRC_INC | wide;
        ControlBlock {
            transfer_info: ti,
            src_addr:      src,
            dest_addr:     dst,
            transfer_len:  len,
            stride:        0,
            next_cb:       BusAddr(0),
        }
    }
}

struct DMA {
    channel_addr: usize,
}

impl DMA {
    fn new(channel: usize) -> Self {
        assert!(matches!(channel, 4 | 5 | 8 | 9 | 10), "bad DMA channel");
        let channel_addr = DMA_BASE + 0x100 * channel;
        dev_barrier();
        put32(
            DMA_ENABLE as u32,
            get32(DMA_ENABLE as u32) | (1 << channel),
        );
        dev_barrier();
        DMA { channel_addr }
    }

    fn write(&self, offset: usize, val: u32) {
        put32((self.channel_addr + offset) as u32, val);
    }

    fn read(&self, offset: usize) -> u32 {
        get32((self.channel_addr + offset) as u32)
    }

    fn is_active(&self) -> bool {
        self.read(0x00) & DMA_CS_ACTIVE != 0
    }

    fn start(&self, first_cb: &ControlBlock) {
        assert!(!self.is_active(), "DMA already running");
        let bus = BusAddr::from_arm(first_cb as *const _ as u32);
        dev_barrier();
        self.write(0x04, bus.0);
        self.write(0x00, DMA_CS_ACTIVE);
        dev_barrier();
    }

    fn wait(&self) {
        while self.is_active() {}
        dev_barrier();
    }
}

unsafe fn get_heap_cb(count: usize) -> &'static mut ControlBlock {
    &mut *(kmalloc::kmalloc_aligned(
            count * core::mem::size_of::<ControlBlock>(), 32
    ) as *mut ControlBlock)
}

fn dma_test_blink() {
    unsafe {
        let dma = DMA::new(5);

        gpio::set_output(PARTHIV_PIN);

        let bus_gpio_set    = BusAddr::from_arm(gpio::GPIO_SET0 as u32);
        let bus_gpio_clr    = BusAddr::from_arm(gpio::GPIO_CLR0 as u32);
        let bus_parthiv_bit = BusAddr::from_arm(addr_of!(PARTHIV_BIT) as u32);

        kmalloc::kmalloc_init_mb(16);

        const scratch_len: usize = 1024 * 32;
        let scratch_src = kmalloc::kmalloc(scratch_len);
        let bus_scratch_src = BusAddr::from_arm(scratch_src as u32);

        const BLINKS:    usize = 7;
        const DELAY_LEN: usize = 5_000;
        const BLOCKS_NEEDED: usize = BLINKS * (1 + DELAY_LEN + 1 + DELAY_LEN) + 1;

        let blocks = kmalloc::kmalloc_aligned(
            BLOCKS_NEEDED * core::mem::size_of::<ControlBlock>(),
            32,
        ) as *mut ControlBlock;

        let mut i = 0usize;
        for _ in 0..BLINKS {
            *blocks.add(i) = ControlBlock::new(bus_parthiv_bit, bus_gpio_set, 4);
            i += 1;
            for _ in 0..DELAY_LEN {
                *blocks.add(i) = ControlBlock::new(bus_scratch_src, bus_scratch_src, scratch_len as u32);
                i += 1;
            }
            *blocks.add(i) = ControlBlock::new(bus_parthiv_bit, bus_gpio_clr, 4);
            i += 1;
            for _ in 0..DELAY_LEN {
                *blocks.add(i) = ControlBlock::new(bus_scratch_src, bus_scratch_src, scratch_len as u32);
                i += 1;
            }
        }

        for j in 0..i - 1 {
            (*blocks.add(j)).next_cb = BusAddr::from_arm(blocks.add(j + 1) as u32);
        }
        (*blocks.add(i - 1)).next_cb = BusAddr(0);

        dma.start(&*blocks);
        dma.wait();
        println!("Completed DMA blink");
    }
}

fn dma_test_add8() {
    unsafe {
        for a in 0usize..256 {
            for b in 0usize..256 {
                let idx = a * 256 + b;
                ADD8S[idx]   = ((a + b) & 0xff) as u8;
                CARRY8S[idx] = if (a + b) >= 256 { 1 } else { 0 };
            }
        }

        let bus_out_sum   = BusAddr::from_arm(addr_of!(OUT_SUM)   as u32);
        let bus_out_carry = BusAddr::from_arm(addr_of!(OUT_CARRY) as u32);
        let bus_scratch   = BusAddr::from_arm(addr_of!(SCRATCH)   as u32);

        // entry[a][b] is at jump_base + a*65536 + b*256
        // cb0: copy ADD8S[a*256+b] -> OUT_SUM, next -> cb1
        // cb1: copy CARRY8S[a*256+b] -> OUT_CARRY, next -> 0
        // dispatch.next_cb = jump_base_bus | (a<<16) | (b<<8) = entry[a][b]
        const JUMP_TABLE_SIZE: usize = 16 * 1024 * 1024;
        const ENTRY_SIZE_BYTES: usize = 256;
        const CBS_PER_ENTRY: usize = ENTRY_SIZE_BYTES / 32; // 8

        kmalloc::kmalloc_init_mb_with_offset(32, JUMP_TABLE_SIZE); // also resets the heap lol
        let jump_base = kmalloc::kmalloc_aligned(JUMP_TABLE_SIZE, JUMP_TABLE_SIZE) as usize;
        let jump_table = jump_base as *mut ControlBlock;

        // fill jump table
        for a in 0usize..256 {
            for b in 0usize..256 {
                let idx      = a * 256 + b;
                let cb_base  = idx * CBS_PER_ENTRY;

                let bus_sum   = BusAddr::from_arm(addr_of!(ADD8S[idx])   as u32);
                let bus_carry = BusAddr::from_arm(addr_of!(CARRY8S[idx]) as u32);

                let cb0 = &mut *jump_table.add(cb_base);
                *cb0 = ControlBlock::new(bus_sum, bus_out_sum, 1);
                cb0.next_cb = BusAddr::from_arm(jump_table.add(cb_base + 1) as u32);

                let cb1 = &mut *jump_table.add(cb_base + 1);
                *cb1 = ControlBlock::new(bus_carry, bus_out_carry, 1);
                cb1.next_cb = BusAddr(0);
            }
        }

        let dispatch = get_heap_cb(1);
        let patch_a  = get_heap_cb(1);
        let patch_b  = get_heap_cb(1);

        // dispatch: nop, next_cb pre-filled with jump_base_bus
        let jump_base_bus = BusAddr::from_arm(jump_base as u32).0;
        *dispatch = ControlBlock::new(bus_scratch, bus_scratch, 4);
        dispatch.next_cb = BusAddr(jump_base_bus);

        let dispatch_next_cb = (dispatch as *mut ControlBlock as u32)
            + core::mem::offset_of!(ControlBlock, next_cb) as u32;

        // patch_b: b_val -> dispatch.next_cb byte1
        *patch_b = ControlBlock::new(
            BusAddr::from_arm(addr_of!(B_VAL) as u32),
            BusAddr::from_arm(dispatch_next_cb + 1),
            1,
        );

        // patch_a: a_val -> dispatch.next_cb byte2
        *patch_a = ControlBlock::new(
            BusAddr::from_arm(addr_of!(A_VAL) as u32),
            BusAddr::from_arm(dispatch_next_cb + 2),
            1,
        );

        // chain: patch_b -> patch_a -> dispatch -> entry[a][b]
        patch_b.next_cb = BusAddr::from_arm(patch_a as *const _ as u32);
        patch_a.next_cb = BusAddr::from_arm(dispatch as *const _ as u32);

        let dma = DMA::new(5);
        let test_cases: [(u8, u8); 6] = [
            (0,   0),
            (1,   2),
            (10,  20),
            (100, 200),
            (255, 1),
            (255, 255),
        ];

        for (a, b) in test_cases {
            core::ptr::write_volatile(addr_of!(OUT_SUM)   as *mut u8, 0);
            core::ptr::write_volatile(addr_of!(OUT_CARRY) as *mut u8, 0);
            core::ptr::write_volatile(addr_of!(A_VAL)     as *mut u8, a);
            core::ptr::write_volatile(addr_of!(B_VAL)     as *mut u8, b);
            
            // dispatch.next_cb = BusAddr(jump_base_bus);

            dma.start(patch_b);
            dma.wait();

            let sum   = addr_of!(OUT_SUM).read_volatile();
            let carry = addr_of!(OUT_CARRY).read_volatile();
            let expected = a as u16 + b as u16;

            println!(
                "add8({:3}, {:3}) = {:3} carry={} (expected {:3} carry={})",
                a, b,
                sum, carry,
                expected & 0xff,
                if expected >= 256 { 1 } else { 0 },
            );
        }
    }
}
fn dma_test_add32() {
    unsafe {
        for carry in 0..2usize {
            for a in 0usize..256 {
                for b in 0usize..256 {
                    let idx = carry * 256 * 256 + a * 256 + b;
                    ADD8S[idx]   = ((a + b + carry) & 0xff) as u8;
                    CARRY8S[idx] = if (a + b + carry) >= 256 { 1 } else { 0 };
                }
            }
        }

        let bus_scratch = BusAddr::from_arm(addr_of!(SCRATCH) as u32);

        const JUMP_TABLE_SIZE: usize = 128 * 1024 * 1024;
        const CBS_PER_ENTRY:   usize = 8;

        kmalloc::kmalloc_init_mb_with_start(128, 0x0000_0000); // need to revert this
        let jump_base  = kmalloc::kmalloc_aligned(JUMP_TABLE_SIZE, JUMP_TABLE_SIZE) as usize;
        let jump_table = jump_base as *mut ControlBlock;

        let bus_outs: [BusAddr; 4] = core::array::from_fn(|i| {
            BusAddr::from_arm(addr_of!(ADD32_OUT) as u32 + i as u32)
        });
        let bus_out_carry = BusAddr::from_arm(addr_of!(ADD32_OUT_CARRY) as u32);

        let patch_b_memory: *mut ControlBlock = 0x1500_0000 as *mut ControlBlock; // safe and away from my massive heap

        let jump_base_bus = BusAddr::from_arm(jump_base as u32).0;
        for byte in 0usize..4 {
            for carry in 0usize..2 {
                let next_dispatch_idx = ((byte + 1) * 2) + carry; 
                let next_dispatch_addr = patch_b_memory.add(byte + 1) as u32;
                // let next_dispatch_addr = addr_of!(PATCH_B_BLOCKS) as u32 + (next_dispatch_idx * size_of::<ControlBlock>()) as u32;
                let idx_offset = (byte * 2 + carry) * 256 * 256;
                // println!("jump table offset: byte={}, carry={} jt_offset={:x}, next_dispatch_addr={:x}", byte, carry, (byte * 2 + carry) * 256 * 256, next_dispatch_addr);
                for a in 0usize..256 {
                    for b in 0usize..256 {
                        let idx     = idx_offset + a * 256 + b;
                        let cb_base = idx * CBS_PER_ENTRY;

                        let table_idx = carry * 256 * 256 + a * 256 + b;
                        if table_idx > 131072 {
                            println!("table index oob: {}", table_idx);
                        }
                        let bus_sum   = BusAddr::from_arm(addr_of!(ADD8S[table_idx])   as u32);
                        let bus_carry = BusAddr::from_arm(addr_of!(CARRY8S[table_idx]) as u32);

                        let cb0 = &mut *jump_table.add(cb_base);
                        let cb1 = &mut *jump_table.add(cb_base + 1);
                        let cb2 = &mut *jump_table.add(cb_base + 2);

                        // if a == 0 && b == 0 && byte == 0 && carry == 0 { println!("cb0 at a=0,b=0,byte=0,carry=0, {}", jump_table.add(cb_base) as u32); }

                        *cb0 = ControlBlock::new(bus_sum, bus_outs[byte], 1);
                        cb0.next_cb = BusAddr::from_arm(jump_table.add(cb_base + 1) as u32);
                        // if byte == 1 { cb0.next_cb = BusAddr(0); }

                        // *cb1 = ControlBlock::new(bus_scratch, bus_scratch, 1);
                        *cb1 = ControlBlock::new(bus_carry, BusAddr(addr_of!(cb2.next_cb) as u32 + 2), 1);
                        cb1.next_cb = BusAddr::from_arm(jump_table.add(cb_base + 2) as u32);

                        *cb2 = ControlBlock::new(bus_carry, bus_out_carry, 2);
                        // cb1.next_cb = BusAddr(0);

                        if byte < 3 {
                            cb2.next_cb = BusAddr::from_arm(
                                next_dispatch_addr
                            );
                        } else {
                            cb2.next_cb = BusAddr(0);
                        }
                    }
                }
                // println!("finished putting together table for carry={}, byte={}", carry, byte);
            }
        }

        // x byte1 byte2 carry | a1 a2 a3 a4 | b1 b2 b3 b4 | x x x x

        for byte in 0usize..4 {
            for carry in 0usize..2 {
                let idx = (byte * 2) + carry; 

                let dispatch = &mut DISPATCH_BLOCKS[idx];
                // let patch_b  = &mut PATCH_B_BLOCKS[idx];
                let patch_b_addr = (patch_b_memory.add(byte) as usize) | (carry << 16);
                let patch_b = &mut *(patch_b_addr as *mut ControlBlock);
                let patch_a  = &mut PATCH_A_BLOCKS[idx];

                *dispatch = ControlBlock::new(bus_scratch, bus_scratch, 4);
                dispatch.next_cb = BusAddr(
                    jump_base_bus + ((byte * 2 + carry) * 256 * 256 * 256) as u32
                );
                // println!("dispatch: byte={}, carry={}, idx={}, next jump={:x}, address={:x}, patch_b_addr={:x}", byte, carry, idx, dispatch.next_cb.0, dispatch as *const _ as u32, patch_b as *const _ as u32);
                // if byte == 1 { dispatch.next_cb = BusAddr(0); }


                let dispatch_next_cb_addr = addr_of!(dispatch.next_cb) as u32;

                *patch_b = ControlBlock::new(
                    BusAddr::from_arm(addr_of!(ADD32_B) as u32 + byte as u32),
                    BusAddr::from_arm(dispatch_next_cb_addr + 1),
                    1,
                );

                *patch_a = ControlBlock::new(
                    BusAddr::from_arm(addr_of!(ADD32_A) as u32 + byte as u32),
                    BusAddr::from_arm(dispatch_next_cb_addr + 2),
                    1,
                );

                patch_b.next_cb = BusAddr::from_arm(patch_a  as *const _ as u32);
                // if byte == 1 { patch_b.next_cb = BusAddr(0); }

                patch_a.next_cb = BusAddr::from_arm(dispatch as *const _ as u32);
                // if byte == 1 { patch_a.next_cb = BusAddr(0); }

                // patch_b -> patch_a -> dispatch -> jump table -> c0 -> c1 -> patch_b (next byte)

                // right now: pb (0) -> pa (0) -> dispatch -> jt -> c0 -> c1 -> pb (1) -> pa (1) -> dispatch (1) -> jt -> c0 -> c1 -> pb (2)
            }
        }

        let dma = DMA::new(5);
        let test_cases: [(u32, u32); 20] = [
            (0,        0),
            (1,        2),
            (2,        4),
            (3,        6),
            (10,       20),
            (100,      200),
            (1_000,    2_000),
            (2_048,    4_096),
            (10_220,   20_035),
            (12_345,   24_690),
            (25_355,   1),
            (42_000,   84_000),
            (65_535,   131_070),
            (255,      255),
            (256,      512),
            (1_024,    2_048),
            (100_000,  200_000),
            (255_235,  234_556),
            (999_999,  1_999_998),
            (123_456,  246_912),
        ];

        for (a, b) in test_cases {
            core::ptr::write_volatile(addr_of!(ADD32_OUT)       as *mut u32, 0);
            core::ptr::write_volatile(addr_of!(ADD32_OUT_CARRY) as *mut u32, 0);
            core::ptr::write_volatile(addr_of!(ADD32_A)         as *mut u32, a);
            core::ptr::write_volatile(addr_of!(ADD32_B)         as *mut u32, b);

            let patch_b: &ControlBlock = &*(patch_b_memory as *const ControlBlock);
            dma.start(patch_b);
            dma.wait();

            let sum   = core::ptr::read_volatile(addr_of!(ADD32_OUT)       as *const u32);
            let carry = core::ptr::read_volatile(addr_of!(ADD32_OUT_CARRY) as *const u32);
            let expected = a.wrapping_add(b);

            println!(
                "add32({}, {}) = {} carry={} (expected {})",
                a, b, sum, carry, expected
            );
            if sum != expected {
                println!("OH MY GOD THERE IS A MISMATCH");
            }
        }
    }
}

pub fn dma_test() {
    println!("Hello from DMA test");
    dma_test_blink();
    dma_test_add8();
    dma_test_add32();
}
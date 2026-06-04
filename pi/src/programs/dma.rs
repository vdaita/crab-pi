use crate::arch::dev_barrier;
use crate::mem::{get32, put32};
use crate::println;
use crate::gpio;
use crate::kmalloc;
use crate::timer::Timer;

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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct BusAddr(pub u32);

impl BusAddr {
    pub fn from_arm(addr: u32) -> Self {
        if addr < 0x2000_0000 {
            BusAddr(addr | 0x4000_0000)       // regular
        } else {
            BusAddr(addr - 0x2000_0000 + 0x7e00_0000)  // peripheral
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

    pub fn then(mut self, next: &ControlBlock) -> Self {
        self.next_cb = BusAddr::from_arm(next as *const _ as u32);
        self
    }

    pub fn end(mut self) -> Self {
        self.next_cb = BusAddr(0);
        self
    }

    pub fn nop(scratch: &u32) -> Self {
        let addr = BusAddr::from_arm(scratch as *const _ as u32);
        Self::new(addr, addr, 4)
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

fn dma_test_blink() {
    let dma = DMA::new(5);
    let parthiv_bit:  u32 = 1 << PARTHIV_PIN;

    gpio::set_output(PARTHIV_PIN);

    let bus_gpio_set = BusAddr::from_arm(gpio::GPIO_SET0 as u32);
    let bus_gpio_clr = BusAddr::from_arm(gpio::GPIO_CLR0 as u32);
    let bus_parthiv_bit  = BusAddr::from_arm(&parthiv_bit  as *const _ as u32);

    let scratch: u32 = 0;
    const BLINKS:    usize = 8;
    const DELAY_LEN: usize = 10000;
    const N: usize = BLINKS * (1 + DELAY_LEN + 1 + DELAY_LEN) + 1;

    let mut blocks: [ControlBlock; N] = [ControlBlock::default(); N];

    // let mut i = 0;
    // for _ in 0..BLINKS {
    //     blocks[i] = ControlBlock::new(bus_parthiv_bit, bus_gpio_set, 4);   
    //     i += 1;

    //     for _ in 0..DELAY_LEN {
    //         blocks[i] = ControlBlock::nop(&scratch);        
    //         i += 1;
    //     }

    //     // blocks[i] = ControlBlock::new(bus_parthiv_bit, bus_gpio_clr, 4);  
    //     i += 1;
    //     for _ in 0..DELAY_LEN {
    //         blocks[i] = ControlBlock::nop(&scratch);        
    //         i += 1;
    //     }
    // }

    // for j in 0..i - 1 {
    //     blocks[j] = blocks[j].then(&blocks[j + 1]);
    // }
    // blocks[i - 1].end();

    let mut block = ControlBlock::new(bus_parthiv_bit, bus_gpio_set, 4);
    block.end();
    dma.start(&block);
    Timer::delay_ms(1000);

    // dma.start(&blocks[0]);
    dma.wait();
}

pub fn dma_test() {
    println!("Hello from DMA test");
    dma_test_blink();
}
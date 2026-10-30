use crate::mem::{put32, get32};
use crate::gpio;
use crate::arch;
use crate::arch::{dev_barrier};

const BASE: u32 = 0x20200000;
const AUX_BASE: u32 = 0x20215000;
const AUXENB_REG: u32 = AUX_BASE + 0x004;
const AUX_MU_BAUD_REG: u32 = AUX_BASE + 0x068;
const AUX_MU_IO_REG: u32 = AUX_BASE + 0x040;
const AUX_MU_LSR_REG: u32 = AUX_BASE + 0x054;
const AUX_MU_CNTL_REG: u32 = AUX_BASE + 0x060;
const AUX_MU_IIR_REG: u32 = AUX_BASE + 0x048;
const AUX_MU_IER_REG: u32 = AUX_BASE + 0x044;
const AUX_MU_LCR_REG: u32 = AUX_BASE + 0x04C;
const AUX_MU_STAT_REG: u32 = AUX_BASE + 0x064;
const AUX_MU_MCR_REG: u32 = AUX_BASE + 0x050;
const GPIO_TX: u32 = 14;
const GPIO_RX: u32 = 15;

pub fn init() {
    dev_barrier();
    
    gpio::set_function(GPIO_TX, 0b010);
    gpio::set_function(GPIO_RX, 0b010);
    
    dev_barrier();
    
    let auxenb_val = get32(AUXENB_REG) | 1;
    put32(AUXENB_REG, auxenb_val);
    
    dev_barrier();
    
    put32(AUX_MU_CNTL_REG, 0);
    put32(AUX_MU_IIR_REG, 0b110);
    put32(AUX_MU_IER_REG, 0);
    put32(AUX_MU_MCR_REG, 0);
    put32(AUX_MU_BAUD_REG, 270);
    put32(AUX_MU_LCR_REG, 0b11);
    put32(AUX_MU_CNTL_REG, 0b11);
    
    dev_barrier();
}

pub fn flush() {
    while !tx_is_empty() {
        arch::wait();
    }
}

pub fn write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        put8(byte);
    }
}

pub fn read_byte() -> u8 {
    dev_barrier();
    while !has_data() {}
    let byte = get32(AUX_MU_IO_REG) as u8;
    dev_barrier();
    byte
}

pub fn read_bytes(buf: &mut [u8]) -> usize {
    if buf.is_empty() {
        return 0;
    }

    buf[0] = read_byte();
    let mut nread = 1;
    while nread < buf.len() && has_data() {
        buf[nread] = get32(AUX_MU_IO_REG) as u8;
        nread += 1;
    }

    nread
}

pub fn read_bytes_nonblocking(buf: &mut [u8]) -> usize {
    if buf.is_empty() { return 0; }
    if !has_data() { return 0; } 
    buf[0] = get32(AUX_MU_IO_REG) as u8;
    let mut nread = 1;
    while nread < buf.len() && has_data() {
        buf[nread] = get32(AUX_MU_IO_REG) as u8;
        nread += 1;
    }
    nread
}

fn can_put8() -> bool {
    let stat = get32(AUX_MU_STAT_REG);
    (stat & (1 << 1)) != 0
}

fn put8(c: u8) {
    dev_barrier();
    while !can_put8() {}
    put32(AUX_MU_IO_REG, c as u32);
    dev_barrier();
}

fn has_data() -> bool {
    let stat = get32(AUX_MU_STAT_REG);
    (stat & 1) != 0
}

fn tx_is_empty() -> bool {
    let lsr = get32(AUX_MU_LSR_REG);
    let idle = (lsr & (1 << 6)) != 0;
    let empty = (lsr & (1 << 5)) != 0;
    idle && empty
}

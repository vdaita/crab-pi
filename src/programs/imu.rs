use crate::gpio::{read, set_function, set_input, set_output, set_on, set_off};
use crate::mem::{get32, put32};
use crate::timer::Timer;
use crate::bit_utils::{bit_get, bit_set, bits_get};
use crate::println;

#[repr(C)]
struct i2c_config {
    control: u32,
    status: u32,
    dlen: u32,
    dev_addr: u32,
    fifo: u32,
    clock_div: u32,
    clock_delay: u32,
    clock_stretch_timeout: u32
}

static mut i2c: *mut i2c_config = (0x20804000) as *mut i2c_config;
const I2C_BASE: u32 = 0x7E80_4000;
const I2C_C: u32 = I2C_BASE + 0x0;
const I2C_S: u32 = I2C_BASE + 0x4;
const I2C_A: u32 = I2C_BASE + 0xc; // slave address
const I2C_FIFO: u32 = I2C_BASE + 0x10;
const I2C_DLEN: u32 = I2C_BASE + 0x14;

const USER_CTRL: u8 = 0x6a;
const ACCEL_XOUT_H: u8 = 0x3b;
const ACCEL_XOUT_L: u8 = 0x3c;

const ACCEL_YOUT_H: u8 = 0x3d;
const ACCEL_YOUT_L: u8 = 0x3e;
const ACCEL_ZOUT_H: u8 = 0x3f;
const ACCEL_ZOUT_L: u8 = 0x40;

// p 41
const PWR_MGMT_1: u8 = 0x6b;
// p 42
const PWR_MGMT_2: u8 = 108;

// p 29
const IMU_INT_STATUS: u8 = 0x3a;
// p 28
const IMU_INT_ENABLE: u8 = 0x38;


fn i2c_write(addr: u32, data: &[u8], nbytes: usize) {
    // wait until the transfer is not active
    while (bit_get(get32(I2C_S), 0) == 1) {
        // Timer::delay_us(10);
    }

    // check in status that: fifo is empty, there was no timeout, and there were no issues
    assert!(bit_get(get32(I2C_S), 5) == 0); // fifo is empty
    assert!(bit_get(get32(I2C_S), 8) == 0); // error ack empty
    assert!(bit_get(get32(I2C_S), 9) == 0); // there was no timeout

    // clear the done field in status
    put32(I2C_S, 1 << 1);

    // set device address and length

    // address
    put32(I2C_A, addr);

    // length
    put32(I2C_DLEN, nbytes as u32);

    // set the control reg to write and start transfer (and fifo)
    put32(I2C_C,  1 << 15 | 1 << 7 | 0b11 << 4); // Write packet transfer and keep the device enabled

    // wait for transfer
    while((bit_get(get32(I2C_S), 0)) == 0) {
        // Timer::delay_us(10);
    }

    for i in 0..nbytes {
        // check that there is a byte available at each time
        while(bit_get(get32(I2C_S), 4) == 0) { // while possible to add
            // Timer::delay_us(10);
        }
        put32(I2C_FIFO, data[i].clone() as u32); // copy that byte and make it a u32
    }

    // wait for done, then check that ta is 0 and that there were no errors
    while(bit_get(get32(I2C_S), 1) != 1) {
        // Timer::delay_us(10);
    }

    assert!(bit_get(get32(I2C_S), 8) == 0); // ERR
    assert!(bit_get(get32(I2C_S), 9) == 0); // CLKT
    assert!(bit_get(get32(I2C_S), 0) == 0); // TA == 0
}

fn i2c_read(addr: u32, nbytes: usize) -> [u8; 32] {
    let mut data: [u8; 32] = [0; 32];

    // wait until the transfer is not active
    while (bit_get(get32(I2C_S), 0) == 1) {
        // Timer::delay_us(10);
    }

    // check in status that: fifo is empty, there was no timeout, and there were no issues
    assert!(bit_get(get32(I2C_S), 5) == 0); // fifo is empty
    assert!(bit_get(get32(I2C_S), 8) == 0); // error ack empty
    assert!(bit_get(get32(I2C_S), 9) == 0); // there was no timeout

    // clear the done field in status
    put32(I2C_S, 1 << 1);

    // set device address and length
    put32(I2C_A, addr);

    // length
    put32(I2C_DLEN, nbytes as u32);

    // clear the fifo
    put32(I2C_C, 0b11 << 4);

    // set the control reg to read and start transfer
    put32(I2C_C, 1 | 1 << 15 | 1 << 7 | 0b11 << 4); // Read packet transfer and keep the device enabled

    // wait for transfer
    while((bit_get(get32(I2C_S), 0)) == 0) { // wait for transfer active bit at the bottom
        // Timer::delay_us(10);
    }

    for idx in 0..nbytes {
        // check that there is a byte available at each time
        while(bit_get(get32(I2C_S), 5) == 0) { // while FIFO can't accept data
            // Timer::delay_us(10);
        } 
        data[idx] = (get32(I2C_FIFO) & 0xFF) as u8; // just get the first 8 bits
    }

    // wait for done, then check that ta is 0 and that there were no errors
    while(bit_get(get32(I2C_S), 1) != 1) {
        // Timer::delay_us(10);
    }

    assert!(bit_get(get32(I2C_S), 8) == 0); // ERR
    assert!(bit_get(get32(I2C_S), 9) == 0); // CLKT
    assert!(bit_get(get32(I2C_S), 0) == 0); // TA == 0

    data
}

fn i2c_init() {
    set_function(0, 0x5); // FSEL_ALT0
    set_function(1, 0x5); // FSEL_ALT1
    put32(I2C_C, 1 << 15); // C register, p 29
    put32(I2C_S, 1 << 9 | 1 << 8 | 1 << 1); // S register, p 31 - clearing CLKT and ERR and DONE
    assert!(get32(I2C_S) == 0); // S register, p 31 - assert! that there are no errors.
}

fn reg_read(addr: u32, reg: u8) -> u8 {
    let data: [u8; 1] = [reg];
    i2c_write(addr, &data, 1);
    return i2c_read(addr, 1)[0];
}

fn reg_write(addr: u32, reg: u8, val: u8) { 
    let data: [u8; 2] = [reg, val];
    i2c_write(addr, &data, 2);
}

fn mpu6050_reset(dev_addr: u32) {
    Timer::delay_ms(100);
    
    // page 41: set bit 7 to 1 in register to reset device
    // PWR_MGMT_1
    reg_write(dev_addr, PWR_MGMT_1, 1 << 7);

    Timer::delay_ms(100);
    
    // clear sleep mode
    reg_write(dev_addr, PWR_MGMT_1, 0);

    Timer::delay_ms(100);


    // enable IMU interrupts
    reg_write(dev_addr, IMU_INT_ENABLE,  1);
}

fn mpu6050_read(dev_addr: u32) {

    while (reg_read(dev_addr, IMU_INT_STATUS) == 0) {
        
    }

    let x_high = reg_read(dev_addr, ACCEL_XOUT_H);
    let x_low = reg_read(dev_addr, ACCEL_XOUT_L);
    
    let y_high = reg_read(dev_addr, ACCEL_YOUT_H);
    let y_low = reg_read(dev_addr, ACCEL_YOUT_L);

    let z_high = reg_read(dev_addr, ACCEL_ZOUT_H);
    let z_low = reg_read(dev_addr, ACCEL_ZOUT_L);

    let x_val: i16 = ((x_high as u16) << 8 | (x_low as u16)) as i16;
    let y_val: i16 = ((y_high as u16) << 8 | (y_low as u16)) as i16;
    let z_val: i16 = ((z_high as u16) << 8 | (z_low as u16)) as i16;

    println!("X: {}, Y: {}, Z: {}", x_val, y_val, z_val);
}

pub fn imu_test() {
    println!("Testing the IMU.");
    let dev_addr: u32 = 0b1101000;
    mpu6050_reset(dev_addr);

    for i in 0..100 {
        mpu6050_read(dev_addr);
        Timer::delay_ms(100);
    }
}
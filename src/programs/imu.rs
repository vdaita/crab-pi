use libm::powf;

use crate::gpio::{read, set_function, set_input, set_output, set_on, set_off};
use crate::mem::{get32, put32};
use crate::timer::Timer;
use crate::bit_utils::{bit_get, bit_set, bits_get};
use crate::println;

const I2C_BASE: u32 = 0x2080_4000;
const I2C_C: u32 = I2C_BASE + 0x0;
const I2C_S: u32 = I2C_BASE + 0x4;
const I2C_DLEN: u32 = I2C_BASE + 0x8;
const I2C_A: u32 = I2C_BASE + 0xc; // slave address
const I2C_FIFO: u32 = I2C_BASE + 0x10;
const I2C_DIV: u32 = I2C_BASE + 0x14;
const I2C_DEL: u32 = I2C_BASE + 0x18;
const I2C_CLKT: u32 = I2C_BASE + 0x1c;

// const powers1046: &[f32] = &[1.0, 1.046, 1.094116, 1.1444453360000002, 1.1970898214560002, 1.2521559532429762, 1.3097551270921532, 1.3700038629383924, 1.4330240406335584, 1.498943146502702, 1.5678945312418264, 1.6400176796789505, 1.7154584929441823, 1.7943695836196147, 1.8769105844661171, 1.9632484713515588, 2.0535579010337304, 2.148021564481282, 2.246830556447421, 2.3501847620440026, 2.458293261098027, 2.5713747511085363, 2.689657989659529, 2.8133822571838674, 2.9427978410143254, 3.0781665417009845, 3.21976220261923, 3.3678712639397146, 3.522793342080942, 3.6848418358166652, 3.854344560264232, 4.031644410036387, 4.2171000528980604, 4.4110866553313715, 4.613996641476615, 4.826240486984539, 5.048247549385828, 5.280466936657577, 5.523368415743826, 5.777443362868041, 6.043205757559972, 6.3211932224077305, 6.611968110638487, 6.916118643727858, 7.234260101339339, 7.567036066000949, 7.915119725036993, 8.279215232388696, 8.660059133078574, 9.05842185320019, 9.475109258447398, 9.91096428433598, 10.366868641415435, 10.843744598920546, 11.342556850470892, 11.864314465592553, 12.410072931009811, 12.980936285836263, 13.578059354984731, 14.20265008531403, 14.855971989238475, 15.539346700743446, 16.254156648977645, 17.00184785483062];

const USER_CTRL: u8 = 0x6a;

const ACCEL_CONFIG: u8 = 28;
const ACCEL_XOUT_H: u8 = 0x3b;
const ACCEL_XOUT_L: u8 = 0x3c;

const ACCEL_YOUT_H: u8 = 0x3d;
const ACCEL_YOUT_L: u8 = 0x3e;
const ACCEL_ZOUT_H: u8 = 0x3f;
const ACCEL_ZOUT_L: u8 = 0x40;

const GYRO_CONFIG: u8 = 27;
const GYRO_XOUT_H: u8 = 67;
const GYRO_XOUT_L: u8 = 68;
const GYRO_YOUT_H: u8 = 69;
const GYRO_YOUT_L: u8 = 70;
const GYRO_ZOUT_H: u8 = 71;
const GYRO_ZOUT_L: u8 = 72;


const SELF_TEST_X: u8 = 13; // 14 -> Y, 15 -> Z

// p 41
const PWR_MGMT_1: u8 = 0x6b;
// p 42
const PWR_MGMT_2: u8 = 108;

// p 29
const IMU_INT_STATUS: u8 = 0x3a;
// p 28
const IMU_INT_ENABLE: u8 = 0x38;

const MPU_ACCEL_FS_8G: u8 = (2 << 3);
const MPU_GYRO_FS_250DPS: u8 = (0 << 3);

#[derive(Clone, Copy)]
struct XYZ {
    x: i16,
    y: i16,
    z: i16
}

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
    set_function(0, 0x4);
    set_function(1, 0x4);
    set_function(2, 0x4); // FSEL_ALT0
    set_function(3, 0x4); // FSEL_ALT0
    put32(I2C_C, 1 << 15); // C register, p 29
    put32(I2C_S, 1 << 9 | 1 << 8 | 1 << 1); // S register, p 31 - clearing CLKT and ERR and DONE
    Timer::delay_ms(10);

    // put32(I2C_DIV, 625);
    put32(I2C_DIV, 0);
    put32(I2C_CLKT, 0);
    println!("cdiv value: {:0x}", get32(I2C_DIV));
    println!("clkt value: {:0x}", get32(I2C_CLKT));
    
    let status = get32(I2C_S);
    println!("Status register: {:0b}", status);
    assert!(bit_get(status, 8) == 0); // S register, p 31 - assert! that there are no errors.
    assert!(bit_get(status, 9) == 0)
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

fn reg_read_multiple(addr: u32, reg: u8, nbytes: u8) -> [u8; 32] {
    let data: [u8; 1] = [reg];
    i2c_write(addr, &data, 1);
    let result = i2c_read(addr, nbytes as usize);
    result
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

fn mpu6050_read_accelerometer(dev_addr: u32) -> XYZ {
    while reg_read(dev_addr, IMU_INT_STATUS) == 0 {
        // wait for interrupt
    }

    let accel_data = reg_read_multiple(dev_addr, ACCEL_XOUT_H, 6);
    let x_val: i16 = ((accel_data[0] as u16) << 8 | (accel_data[1] as u16)) as i16;
    let y_val: i16 = ((accel_data[2] as u16) << 8 | (accel_data[3] as u16)) as i16;
    let z_val: i16 = ((accel_data[4] as u16) << 8 | (accel_data[5] as u16)) as i16;

    // println!("X: {}, Y: {}, Z: {}", x_val, y_val, z_val);
    return XYZ {x: x_val, y: y_val, z: z_val};
}

pub fn imu_accelerometer_test(dev_addr: u32) {
    for i in 0..100 {
        mpu6050_read_accelerometer(dev_addr);
        Timer::delay_ms(100);
    }
}

fn mpu6050_read_gyro(dev_addr: u32) -> XYZ {
    while reg_read(dev_addr, IMU_INT_STATUS) == 0 {

    }
    let gyro_data = reg_read_multiple(dev_addr, GYRO_XOUT_H, 6);
    // let x_val: i16 = ((gyro_data[0] as u16) << 8 | (gyro_data[1] as u16)) as i16;
    // let y_val: i16 = ((gyro_data[2] as u16) << 8 | (gyro_data[3] as u16)) as i16;
    // let z_val: i16 = ((gyro_data[4] as u16) << 8 | (gyro_data[5] as u16)) as i16;
    let x_val = i16::from_be_bytes([gyro_data[0], gyro_data[1]]);
    let y_val = i16::from_be_bytes([gyro_data[2], gyro_data[3]]);
    let z_val = i16::from_be_bytes([gyro_data[4], gyro_data[5]]);

    // let x_val: i16 = (((reg_read(dev_addr, GYRO_XOUT_H) as u16) << 8) | (reg_read(dev_addr, GYRO_XOUT_L) as u16)) as i16;
    // let y_val: i16 = (((reg_read(dev_addr, GYRO_YOUT_H) as u16) << 8) | (reg_read(dev_addr, GYRO_YOUT_L) as u16)) as i16;
    // let z_val: i16 = (((reg_read(dev_addr, GYRO_ZOUT_H) as u16) << 8) | (reg_read(dev_addr, GYRO_ZOUT_L) as u16)) as i16;



    return XYZ { x: x_val, y: y_val, z: z_val }
}

pub fn imu_gyro_test(dev_addr: u32) {
    for i in 0..100 {
        let xyz = mpu6050_read_gyro(dev_addr);
        println!("x={}, y={}, z={}", xyz.x, xyz.y, xyz.z);
        Timer::delay_ms(100);
    }
}

#[unsafe(no_mangle)]
pub fn self_test_gyro(dev_addr: u32) {
    reg_write(dev_addr, GYRO_CONFIG, MPU_GYRO_FS_250DPS);
    Timer::delay_ms(250);

    println!("Running self_test_gyro");
    let mut gyro_results_base = [XYZ { x: 0, y: 0, z: 0 }; 25];
    for item in &mut gyro_results_base {
        *item = mpu6050_read_gyro(dev_addr);
        Timer::delay_ms(100);
    }
    println!("Finished collecting base results");

    Timer::delay_ms(250);


    // turn self-test on in the GYRO_CONFIG register
    reg_write(dev_addr, ACCEL_CONFIG, 0b111 << 5);
    reg_write(dev_addr, GYRO_CONFIG, MPU_GYRO_FS_250DPS | 0b11100000);

    // // wait till it settles
    Timer::delay_ms(250);
    for _ in 0..20 {
        mpu6050_read_gyro(dev_addr);
        Timer::delay_ms(100);
    }

    // // read the gyro values
    let mut gyro_results_self_test = [XYZ { x: 0, y: 0, z: 0 }; 25];
    for item in &mut gyro_results_self_test {
        *item = mpu6050_read_gyro(dev_addr);
        Timer::delay_ms(100);
    }
    println!("Finished collecting self-test results");


    // println!("x: base={}, self_test={}", gyro_results_base[0].x, gyro_results_self_test[0].x);
    // println!("y: base={}, self_test={}", gyro_results_base[0].y, gyro_results_self_test[0].y);
    // println!("z: base={}, self_test={}", gyro_results_base[0].z, gyro_results_self_test[0].z);


    // self-test
    let self_test_values = reg_read_multiple(dev_addr, SELF_TEST_X, 3);
    let stx = self_test_values[0] & (0b11111);
    let sty = self_test_values[1] & (0b11111);
    let stz = self_test_values[2] & (0b11111);

    println!("self-test bits: x={:0b}, y={:0b}, z={:0b}", stx, sty, stz);

    let ft_z = 25. * 131. * powf(1.046, (stz - 1) as f32);
    let ft_x = 25. * 131. * powf(1.046, (stx - 1) as f32);
    let ft_y = -25. * 131. * powf(1.046, (sty - 1) as f32);

    println!("FT x={}, y={}, z={}", ft_x, ft_y, ft_z);

    for i in 0..25 {
        let x_diff = (((gyro_results_self_test[i].x - gyro_results_base[i].x) as f32) - ft_x) / ft_x;
        let y_diff = (((gyro_results_self_test[i].y - gyro_results_base[i].y) as f32) - ft_y) / ft_y;
        let z_diff = (((gyro_results_self_test[i].z - gyro_results_base[i].z) as f32) - ft_z) / ft_z;
        println!(
            "x_before={}, x_after={}, y_before={}, y_after={}, z_before={}, z_after={}, x_diff={}, y_diff={}, z_diff={}", 
            gyro_results_base[i].x, gyro_results_self_test[i].x, 
            gyro_results_base[i].y, gyro_results_self_test[i].y,  
            gyro_results_base[i].z, gyro_results_self_test[i].z, 
            x_diff, y_diff, z_diff
        );
    }

}


#[unsafe(no_mangle)]
pub fn self_test_accel(dev_addr: u32) {
    // reg_write(dev_addr, ACCEL_CONFIG, MPU_ACCEL_FS_8G);
    reg_write(dev_addr, ACCEL_CONFIG, 0b00010000);
    Timer::delay_ms(250);

    println!("Running self_test_accel");
    let mut accel_results_base = [XYZ { x: 0, y: 0, z: 0 }; 10];
    for item in &mut accel_results_base {
        *item = mpu6050_read_accelerometer(dev_addr);
        Timer::delay_ms(100);
    }
    println!("Finished collecting base results");

    Timer::delay_ms(250);


    // turn self-test on in the GYRO_CONFIG register
    // reg_write(dev_addr, ACCEL_CONFIG, 0b11100000 | MPU_ACCEL_FS_8G);
    reg_write(dev_addr, ACCEL_CONFIG, 0b11110000);
    // reg_write(dev_addr, GYRO_CONFIG, MPU_GYRO_FS_250DPS | 0b11100000);

    // // wait till it settles
    Timer::delay_ms(250);
    for _ in 0..20 {
        mpu6050_read_accelerometer(dev_addr);
        Timer::delay_ms(100);
    }

    // // read the gyro values
    let mut accel_results_self_test = [XYZ { x: 0, y: 0, z: 0 }; 10];
    for item in &mut accel_results_self_test {
        *item = mpu6050_read_accelerometer(dev_addr);
        Timer::delay_ms(100);
    }
    println!("Finished collecting self-test results");


    // println!("x: base={}, self_test={}", gyro_results_base[0].x, gyro_results_self_test[0].x);
    // println!("y: base={}, self_test={}", gyro_results_base[0].y, gyro_results_self_test[0].y);
    // println!("z: base={}, self_test={}", gyro_results_base[0].z, gyro_results_self_test[0].z);


    // self-test
    let self_test_values = reg_read_multiple(dev_addr, SELF_TEST_X, 4);
    let stx = ((self_test_values[0] & (0b11100000)) >> 3) | ((self_test_values[3] & (0b110000)) >> 4);
    let sty = ((self_test_values[1] & (0b11100000)) >> 3) | ((self_test_values[3] & (0b1100)) >> 2);
    let stz = ((self_test_values[2] & (0b11100000)) >> 3) | (self_test_values[3] & (0b11));

    println!("self-test bits: x={:0b}, y={:0b}, z={:0b}", stx, sty, stz);

    let ft_z = 4096. * 0.34 * powf(0.92 / 0.34, ((stz - 1) as f32) / (30.));
    let ft_x = 4096. * 0.34 * powf(0.92 / 0.34, ((stx - 1) as f32) / (30.));
    let ft_y = 4096. * 0.34 * powf(0.92 / 0.34, ((sty - 1) as f32) / (30.));

    println!("FT x={}, y={}, z={}", ft_x, ft_y, ft_z);

    for i in 0..10 {
        let x_diff = (((accel_results_self_test[i].x - accel_results_base[i].x) as f32) - ft_x) / ft_x;
        let y_diff = (((accel_results_self_test[i].y - accel_results_base[i].y) as f32) - ft_y) / ft_y;
        let z_diff = (((accel_results_self_test[i].z - accel_results_base[i].z) as f32) - ft_z) / ft_z;
        println!(
            "x_before={}, x_after={}, y_before={}, y_after={}, z_before={}, z_after={}, x_diff={}, y_diff={}, z_diff={}", 
            accel_results_base[i].x, accel_results_self_test[i].x, 
            accel_results_base[i].y, accel_results_self_test[i].y,  
            accel_results_base[i].z, accel_results_self_test[i].z, 
            x_diff, y_diff, z_diff
        );
    }

}



pub fn imu_test() {
    println!("Testing the IMU.");
    i2c_init();

    let dev_addr: u32 = 0b1101000;
    mpu6050_reset(dev_addr);
    println!("Finished reset");

    // let val: f32 = 0.0;
    // println!("{}", val as u32);

    // let val2: f32 = 1.0 * 2.0;
    // println!("{}", val2);
    // println!("{}", 1.0 * 2.0);

    self_test_accel(dev_addr);
    self_test_gyro(dev_addr);

    // imu_accelerometer_test(dev_addr);
    // println!("doing gyro test now.");
    // imu_gyro_test(dev_addr);
}
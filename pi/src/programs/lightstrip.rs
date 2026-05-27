use crate::gpio::{set_off_21, set_on_21, set_on_27, set_off_27, set_output};
use crate::os::utils::{enable_branch_prediction, enable_l1_instruction_cache};
use crate::timer::{Timer};
use crate::{println};
use crate::programs::imu::{mpu6050_read_gyro, i2c_init, mpu6050_reset};

const T0H: u32 = 350;
const T0L: u32 = 900;

const T1H: u32 = 900;
const T1L: u32 = 350;
const RESET_USEC: u32 = 52;

#[derive(Copy, Clone)]
struct Color {
    r: u8,
    g: u8,
    b: u8
}

#[inline(always)]
fn send_one() {
    set_on_21();
    Timer::delay_ns(T1H);
    set_off_21();
    Timer::delay_ns(T1L);
}

#[inline(always)]
fn send_zero() {
    set_on_21();
    Timer::delay_ns(T0H);
    set_off_21();
    Timer::delay_ns(T0L);
}

#[inline(always)]
fn flush() {
    set_off_21();
    Timer::delay_us(RESET_USEC);
} 

#[inline(always)]
fn send_byte(byte: u8) {
    for i in (0..8).rev() {
        if (byte & (1u8 << i)) != 0 {
            send_one();
        } else {
            send_zero();
        }
    }
}

#[inline(always)]
fn send_rgb(color: Color) {
    send_byte(color.g);
    send_byte(color.r);
    send_byte(color.b);
}

#[inline(always)]
fn send_array(data: [Color; 32]){
    for color in data {
        send_rgb(color);
    }
    flush();
}

fn latency_test() {
    for i in 1..20 {
        let start_time_thousand = Timer::get_usec();
        Timer::delay_ns(1_000_000 * i);
        let end_time_thousand = Timer::get_usec();
        println!("Time to wait {} ns: {}", 1_000_000 * i, end_time_thousand - start_time_thousand);
    }

    let start_time_set_on_unsafe = Timer::get_usec();
    set_on_21();
    let end_time_set_on_unsafe = Timer::get_usec();
    set_off_21();
    println!("Time to set on: {}", end_time_set_on_unsafe.wrapping_sub(start_time_set_on_unsafe));

    // for i in 1..20 {
    //     let start_time_set_on_loop = Timer::get_usec();
    //     for _ in 0..i {
    //         send_rgb(Color {r: 1, g: 0, b: 0});
    //     }
    //     let end_time_set_on_loop = Timer::get_usec();
    //     println!("Time to call set_on and set off {} times: {}", i, end_time_set_on_loop - start_time_set_on_loop);
    // }

    // for i in 1..20 {
    //     let start_time_set_on_loop = Timer::get_usec();
    //     for _ in 0..i {
    //         set_on_21();
    //         set_off_21();
    //     }
    //     let end_time_set_on_loop = Timer::get_usec();
    //     println!("Time to call set_on and set off {} times: {}", i, end_time_set_on_loop - start_time_set_on_loop);
    // }

    // for i in 1..20 {
    //     let start_time_set_off_loop = Timer::get_usec();
    //     for _ in 0..i {
    //         set_off_21();
    //     }
    //     let end_time_set_off_loop = Timer::get_usec();
    //     println!("Time to call set_off {} times: {}", i, start_time_set_off_loop - end_time_set_off_loop);
    // }

    let start_time_one = Timer::get_usec();
    send_one();
    let end_time_one = Timer::get_usec();
    println!("Time to send one: {}", end_time_one - start_time_one);

    let start_time_zero = Timer::get_usec();
    send_zero();
    let end_time_zero = Timer::get_usec();
    println!("Time to send zero: {}", end_time_zero - start_time_zero);
}

fn clear_out() {
    for i in 0..32 {
        send_rgb(Color { r: 0, g: 0, b: 0});
    }
}

pub fn basic_run() {
    set_output(21); // this is the lightstrip pin
    set_output(27);

    // enable_branch_prediction();
    // enable_l1_instruction_cache();
    
    latency_test();
    // flush();
    clear_out();

    let red = Color { r: 255, g: 0, b: 0 };
    let green = Color { r: 0, g: 255, b: 0 };
    let blue = Color { r: 0, g: 0, b: 255 };
    let white = Color { r: 255, g: 255, b: 255 };
    let off = Color { r: 0, g: 0, b: 0 };
    let yellow = Color { r: 255, g: 255, b: 0 };
    let cyan = Color { r: 0, g: 255, b: 255 };
    let magenta = Color { r: 255, g: 0, b: 255 };

    Timer::delay_ms(100);
    flush();

    let mut pattern = [off; 32];
    for i in 0..32 {
        match i % 8 {
            0 => pattern[i] = red,
            1 => pattern[i] = green,
            2 => pattern[i] = blue,
            3 => pattern[i] = yellow,
            4 => pattern[i] = cyan,
            5 => pattern[i] = magenta,
            6 => pattern[i] = white,
            _ => pattern[i] = off,
        }
    }
    send_array(pattern);
    Timer::delay_ms(100);
}

pub fn use_imu_to_color() {
    i2c_init(-1);
    let dev_addr: u32 = 0b1101000;
    mpu6050_reset(dev_addr);

    let mut buffer: [Color; 32000] = [Color {r: 0, g: 0, b: 0}; 32000];
    let mut buf_index = 0;
    for i in 0..200 {
        let xyz = mpu6050_read_gyro(dev_addr);
        let x_color = 256 * ((xyz.x as u32) + 32000) / 64000;
        let y_color = 256 * ((xyz.y as u32) + 32000) / 64000;
        let z_color = 256 * ((xyz.z as u32) + 32000) / 64000;
        println!("x: {}->{}, y: {}->{}, z: {}->{}", xyz.x, x_color, xyz.y, y_color, xyz.z, z_color);
        
        for j in 0..3 {
            buffer[buf_index] = Color {r: x_color as u8, g: y_color as u8, b: z_color as u8};
            buf_index += 1;
            buffer[buf_index] = Color {r: x_color as u8, g: y_color as u8, b: z_color as u8};
            buf_index += 1;
            buffer[buf_index] = Color {r: x_color as u8, g: y_color as u8, b: z_color as u8};
            buf_index += 1;
        }

        let start = core::cmp::max(0, (buf_index as i32) - 32);
        let end = start + 32;
        for j in start..end {
            send_rgb(buffer[j as usize]);
        }

        Timer::delay_ms(100);
    }
}
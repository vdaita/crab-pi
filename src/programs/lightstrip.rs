use crate::gpio::{set_output, set_on_23, set_off_23};
use crate::os::utils::{enable_branch_prediction, enable_l1_instruction_cache};
use crate::timer::{Timer};
use crate::{println};

const LIGHTSTRIP_PIN: u32 = 23;
const T0H: u32 = 350;
const T1H: u32 = 900;
const T0L: u32 = 900;
const T1L: u32 = 350;
const RESET: u32 = 50000;

#[derive(Copy, Clone)]
struct Color {
    r: u8,
    g: u8,
    b: u8
}

#[inline(always)]
fn send_one() {
    set_on_23();
    Timer::delay_ns(T1H);
    set_off_23();
    Timer::delay_ns(T1L);
}

#[inline(always)]
fn send_zero() {
    set_on_23();
    Timer::delay_ns(T0H);
    set_off_23();
    Timer::delay_ns(T0L);
}

#[inline(always)]
fn flush() {
    set_off_23();
    Timer::delay_ns(RESET);
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
    set_on_23();
    let end_time_set_on_unsafe = Timer::get_usec();
    set_off_23();
    println!("Time to set on: {}", end_time_set_on_unsafe - start_time_set_on_unsafe);

    for i in 1..20 {
        let start_time_set_on_loop = Timer::get_usec();
        for _ in 0..i {
            set_on_23();
            // set_off_23();
        }
        let end_time_set_on_loop = Timer::get_usec();
        println!("Time to call set_on {} times: {}", i, end_time_set_on_loop - start_time_set_on_loop);
    }

    let start_time_one = Timer::get_usec();
    send_one();
    let end_time_one = Timer::get_usec();
    println!("Time to send one: {}", end_time_one - start_time_one);

    let start_time_zero = Timer::get_usec();
    send_zero();
    let end_time_zero = Timer::get_usec();
    println!("Time to send zero: {}", end_time_zero - start_time_zero);
}

pub fn basic_run() {
    set_output(LIGHTSTRIP_PIN);
    enable_branch_prediction();
    enable_l1_instruction_cache();
    
    latency_test();

    let red = Color { r: 255, g: 0, b: 0 };
    let green = Color { r: 0, g: 255, b: 0 };
    let blue = Color { r: 0, g: 0, b: 255 };
    let white = Color { r: 255, g: 255, b: 255 };
    let off = Color { r: 0, g: 0, b: 0 };
    let yellow = Color { r: 255, g: 255, b: 0 };
    let cyan = Color { r: 0, g: 255, b: 255 };
    let magenta = Color { r: 255, g: 0, b: 255 };

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
}
use crate::arch::dev_barrier;
use crate::gpio::{read, set_input};
use crate::print;
use crate::println;
use crate::timer::{Timer};
use core::cmp::min;
use core::arch::global_asm;

// defined in rpi_constants.h. put the stack for this above the regular stack. 
global_asm!(r#"
.align 5
.global interrupt_vec
interrupt_vec:
  ldr pc, =unhandled_reset
  ldr pc, =unhandled_undefined_instruction
  ldr pc, =unhandled_swi
  ldr pc, =unhandled_prefetch_abort
  ldr pc, =unhandled_data_abort
  ldr pc, =interrupt_asm

interrupt_asm:
  mov sp, 0x9000000
  sub lr, lr, #4
  push {{r0-r12, lr}}
  mov r0, lr
  bl interrupt_vector
  pop {{r0-r12, lr}}
  movs pc, lr
"#);

const PIN: u32 = 21;
const BIG: u32 = 1_000_000_000u32;

const PREV_SIGNAL_VALUES: &[u32] = &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
const PREV_SIGNAL_LENGTHS: &[u32] = &[588, 586, 580, 560, 606, 561, 604, 587, 579, 562, 603, 586, 579, 560, 604, 587, 579, 1662, 580, 1663, 578, 1663, 580, 1661, 580, 1662, 580, 1663, 580, 1662, 580, 1663, 579, 1663, 555, 611, 577, 1664, 555, 584, 580, 610, 578, 562, 578, 1687, 558, 608, 552, 586, 579, 1688, 553, 611, 575, 1666, 554, 1687, 554, 1687, 552, 589, 598, 1664, 553, 20001];

const NEXT_SIGNAL_VALUES: &[u32] = &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
const NEXT_SIGNAL_LENGTHS: &[u32] = &[590, 558, 608, 585, 582, 560, 606, 586, 583, 585, 581, 560, 607, 585, 581, 585, 582, 1661, 583, 1663, 558, 1687, 584, 1662, 558, 1688, 559, 1687, 582, 1664, 559, 1687, 558, 609, 557, 1688, 557, 1688, 557, 609, 557, 586, 581, 610, 557, 1688, 558, 610, 557, 1688, 557, 585, 582, 609, 578, 1666, 557, 1687, 556, 1687, 557, 610, 555, 1688, 558, 20001];

const PLAY_PAUSE_SIGNAL_VALUES: &[u32] = &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
const PLAY_PAUSE_SIGNAL_LENGTHS: &[u32] = &[569, 577, 583, 583, 577, 588, 580, 584, 582, 585, 580, 587, 578, 586, 580, 586, 578, 1667, 584, 1658, 586, 1657, 586, 1656, 582, 1661, 585, 1658, 586, 1656, 578, 1664, 586, 1656, 584, 1658, 586, 1657, 587, 575, 586, 579, 584, 582, 617, 1628, 589, 574, 588, 579, 612, 551, 612, 553, 615, 1656, 586, 1656, 584, 1659, 587, 552, 614, 1655, 586, 20001];

const EQ_SIGNAL_VALUES: &[u32] = &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1];
const EQ_SIGNAL_LENGTHS: &[u32] = &[561, 610, 555, 610, 553, 611, 553, 612, 553, 612, 552, 611, 553, 612, 554, 586, 578, 1688, 554, 1688, 555, 1689, 553, 1688, 554, 1688, 552, 1688, 553, 1690, 552, 1689, 553, 1688, 553, 613, 552, 613, 553, 1688, 553, 586, 580, 612, 553, 612, 552, 612, 553, 611, 552, 1689, 554, 1689, 553, 612, 552, 1688, 553, 1689, 553, 1690, 553, 1689, 553, 20001];

const GPIO_BASE: u32 = 0x2020_0000;
const GPIO_RISING_ENABLE: u32 = GPIO_BASE + 0x004C;
const GPIO_FALLING_ENABLE: u32 = GPIO_BASE + 0x0058;
const GPIO_EVENT_DETECT: u32 = GPIO_BASE + 0x0040;
const GPIO_INT0: u32 = 49;
const IRQ_Enable_2: u32 = 0x2000_b200 + 0x14;

pub fn get_value() -> (u32, u32) { // (value, length)
    let start_time = Timer::get_usec();
    let value: u32 = read(PIN);
    while(true) {
        let curr_value = read(PIN);
        let curr_time = Timer::get_usec();
        if(curr_value != value || (curr_time.wrapping_sub(start_time)) > 20000) {
            break;
        }
    }
    let end_time = Timer::get_usec();
    return (value, end_time.wrapping_sub(start_time));
}

pub fn is_around(x: u32, base: u32) -> bool {
    return ((4 * base / 5) <= x && (6 * base / 5) >= x);
}

pub fn is_close_match(values: &[u32], lengths: &[u32], ref_values: &[u32], ref_lengths: &[u32]) -> bool {
    let mut works = true;
    for i in 0..min(values.len(), ref_values.len()) {
        if !(values[i] == ref_values[i] && is_around(lengths[i], ref_lengths[i])) {
            works = false;
        }
    }
    return works;
}

pub fn gpio_int_rising_edge(pin: u32) {
    dev_barrier();
    let enable_addr = (GPIO_RISING_ENABLE + (pin / 32) * 4) as *mut u32;
    let mask = 1u32 << (pin % 32);
    unsafe {
        let old_value = core::ptr::read_volatile(enable_addr);
        core::ptr::write_volatile(enable_addr, old_value | mask);
    }
    dev_barrier();
    unsafe {
        core::ptr::write_volatile(IRQ_Enable_2 as *mut u32, 1 << (GPIO_INT0 % 32));
    }
    dev_barrier();
}

pub fn gpio_int_falling_edge(pin: u32) {
    dev_barrier();
    let enable_addr = (GPIO_FALLING_ENABLE + (pin / 32) * 4) as *mut u32;
    let mask = 1u32 << (pin % 32);
    unsafe {
        let old_value = core::ptr::read_volatile(enable_addr);
        core::ptr::write_volatile(enable_addr, old_value | mask);
    }
    dev_barrier();
    unsafe {
        core::ptr::write_volatile(IRQ_Enable_2 as *mut u32, 1 << (GPIO_INT0 % 32));
    }
    dev_barrier();
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector(pc: u32) {
    println!("in interrupt vector to process a signal. pc={}", pc);
    process_data();
}


pub fn ir_main() {
    set_input(PIN);
    gpio_int_falling_edge(PIN);
    gpio_int_rising_edge(PIN);

    let mut prev_run: u32 = 0;
    while (true) {
        Timer::delay_us(100);
        let curr_run = Timer::get_usec();
        println!("While loop, delay since previous = {}", curr_run - prev_run);
        prev_run = curr_run;
    }
}

pub fn process_data(){
    let (mut prev_value, mut prev_time) = (0u32, 0u32);
    while(true) {
        let (curr_value, curr_time) = get_value();
        if (prev_value == 0 && curr_value == 1 && is_around(prev_time, 9000) && is_around(curr_time, 4500)) {
            break;
        }
        
        (prev_value, prev_time) = (curr_value, curr_time);
    }

    // now that we are reading signals
    let mut signal_values: [u32; 100] = [BIG; 100];
    let mut signal_length: [u32; 100] = [0u32; 100];
    for i in 0..100 {
        let (curr_value, curr_time) = get_value();
        signal_values[i] = curr_value;
        signal_length[i] = curr_time;
        if(curr_value == 1 && curr_time > 10000) {
            break;
        }
    }

    if(is_close_match(&signal_values, &signal_length, PREV_SIGNAL_VALUES, PREV_SIGNAL_LENGTHS)) {
        println!("it's prev");
    } else if (is_close_match(&signal_values, &signal_length, NEXT_SIGNAL_VALUES, NEXT_SIGNAL_LENGTHS)) {
        println!("it's next");
    } else if (is_close_match(&signal_values, &signal_length, PLAY_PAUSE_SIGNAL_VALUES, PLAY_PAUSE_SIGNAL_LENGTHS)) {
        println!("it's play pause");
    } else if (is_close_match(&signal_values, &signal_length, EQ_SIGNAL_VALUES, EQ_SIGNAL_LENGTHS)) {
        println!("it's eq");
    } else {
        println!("idk");
    }
    
    print!("let signal_values = [");
    for i in 0..100 {
        print!("{}", signal_values[i]);
        if(i == 100 - 1 || signal_values[i + 1] > 1) {
            break;
        }
        print!(", ");
    }
    print!("];\n");

    print!("let signal_lengths = [");
    for i in 0..100 {
        print!("{}", signal_length[i]);
        if(i == 100 - 1 || signal_values[i + 1] > 1) {
            break;
        }
        print!(", ");
    }
    print!("];\n");
}

// pub fn ir_main() {
//     set_input(PIN);

//     while(true) {
//         process_data();
//     }
// }
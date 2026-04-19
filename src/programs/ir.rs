use crate::arch::{dev_barrier, gcc_mb};
use crate::gpio::{read, set_input, set_output, set_on, set_off};
use crate::{print, start};
use crate::println;
use crate::timer::{Timer};
use core::cmp::min;
use core::arch::{asm, global_asm};

// defined in rpi_constants.h. put the stack for this above the regular stack. 
global_asm!(r#"
.globl _interrupt_table_ir
.globl _interrupt_table_end_ir
_interrupt_table_ir:
  @ Q: why can we copy these ldr jumps and have
  @ them work the same?
    ldr pc, _reset_asm_ir                    @ 0x0: Q: why this order?[A2-16]
    ldr pc, _undefined_instruction_asm_ir    @ 0x4
    ldr pc, _software_interrupt_asm_ir       @ 0x8
    ldr pc, _prefetch_abort_asm_ir
    ldr pc, _data_abort_asm_ir
    ldr pc, _reset_asm_ir
    ldr pc, _interrupt_asm_ir
infinite_loop:
    bl infinite_loop
fast_interrupt_asm:
  sub   lr, lr, #4 @First instr of FIQ handler
  push  {{lr}}
  push  {{r0-r12}}
  mov   r0, lr              @ Pass old pc
    bl    fast_interrupt_vector_ir    @ C function
  pop   {{r0-r12}}
  ldm   sp!, {{pc}}^
_reset_asm_ir:                   .word reset_asm_ir
_undefined_instruction_asm_ir:   .word undefined_instruction_asm_ir
_software_interrupt_asm_ir:      .word software_interrupt_asm_ir
_prefetch_abort_asm_ir:          .word prefetch_abort_asm_ir
_data_abort_asm_ir:              .word data_abort_asm_ir
_interrupt_asm_ir:               .word interrupt_asm_ir
_interrupt_table_end_ir:   @ end of the table.

undefined_instruction_asm_ir:                      @ A2-19
    bx lr  
software_interrupt_asm_ir:                         @ A2-20
    bx lr
prefetch_abort_asm_ir:
    bx lr
data_abort_asm_ir:
    bx lr
reset_asm_ir:
    bx lr


interrupt_asm_ir:
  @ NOTE:
  @  - each mode has its own <sp> that persists when
  @    we switch out of the mode (i.e., will be the same
  @    when switch back).
  @  - <INT_STACK_ADDR> is a physical address we reserve 
  @   for exception stacks today.  we don't do recursive
  @   exception/interupts so one stack is enough.
  mov sp, 0x9000000   @ Q: what if you delete?
  sub   lr, lr, #4

  @ push regs: beter match a pop
  push  {{r0-r12,lr}}         @ XXX: pushing too many 
                            @ registers: only need caller
                            @ saved.

  mov   r0, lr              @ Pass old pc as arg 0
    bl    interrupt_vector_ir    @ C function: expects C 
                            @ calling conventions.

  @ pop regs: better match push (what happens if not?)
  pop   {{r0-r12,lr}} 	    @ pop integer registers
                            @ this MUST MATCH the push.
                            @ very common mistake.

  @ return from interrupt handler: will re-enable general ints.
  @ Q: what happens if you do "mov" instead?
  @ Q: what other instructions could we use?
  movs    pc, lr        @ 1: moves <spsr> into <cpsr> 
                        @ 2. moves <lr> into the <pc> of that
                        @    mode.

.globl enable_interrupts_ir
enable_interrupts_ir:
    mrs r0, cpsr @ move cpsr to r0
    bic r0,r0,#(1<<7)	@ clear 7th bit.
    msr cpsr_c,r0		@ move r0 back to PSR
    bx lr		        @ return

.globl disable_interrupts_ir
disable_interrupts_ir:
    mrs r0,cpsr		       
    orr r0,r0,#(1<<7)	@ set 7th bit
    msr cpsr_c,r0
    bx lr
"#);

unsafe extern "C" {
    #[link_name = "enable_interrupts_ir"]
    fn enable_interrupts_asm();
    
    #[link_name = "disable_interrupts_ir"]
    unsafe fn disable_interrupts_asm();

    #[link_name = "interrupt_asm_ir"]
    unsafe fn interrupt_asm();

    #[link_name = "_interrupt_table_ir"]
    static INTERRUPT_TABLE_START: u8;

    #[link_name = "_interrupt_table_end_ir"]
    static INTERRUPT_TABLE_END: u8;
}

pub fn move_table() {
    let start: *const u32 = core::ptr::addr_of!(INTERRUPT_TABLE_START) as *const u32;
    let end: *const u32 = core::ptr::addr_of!(INTERRUPT_TABLE_END) as *const u32;
    let len = ((end as usize) - (start as usize)) / 4;
    let dst = core::ptr::without_provenance_mut::<u32>(0);
    unsafe {
        for i in 0..len {
            core::arch::asm!(
                "ldr {t}, [{i}]",
                "str {t}, [{o}]",
                t = out(reg) _,
                i = in(reg) start.add(i),
                o = in(reg) dst.add(i),
            )
        }
        // core::ptr::copy_nonoverlapping(start, dst, len);
    }
}

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
const IRQ_Disable_1: u32 = 0x2000_b200 + 0x1c;
const IRQ_Disable_2: u32 = 0x2000_b200 + 0x20;

static mut was_interrupted: u32 = 0;

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

pub fn gpio_event_clear(pin: u32) {
    dev_barrier();
    unsafe {
        core::ptr::write_volatile(GPIO_EVENT_DETECT as *mut u32, 1 << pin);
    }
    dev_barrier();
}

#[unsafe(no_mangle)]
pub extern "C" fn interrupt_vector_ir(pc: u32) {
    println!("in interrupt vector to process a signal. pc={}", pc);
    unsafe { was_interrupted = 1; }
    process_data();
    gpio_event_clear(PIN);
    println!("done with process data. go back to while loop.");
}

#[unsafe(no_mangle)]
pub extern "C" fn fast_interrupt_vector_ir(pc: u32) {
    println!("in fast interrupt vector. pc={}", pc);
}

pub fn ir_main() {    
    set_input(PIN);
    set_output(27);

    println!("about to install interrupts");
    unsafe {
        disable_interrupts_asm();
        core::ptr::write_volatile(IRQ_Disable_1 as *mut u32, 0xffffffff);
        core::ptr::write_volatile(IRQ_Disable_2 as *mut u32,  0xffffffff);
        println!("Just disabled interrupts.");
    }
    dev_barrier();

    gcc_mb();
    move_table();
    gcc_mb();
    println!("Just moved tables.");

    gpio_event_clear(PIN);
    // gpio_int_falling_edge(PIN);
    gpio_int_rising_edge(PIN);
    println!("Just finished setting gpio_int_falling_edge");

    unsafe {
        enable_interrupts_asm();
    }
    println!("Just enabled interrupts");

    let mut prev_run: u32 = 0;
    while (true) {
        // unsafe {
        //     if(was_interrupted == 1) {
        //         disable_interrupts_asm();
        //     }
        // }
        Timer::delay_us(100000);
        unsafe { disable_interrupts_asm(); }
        let curr_run = Timer::get_usec();
        println!("While loop, delay since previous = {}", curr_run - prev_run);
        prev_run = curr_run;
        unsafe { enable_interrupts_asm(); }
    }
}

pub fn process_data(){
    let (mut prev_value, mut prev_time) = (0u32, 0u32);
    let start_time = Timer::get_usec();
    while(true) {
        let (curr_value, curr_time) = get_value();
        if (prev_value == 0 && curr_value == 1 && is_around(prev_time, 9000) && is_around(curr_time, 4500)) {
            break;
        }
        // if(curr_time - start_time >= 1000000) {
        //     println!("nothing happened for a second");
        //     return;
        // }
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
        println!("BUTTON NAME: it's prev");
    } else if (is_close_match(&signal_values, &signal_length, NEXT_SIGNAL_VALUES, NEXT_SIGNAL_LENGTHS)) {
        println!("BUTTON NAME: it's next");
    } else if (is_close_match(&signal_values, &signal_length, PLAY_PAUSE_SIGNAL_VALUES, PLAY_PAUSE_SIGNAL_LENGTHS)) {
        println!("BUTTON NAME: it's play pause");
    } else if (is_close_match(&signal_values, &signal_length, EQ_SIGNAL_VALUES, EQ_SIGNAL_LENGTHS)) {
        println!("BUTTON NAME: it's eq");
    } else {
        println!("BUTTON NAME: idk");
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

    Timer::delay_us(2000);

    println!("done with process_data!");
}

// pub fn ir_main() {
//     set_input(PIN);

//     while(true) {
//         process_data();
//     }
// }
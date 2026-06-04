use crate::mem::{get32, put32};

pub const GPIO_MAX_PIN: u32 = 53;
pub const GPIO_BASE: u32 = 0x20200000;
pub const GPIO_SET0: u32 = GPIO_BASE + 0x1C;
pub const GPIO_CLR0: u32 = GPIO_BASE + 0x28;
pub const GPIO_LEV0: u32 = GPIO_BASE + 0x34;

#[macro_export]
macro_rules! hardcode_gpio_pin {
    ($set_on_fn:ident, $set_off_fn:ident, $pin:expr) => {
        #[inline(always)]
        pub fn $set_on_fn() {
            const PIN: u32 = $pin;
            const ADDR: u32 = GPIO_SET0 + (PIN / 32) * 4;
            const SHIFT: u32 = 1 << (PIN % 32);
            $crate::mem::put32(ADDR, SHIFT);
            unsafe { core::arch::asm!(""); }
        }

        #[inline(always)]
        pub fn $set_off_fn() {
            const PIN: u32 = $pin;
            const ADDR: u32 = GPIO_CLR0 + (PIN / 32) * 4;
            const SHIFT: u32 = 1 << (PIN % 32);
            $crate::mem::put32(ADDR, SHIFT);
        }
    };
}

hardcode_gpio_pin!(set_on_21, set_off_21, 21);
hardcode_gpio_pin!(set_on_27, set_off_27, 27);

pub fn set_output(pin: u32) {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    let addr = GPIO_BASE + (pin / 10) * 4;
    let mut mode = get32(addr);
    mode &= !(0b111 << (3 * (pin % 10)));
    mode |= 1 << (3 * (pin % 10));
    put32(addr, mode);
}

#[inline(always)]
pub fn set_on_unsafe(pin: u32) {
    let addr = GPIO_SET0 + (pin / 32) * 4;
    let shift = 1 << (pin % 32);
    put32(addr, shift);
}

#[inline(always)]
pub fn set_off_unsafe(pin: u32) {
    let addr = GPIO_CLR0 + (pin / 32) * 4;
    let shift = 1 << (pin % 32);
    put32(addr, shift);
}


#[inline(always)]
pub fn set_on(pin: u32) {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    let addr = GPIO_SET0 + (pin / 32) * 4;
    let shift = 1 << (pin % 32);
    put32(addr, shift);
}

#[inline(always)]
pub fn set_off(pin: u32) {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    let addr = GPIO_CLR0 + (pin / 32) * 4;
    let shift = 1 << (pin % 32);
    put32(addr, shift);
}

#[inline]
pub fn write(pin: u32, v: u32) {
    if v != 0 {
        set_on(pin);
    } else {
        set_off(pin);
    }
}

pub fn set_input(pin: u32) {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    let addr = GPIO_BASE + (pin / 10) * 4;
    let mut mode = get32(addr);
    mode &= !(0b111 << (3 * (pin % 10)));
    put32(addr, mode);
}

#[inline]
pub fn read(pin: u32) -> u32 {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    let addr = GPIO_LEV0 + (pin / 32) * 4;
    let mut v = get32(addr);
    v >>= pin % 32;
    v &= 1;
    v
}

pub fn set_function(pin: u32, func: u32) {
    if pin > GPIO_MAX_PIN {
        panic!("illegal pin={}", pin);
    }
    if (func & 0b111) != func {
        panic!("illegal func={:x}", func);
    }
    let addr = GPIO_BASE + (pin / 10) * 4;
    let mut mode = get32(addr);
    mode &= !(0b111 << (3 * (pin % 10)));
    mode |= func << (3 * (pin % 10));
    put32(addr, mode);
}
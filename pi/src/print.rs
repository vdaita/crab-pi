pub struct UartProxy;

impl ::core::fmt::Write for UartProxy {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        crate::uart::write_bytes(s.as_bytes());
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($args:tt)*) => {
        {
            #[allow(unused)]
            use ::core::fmt::Write;
            let _ = ::core::write!(&mut $crate::print::UartProxy, $($args)*);
            $crate::uart::flush();
        }
    }
}
#[macro_export]
macro_rules! println {
    ($($args:tt)*) => {
        {
            #[allow(unused)]
            use ::core::fmt::Write;
            let _ = ::core::writeln!(&mut $crate::print::UartProxy, $($args)*);
            $crate::uart::flush();
        }
    }
}

pub fn print_binary_table(label: &str, value: u32) {
    let bit_width = 32 - value.leading_zeros() as usize;
    let bit_width = if bit_width == 0 { 1 } else { bit_width };

    print!("{} ({:#b}):\n", label, value);
    print!("Index | Bit\n");
    print!("------+----\n");

    // MSB -> LSB
    for i in (0..bit_width).rev() {
        let bit = (value >> i) & 1;
        print!("{:>5} |  {}\n", i, bit);
    }
}

pub fn print_binary_compare(label: &str, a: u32, b: u32) {
    let width_a = 32 - a.leading_zeros() as usize;
    let width_b = 32 - b.leading_zeros() as usize;
    let bit_width = width_a.max(width_b).max(1);

    print!("{} (a={:#b}, b={:#b}):\n", label, a, b);
    print!("Index | A | B\n");
    print!("------+---+---\n");

    // MSB -> LSB
    for i in (0..bit_width).rev() {
        let bit_a = (a >> i) & 1;
        let bit_b = (b >> i) & 1;
        print!("{:>5} | {} | {}\n", i, bit_a, bit_b);
    }
}
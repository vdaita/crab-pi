use crate::programs::imu::{i2c_init, i2c_write};
use crate::timer::Timer;

const SSD1306_DISPLAY_ADDRESS: u32 = 0x3c;
const SSD1306_DISPLAY_WIDTH: usize = 128;
const SSD1306_DISPLAY_HEIGHT: usize = 64;
const SSD1306_DISPLAY_BUFFER_SIZE: usize = SSD1306_DISPLAY_WIDTH * ((SSD1306_DISPLAY_HEIGHT + 7) / 8);
const SSD1306_I2C_BUFFER_SIZE: usize = SSD1306_DISPLAY_BUFFER_SIZE + 1;

fn ssd1306_display_send_command(cmd: u8) {
    let cmd_buf: [u8; 2] = [0x00, cmd];
    i2c_write(SSD1306_DISPLAY_ADDRESS, &cmd_buf, 2);
}

fn turn_bit_on(x: usize, y: usize, grid: &mut [u8; SSD1306_I2C_BUFFER_SIZE]) {
    grid[(y / 8) * (SSD1306_DISPLAY_WIDTH as usize) + x] |= (1 << (y & 7));
}

fn clear_grid(grid: &mut [u8; SSD1306_I2C_BUFFER_SIZE]) {
    for i in 1..SSD1306_I2C_BUFFER_SIZE {
        grid[i] = 0;
    }
}

pub fn test_oled_display() {
    Timer::delay_ms(100);
    i2c_init(1500);
    Timer::delay_ms(100);

    // i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xae, 0x8d, 0x14, 0xaf, 0xa5], 6);

    ssd1306_display_send_command(0xae);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0x8D, 0x14], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0x20, 0x00], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0x40], 2);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xA1], 2);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xC8], 2);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xDA, 0x12], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0x81, 0x7F], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xd9, 0xf1], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xdb, 0x40], 3);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xa4], 2);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xa6], 2);
    i2c_write(SSD1306_DISPLAY_ADDRESS, &[0x00, 0xaf], 2);

    let mut grid: [u8; SSD1306_I2C_BUFFER_SIZE] = [0; SSD1306_I2C_BUFFER_SIZE];
    grid[0] = 0x40;
    for x in 0..(SSD1306_DISPLAY_WIDTH / 2) {
        turn_bit_on(x, x, &mut grid);
        turn_bit_on(128 - x, x, &mut grid);
        i2c_write(SSD1306_DISPLAY_ADDRESS, &mut grid, 1025);
        Timer::delay_ms(5);
    }

    for x in 0..SSD1306_DISPLAY_WIDTH {
        let mut grid: [u8; SSD1306_I2C_BUFFER_SIZE] = [0; SSD1306_I2C_BUFFER_SIZE];
        grid[0] = 0x40;
        for y in 0..(SSD1306_DISPLAY_HEIGHT) {
            turn_bit_on(x, y, &mut grid);
        }

        i2c_write(SSD1306_DISPLAY_ADDRESS, &mut grid, 1025);
        Timer::delay_ms(5);
    }
    
    
    for y in 0..SSD1306_DISPLAY_HEIGHT {
        let mut grid: [u8; SSD1306_I2C_BUFFER_SIZE] = [0; SSD1306_I2C_BUFFER_SIZE];
        grid[0] = 0x40;
        for x in 0..(SSD1306_DISPLAY_WIDTH) {
            turn_bit_on(x, y, &mut grid);
        }

        i2c_write(SSD1306_DISPLAY_ADDRESS, &mut grid, 1025);
        Timer::delay_ms(5);
    }


   

}
#![allow(dead_code)]
use core::cell::UnsafeCell;
use core::ptr;

use crate::gpio;
use crate::timer::Timer;

const EMMC_DEBUG: bool = false;

const SD_CLOCK_ID: u32 = 400_000;
const SD_CLOCK_NORMAL: u32 = 25_000_000;
const SD_CLOCK_HIGH: u32 = 50_000_000;
const SD_CLOCK_100: u32 = 100_000_000;
const SD_CLOCK_208: u32 = 208_000_000;

const SD_COMMAND_COMPLETE: u32 = 1;
const SD_TRANSFER_COMPLETE: u32 = 1 << 1;
const SD_BLOCK_GAP_EVENT: u32 = 1 << 2;
const SD_DMA_INTERRUPT: u32 = 1 << 3;
const SD_BUFFER_WRITE_READY: u32 = 1 << 4;
const SD_BUFFER_READ_READY: u32 = 1 << 5;
const SD_CARD_INSERTION: u32 = 1 << 6;
const SD_CARD_REMOVAL: u32 = 1 << 7;
const SD_CARD_INTERRUPT: u32 = 1 << 8;

const EMMC_BASE: usize = 0x2030_0000;

const EMMC_CTRL1_RESET_DATA: u32 = 1 << 26;
const EMMC_CTRL1_RESET_CMD: u32 = 1 << 25;
const EMMC_CTRL1_RESET_HOST: u32 = 1 << 24;
const EMMC_CTRL1_RESET_ALL: u32 = EMMC_CTRL1_RESET_DATA | EMMC_CTRL1_RESET_CMD | EMMC_CTRL1_RESET_HOST;

const EMMC_CTRL1_CLK_GENSEL: u32 = 1 << 5;
const EMMC_CTRL1_CLK_ENABLE: u32 = 1 << 2;
const EMMC_CTRL1_CLK_STABLE: u32 = 1 << 1;
const EMMC_CTRL1_CLK_INT_EN: u32 = 1 << 0;

const EMMC_CTRL0_ALT_BOOT_EN: u32 = 1 << 22;
const EMMC_CTRL0_BOOT_EN: u32 = 1 << 21;
const EMMC_CTRL0_SPI_MODE: u32 = 1 << 20;

const EMMC_STATUS_DAT_INHIBIT: u32 = 1 << 1;
const EMMC_STATUS_CMD_INHIBIT: u32 = 1 << 0;

const GPIO_FUNC_INPUT: u32 = 0;
const GPIO_FUNC_ALT3: u32 = 7;

#[repr(transparent)]
struct Volatile<T>(UnsafeCell<T>);

unsafe impl<T> Sync for Volatile<T> {}

impl Volatile<u32> {
    #[inline(always)]
    fn read(&self) -> u32 {
        unsafe { ptr::read_volatile(self.0.get()) }
    }

    #[inline(always)]
    fn write(&self, val: u32) {
        unsafe { ptr::write_volatile(self.0.get(), val) }
    }
}

#[repr(C)]
struct EmmcRegs {
    arg2: Volatile<u32>,
    block_size_count: Volatile<u32>,
    arg1: Volatile<u32>,
    cmd_xfer_mode: Volatile<u32>,
    response: [Volatile<u32>; 4],
    data: Volatile<u32>,
    status: Volatile<u32>,
    control: [Volatile<u32>; 2],
    int_flags: Volatile<u32>,
    int_mask: Volatile<u32>,
    int_enable: Volatile<u32>,
    control2: Volatile<u32>,
    cap1: Volatile<u32>,
    cap2: Volatile<u32>,
    res0: [Volatile<u32>; 2],
    force_int: Volatile<u32>,
    res1: [Volatile<u32>; 7],
    boot_timeout: Volatile<u32>,
    debug_config: Volatile<u32>,
    res2: [Volatile<u32>; 2],
    ext_fifo_config: Volatile<u32>,
    ext_fifo_enable: Volatile<u32>,
    tune_step: Volatile<u32>,
    tune_SDR: Volatile<u32>,
    tune_DDR: Volatile<u32>,
    res3: [Volatile<u32>; 23],
    spi_int_support: Volatile<u32>,
    res4: [Volatile<u32>; 2],
    slot_int_status: Volatile<u32>,
}

#[inline(always)]
fn emmc() -> &'static EmmcRegs {
    unsafe { &*(EMMC_BASE as *const EmmcRegs) }
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
enum CmdRespType {
    RTNone = 0,
    RT136 = 1,
    RT48 = 2,
    RT48Busy = 3,
}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq)]
enum CmdType {
    CTGoIdle = 0,
    CTSendCide = 2,
    CTSendRelativeAddr = 3,
    CTIOSetOpCond = 5,
    CTSelectCard = 7,
    CTSendIfCond = 8,
    CTSetBlockLen = 16,
    CTReadBlock = 17,
    CTReadMultiple = 18,
    CTWriteBlock = 24,
    CTWriteMultiple = 25,
    CTOcrCheck = 41,
    CTSendSCR = 51,
    CTApp = 55,
}

#[repr(u8)]
#[derive(Copy, Clone)]
enum SdError {
    SDECommandTimeout,
    SDECommandCrc,
    SDECommandEndBit,
    SDECommandIndex,
    SDEDataTimeout,
    SDEDataCrc,
    SDEDataEndBit,
    SDECurrentLimit,
    SDEAutoCmd12,
    SDEADma,
    SDETuning,
    SDERsvd,
}

#[derive(Copy, Clone)]
struct EmmcCmd {
    resp_a: u8,
    block_count: u8,
    auto_command: u8,
    direction: u8,
    multiblock: u8,
    resp_b: u16,
    response_type: CmdRespType,
    res0: u8,
    crc_enable: u8,
    idx_enable: u8,
    is_data: u8,
    cmd_type: u8,
    index: u8,
    res1: u8,
}

impl EmmcCmd {
    const fn pack(&self) -> u32 {
        let mut v = 0u32;
        v |= (self.resp_a as u32) & 0x1;
        v |= ((self.block_count as u32) & 0x1) << 1;
        v |= ((self.auto_command as u32) & 0x3) << 2;
        v |= ((self.direction as u32) & 0x1) << 4;
        v |= ((self.multiblock as u32) & 0x1) << 5;
        v |= ((self.resp_b as u32) & 0x3FF) << 6;
        v |= ((self.response_type as u32) & 0x3) << 16;
        v |= ((self.res0 as u32) & 0x1) << 18;
        v |= ((self.crc_enable as u32) & 0x1) << 19;
        v |= ((self.idx_enable as u32) & 0x1) << 20;
        v |= ((self.is_data as u32) & 0x1) << 21;
        v |= ((self.cmd_type as u32) & 0x3) << 22;
        v |= ((self.index as u32) & 0x3F) << 24;
        v |= ((self.res1 as u32) & 0x3) << 30;
        v
    }
}

#[derive(Copy, Clone)]
struct ScrRegister {
    scr: [u32; 2],
    bus_widths: u32,
    version: u32,
}

#[derive(Copy, Clone)]
struct EmmcDevice {
    last_success: bool,
    transfer_blocks: u32,
    last_command: EmmcCmd,
    last_command_value: u32,
    block_size: u32,
    last_response: [u32; 4],
    sdhc: bool,
    ocr: u16,
    rca: u32,
    offset: u64,
    buffer: *mut u8,
    base_clock: u32,
    last_error: u32,
    last_interrupt: u32,
    scr: ScrRegister,
}

impl EmmcDevice {
    const fn zeroed() -> Self {
        EmmcDevice {
            last_success: false,
            transfer_blocks: 0,
            last_command: INVALID_CMD,
            last_command_value: 0,
            block_size: 0,
            last_response: [0; 4],
            sdhc: false,
            ocr: 0,
            rca: 0,
            offset: 0,
            buffer: core::ptr::null_mut(),
            base_clock: 0,
            last_error: 0,
            last_interrupt: 0,
            scr: ScrRegister { scr: [0; 2], bus_widths: 0, version: 0 },
        }
    }
}

const RES_CMD: EmmcCmd = EmmcCmd {
    resp_a: 1,
    block_count: 1,
    auto_command: 3,
    direction: 1,
    multiblock: 1,
    resp_b: 0xF,
    response_type: CmdRespType::RT48,
    res0: 1,
    crc_enable: 1,
    idx_enable: 1,
    is_data: 1,
    cmd_type: 3,
    index: 0xF,
    res1: 3,
};

const INVALID_CMD: EmmcCmd = RES_CMD;

const COMMANDS: [EmmcCmd; 56] = [
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RTNone, res0: 0, crc_enable: 0, idx_enable: 0, is_data: 0, cmd_type: 0, index: 0, res1: 0 },
    RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT136, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 2, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 3, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RTNone, res0: 0, crc_enable: 0, idx_enable: 0, is_data: 0, cmd_type: 0, index: 4, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT136, res0: 0, crc_enable: 0, idx_enable: 0, is_data: 0, cmd_type: 0, index: 5, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 6, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48Busy, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 7, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 8, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT136, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 9, res1: 0 },
    RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 16, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 1, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 1, cmd_type: 0, index: 17, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 1, auto_command: 1, direction: 1, multiblock: 1, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 1, cmd_type: 0, index: 18, res1: 0 },
    RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 1, cmd_type: 0, index: 24, res1: 0 },
    EmmcCmd { resp_a: 0, block_count: 1, auto_command: 1, direction: 0, multiblock: 1, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 1, cmd_type: 0, index: 25, res1: 0 },
    RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 0, idx_enable: 0, is_data: 0, cmd_type: 0, index: 41, res1: 0 },
    RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD, RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 1, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 1, cmd_type: 0, index: 51, res1: 0 },
    RES_CMD, RES_CMD, RES_CMD,
    EmmcCmd { resp_a: 0, block_count: 0, auto_command: 0, direction: 0, multiblock: 0, resp_b: 0, response_type: CmdRespType::RT48, res0: 0, crc_enable: 1, idx_enable: 0, is_data: 0, cmd_type: 0, index: 55, res1: 0 },
];

static mut DEVICE: EmmcDevice = EmmcDevice::zeroed();

#[inline(always)]
fn bswap32(x: u32) -> u32 {
    ((x << 24) & 0xff00_0000) | ((x << 8) & 0x00ff_0000) | ((x >> 8) & 0x0000_ff00) | ((x >> 24) & 0x0000_00ff)
}

fn wait_reg_mask(reg: &Volatile<u32>, mask: u32, set: bool, timeout: u32) -> bool {
    for _ in 0..=timeout * 10 {
        let v = reg.read();
        if if set { (v & mask) != 0 } else { (v & mask) == 0 } {
            return true;
        }
        Timer::delay_us(100);
    }
    false
}

fn get_clock_divider(base_clock: u32) -> u32 {
    const TARGET_RATE: u32 = SD_CLOCK_HIGH;
    let mut target_div: u32 = 1;

    if TARGET_RATE <= base_clock {
        target_div = base_clock / TARGET_RATE;
        if base_clock % TARGET_RATE != 0 {
            target_div = 0;
        }
    }

    let mut div: i32 = -1;
    for fb in (0..=31).rev() {
        let bt = 1u32 << fb;
        if (target_div & bt) != 0 {
            div = fb as i32;
            target_div &= !bt;
            if target_div != 0 {
                div += 1;
            }
            break;
        }
    }

    if div == -1 {
        div = 31;
    }
    if div >= 32 {
        div = 31;
    }
    if div != 0 {
        div = 1 << (div - 1);
    }
    if div >= 0x400 {
        div = 0x3FF;
    }

    let freqSel = (div as u32) & 0xff;
    let upper = ((div as u32) >> 8) & 0x3;
    (freqSel << 8) | (upper << 6) | (0 << 5)
}

fn emmc_setup_clock() -> bool {
    let regs = emmc();
    regs.control2.write(0);

    let rate = 250_000_000u32;

    let mut n = regs.control[1].read();
    n |= EMMC_CTRL1_CLK_INT_EN;
    n |= get_clock_divider(rate);
    n &= !(0xf << 16);
    n |= 11 << 16;

    regs.control[1].write(n);

    if !wait_reg_mask(&regs.control[1], EMMC_CTRL1_CLK_STABLE, true, 2000) {
        crate::println!("EMMC_ERR: SD CLOCK NOT STABLE");
        return false;
    }

    Timer::delay_ms(30);
    regs.control[1].write(regs.control[1].read() | EMMC_CTRL1_CLK_ENABLE);
    Timer::delay_ms(30);

    true
}

fn sd_error_mask(err: SdError) -> u32 {
    1 << (16 + (err as u32))
}

fn set_last_error(intr_val: u32) {
    unsafe {
        DEVICE.last_error = intr_val & 0xFFFF_0000;
        DEVICE.last_interrupt = intr_val;
    }
}

fn do_data_transfer(cmd: EmmcCmd) -> bool {
    let regs = emmc();
    let mut wrIrpt = 0u32;
    let mut write = false;

    if cmd.direction != 0 {
        wrIrpt = 1 << 5;
    } else {
        wrIrpt = 1 << 4;
        write = true;
    }

    let mut data = unsafe { DEVICE.buffer as *mut u32 };

    for _ in 0..unsafe { DEVICE.transfer_blocks } {
        wait_reg_mask(&regs.int_flags, wrIrpt | 0x8000, true, 2000);
        let intr_val = regs.int_flags.read();
        regs.int_flags.write(wrIrpt | 0x8000);

        if (intr_val & (0xffff_0000 | wrIrpt)) != wrIrpt {
            set_last_error(intr_val);
            return false;
        }

        let mut length = unsafe { DEVICE.block_size };

        if write {
            while length > 0 {
                regs.data.write(unsafe { ptr::read_unaligned(data) });
                data = unsafe { data.add(1) };
                length -= 4;
            }
        } else {
            while length > 0 {
                unsafe { ptr::write_unaligned(data, regs.data.read()); }
                data = unsafe { data.add(1) };
                length -= 4;
            }
        }
    }

    true
}

fn emmc_issue_command(cmd: EmmcCmd, arg: u32, timeout: u32) -> bool {
    let regs = emmc();
    unsafe {
        DEVICE.last_command_value = cmd.pack();
    }
    let command_reg = unsafe { DEVICE.last_command_value };

    if unsafe { DEVICE.transfer_blocks } > 0xFFFF {
        crate::println!("EMMC_ERR: transferBlocks too large: {}", unsafe { DEVICE.transfer_blocks });
        return false;
    }

    regs.block_size_count.write(unsafe { DEVICE.block_size } | (unsafe { DEVICE.transfer_blocks } << 16));
    regs.arg1.write(arg);
    regs.cmd_xfer_mode.write(command_reg);

    let mut times = 0u32;
    while times < timeout {
        let reg = regs.int_flags.read();
        if (reg & 0x8001) != 0 {
            break;
        }
        Timer::delay_ms(1);
        times += 1;
    }

    if times >= timeout {
        crate::println!("EMMC_WARN: emmc_issue_command timed out");
        unsafe { DEVICE.last_success = false; }
        return false;
    }

    let mut intr_val = regs.int_flags.read();
    regs.int_flags.write(0xFFFF_0001);

    if (intr_val & 0xFFFF_0001) != 1 {
        if EMMC_DEBUG {
            crate::println!("EMMC_DEBUG: Error waiting for command interrupt complete: {}", cmd.index);
        }
        set_last_error(intr_val);
        if EMMC_DEBUG {
            crate::println!("EMMC_DEBUG: IRQFLAGS: {:x} - {:x} - {:x}", regs.int_flags.read(), regs.status.read(), intr_val);
        }
        unsafe { DEVICE.last_success = false; }
        return false;
    }

    match cmd.response_type {
        CmdRespType::RT48 | CmdRespType::RT48Busy => unsafe {
            DEVICE.last_response[0] = regs.response[0].read();
        },
        CmdRespType::RT136 => unsafe {
            DEVICE.last_response[0] = regs.response[0].read();
            DEVICE.last_response[1] = regs.response[1].read();
            DEVICE.last_response[2] = regs.response[2].read();
            DEVICE.last_response[3] = regs.response[3].read();
        },
        CmdRespType::RTNone => {}
    }

    if cmd.is_data != 0 {
        do_data_transfer(cmd);
    }

    if cmd.response_type == CmdRespType::RT48Busy || cmd.is_data != 0 {
        wait_reg_mask(&regs.int_flags, 0x8002, true, 2000);
        intr_val = regs.int_flags.read();
        regs.int_flags.write(0xFFFF_0002);

        if (intr_val & 0xFFFF_0002) != 2 && (intr_val & 0xFFFF_0002) != 0x100002 {
            set_last_error(intr_val);
            return false;
        }

        regs.int_flags.write(0xFFFF_0002);
    }

    unsafe { DEVICE.last_success = true; }
    true
}

fn emmc_command(command: u32, arg: u32, timeout: u32) -> bool {
    if (command & 0x8000_0000) != 0 {
        crate::println!("EMMC_ERR: COMMAND ERROR NOT APP");
        return false;
    }

    let cmd = COMMANDS[command as usize];
    unsafe { DEVICE.last_command = cmd; }

    if cmd.pack() == INVALID_CMD.pack() {
        crate::println!("EMMC_ERR: INVALID COMMAND!");
        return false;
    }

    emmc_issue_command(cmd, arg, timeout)
}

fn reset_command() -> bool {
    let regs = emmc();
    regs.control[1].write(regs.control[1].read() | EMMC_CTRL1_RESET_CMD);

    for _ in 0..10_000 {
        if (regs.control[1].read() & EMMC_CTRL1_RESET_CMD) == 0 {
            return true;
        }
        Timer::delay_ms(1);
    }

    crate::println!("EMMC_ERR: Command line failed to reset properly: {:x}", regs.control[1].read());
    false
}

fn emmc_app_command(command: u32, arg: u32, timeout: u32) -> bool {
    if COMMANDS[command as usize].index >= 60 {
        crate::println!("EMMC_ERR: INVALID APP COMMAND");
        return false;
    }

    unsafe { DEVICE.last_command = COMMANDS[CmdType::CTApp as usize]; }

    let mut rca = 0u32;
    unsafe {
        if DEVICE.rca != 0 {
            rca = DEVICE.rca << 16;
        }
    }

    if emmc_issue_command(unsafe { DEVICE.last_command }, rca, 2000) {
        unsafe { DEVICE.last_command = COMMANDS[command as usize]; }
        return emmc_issue_command(unsafe { DEVICE.last_command }, arg, 2000);
    }

    false
}

fn check_v2_card() -> bool {
    let mut v2Card = false;

    if !emmc_command(CmdType::CTSendIfCond as u32, 0x1AA, 200) {
        unsafe {
            if DEVICE.last_error == 0 {
                crate::println!("EMMC_ERR: SEND_IF_COND Timeout");
            } else if (DEVICE.last_error & (1 << 16)) != 0 {
                if !reset_command() {
                    return false;
                }
                emmc().int_flags.write(sd_error_mask(SdError::SDECommandTimeout));
                crate::println!("EMMC_ERR: SEND_IF_COND CMD TIMEOUT");
            } else {
                crate::println!("EMMC_ERR: Failure sending SEND_IF_COND");
                return false;
            }
        }
    } else {
        unsafe {
            if (DEVICE.last_response[0] & 0xFFF) != 0x1AA {
                crate::println!("EMMC_ERR: Unusable SD Card: {:x}", DEVICE.last_response[0]);
                return false;
            }
        }
        v2Card = true;
    }

    v2Card
}

fn check_usable_card() -> bool {
    if !emmc_command(CmdType::CTIOSetOpCond as u32, 0, 1000) {
        unsafe {
            if DEVICE.last_error == 0 {
                crate::println!("EMMC_ERR: CTIOSetOpCond Timeout");
            } else if (DEVICE.last_error & (1 << 16)) != 0 {
                if !reset_command() {
                    return false;
                }
                emmc().int_flags.write(sd_error_mask(SdError::SDECommandTimeout));
            } else {
                crate::println!("EMMC_ERR: SDIO Card not supported");
                return false;
            }
        }
    }
    true
}

fn check_sdhc_support(v2_card: bool) -> bool {
    let mut card_busy = true;

    while card_busy {
        let mut v2_flags = 0u32;
        if v2_card {
            v2_flags |= 1 << 30;
        }

        if !emmc_app_command(CmdType::CTOcrCheck as u32, 0x00FF8000 | v2_flags, 2000) {
            crate::println!("EMMC_ERR: APP CMD 41 FAILED 2nd");
            return false;
        }

        unsafe {
            if (DEVICE.last_response[0] >> 31) & 1 != 0 {
                DEVICE.ocr = ((DEVICE.last_response[0] >> 8) & 0xFFFF) as u16;
                DEVICE.sdhc = ((DEVICE.last_response[0] >> 30) & 1) != 0;
                card_busy = false;
            } else {
                if EMMC_DEBUG {
                    crate::println!("EMMC_DEBUG: SLEEPING: {:x}", DEVICE.last_response[0]);
                }
                Timer::delay_ms(500);
            }
        }
    }

    true
}

fn check_ocr() -> bool {
    let mut passed = false;

    for i in 0..5 {
        if !emmc_app_command(CmdType::CTOcrCheck as u32, 0, 2000) {
            crate::println!("EMMC_WARN: APP CMD OCR CHECK TRY {} FAILED", i + 1);
            passed = false;
        } else {
            passed = true;
        }

        if passed {
            break;
        }

        return false;
    }

    if !passed {
        crate::println!("EMMC_ERR: APP CMD 41 FAILED");
        return false;
    }

    unsafe {
        DEVICE.ocr = ((DEVICE.last_response[0] >> 8) & 0xFFFF) as u16;
        if EMMC_DEBUG {
            let ocr = DEVICE.ocr;
            crate::println!("MEMORY OCR: {:x}", ocr);
        }
    }

    true
}

fn check_rca() -> bool {
    if !emmc_command(CmdType::CTSendCide as u32, 0, 2000) {
        crate::println!("EMMC_ERR: Failed to send CID");
        return false;
    }

    if EMMC_DEBUG {
        unsafe {
            crate::println!("EMMC_DEBUG: CARD ID: {:x}.{:x}.{:x}.{:x}", DEVICE.last_response[0], DEVICE.last_response[1], DEVICE.last_response[2], DEVICE.last_response[3]);
        }
    }

    if !emmc_command(CmdType::CTSendRelativeAddr as u32, 0, 2000) {
        crate::println!("EMMC_ERR: Failed to send Relative Addr");
        return false;
    }

    unsafe {
        DEVICE.rca = (DEVICE.last_response[0] >> 16) & 0xFFFF;

        if EMMC_DEBUG {
            let rca = DEVICE.rca;
            crate::println!("EMMC_DEBUG: RCA: {:x}", rca);
            crate::println!("EMMC_DEBUG: CRC_ERR: {}", (DEVICE.last_response[0] >> 15) & 1);
            crate::println!("EMMC_DEBUG: CMD_ERR: {}", (DEVICE.last_response[0] >> 14) & 1);
            crate::println!("EMMC_DEBUG: GEN_ERR: {}", (DEVICE.last_response[0] >> 13) & 1);
            crate::println!("EMMC_DEBUG: STS_ERR: {}", (DEVICE.last_response[0] >> 9) & 1);
            crate::println!("EMMC_DEBUG: READY  : {}", (DEVICE.last_response[0] >> 8) & 1);
        }

        if ((DEVICE.last_response[0] >> 8) & 1) == 0 {
            crate::println!("EMMC_ERR: Failed to read RCA");
            return false;
        }
    }

    true
}

fn select_card() -> bool {
    unsafe {
        if !emmc_command(CmdType::CTSelectCard as u32, DEVICE.rca << 16, 2000) {
            crate::println!("EMMC_ERR: Failed to select card");
            return false;
        }

        if EMMC_DEBUG {
            crate::println!("EMMC_DEBUG: Selected Card");
        }

        let status = (DEVICE.last_response[0] >> 9) & 0xF;
        if status != 3 && status != 4 {
            crate::println!("EMMC_ERR: Invalid Status: {}", status);
            return false;
        }

        if EMMC_DEBUG {
            crate::println!("EMMC_DEBUG: Status: {}", status);
        }
    }
    true
}

fn set_scr() -> bool {
    let regs = emmc();
    unsafe {
        if !DEVICE.sdhc {
            if !emmc_command(CmdType::CTSetBlockLen as u32, 512, 2000) {
                crate::println!("EMMC_ERR: Failed to set block len");
                return false;
            }
        }

        let mut bsc = regs.block_size_count.read();
        bsc &= !0xFFF;
        bsc |= 0x200;
        regs.block_size_count.write(bsc);

        DEVICE.buffer = core::ptr::addr_of_mut!(DEVICE.scr.scr[0]) as *mut u8;
        DEVICE.block_size = 8;
        DEVICE.transfer_blocks = 1;

        if !emmc_app_command(CmdType::CTSendSCR as u32, 0, 30000) {
            crate::println!("EMMC_ERR: Failed to send SCR");
            return false;
        }

        if EMMC_DEBUG {
            let scr0_raw = DEVICE.scr.scr[0];
            let scr1_raw = DEVICE.scr.scr[1];
            let bus_widths = DEVICE.scr.bus_widths;
            crate::println!("EMMC_DEBUG: GOT SRC: SCR0: {:x} SCR1: {:x} BWID: {:x}", scr0_raw, scr1_raw, bus_widths);
        }

        DEVICE.block_size = 512;

        let scr0 = bswap32(DEVICE.scr.scr[0]);
        DEVICE.scr.version = 0xFFFF_FFFF;
        let spec = (scr0 >> (56 - 32)) & 0xf;
        let spec3 = (scr0 >> (47 - 32)) & 0x1;
        let spec4 = (scr0 >> (42 - 32)) & 0x1;

        if spec == 0 {
            DEVICE.scr.version = 1;
        } else if spec == 1 {
            DEVICE.scr.version = 11;
        } else if spec == 2 {
            if spec3 == 0 {
                DEVICE.scr.version = 2;
            } else if spec3 == 1 {
                if spec4 == 0 {
                    DEVICE.scr.version = 3;
                }
                if spec4 == 1 {
                    DEVICE.scr.version = 4;
                }
            }
        }

        if EMMC_DEBUG {
            let version = DEVICE.scr.version;
            crate::println!("EMMC_DEBUG: SCR Version: {}", version);
        }
    }

    true
}

fn emmc_card_reset() -> bool {
    let regs = emmc();
    regs.control[1].write(EMMC_CTRL1_RESET_HOST);

    if EMMC_DEBUG {
        crate::println!("EMMC_DEBUG: Card resetting...");
    }

    if !wait_reg_mask(&regs.control[1], EMMC_CTRL1_RESET_ALL, false, 2000) {
        crate::println!("EMMC_ERR: Card reset timeout!");
        return false;
    }

    if !emmc_setup_clock() {
        return false;
    }

    regs.int_enable.write(0);
    regs.int_flags.write(0xFFFF_FFFF);
    regs.int_mask.write(0xFFFF_FFFF);

    Timer::delay_ms(203);

    unsafe {
        DEVICE.transfer_blocks = 0;
        DEVICE.last_command_value = 0;
        DEVICE.last_success = false;
        DEVICE.block_size = 0;
    }

    if !emmc_command(CmdType::CTGoIdle as u32, 0, 2000) {
        crate::println!("EMMC_ERR: NO GO_IDLE RESPONSE");
        return false;
    }

    let v2_card = check_v2_card();

    if !check_usable_card() {
        return false;
    }

    if !check_ocr() {
        return false;
    }

    if !check_sdhc_support(v2_card) {
        return false;
    }

    Timer::delay_ms(10);

    if !check_rca() {
        return false;
    }

    if !select_card() {
        return false;
    }

    if !set_scr() {
        return false;
    }

    regs.int_flags.write(0xFFFF_FFFF);

    if EMMC_DEBUG {
        crate::println!("EMMC_DEBUG: Card reset!");
    }

    true
}

fn do_data_command(write: bool, b: *mut u8, bsize: u32, mut block_no: u32) -> bool {
    unsafe {
        if !DEVICE.sdhc {
            block_no *= 512;
        }

        if bsize < DEVICE.block_size {
            crate::println!("EMMC_ERR: INVALID BLOCK SIZE: {}", bsize);
            return false;
        }

        assert!(DEVICE.block_size == 512);
        DEVICE.transfer_blocks = bsize / 512;

        if (bsize & 0x1ff) != 0 {
            crate::println!("EMMC_ERR: BAD BLOCK SIZE");
            return false;
        }

        DEVICE.buffer = b;
    }

    let mut command = CmdType::CTReadBlock as u32;
    unsafe {
        if write && DEVICE.transfer_blocks > 1 {
            command = CmdType::CTWriteMultiple as u32;
        } else if write {
            command = CmdType::CTWriteBlock as u32;
        } else if !write && DEVICE.transfer_blocks > 1 {
            command = CmdType::CTReadMultiple as u32;
        }
    }

    let mut retry_count = 0;
    let max_retries = 3;

    if EMMC_DEBUG {
        crate::println!("EMMC_DEBUG: Sending command: {}", command);
    }

    while retry_count < max_retries {
        if emmc_command(command, block_no, 5000) {
            break;
        }
        retry_count += 1;
        if retry_count < max_retries {
            crate::println!("EMMC_WARN: Retrying data command");
        } else {
            crate::println!("EMMC_ERR: Giving up data command");
            return false;
        }
    }

    true
}

pub fn emmc_read(sector: u32, buffer: *mut u8, size: u32) -> i32 {
    assert!(size % 512 == 0);
    let success = do_data_command(false, buffer, size, sector);
    if !success {
        crate::println!("EMMC_ERR: READ FAILED: sector={}, size={}", sector, size);
        return -1;
    }
    size as i32
}

pub fn emmc_write(sector: u32, buffer: *mut u8, size: u32) -> i32 {
    assert!(size % 512 == 0);
    let r = do_data_command(true, buffer, size, sector);
    if !r {
        crate::println!("EMMC_ERR: WRITE FAILED");
        return -1;
    }
    size as i32
}

pub fn emmc_init() -> bool {
    gpio::set_function(34, GPIO_FUNC_INPUT);
    gpio::set_function(35, GPIO_FUNC_INPUT);
    gpio::set_function(36, GPIO_FUNC_INPUT);
    gpio::set_function(37, GPIO_FUNC_INPUT);
    gpio::set_function(38, GPIO_FUNC_INPUT);
    gpio::set_function(39, GPIO_FUNC_INPUT);

    gpio::set_function(48, GPIO_FUNC_ALT3);
    gpio::set_function(49, GPIO_FUNC_ALT3);
    gpio::set_function(50, GPIO_FUNC_ALT3);
    gpio::set_function(51, GPIO_FUNC_ALT3);
    gpio::set_function(52, GPIO_FUNC_ALT3);

    unsafe {
        DEVICE.transfer_blocks = 0;
        DEVICE.last_command_value = 0;
        DEVICE.last_success = false;
        DEVICE.block_size = 0;
        DEVICE.sdhc = false;
        DEVICE.ocr = 0;
        DEVICE.rca = 0;
        DEVICE.base_clock = 0;
    }

    let mut success = false;
    for _ in 0..10 {
        success = emmc_card_reset();
        if success {
            break;
        }
        Timer::delay_ms(100);
        crate::println!("EMMC_WARN: Failed to reset card, trying again...");
    }

    success
}

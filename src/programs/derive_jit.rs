#![no_std]
#![no_main]

use crate::print;
use crate::println;

macro_rules! define_mov_deriver {
    ($fn_name: ident, $rd0: literal, $rs0: literal, $rd1: literal, $rs1: literal) => {
        pub fn $fn_name() {
            unsafe {
                core::arch::asm!(
                    ".word 0x12345678",
                    concat!("mov ", $rd0, ", ", $rs0),
                    concat!("mov ", $rd1, ", ", $rs1),
                    ".word 0x12345678"
                )
            }
        }
    }
}


define_mov_deriver!(derive_mov_src_cheat, "r0", "r0", "r0", "r15");
define_mov_deriver!(derive_mov_dst_cheat, "r0", "r0", "r15", "r0");

static inline uint32_t 
armv6_mov_imm8_rot4(reg_t rd, uint32_t imm8, unsigned rot4) {
    if(imm8>>8)
        panic("immediate %d does not fit in 8 bits!\n", imm8);
    if(rot4 % 2)
        panic("rotation %d must be divisible by 2!\n", rot4);
    rot4 /= 2;
    if(rot4>>4)
        panic("rotation %d does not fit in 4 bits!\n", rot4);

    return cond_always << 28
        | 0b1 << 25
        | op_mov << 21
        | rd.reg << 12
        | rot4 << 8
        | imm8;
}

static inline uint32_t 
armv6_bx(reg_t rd) {
    // todo("implement bx\n");
    return 0xe12fff10 | rd.reg;
}

static inline uint32_t 
armv6_orr_imm8_rot4(reg_t rd, reg_t rn, unsigned imm8, unsigned rot4) {
    if(imm8>>8)
        panic("immediate %d does not fit in 8 bits!\n", imm8);
    if(rot4 % 2)
        panic("rotation %d must be divisible by 2!\n", rot4);
    rot4 /= 2;
    if(rot4>>4)
        panic("rotation %d does not fit in 4 bits!\n", rot4);

    return cond_always << 28
        | 0b1 << 25
        | armv6_orr << 21
        | rn.reg << 16
        | rd.reg << 12
        | rot4 << 8
        | imm8;
}

static inline uint32_t 
armv6_mult(reg_t rd, reg_t rm, reg_t rs) {
    return cond_always << 28
        | rd.reg << 16
        | rs.reg << 8
        | 0b1001 << 4
        | rm.reg;
}

static inline uint32_t 
armv6_ldr_off12(reg_t rd, reg_t rn, int offset) {
    unsigned U = 1;
    if(offset < 0) {
        U = 0;
        offset = -offset;
    }

    return cond_always << 28
        | 0b01 << 26
        | 0b1 << 24       
        | U << 23
        | 0b1 << 20 // this is a load     
        | rn.reg << 16
        | rd.reg << 12
        | offset;
}

static inline uint32_t
armv6_mla(reg_t rd, reg_t rm, reg_t rs, reg_t rn) {    
    // todo("implement multiply accumulate\n");
    return cond_always << 28
        | 0b1 << 21
        | rd.reg << 16
        | rn.reg << 12
        | rs.reg << 8
        | 0b1001 << 4
        | rm.reg;
}


pub fn derive_add_dst_cheat() {
    unsafe {
        core::arch::asm!(
            ".word 0x12345678",
            "add r0, r0, r0",
            "add r15, r0, r0",
            ".word 0x12345678"
        )
    }
}

pub fn derive_add_src1_cheat() {
    unsafe {
        core::arch::asm!(
            ".word 0x12345678",
            "add r0, r0, r0",
            "add r0, r15, r0",
            ".word 0x12345678"
        )
    }
}

pub fn derive_add_src2_cheat() {
    unsafe {
        core::arch::asm!(
            ".word 0x12345678",
            "add r0, r0, r0",
            "add r0, r0, r15",
            ".word 0x12345678"
        )
    }
}

pub fn get_changed(func: *const ()) -> u32 {
    unsafe {
        let inst_ptr = func as *const u32;
        let mut i: usize = 0;

        let mut always_one = !0;
        let mut always_zero = !0;
        let mut saw_sentinel: bool = false;

        while (true) {
            let inst = *inst_ptr.add(i);
            // println!("get_changed is processing instruction 0x{:08x}", inst);
        
            if (inst == 0x12345678) {
                // sentinel found here
                if(saw_sentinel) {
                    break;
                }
                saw_sentinel = true;
            } else {
                if(saw_sentinel) {
                    always_zero &= !inst;
                    always_one &= inst;
                }
            }
            i += 1;
        }

        let changed: u32 = !(always_zero | always_one);
        // println!("Always zero: 0b{:032b}", always_zero);
        // println!("Always one: 0b{:032b}", always_one);
        // println!("Changed bits: 0b{:032b}", changed);
        let unchanged: u32 = (always_zero | always_one);
        
        changed
    }
}

pub fn derive_add() {
    let src1: u32 = get_changed(derive_add_src1_cheat as (*const ()));
    let src2: u32 = get_changed(derive_add_src2_cheat as (*const ()));
    let dst: u32 = get_changed(derive_add_dst_cheat as (*const ()));

    let opcode = !(src2|src1|dst);

    println!(
        "add opcode=0x{:08x}, src1=0b{:032b}, src2=0b{:032b}, dst=0b{:032b}",
        opcode, src1, src2, dst
    );
}

pub fn derive_mov() {
    let dst: u32 = get_changed(derive_mov_dst_cheat as (*const ()));
    let src: u32 = get_changed(derive_mov_src_cheat as (*const ()));
    let opcode = !(src | dst);
    println!(
        "mov opcode=0x{:08x}, src=0b{:032b}, dst=0b{:032b}",
        opcode, src, dst
    );
}

pub fn derive_main(){
    derive_mov();
    derive_add();
}
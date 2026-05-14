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

macro_rules! define_pair_deriver {
    ($fn_name: ident, $inst1: literal, $inst2: literal) => {
        pub fn $fn_name() {
            unsafe {
                core::arch::asm!(
                    ".word 0x12345678",
                    $inst1,
                    $inst2,
                    ".word 0x12345678"
                )
            }
        }
    }
}

define_mov_deriver!(derive_mov_src_cheat, "r0", "r0", "r0", "r15");
define_mov_deriver!(derive_mov_dst_cheat, "r0", "r0", "r15", "r0");
define_pair_deriver!(derive_bx_rd_cheat, "bx r0", "bx r15");
define_pair_deriver!(derive_mult_rd_cheat, "mul r0, r0, r0", "mul r14, r0, r0");
define_pair_deriver!(derive_mult_rm_cheat, "mul r0, r0, r0", "mul r0, r14, r0");
define_pair_deriver!(derive_mult_rs_cheat, "mul r0, r0, r0", "mul r0, r0, r14");
define_pair_deriver!(derive_mla_rd_cheat, "mla r0, r0, r0, r0", "mla r14, r0, r0, r0");
define_pair_deriver!(derive_mla_rm_cheat, "mla r0, r0, r0, r0", "mla r0, r14, r0, r0");
define_pair_deriver!(derive_mla_rs_cheat, "mla r0, r0, r0, r0", "mla r0, r0, r14, r0");
define_pair_deriver!(derive_mla_rn_cheat, "mla r0, r0, r0, r0", "mla r0, r0, r0, r14");

define_pair_deriver!(derive_orr_imm8_rot4_rd_cheat, "orr r0, r0, #0", "orr r15, r0, #0");
define_pair_deriver!(derive_orr_imm8_rot4_rn_cheat, "orr r0, r0, #0", "orr r0, r15, #0");
define_pair_deriver!(derive_orr_imm8_rot4_imm8_cheat, "orr r0, r0, #0", "orr r0, r0, #255");

define_pair_deriver!(derive_mov_imm8_rot4_rd_cheat, "mov r0, #0", "mov r15, #0");
define_pair_deriver!(derive_mov_imm8_rot4_imm8_cheat, "mov r0, #0", "mov r0, #255");

define_pair_deriver!(derive_mvn_imm8_rd_cheat, "mvn r0, #0", "mvn r15, #0");
define_pair_deriver!(derive_mvn_imm8_imm8_cheat, "mvn r0, #0", "mvn r0, #255");

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

pub fn get_changed(func: *const ()) -> (u32, u32) {
    unsafe {
        let inst_ptr = func as *const u32;
        let mut i: usize = 0;

        let mut always_one = !0;
        let mut always_zero = !0;
        let mut saw_sentinel: bool = false;

        let mut inst = *inst_ptr.add(i);
        loop {
            // println!("get_changed is processing instruction 0x{:08x}", inst);
        
            if inst == 0x12345678 {
                // sentinel found here
                if saw_sentinel {
                    break;
                }
                saw_sentinel = true;
            } else {
                if saw_sentinel {
                    always_zero &= !inst;
                    always_one &= inst;
                }
            }
            i += 1;
            inst = *inst_ptr.add(i);
        }

        let changed: u32 = !(always_zero | always_one);
        // println!("Always zero: 0b{:032b}", always_zero);
        // println!("Always one: 0b{:032b}", always_one);
        // println!("Changed bits: 0b{:032b}", changed);
        let unchanged: u32 = always_zero | always_one;
        
        (changed, inst & unchanged)
    }
}

pub fn derive_add() {
    let (src1, src1_unchanged) = get_changed(derive_add_src1_cheat as *const ());
    let (src2, src2_unchanged) = get_changed(derive_add_src2_cheat as *const ());
    let (dst, dst_unchanged) = get_changed(derive_add_dst_cheat as *const ());

    let opcode = (!(src1 | src2 | dst)) & src1_unchanged & src2_unchanged & dst_unchanged;

    println!(
        "add opcode=0x{:08x}, src1=0b{:032b}, src2=0b{:032b}, dst=0b{:032b}",
        opcode, src1, src2, dst
    );

    println!(
        "uint32_t gen_add(uint32_t dst, uint32_t src1, uint32_t src2) {{\n    return 0x{:08x} | (dst & 0x{:08x}) | (src1 & 0x{:08x}) | (src2 & 0x{:08x});\n}}",
        opcode, dst, src1, src2
    );
}

pub fn derive_mov() {
    let (dst, dst_unchanged) = get_changed(derive_mov_dst_cheat as *const ());
    let (src, src_unchanged) = get_changed(derive_mov_src_cheat as *const ());
    let opcode = (!(src | dst)) & src_unchanged & dst_unchanged;
    println!(
        "mov opcode=0x{:08x}, src=0b{:032b}, dst=0b{:032b}",
        opcode, src, dst
    );

    println!(
        "uint32_t gen_mov(uint32_t dst, uint32_t src) {{\n    return 0x{:08x} | (dst & 0x{:08x}) | (src & 0x{:08x});\n}}",
        opcode, dst, src
    );
}

pub fn derive_bx() {
    let (rd, rd_unchanged) = get_changed(derive_bx_rd_cheat as *const ());
    let opcode = (!rd) & rd_unchanged;
    println!("bx opcode=0x{:08x}, rd=0b{:032b}", opcode, rd);

    println!(
        "uint32_t gen_bx(uint32_t rd) {{\n    return 0x{:08x} | (rd & 0x{:08x});\n}}",
        opcode, rd
    );
}

pub fn derive_mult() {
    let (rd, rd_unchanged) = get_changed(derive_mult_rd_cheat as *const ());
    let (rm, rm_unchanged) = get_changed(derive_mult_rm_cheat as *const ());
    let (rs, rs_unchanged) = get_changed(derive_mult_rs_cheat as *const ());
    let opcode = (!(rd | rm | rs)) & rd_unchanged & rm_unchanged & rs_unchanged;
    println!(
        "mult opcode=0x{:08x}, rd=0b{:032b}, rm=0b{:032b}, rs=0b{:032b}",
        opcode, rd, rm, rs
    );

    println!(
        "uint32_t gen_mult(uint32_t rd, uint32_t rm, uint32_t rs) {{\n    return 0x{:08x} | (rd & 0x{:08x}) | (rm & 0x{:08x}) | (rs & 0x{:08x});\n}}",
        opcode, rd, rm, rs
    );
}

pub fn derive_mla() {
    let (rd, rd_unchanged) = get_changed(derive_mla_rd_cheat as *const ());
    let (rm, rm_unchanged) = get_changed(derive_mla_rm_cheat as *const ());
    let (rs, rs_unchanged) = get_changed(derive_mla_rs_cheat as *const ());
    let (rn, rn_unchanged) = get_changed(derive_mla_rn_cheat as *const ());
    let opcode = (!(rd | rm | rs | rn)) & rd_unchanged & rm_unchanged & rs_unchanged & rn_unchanged;

    println!(
        "mla opcode=0x{:08x}, rd=0b{:032b}, rm=0b{:032b}, rs=0b{:032b}, rn=0b{:032b}",
        opcode, rd, rm, rs, rn
    );

    println!(
        "uint32_t gen_mla(uint32_t rd, uint32_t rm, uint32_t rs, uint32_t rn) {{\n    return 0x{:08x} | (rd & 0x{:08x}) | (rm & 0x{:08x}) | (rs & 0x{:08x}) | (rn & 0x{:08x});\n}}",
        opcode, rd, rm, rs, rn
    );
}

pub fn derive_orr() {
    let (rd, rd_unchanged) = get_changed(derive_orr_imm8_rot4_rd_cheat as *const ());
    let (rn, rn_unchanged) = get_changed(derive_orr_imm8_rot4_rn_cheat as *const ());
    let (imm8, imm8_unchanged) = get_changed(derive_orr_imm8_rot4_imm8_cheat as *const ());
    let (rot, rot_unchanged) = (0b1111 << 8, 0);

    let opcode = (!(rd | rn | imm8 | rot)) & (rd_unchanged & rn_unchanged & imm8_unchanged & rot_unchanged);
    println!(
        "orr opcode=0x{:08x}, rd=0b{:032b}, rm=0b{:032b}, imm8=0b{:032b}, rot=0b{:032b}",
        opcode, rd, rn, imm8, rot
    );

    println!(
        "uint32_t gen_orr_imm8_rot4(uint32_t rd, uint32_t rn, uint32_t imm8, uint32_t rot) {{\n    return 0x{:08x} | (rd & 0x{:08x}) | (rn & 0x{:08x}) | (imm8 & 0x{:08x}) | (rot & 0x{:08x});\n}}",
        opcode, rd, rn, imm8, rot
    );
}

pub fn derive_mov_imm8_rot4() {
    let (rd, rd_unchanged) = get_changed(derive_mov_imm8_rot4_rd_cheat as *const ());
    let (imm8, imm8_unchanged) = get_changed(derive_mov_imm8_rot4_imm8_cheat as *const ());
    let (rot, rot_unchanged) = (0b1111 << 8, 0);

    let opcode = (!(rd | imm8 | rot)) & (rd_unchanged & imm8_unchanged & rot_unchanged);
    println!(
        "mov_imm8_rot4 opcode=0x{:08x}, rd=0b{:032b}, imm8=0b{:032b}, rot=0b{:032b}",
        opcode, rd, imm8, rot
    );

    println!(
        "uint32_t gen_mov_imm8_rot4(uint32_t rd, uint32_t imm8, uint32_t rot) {{\n    return 0x{:08x} | (rd & 0x{:08x}) | (imm8 & 0x{:08x}) | (rot & 0x{:08x});\n}}",
        opcode, rd, imm8, rot
    );
}

pub fn derive_mvn_imm8() {
    let (rd, rd_unchanged) = get_changed(derive_mvn_imm8_rd_cheat as *const ());
    let (imm8, imm8_unchanged) = get_changed(derive_mvn_imm8_imm8_cheat as *const ());
    let opcode = (!(rd | imm8)) & (rd_unchanged & imm8_unchanged);

    println!(
        "mvn_imm8 opcode=0x{:08x}, rd=0b{:032b}, imm8=0b{:032b}",
        opcode, rd, imm8
    );

    println!(
        "uint32_t gen_mvn_imm8(uint32_t rd, uint32_t imm8) {{\n    return 0x{:08x} | (rd & 0x{:08x}) | (imm8 & 0x{:08x});\n}}",
        opcode, rd, imm8
    );
}

pub fn derive_main(){
    derive_mov();
    derive_add();
    derive_bx();
    derive_mult();
    derive_mla();
    derive_orr();
    derive_mov_imm8_rot4();
    derive_mvn_imm8();
}
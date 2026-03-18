// Load the model weights in order, just like they were done in matrix_load_test.rs
use crate::gpu::{GpuKernel};
use crate::fat32::{self};
use crate::println;
use crate::timer::Timer;

pub fn load_model() {
    println!("Reading the MBR.");
    let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");

    println!("Loading the FAT.");
    let fs = fat32::fat32_mk(&partition);

    println!("Loading the root directory.");
    let root = fat32::fat32_get_root(&fs);

    let start_load = Timer::get_usec();
    let tok_emb = fat32::load_matrix_from_file(&fs, &root, "TOK_EMB.BIN", 92 * 128);
    let pos_emb = fat32::load_matrix_from_file(&fs, &root, "POS_EMB.BIN", 128 * 128);

    let l00_ln1_w = fat32::load_matrix_from_file(&fs, &root, "L0LN1_W.BIN", 128);
    let l00_ln1_b = fat32::load_matrix_from_file(&fs, &root, "L0LN1_B.BIN", 128);
    let l00_attn_q_w = fat32::load_matrix_from_file(&fs, &root, "L0A_QW.BIN", 128 * 128);
    let l00_attn_k_w = fat32::load_matrix_from_file(&fs, &root, "L0A_KW.BIN", 128 * 128);
    let l00_attn_v_w = fat32::load_matrix_from_file(&fs, &root, "L0A_VW.BIN", 128 * 128);
    let l00_attn_o_w = fat32::load_matrix_from_file(&fs, &root, "L0A_OW.BIN", 128 * 128);
    let l00_ln2_w = fat32::load_matrix_from_file(&fs, &root, "L0LN2_W.BIN", 128);
    let l00_ln2_b = fat32::load_matrix_from_file(&fs, &root, "L0LN2_B.BIN", 128);
    let l00_mlp_fc_w = fat32::load_matrix_from_file(&fs, &root, "L0M_FC_W.BIN", 128 * 512);
    let l00_mlp_proj_w = fat32::load_matrix_from_file(&fs, &root, "L0M_P_W.BIN", 512 * 128);

    let l01_ln1_w = fat32::load_matrix_from_file(&fs, &root, "L1LN1_W.BIN", 128);
    let l01_ln1_b = fat32::load_matrix_from_file(&fs, &root, "L1LN1_B.BIN", 128);
    let l01_attn_q_w = fat32::load_matrix_from_file(&fs, &root, "L1A_QW.BIN", 128 * 128);
    let l01_attn_k_w = fat32::load_matrix_from_file(&fs, &root, "L1A_KW.BIN", 128 * 128);
    let l01_attn_v_w = fat32::load_matrix_from_file(&fs, &root, "L1A_VW.BIN", 128 * 128);
    let l01_attn_o_w = fat32::load_matrix_from_file(&fs, &root, "L1A_OW.BIN", 128 * 128);
    let l01_ln2_w = fat32::load_matrix_from_file(&fs, &root, "L1LN2_W.BIN", 128);
    let l01_ln2_b = fat32::load_matrix_from_file(&fs, &root, "L1LN2_B.BIN", 128);
    let l01_mlp_fc_w = fat32::load_matrix_from_file(&fs, &root, "L1M_FC_W.BIN", 128 * 512);
    let l01_mlp_proj_w = fat32::load_matrix_from_file(&fs, &root, "L1M_P_W.BIN", 512 * 128);

    let ln_f_w = fat32::load_matrix_from_file(&fs, &root, "LN_F_W.BIN", 128);
    let ln_f_b = fat32::load_matrix_from_file(&fs, &root, "LN_F_B.BIN", 128);
    let lm_head_w = fat32::load_matrix_from_file(&fs, &root, "LM_HD_W.BIN", 128 * 92);
    let end_load = Timer::get_usec();

    

    println!("Finished loading the matrix in {} usec.", end_load - start_load);
}
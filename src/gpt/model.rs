use crate::fat32::{self};
use crate::gpt::attention::causal_attention_step;
use crate::gpt::embedding::{add_in_place, Embedding};
use crate::gpt::gelu;
use crate::gpt::layernorm::LayerNorm;
use crate::gpt::linear::Linear;
use crate::gpu::{GpuKernel, DMA_TEST_CODE};
use crate::print;
use crate::println;
use crate::timer::Timer;

const VOCAB_SIZE: usize = 92;
const BLOCK_SIZE: usize = 384;
const N_HEAD: usize = 4;
const N_EMBD: usize = 96;
const MAX_GEN_TOKENS: usize = 128;

const SLOT_LAYER0_K_CACHE: usize = 3;
const SLOT_LAYER0_V_CACHE: usize = 4;
const SLOT_LAYER1_K_CACHE: usize = 5;
const SLOT_LAYER1_V_CACHE: usize = 6;

const SLOT_S0_LN1: usize = 7;
const SLOT_S0_Q: usize = 8;
const SLOT_S0_K: usize = 9;
const SLOT_S0_V: usize = 10;
const SLOT_S0_CTX: usize = 11;
const SLOT_S0_ATTN: usize = 12;
const SLOT_S0_X_ATTN: usize = 13;
const SLOT_S0_LN2: usize = 14;
const SLOT_S0_MLP_H: usize = 15;
const SLOT_S0_MLP_O: usize = 16;

const SLOT_S1_LN1: usize = 17;
const SLOT_S1_Q: usize = 18;
const SLOT_S1_K: usize = 19;
const SLOT_S1_V: usize = 20;
const SLOT_S1_CTX: usize = 21;
const SLOT_S1_ATTN: usize = 22;
const SLOT_S1_X_ATTN: usize = 23;
const SLOT_S1_LN2: usize = 24;
const SLOT_S1_MLP_H: usize = 25;
const SLOT_S1_MLP_O: usize = 26;

const SLOT_H0_IN: usize = 27;
const SLOT_H0_OUT: usize = 28;
const SLOT_H1_OUT: usize = 29;
const SLOT_LOGITS: usize = 30;
const SLOT_LN_OUT: usize = 31;
const SLOT_POS_TMP: usize = 32;


const TRAIN_CHAR_VOCAB: [char; VOCAB_SIZE] = [
    '\n', ' ', '!', '"', '$', '&', '\'', '*', ',', '-', '.', '0', '1', '2', '3', '4', '5', '6',
    '7', '8', '9', ':', ';', '?', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f',
    'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y',
    'z', '¡', '¦', '©', '«', '±', '³', '»', 'Â', 'Ã', 'â', 'œ', '˜', '“', '”', '€', '™',
];

unsafe fn gpu_slot_mut<'a>(gpu_ptr: *mut GpuKernel, slot: usize, len: usize) -> &'a mut [f32] {
    let full = unsafe { (&mut *gpu_ptr).data_slot_as_mut_f32(slot) };
    assert!(len <= full.len(), "gpu slot {} too small", slot);
    &mut full[..len]
}

unsafe fn gpu_slot<'a>(gpu_ptr: *mut GpuKernel, slot: usize, len: usize) -> &'a [f32] {
    let full = unsafe { (&*gpu_ptr).data_slot_as_f32(slot) };
    assert!(len <= full.len(), "gpu slot {} too small", slot);
    &full[..len]
}

fn vocab_find_id(vocab: &[char], ch: char) -> usize {
    for (i, &v) in vocab.iter().enumerate() {
        if v == ch {
            return i;
        }
    }
    for (i, &v) in vocab.iter().enumerate() {
        if v == ' ' {
            return i;
        }
    }
    0
}

fn tokenize(prompt: &str, vocab: &[char], out_ids: &mut [usize]) -> usize {
    let mut n = 0usize;
    for ch in prompt.chars() {
        if n >= out_ids.len() {
            break;
        }
        out_ids[n] = vocab_find_id(vocab, ch);
        n += 1;
    }
    n
}

fn detokenize(ids: &[usize], vocab: &[char], out: &mut [u8]) -> usize {
    let mut cursor = 0usize;
    for &id in ids {
        let ch = if id < vocab.len() { vocab[id] } else { '?' };
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        let bytes = encoded.as_bytes();
        if cursor + bytes.len() > out.len() {
            break;
        }
        out[cursor..cursor + bytes.len()].copy_from_slice(bytes);
        cursor += bytes.len();
    }
    cursor
}

unsafe fn block_step(
    gpu_ptr: *mut GpuKernel,
    x_in_slot: usize,
    x_out_slot: usize,
    cache_len: usize,
    cache_k_slot: usize,
    cache_v_slot: usize,
    ln1: &LayerNorm,
    q_proj: &Linear,
    k_proj: &Linear,
    v_proj: &Linear,
    o_proj: &Linear,
    ln2: &LayerNorm,
    fc: &Linear,
    proj: &Linear,
    scratch_ln1_slot: usize,
    scratch_q_slot: usize,
    scratch_k_slot: usize,
    scratch_v_slot: usize,
    scratch_ctx_slot: usize,
    scratch_attn_slot: usize,
    scratch_x_attn_slot: usize,
    scratch_ln2_slot: usize,
    scratch_mlp_h_slot: usize,
    scratch_mlp_o_slot: usize,
) {
    assert!(cache_len < BLOCK_SIZE);

    {
        let x_in = unsafe { gpu_slot(gpu_ptr, x_in_slot, N_EMBD) };
        let ln1_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_ln1_slot, N_EMBD) };
        ln1.forward(x_in, ln1_buf, N_EMBD);
    }

    {
        let ln1_buf = unsafe { gpu_slot(gpu_ptr, scratch_ln1_slot, N_EMBD) };
        let q_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_q_slot, N_EMBD) };
        q_proj.forward(unsafe { &mut *gpu_ptr }, ln1_buf, q_buf, 1);
    }
    {
        let ln1_buf = unsafe { gpu_slot(gpu_ptr, scratch_ln1_slot, N_EMBD) };
        let k_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_k_slot, N_EMBD) };
        k_proj.forward(unsafe { &mut *gpu_ptr }, ln1_buf, k_buf, 1);
    }
    {
        let ln1_buf = unsafe { gpu_slot(gpu_ptr, scratch_ln1_slot, N_EMBD) };
        let v_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_v_slot, N_EMBD) };
        v_proj.forward(unsafe { &mut *gpu_ptr }, ln1_buf, v_buf, 1);
    }

    {
        let k_buf = unsafe { gpu_slot(gpu_ptr, scratch_k_slot, N_EMBD) };
        let v_buf = unsafe { gpu_slot(gpu_ptr, scratch_v_slot, N_EMBD) };
        let cache_k = unsafe { gpu_slot_mut(gpu_ptr, cache_k_slot, BLOCK_SIZE * N_EMBD) };
        let cache_v = unsafe { gpu_slot_mut(gpu_ptr, cache_v_slot, BLOCK_SIZE * N_EMBD) };
        let dst_k = &mut cache_k[cache_len * N_EMBD..(cache_len + 1) * N_EMBD];
        let dst_v = &mut cache_v[cache_len * N_EMBD..(cache_len + 1) * N_EMBD];
        dst_k.copy_from_slice(k_buf);
        dst_v.copy_from_slice(v_buf);
    }

    {
        let q_buf = unsafe { gpu_slot(gpu_ptr, scratch_q_slot, N_EMBD) };
        let cache_k = unsafe { gpu_slot(gpu_ptr, cache_k_slot, BLOCK_SIZE * N_EMBD) };
        let cache_v = unsafe { gpu_slot(gpu_ptr, cache_v_slot, BLOCK_SIZE * N_EMBD) };
        let ctx_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_ctx_slot, N_EMBD) };
        causal_attention_step(q_buf, cache_k, cache_v, cache_len, N_HEAD, N_EMBD, ctx_buf);
    }

    {
        let ctx_buf = unsafe { gpu_slot(gpu_ptr, scratch_ctx_slot, N_EMBD) };
        let attn_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_attn_slot, N_EMBD) };
        o_proj.forward(unsafe { &mut *gpu_ptr }, ctx_buf, attn_buf, 1);
    }

    {
        let x_in = unsafe { gpu_slot(gpu_ptr, x_in_slot, N_EMBD) };
        let attn_buf = unsafe { gpu_slot(gpu_ptr, scratch_attn_slot, N_EMBD) };
        let x_attn_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_x_attn_slot, N_EMBD) };
        x_attn_buf.copy_from_slice(x_in);
        add_in_place(x_attn_buf, attn_buf);
    }

    {
        let x_attn_buf = unsafe { gpu_slot(gpu_ptr, scratch_x_attn_slot, N_EMBD) };
        let ln2_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_ln2_slot, N_EMBD) };
        ln2.forward(x_attn_buf, ln2_buf, N_EMBD);
    }

    {
        let ln2_buf = unsafe { gpu_slot(gpu_ptr, scratch_ln2_slot, N_EMBD) };
        let mlp_h_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_mlp_h_slot, 4 * N_EMBD) };
        fc.forward(unsafe { &mut *gpu_ptr }, ln2_buf, mlp_h_buf, 1);
    }

    {
        let mlp_h_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_mlp_h_slot, 4 * N_EMBD) };
        gelu::gelu_in_place(mlp_h_buf);
    }

    {
        let mlp_h_buf = unsafe { gpu_slot(gpu_ptr, scratch_mlp_h_slot, 4 * N_EMBD) };
        let mlp_o_buf = unsafe { gpu_slot_mut(gpu_ptr, scratch_mlp_o_slot, N_EMBD) };
        proj.forward(unsafe { &mut *gpu_ptr }, mlp_h_buf, mlp_o_buf, 1);
    }

    {
        let x_in = unsafe { gpu_slot(gpu_ptr, x_in_slot, N_EMBD) };
        let mlp_o_buf = unsafe { gpu_slot(gpu_ptr, scratch_mlp_o_slot, N_EMBD) };
        let x_out = unsafe { gpu_slot_mut(gpu_ptr, x_out_slot, N_EMBD) };
        for i in 0..N_EMBD {
            // Match Python block exactly.
            x_out[i] = x_in[i] + mlp_o_buf[i];
        }
    }
}

unsafe fn head_logits(
    gpu_ptr: *mut GpuKernel,
    x_slot: usize,
    ln_out_slot: usize,
    logits_slot: usize,
    ln_f: &LayerNorm,
    lm_head: &Linear,
) {
    let x = unsafe { gpu_slot(gpu_ptr, x_slot, N_EMBD) };
    let ln_out = unsafe { gpu_slot_mut(gpu_ptr, ln_out_slot, N_EMBD) };
    ln_f.forward(x, ln_out, N_EMBD);

    let ln_out = unsafe { gpu_slot(gpu_ptr, ln_out_slot, N_EMBD) };
    let logits = unsafe { gpu_slot_mut(gpu_ptr, logits_slot, VOCAB_SIZE) };
    lm_head.forward(unsafe { &mut *gpu_ptr }, ln_out, logits, 1);
}

pub fn infer_model() {
    println!("Reading the MBR.");
    let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");

    println!("Loading the FAT.");
    let fs = fat32::fat32_mk(&partition);

    println!("Loading the root directory.");
    let root = fat32::fat32_get_root(&fs);

    let start_load = Timer::get_usec();
    let tok_emb = fat32::load_matrix_from_file(&fs, &root, "TOK_EMB.BIN", VOCAB_SIZE * N_EMBD);
    let pos_emb = fat32::load_matrix_from_file(&fs, &root, "POS_EMB.BIN", BLOCK_SIZE * N_EMBD);

    let l0_ln1_w = fat32::load_matrix_from_file(&fs, &root, "L0LN1_W.BIN", N_EMBD);
    let l0_ln1_b = fat32::load_matrix_from_file(&fs, &root, "L0LN1_B.BIN", N_EMBD);
    let l0_q_w = fat32::load_matrix_from_file(&fs, &root, "L0A_QW.BIN", N_EMBD * N_EMBD);
    let l0_k_w = fat32::load_matrix_from_file(&fs, &root, "L0A_KW.BIN", N_EMBD * N_EMBD);
    let l0_v_w = fat32::load_matrix_from_file(&fs, &root, "L0A_VW.BIN", N_EMBD * N_EMBD);
    let l0_o_w = fat32::load_matrix_from_file(&fs, &root, "L0A_OW.BIN", N_EMBD * N_EMBD);
    let l0_ln2_w = fat32::load_matrix_from_file(&fs, &root, "L0LN2_W.BIN", N_EMBD);
    let l0_ln2_b = fat32::load_matrix_from_file(&fs, &root, "L0LN2_B.BIN", N_EMBD);
    let l0_fc_w = fat32::load_matrix_from_file(&fs, &root, "L0M_FC_W.BIN", N_EMBD * 4 * N_EMBD);
    let l0_proj_w = fat32::load_matrix_from_file(&fs, &root, "L0M_P_W.BIN", 4 * N_EMBD * N_EMBD);

    let l1_ln1_w = fat32::load_matrix_from_file(&fs, &root, "L1LN1_W.BIN", N_EMBD);
    let l1_ln1_b = fat32::load_matrix_from_file(&fs, &root, "L1LN1_B.BIN", N_EMBD);
    let l1_q_w = fat32::load_matrix_from_file(&fs, &root, "L1A_QW.BIN", N_EMBD * N_EMBD);
    let l1_k_w = fat32::load_matrix_from_file(&fs, &root, "L1A_KW.BIN", N_EMBD * N_EMBD);
    let l1_v_w = fat32::load_matrix_from_file(&fs, &root, "L1A_VW.BIN", N_EMBD * N_EMBD);
    let l1_o_w = fat32::load_matrix_from_file(&fs, &root, "L1A_OW.BIN", N_EMBD * N_EMBD);
    let l1_ln2_w = fat32::load_matrix_from_file(&fs, &root, "L1LN2_W.BIN", N_EMBD);
    let l1_ln2_b = fat32::load_matrix_from_file(&fs, &root, "L1LN2_B.BIN", N_EMBD);
    let l1_fc_w = fat32::load_matrix_from_file(&fs, &root, "L1M_FC_W.BIN", N_EMBD * 4 * N_EMBD);
    let l1_proj_w = fat32::load_matrix_from_file(&fs, &root, "L1M_P_W.BIN", 4 * N_EMBD * N_EMBD);

    let ln_f_w = fat32::load_matrix_from_file(&fs, &root, "LN_F_W.BIN", N_EMBD);
    let ln_f_b = fat32::load_matrix_from_file(&fs, &root, "LN_F_B.BIN", N_EMBD);
    let lm_head_w = fat32::load_matrix_from_file(&fs, &root, "LM_HD_W.BIN", N_EMBD * VOCAB_SIZE);
    let end_load = Timer::get_usec();

    println!("Finished loading the matrix in {} usec.", end_load - start_load);

    let vocab = &TRAIN_CHAR_VOCAB;
    let prompt = "Once upon a time";
    let new_tokens = 100usize;

    let mut token_ids = [0usize; MAX_GEN_TOKENS];
    let seed_len = tokenize(prompt, vocab, &mut token_ids);
    let mut total_len = seed_len;
    assert!(seed_len > 0, "empty prompt unsupported in this quick path");
    assert!(seed_len + new_tokens < BLOCK_SIZE, "increase BLOCK_SIZE or reduce prompt/new_tokens");

    let tok_embedding = Embedding::new(tok_emb, VOCAB_SIZE, N_EMBD);
    let pos_embedding = Embedding::new(pos_emb, BLOCK_SIZE, N_EMBD);

    let l0_ln1 = LayerNorm::new(l0_ln1_w, Some(l0_ln1_b), 1e-5);
    let l0_q = Linear::new(l0_q_w, None, N_EMBD, N_EMBD);
    let l0_k = Linear::new(l0_k_w, None, N_EMBD, N_EMBD);
    let l0_v = Linear::new(l0_v_w, None, N_EMBD, N_EMBD);
    let l0_o = Linear::new(l0_o_w, None, N_EMBD, N_EMBD);
    let l0_ln2 = LayerNorm::new(l0_ln2_w, Some(l0_ln2_b), 1e-5);
    let l0_fc = Linear::new(l0_fc_w, None, N_EMBD, 4 * N_EMBD);
    let l0_proj = Linear::new(l0_proj_w, None, 4 * N_EMBD, N_EMBD);

    let l1_ln1 = LayerNorm::new(l1_ln1_w, Some(l1_ln1_b), 1e-5);
    let l1_q = Linear::new(l1_q_w, None, N_EMBD, N_EMBD);
    let l1_k = Linear::new(l1_k_w, None, N_EMBD, N_EMBD);
    let l1_v = Linear::new(l1_v_w, None, N_EMBD, N_EMBD);
    let l1_o = Linear::new(l1_o_w, None, N_EMBD, N_EMBD);
    let l1_ln2 = LayerNorm::new(l1_ln2_w, Some(l1_ln2_b), 1e-5);
    let l1_fc = Linear::new(l1_fc_w, None, N_EMBD, 4 * N_EMBD);
    let l1_proj = Linear::new(l1_proj_w, None, 4 * N_EMBD, N_EMBD);

    let ln_f = LayerNorm::new(ln_f_w, Some(ln_f_b), 1e-5);
    let lm_head = Linear::new(lm_head_w, None, N_EMBD, VOCAB_SIZE);

    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);

        {
            let cache0_k = gpu_slot_mut(gpu_ptr, SLOT_LAYER0_K_CACHE, BLOCK_SIZE * N_EMBD);
            let cache0_v = gpu_slot_mut(gpu_ptr, SLOT_LAYER0_V_CACHE, BLOCK_SIZE * N_EMBD);
            let cache1_k = gpu_slot_mut(gpu_ptr, SLOT_LAYER1_K_CACHE, BLOCK_SIZE * N_EMBD);
            let cache1_v = gpu_slot_mut(gpu_ptr, SLOT_LAYER1_V_CACHE, BLOCK_SIZE * N_EMBD);
            for v in cache0_k.iter_mut() {
                *v = 0.0;
            }
            for v in cache0_v.iter_mut() {
                *v = 0.0;
            }
            for v in cache1_k.iter_mut() {
                *v = 0.0;
            }
            for v in cache1_v.iter_mut() {
                *v = 0.0;
            }
        }

        // Prefill caches with prompt tokens.
        let mut cache_len = 0usize;
        for pos in 0..seed_len {
            let tok = token_ids[pos];
            let h0_in = gpu_slot_mut(gpu_ptr, SLOT_H0_IN, N_EMBD);
            tok_embedding.lookup(tok, h0_in);
            let pos_tmp = gpu_slot_mut(gpu_ptr, SLOT_POS_TMP, N_EMBD);
            pos_embedding.lookup(pos, pos_tmp);
            add_in_place(h0_in, pos_tmp);

            block_step(
                gpu_ptr,
                SLOT_H0_IN,
                SLOT_H0_OUT,
                cache_len,
                SLOT_LAYER0_K_CACHE,
                SLOT_LAYER0_V_CACHE,
                &l0_ln1,
                &l0_q,
                &l0_k,
                &l0_v,
                &l0_o,
                &l0_ln2,
                &l0_fc,
                &l0_proj,
                SLOT_S0_LN1,
                SLOT_S0_Q,
                SLOT_S0_K,
                SLOT_S0_V,
                SLOT_S0_CTX,
                SLOT_S0_ATTN,
                SLOT_S0_X_ATTN,
                SLOT_S0_LN2,
                SLOT_S0_MLP_H,
                SLOT_S0_MLP_O,
            );
            block_step(
                gpu_ptr,
                SLOT_H0_OUT,
                SLOT_H1_OUT,
                cache_len,
                SLOT_LAYER1_K_CACHE,
                SLOT_LAYER1_V_CACHE,
                &l1_ln1,
                &l1_q,
                &l1_k,
                &l1_v,
                &l1_o,
                &l1_ln2,
                &l1_fc,
                &l1_proj,
                SLOT_S1_LN1,
                SLOT_S1_Q,
                SLOT_S1_K,
                SLOT_S1_V,
                SLOT_S1_CTX,
                SLOT_S1_ATTN,
                SLOT_S1_X_ATTN,
                SLOT_S1_LN2,
                SLOT_S1_MLP_H,
                SLOT_S1_MLP_O,
            );
            cache_len += 1;
        }

        let start_inf = Timer::get_usec();
        let mut per_token_us = [0u32; MAX_GEN_TOKENS];

        for gen_i in 0..new_tokens {
            let tok_start = Timer::get_usec();

            // Choose next token from current last hidden state.
            head_logits(gpu_ptr, SLOT_H1_OUT, SLOT_LN_OUT, SLOT_LOGITS, &ln_f, &lm_head);
            let logits = gpu_slot(gpu_ptr, SLOT_LOGITS, VOCAB_SIZE);
            let mut best_id = 0usize;
            let mut best_val = f32::NEG_INFINITY;
            for i in 0..VOCAB_SIZE {
                if logits[i] > best_val {
                    best_val = logits[i];
                    best_id = i;
                }
            }

            token_ids[total_len] = best_id;
            total_len += 1;

            let ch = if best_id < vocab.len() { vocab[best_id] } else { '?' };
            print!("{}", ch);

            // Feed generated token to extend KV cache for following token.
            if gen_i + 1 < new_tokens {
                let pos = total_len - 1;
                let h0_in = gpu_slot_mut(gpu_ptr, SLOT_H0_IN, N_EMBD);
                tok_embedding.lookup(best_id, h0_in);
                let pos_tmp = gpu_slot_mut(gpu_ptr, SLOT_POS_TMP, N_EMBD);
                pos_embedding.lookup(pos, pos_tmp);
                add_in_place(h0_in, pos_tmp);

                block_step(
                    gpu_ptr,
                    SLOT_H0_IN,
                    SLOT_H0_OUT,
                    cache_len,
                    SLOT_LAYER0_K_CACHE,
                    SLOT_LAYER0_V_CACHE,
                    &l0_ln1,
                    &l0_q,
                    &l0_k,
                    &l0_v,
                    &l0_o,
                    &l0_ln2,
                    &l0_fc,
                    &l0_proj,
                    SLOT_S0_LN1,
                    SLOT_S0_Q,
                    SLOT_S0_K,
                    SLOT_S0_V,
                    SLOT_S0_CTX,
                    SLOT_S0_ATTN,
                    SLOT_S0_X_ATTN,
                    SLOT_S0_LN2,
                    SLOT_S0_MLP_H,
                    SLOT_S0_MLP_O,
                );
                block_step(
                    gpu_ptr,
                    SLOT_H0_OUT,
                    SLOT_H1_OUT,
                    cache_len,
                    SLOT_LAYER1_K_CACHE,
                    SLOT_LAYER1_V_CACHE,
                    &l1_ln1,
                    &l1_q,
                    &l1_k,
                    &l1_v,
                    &l1_o,
                    &l1_ln2,
                    &l1_fc,
                    &l1_proj,
                    SLOT_S1_LN1,
                    SLOT_S1_Q,
                    SLOT_S1_K,
                    SLOT_S1_V,
                    SLOT_S1_CTX,
                    SLOT_S1_ATTN,
                    SLOT_S1_X_ATTN,
                    SLOT_S1_LN2,
                    SLOT_S1_MLP_H,
                    SLOT_S1_MLP_O,
                );
                cache_len += 1;
            }

            let tok_end = Timer::get_usec();
            per_token_us[gen_i] = (tok_end - tok_start) as u32;
        }
        println!("");

        let end_inf = Timer::get_usec();
        let elapsed_us = end_inf - start_inf;
        let generated_tokens = total_len.saturating_sub(seed_len);
        let toks_per_sec = if elapsed_us > 0 {
            (generated_tokens as f32) * 1_000_000.0 / (elapsed_us as f32)
        } else {
            0.0
        };

        let mut out_text = [0u8; MAX_GEN_TOKENS * 4];
        let out_n = detokenize(&token_ids[..total_len], vocab, &mut out_text);
        let out_str = core::str::from_utf8(&out_text[..out_n]).unwrap_or("<invalid utf8>");

        println!("Generation ran in {} usec", elapsed_us);
        println!("Prompt: {}", prompt);
        println!("Seed token ids: {:?}", &token_ids[..seed_len]);
        println!("Generated sequence ids: {:?}", &token_ids[..total_len]);
        println!("Generated tokens: {}", generated_tokens);
        println!("Tokens/sec: {:.2}", toks_per_sec);
        println!("Per-token latency (usec):");
        for i in 0..generated_tokens {
            let id = token_ids[seed_len + i];
            let ch = if id < vocab.len() { vocab[id] } else { '?' };
            println!("  token {} '{}' -> {} usec", i, ch, per_token_us[i]);
        }
        println!("Detokenized output: {}", out_str);

        gpu.release();
    }
}
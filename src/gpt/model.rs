// Quick CPU-only inference path matching utils/train_tiny_model.py.
use crate::fat32::{self};
use crate::kmalloc;
use crate::println;
use crate::timer::Timer;
use libm::{expf, sqrtf, tanhf};

const VOCAB_SIZE: usize = 92;
const BLOCK_SIZE: usize = 128;
const N_HEAD: usize = 4;
const N_EMBD: usize = 128;
const HEAD_DIM: usize = N_EMBD / N_HEAD;
const MAX_GEN_TOKENS: usize = 64;

// Exact vocab order from utils/test_tiny_model.py:
// chars = sorted(set(text)) over TinyStories training stream.
const TRAIN_CHAR_VOCAB: [char; VOCAB_SIZE] = [
    '\n', ' ', '!', '"', '$', '&', '\'', '*', ',', '-', '.',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ':', ';', '?',
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    '¡', '¦', '©', '«', '±', '³', '»', 'Â', 'Ã', 'â', 'œ', '˜', '“', '”', '€', '™',
];

struct AttentionScratch<'a> {
    q: &'a mut [f32],
    k: &'a mut [f32],
    v: &'a mut [f32],
    ctx: &'a mut [f32],
}

struct BlockScratch<'a> {
    x0: &'a mut [f32],
    ln1: &'a mut [f32],
    attn: &'a mut [f32],
    ln2: &'a mut [f32],
    mlp_h: &'a mut [f32],
    mlp_o: &'a mut [f32],
}

fn alloc_f32(len: usize) -> &'static mut [f32] {
    let ptr = unsafe { kmalloc::kmalloc_t::<f32>(len) };
    unsafe { core::slice::from_raw_parts_mut(ptr, len) }
}

fn vocab_find_id(vocab: &[char], ch: char) -> usize {
    for (i, &v) in vocab.iter().enumerate() {
        if v == ch {
            return i;
        }
    }

    // Fallback to space if present, else zero.
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

fn add_in_place(dst: &mut [f32], rhs: &[f32]) {
    assert!(dst.len() == rhs.len());
    for i in 0..dst.len() {
        dst[i] += rhs[i];
    }
}

fn layer_norm(x: &[f32], out: &mut [f32], w: &[f32], b: &[f32], rows: usize, dim: usize) {
    assert!(x.len() == rows * dim && out.len() == rows * dim);
    assert!(w.len() == dim && b.len() == dim);

    for r in 0..rows {
        let row = &x[r * dim..(r + 1) * dim];
        let mean = row.iter().sum::<f32>() / dim as f32;

        let mut var = 0.0;
        for &v in row {
            let d = v - mean;
            var += d * d;
        }
        var /= dim as f32;

        let inv_std = 1.0 / sqrtf(var + 1e-5);
        for i in 0..dim {
            out[r * dim + i] = (row[i] - mean) * inv_std * w[i] + b[i];
        }
    }
}

fn linear(x: &[f32], out: &mut [f32], w: &[f32], rows: usize, in_dim: usize, out_dim: usize) {
    assert!(x.len() == rows * in_dim);
    assert!(w.len() == in_dim * out_dim);
    assert!(out.len() == rows * out_dim);

    for r in 0..rows {
        for c in 0..out_dim {
            let mut acc = 0.0;
            for k in 0..in_dim {
                acc += x[r * in_dim + k] * w[k * out_dim + c];
            }
            out[r * out_dim + c] = acc;
        }
    }
}

fn gelu_in_place(x: &mut [f32]) {
    for v in x.iter_mut() {
        let xv = *v;
        let x3 = xv * xv * xv;
        let inner = 0.7978846 * (xv + 0.044715 * x3);
        *v = 0.5 * xv * (1.0 + tanhf(inner));
    }
}

fn causal_attention(
    x: &[f32],
    out: &mut [f32],
    seq_len: usize,
    q_w: &[f32],
    k_w: &[f32],
    v_w: &[f32],
    o_w: &[f32],
    scratch: &mut AttentionScratch,
) {
    let q = &mut scratch.q[..seq_len * N_EMBD];
    let k = &mut scratch.k[..seq_len * N_EMBD];
    let v = &mut scratch.v[..seq_len * N_EMBD];
    let ctx = &mut scratch.ctx[..seq_len * N_EMBD];

    linear(x, q, q_w, seq_len, N_EMBD, N_EMBD);
    linear(x, k, k_w, seq_len, N_EMBD, N_EMBD);
    linear(x, v, v_w, seq_len, N_EMBD, N_EMBD);

    let scale = 1.0 / sqrtf(HEAD_DIM as f32);

    for h in 0..N_HEAD {
        for t in 0..seq_len {
            let mut max_logit = f32::NEG_INFINITY;
            for tp in 0..=t {
                let mut dot = 0.0;
                for d in 0..HEAD_DIM {
                    let qi = t * N_EMBD + h * HEAD_DIM + d;
                    let ki = tp * N_EMBD + h * HEAD_DIM + d;
                    dot += q[qi] * k[ki];
                }
                let logit = dot * scale;
                if logit > max_logit {
                    max_logit = logit;
                }
            }

            let mut denom = 0.0;
            for tp in 0..=t {
                let mut dot = 0.0;
                for d in 0..HEAD_DIM {
                    let qi = t * N_EMBD + h * HEAD_DIM + d;
                    let ki = tp * N_EMBD + h * HEAD_DIM + d;
                    dot += q[qi] * k[ki];
                }
                denom += expf(dot * scale - max_logit);
            }

            for d in 0..HEAD_DIM {
                let mut acc = 0.0;
                for tp in 0..=t {
                    let mut dot = 0.0;
                    for dd in 0..HEAD_DIM {
                        let qi = t * N_EMBD + h * HEAD_DIM + dd;
                        let ki = tp * N_EMBD + h * HEAD_DIM + dd;
                        dot += q[qi] * k[ki];
                    }
                    let w = expf(dot * scale - max_logit) / denom;
                    let vi = tp * N_EMBD + h * HEAD_DIM + d;
                    acc += w * v[vi];
                }
                ctx[t * N_EMBD + h * HEAD_DIM + d] = acc;
            }
        }
    }

    linear(ctx, out, o_w, seq_len, N_EMBD, N_EMBD);
}

fn block_forward(
    x: &mut [f32],
    seq_len: usize,
    ln1_w: &[f32],
    ln1_b: &[f32],
    q_w: &[f32],
    k_w: &[f32],
    v_w: &[f32],
    o_w: &[f32],
    ln2_w: &[f32],
    ln2_b: &[f32],
    fc_w: &[f32],
    proj_w: &[f32],
    block_scratch: &mut BlockScratch,
    attn_scratch: &mut AttentionScratch,
) {
    let x0 = &mut block_scratch.x0[..seq_len * N_EMBD];
    let ln1 = &mut block_scratch.ln1[..seq_len * N_EMBD];
    let attn = &mut block_scratch.attn[..seq_len * N_EMBD];
    let ln2 = &mut block_scratch.ln2[..seq_len * N_EMBD];
    let mlp_h = &mut block_scratch.mlp_h[..seq_len * 4 * N_EMBD];
    let mlp_o = &mut block_scratch.mlp_o[..seq_len * N_EMBD];

    // Match Python exactly:
    // return x + mlp(ln2(x + attn(ln1(x))))
    // i.e. final residual uses original x, not (x + attn(...)).
    x0.copy_from_slice(x);

    layer_norm(x0, ln1, ln1_w, ln1_b, seq_len, N_EMBD);
    causal_attention(ln1, attn, seq_len, q_w, k_w, v_w, o_w, attn_scratch);

    for i in 0..(seq_len * N_EMBD) {
        ln2[i] = x0[i] + attn[i];
    }

    layer_norm(ln2, ln1, ln2_w, ln2_b, seq_len, N_EMBD);
    linear(ln1, mlp_h, fc_w, seq_len, N_EMBD, 4 * N_EMBD);
    gelu_in_place(mlp_h);
    linear(mlp_h, mlp_o, proj_w, seq_len, 4 * N_EMBD, N_EMBD);

    for i in 0..(seq_len * N_EMBD) {
        x[i] = x0[i] + mlp_o[i];
    }
}

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

    let vocab = &TRAIN_CHAR_VOCAB;

    let prompt = "Once upon a time";
    let new_tokens = 12usize;

    let mut token_ids = [0usize; MAX_GEN_TOKENS];
    let seed_len = tokenize(prompt, vocab, &mut token_ids);
    let mut total_len = seed_len;

    let x = alloc_f32(BLOCK_SIZE * N_EMBD);
    let x_ln = alloc_f32(BLOCK_SIZE * N_EMBD);
    let logits = alloc_f32(BLOCK_SIZE * VOCAB_SIZE);

    let mut block_scratch = BlockScratch {
        x0: alloc_f32(BLOCK_SIZE * N_EMBD),
        ln1: alloc_f32(BLOCK_SIZE * N_EMBD),
        attn: alloc_f32(BLOCK_SIZE * N_EMBD),
        ln2: alloc_f32(BLOCK_SIZE * N_EMBD),
        mlp_h: alloc_f32(BLOCK_SIZE * 4 * N_EMBD),
        mlp_o: alloc_f32(BLOCK_SIZE * N_EMBD),
    };
    let mut attn_scratch = AttentionScratch {
        q: alloc_f32(BLOCK_SIZE * N_EMBD),
        k: alloc_f32(BLOCK_SIZE * N_EMBD),
        v: alloc_f32(BLOCK_SIZE * N_EMBD),
        ctx: alloc_f32(BLOCK_SIZE * N_EMBD),
    };

    let start_inf = Timer::get_usec();

    for _ in 0..new_tokens {
        let ctx_start = total_len.saturating_sub(BLOCK_SIZE);
        let ctx_len = total_len - ctx_start;

        for t in 0..ctx_len {
            let tok = token_ids[ctx_start + t];
            for d in 0..N_EMBD {
                let te = tok_emb[tok * N_EMBD + d];
                let pe = pos_emb[t * N_EMBD + d];
                x[t * N_EMBD + d] = te + pe;
            }
        }

        block_forward(
            &mut x[..ctx_len * N_EMBD],
            ctx_len,
            l00_ln1_w,
            l00_ln1_b,
            l00_attn_q_w,
            l00_attn_k_w,
            l00_attn_v_w,
            l00_attn_o_w,
            l00_ln2_w,
            l00_ln2_b,
            l00_mlp_fc_w,
            l00_mlp_proj_w,
            &mut block_scratch,
            &mut attn_scratch,
        );
        block_forward(
            &mut x[..ctx_len * N_EMBD],
            ctx_len,
            l01_ln1_w,
            l01_ln1_b,
            l01_attn_q_w,
            l01_attn_k_w,
            l01_attn_v_w,
            l01_attn_o_w,
            l01_ln2_w,
            l01_ln2_b,
            l01_mlp_fc_w,
            l01_mlp_proj_w,
            &mut block_scratch,
            &mut attn_scratch,
        );

        layer_norm(
            &x[..ctx_len * N_EMBD],
            &mut x_ln[..ctx_len * N_EMBD],
            ln_f_w,
            ln_f_b,
            ctx_len,
            N_EMBD,
        );
        linear(
            &x_ln[..ctx_len * N_EMBD],
            &mut logits[..ctx_len * VOCAB_SIZE],
            lm_head_w,
            ctx_len,
            N_EMBD,
            VOCAB_SIZE,
        );

        let last = &logits[(ctx_len - 1) * VOCAB_SIZE..ctx_len * VOCAB_SIZE];
        let mut best_id = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for i in 0..VOCAB_SIZE {
            if last[i] > best_val {
                best_val = last[i];
                best_id = i;
            }
        }

        if total_len < MAX_GEN_TOKENS {
            token_ids[total_len] = best_id;
            total_len += 1;
        } else {
            break;
        }
    }

    let end_inf = Timer::get_usec();
    let elapsed_us = end_inf - start_inf;
    let generated_tokens = total_len.saturating_sub(seed_len);
    let toks_per_sec = if elapsed_us > 0 {
        (generated_tokens as u64) * 1_000_000 / (elapsed_us as u64)
    } else {
        0
    };

    let mut out_text = [0u8; MAX_GEN_TOKENS];
    let out_n = detokenize(&token_ids[..total_len], vocab, &mut out_text);
    let out_str = core::str::from_utf8(&out_text[..out_n]).unwrap_or("<invalid utf8>");

    println!("Generation ran in {} usec", elapsed_us);
    println!("Prompt: {}", prompt);
    println!("Seed token ids: {:?}", &token_ids[..seed_len]);
    println!("Generated sequence ids: {:?}", &token_ids[..total_len]);
    println!("Generated tokens: {}", generated_tokens);
    println!("Tokens/sec: {}", toks_per_sec);
    println!("Detokenized output: {}", out_str);
}
// Quick CPU-only inference path matching utils/train_tiny_model.py.
use crate::fat32::{self};
use crate::kmalloc;
use crate::println;
use crate::print;
use crate::timer::Timer;
use libm::{expf, sqrtf, tanhf};

const VOCAB_SIZE: usize = 92;
const BLOCK_SIZE: usize = 128;
const N_HEAD: usize = 4;
const N_EMBD: usize = 128;
const HEAD_DIM: usize = N_EMBD / N_HEAD;
const MAX_GEN_TOKENS: usize = 64;


const TRAIN_CHAR_VOCAB: [char; VOCAB_SIZE] = [
    '\n', ' ', '!', '"', '$', '&', '\'', '*', ',', '-', '.', '0', '1', '2', '3', '4', '5', '6',
    '7', '8', '9', ':', ';', '?', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f',
    'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y',
    'z', '¡', '¦', '©', '«', '±', '³', '»', 'Â', 'Ã', 'â', 'œ', '˜', '“', '”', '€', '™',
];

struct BlockWeights<'a> {
    ln1_w: &'a [f32],
    ln1_b: &'a [f32],
    q_w: &'a [f32],
    k_w: &'a [f32],
    v_w: &'a [f32],
    o_w: &'a [f32],
    ln2_w: &'a [f32],
    ln2_b: &'a [f32],
    fc_w: &'a [f32],
    proj_w: &'a [f32],
}

struct LayerKvCache<'a> {
    k: &'a mut [f32],
    v: &'a mut [f32],
}

struct StepScratch<'a> {
    ln1: &'a mut [f32],
    q: &'a mut [f32],
    k: &'a mut [f32],
    v: &'a mut [f32],
    ctx: &'a mut [f32],
    attn: &'a mut [f32],
    x_attn: &'a mut [f32],
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

fn layer_norm_one(x: &[f32], out: &mut [f32], w: &[f32], b: &[f32]) {
    assert!(x.len() == N_EMBD && out.len() == N_EMBD);
    let mean = x.iter().sum::<f32>() / N_EMBD as f32;
    let mut var = 0.0;
    for &v in x {
        let d = v - mean;
        var += d * d;
    }
    var /= N_EMBD as f32;
    let inv_std = 1.0 / sqrtf(var + 1e-5);
    for i in 0..N_EMBD {
        out[i] = (x[i] - mean) * inv_std * w[i] + b[i];
    }
}

fn linear_one(x: &[f32], w: &[f32], in_dim: usize, out_dim: usize, out: &mut [f32]) {
    assert!(x.len() == in_dim && out.len() == out_dim);
    assert!(w.len() == in_dim * out_dim);
    for c in 0..out_dim {
        let mut acc = 0.0;
        for k in 0..in_dim {
            acc += x[k] * w[k * out_dim + c];
        }
        out[c] = acc;
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

fn block_step(
    x_in: &[f32],
    x_out: &mut [f32],
    cache_len: usize,
    w: &BlockWeights,
    cache: &mut LayerKvCache,
    scratch: &mut StepScratch,
) {
    assert!(x_in.len() == N_EMBD && x_out.len() == N_EMBD);
    assert!(cache_len < BLOCK_SIZE);

    layer_norm_one(x_in, scratch.ln1, w.ln1_w, w.ln1_b);
    linear_one(scratch.ln1, w.q_w, N_EMBD, N_EMBD, scratch.q);
    linear_one(scratch.ln1, w.k_w, N_EMBD, N_EMBD, scratch.k);
    linear_one(scratch.ln1, w.v_w, N_EMBD, N_EMBD, scratch.v);

    let dst_k = &mut cache.k[cache_len * N_EMBD..(cache_len + 1) * N_EMBD];
    let dst_v = &mut cache.v[cache_len * N_EMBD..(cache_len + 1) * N_EMBD];
    dst_k.copy_from_slice(scratch.k);
    dst_v.copy_from_slice(scratch.v);

    let scale = 1.0 / sqrtf(HEAD_DIM as f32);
    for h in 0..N_HEAD {
        let qh = &scratch.q[h * HEAD_DIM..(h + 1) * HEAD_DIM];

        let mut max_logit = f32::NEG_INFINITY;
        for t in 0..=cache_len {
            let kh = &cache.k[t * N_EMBD + h * HEAD_DIM..t * N_EMBD + (h + 1) * HEAD_DIM];
            let mut dot = 0.0;
            for d in 0..HEAD_DIM {
                dot += qh[d] * kh[d];
            }
            let logit = dot * scale;
            if logit > max_logit {
                max_logit = logit;
            }
        }

        let mut denom = 0.0;
        for t in 0..=cache_len {
            let kh = &cache.k[t * N_EMBD + h * HEAD_DIM..t * N_EMBD + (h + 1) * HEAD_DIM];
            let mut dot = 0.0;
            for d in 0..HEAD_DIM {
                dot += qh[d] * kh[d];
            }
            denom += expf(dot * scale - max_logit);
        }

        for d in 0..HEAD_DIM {
            let mut acc = 0.0;
            for t in 0..=cache_len {
                let kh = &cache.k[t * N_EMBD + h * HEAD_DIM..t * N_EMBD + (h + 1) * HEAD_DIM];
                let vh = &cache.v[t * N_EMBD + h * HEAD_DIM..t * N_EMBD + (h + 1) * HEAD_DIM];
                let mut dot = 0.0;
                for dd in 0..HEAD_DIM {
                    dot += qh[dd] * kh[dd];
                }
                let p = expf(dot * scale - max_logit) / denom;
                acc += p * vh[d];
            }
            scratch.ctx[h * HEAD_DIM + d] = acc;
        }
    }

    linear_one(scratch.ctx, w.o_w, N_EMBD, N_EMBD, scratch.attn);
    for i in 0..N_EMBD {
        scratch.x_attn[i] = x_in[i] + scratch.attn[i];
    }

    layer_norm_one(scratch.x_attn, scratch.ln2, w.ln2_w, w.ln2_b);
    linear_one(scratch.ln2, w.fc_w, N_EMBD, 4 * N_EMBD, scratch.mlp_h);
    gelu_in_place(scratch.mlp_h);
    linear_one(scratch.mlp_h, w.proj_w, 4 * N_EMBD, N_EMBD, scratch.mlp_o);

    for i in 0..N_EMBD {
        // Match Python block exactly.
        x_out[i] = x_in[i] + scratch.mlp_o[i];
    }
}

fn head_logits(x: &[f32], ln_w: &[f32], ln_b: &[f32], head_w: &[f32], out: &mut [f32]) {
    let ln_out = alloc_f32(N_EMBD);
    layer_norm_one(x, ln_out, ln_w, ln_b);
    linear_one(ln_out, head_w, N_EMBD, VOCAB_SIZE, out);
}

pub fn load_model() {
    println!("Reading the MBR.");
    let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");

    println!("Loading the FAT.");
    let fs = fat32::fat32_mk(&partition);

    println!("Loading the root directory.");
    let root = fat32::fat32_get_root(&fs);

    let start_load = Timer::get_usec();
    let tok_emb = fat32::load_matrix_from_file(&fs, &root, "TOK_EMB.BIN", VOCAB_SIZE * N_EMBD);
    let pos_emb = fat32::load_matrix_from_file(&fs, &root, "POS_EMB.BIN", BLOCK_SIZE * N_EMBD);

    let l00 = BlockWeights {
        ln1_w: fat32::load_matrix_from_file(&fs, &root, "L0LN1_W.BIN", N_EMBD),
        ln1_b: fat32::load_matrix_from_file(&fs, &root, "L0LN1_B.BIN", N_EMBD),
        q_w: fat32::load_matrix_from_file(&fs, &root, "L0A_QW.BIN", N_EMBD * N_EMBD),
        k_w: fat32::load_matrix_from_file(&fs, &root, "L0A_KW.BIN", N_EMBD * N_EMBD),
        v_w: fat32::load_matrix_from_file(&fs, &root, "L0A_VW.BIN", N_EMBD * N_EMBD),
        o_w: fat32::load_matrix_from_file(&fs, &root, "L0A_OW.BIN", N_EMBD * N_EMBD),
        ln2_w: fat32::load_matrix_from_file(&fs, &root, "L0LN2_W.BIN", N_EMBD),
        ln2_b: fat32::load_matrix_from_file(&fs, &root, "L0LN2_B.BIN", N_EMBD),
        fc_w: fat32::load_matrix_from_file(&fs, &root, "L0M_FC_W.BIN", N_EMBD * 4 * N_EMBD),
        proj_w: fat32::load_matrix_from_file(&fs, &root, "L0M_P_W.BIN", 4 * N_EMBD * N_EMBD),
    };

    let l01 = BlockWeights {
        ln1_w: fat32::load_matrix_from_file(&fs, &root, "L1LN1_W.BIN", N_EMBD),
        ln1_b: fat32::load_matrix_from_file(&fs, &root, "L1LN1_B.BIN", N_EMBD),
        q_w: fat32::load_matrix_from_file(&fs, &root, "L1A_QW.BIN", N_EMBD * N_EMBD),
        k_w: fat32::load_matrix_from_file(&fs, &root, "L1A_KW.BIN", N_EMBD * N_EMBD),
        v_w: fat32::load_matrix_from_file(&fs, &root, "L1A_VW.BIN", N_EMBD * N_EMBD),
        o_w: fat32::load_matrix_from_file(&fs, &root, "L1A_OW.BIN", N_EMBD * N_EMBD),
        ln2_w: fat32::load_matrix_from_file(&fs, &root, "L1LN2_W.BIN", N_EMBD),
        ln2_b: fat32::load_matrix_from_file(&fs, &root, "L1LN2_B.BIN", N_EMBD),
        fc_w: fat32::load_matrix_from_file(&fs, &root, "L1M_FC_W.BIN", N_EMBD * 4 * N_EMBD),
        proj_w: fat32::load_matrix_from_file(&fs, &root, "L1M_P_W.BIN", 4 * N_EMBD * N_EMBD),
    };

    let ln_f_w = fat32::load_matrix_from_file(&fs, &root, "LN_F_W.BIN", N_EMBD);
    let ln_f_b = fat32::load_matrix_from_file(&fs, &root, "LN_F_B.BIN", N_EMBD);
    let lm_head_w = fat32::load_matrix_from_file(&fs, &root, "LM_HD_W.BIN", N_EMBD * VOCAB_SIZE);
    let end_load = Timer::get_usec();

    println!("Finished loading the matrix in {} usec.", end_load - start_load);

    let vocab = &TRAIN_CHAR_VOCAB;
    let prompt = "Once upon a time";
    let new_tokens = 12usize;

    let mut token_ids = [0usize; MAX_GEN_TOKENS];
    let seed_len = tokenize(prompt, vocab, &mut token_ids);
    let mut total_len = seed_len;
    assert!(seed_len > 0, "empty prompt unsupported in this quick path");
    assert!(seed_len + new_tokens < BLOCK_SIZE, "increase BLOCK_SIZE or reduce prompt/new_tokens");

    let mut layer0_cache = LayerKvCache {
        k: alloc_f32(BLOCK_SIZE * N_EMBD),
        v: alloc_f32(BLOCK_SIZE * N_EMBD),
    };
    let mut layer1_cache = LayerKvCache {
        k: alloc_f32(BLOCK_SIZE * N_EMBD),
        v: alloc_f32(BLOCK_SIZE * N_EMBD),
    };

    let mut s0 = StepScratch {
        ln1: alloc_f32(N_EMBD),
        q: alloc_f32(N_EMBD),
        k: alloc_f32(N_EMBD),
        v: alloc_f32(N_EMBD),
        ctx: alloc_f32(N_EMBD),
        attn: alloc_f32(N_EMBD),
        x_attn: alloc_f32(N_EMBD),
        ln2: alloc_f32(N_EMBD),
        mlp_h: alloc_f32(4 * N_EMBD),
        mlp_o: alloc_f32(N_EMBD),
    };
    let mut s1 = StepScratch {
        ln1: alloc_f32(N_EMBD),
        q: alloc_f32(N_EMBD),
        k: alloc_f32(N_EMBD),
        v: alloc_f32(N_EMBD),
        ctx: alloc_f32(N_EMBD),
        attn: alloc_f32(N_EMBD),
        x_attn: alloc_f32(N_EMBD),
        ln2: alloc_f32(N_EMBD),
        mlp_h: alloc_f32(4 * N_EMBD),
        mlp_o: alloc_f32(N_EMBD),
    };

    let h0_in = alloc_f32(N_EMBD);
    let h0_out = alloc_f32(N_EMBD);
    let h1_out = alloc_f32(N_EMBD);
    let logits = alloc_f32(VOCAB_SIZE);

    // Prefill caches with prompt tokens.
    let mut cache_len = 0usize;
    for pos in 0..seed_len {
        let tok = token_ids[pos];
        for d in 0..N_EMBD {
            h0_in[d] = tok_emb[tok * N_EMBD + d] + pos_emb[pos * N_EMBD + d];
        }

        block_step(h0_in, h0_out, cache_len, &l00, &mut layer0_cache, &mut s0);
        block_step(h0_out, h1_out, cache_len, &l01, &mut layer1_cache, &mut s1);
        cache_len += 1;
    }

    let start_inf = Timer::get_usec();
    let mut per_token_us = [0u32; MAX_GEN_TOKENS];

    for gen_i in 0..new_tokens {
        let tok_start = Timer::get_usec();

        // Choose next token from current last hidden state.
        head_logits(h1_out, ln_f_w, ln_f_b, lm_head_w, logits);
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
            for d in 0..N_EMBD {
                h0_in[d] = tok_emb[best_id * N_EMBD + d] + pos_emb[pos * N_EMBD + d];
            }
            block_step(h0_in, h0_out, cache_len, &l00, &mut layer0_cache, &mut s0);
            block_step(h0_out, h1_out, cache_len, &l01, &mut layer1_cache, &mut s1);
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
}
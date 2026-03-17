#![allow(dead_code)]

pub const MAX_SEQ: usize = 64;
pub const MAX_DMODEL: usize = 64;
pub const MAX_FF: usize = 256;
pub const MAX_VOCAB: usize = 50257;
pub const MAX_TOKENIZER_VOCAB: usize = 4096;
pub const MAX_TOKEN_TEXT: usize = 24;
pub const MAX_LAYERS: usize = 8;
pub const MAX_HEADS: usize = 4;
pub const HEAD_DIM: usize = 16;
pub const LM_HEAD_CHUNK: usize = 256;

pub const GPT2_VOCAB: usize = 50257;
pub const GPT2_CONTEXT: usize = 1024;
pub const GPT2_DMODEL: usize = 64;
pub const GPT2_FF: usize = 256;
pub const GPT2_LAYERS: usize = 8;
pub const GPT2_HEADS: usize = 4;

#[derive(Clone, Copy)]
pub struct GptConfig {
    pub vocab_size: usize,
    pub context_size: usize,
    pub d_model: usize,
    pub d_ff: usize,
    pub n_layers: usize,
    pub n_heads: usize,
}

#[derive(Clone, Copy)]
pub struct LayerWeights<'a> {
    pub ln1_g: &'a [f32],
    pub ln1_b: &'a [f32],
    pub c_attn_w: &'a [f32],
    pub c_attn_b: &'a [f32],
    pub c_proj_w: &'a [f32],
    pub c_proj_b: &'a [f32],
    pub ln2_g: &'a [f32],
    pub ln2_b: &'a [f32],
    pub ff_w1: &'a [f32],
    pub ff_b1: &'a [f32],
    pub ff_w2: &'a [f32],
    pub ff_b2: &'a [f32],
}

impl<'a> LayerWeights<'a> {
    pub const EMPTY: Self = Self {
        ln1_g: &[],
        ln1_b: &[],
        c_attn_w: &[],
        c_attn_b: &[],
        c_proj_w: &[],
        c_proj_b: &[],
        ln2_g: &[],
        ln2_b: &[],
        ff_w1: &[],
        ff_b1: &[],
        ff_w2: &[],
        ff_b2: &[],
    };
}

pub struct GptWeights<'a> {
    pub lm_head: &'a [f32],
    pub ln_f_g: &'a [f32],
    pub ln_f_b: &'a [f32],
    pub wpe: &'a [f32],
    pub wte: &'a [f32],
    pub layers: [LayerWeights<'a>; MAX_LAYERS],
}

pub struct Scratch {
    pub x: [f32; MAX_SEQ * MAX_DMODEL],
    pub x_norm: [f32; MAX_SEQ * MAX_DMODEL],
    pub qkv: [f32; MAX_SEQ * MAX_DMODEL * 3],
    pub q: [f32; MAX_SEQ * MAX_DMODEL],
    pub k: [f32; MAX_SEQ * MAX_DMODEL],
    pub v: [f32; MAX_SEQ * MAX_DMODEL],
    pub k_t: [f32; HEAD_DIM * MAX_SEQ],
    pub att: [f32; MAX_SEQ * MAX_SEQ],
    pub ctx: [f32; MAX_SEQ * MAX_DMODEL],
    pub proj: [f32; MAX_SEQ * MAX_DMODEL],
    pub ff1: [f32; MAX_SEQ * MAX_FF],
    pub ff2: [f32; MAX_SEQ * MAX_DMODEL],
    pub head_q: [f32; MAX_SEQ * HEAD_DIM],
    pub head_k: [f32; MAX_SEQ * HEAD_DIM],
    pub head_v: [f32; MAX_SEQ * HEAD_DIM],
    pub head_ctx: [f32; MAX_SEQ * HEAD_DIM],
    pub lm_head_chunk_t: [f32; MAX_DMODEL * LM_HEAD_CHUNK],
    pub logits_chunk: [f32; LM_HEAD_CHUNK],
    pub logits: [f32; MAX_VOCAB],
}

pub static mut SCRATCH: Scratch = Scratch {
    x: [0.0; MAX_SEQ * MAX_DMODEL],
    x_norm: [0.0; MAX_SEQ * MAX_DMODEL],
    qkv: [0.0; MAX_SEQ * MAX_DMODEL * 3],
    q: [0.0; MAX_SEQ * MAX_DMODEL],
    k: [0.0; MAX_SEQ * MAX_DMODEL],
    v: [0.0; MAX_SEQ * MAX_DMODEL],
    k_t: [0.0; HEAD_DIM * MAX_SEQ],
    att: [0.0; MAX_SEQ * MAX_SEQ],
    ctx: [0.0; MAX_SEQ * MAX_DMODEL],
    proj: [0.0; MAX_SEQ * MAX_DMODEL],
    ff1: [0.0; MAX_SEQ * MAX_FF],
    ff2: [0.0; MAX_SEQ * MAX_DMODEL],
    head_q: [0.0; MAX_SEQ * HEAD_DIM],
    head_k: [0.0; MAX_SEQ * HEAD_DIM],
    head_v: [0.0; MAX_SEQ * HEAD_DIM],
    head_ctx: [0.0; MAX_SEQ * HEAD_DIM],
    lm_head_chunk_t: [0.0; MAX_DMODEL * LM_HEAD_CHUNK],
    logits_chunk: [0.0; LM_HEAD_CHUNK],
    logits: [0.0; MAX_VOCAB],
};

#[inline]
pub fn inv_sqrt(x: f32) -> f32 {
    let x_half = 0.5 * x;
    let mut i = x.to_bits();
    i = 0x5f3759dfu32.wrapping_sub(i >> 1);
    let mut y = f32::from_bits(i);
    y = y * (1.5 - x_half * y * y);
    y = y * (1.5 - x_half * y * y);
    y
}

#[inline]
pub fn gelu_approx(x: f32) -> f32 {
    let x3 = x * x * x;
    let t = 0.79788456 * (x + 0.044715 * x3);
    let s = t / (1.0 + t.abs());
    0.5 * x * (1.0 + s)
}

pub fn validate_config(cfg: &GptConfig, w: &GptWeights<'_>) -> bool {
    if cfg.vocab_size != GPT2_VOCAB
        || cfg.context_size != GPT2_CONTEXT
        || cfg.d_model != GPT2_DMODEL
        || cfg.d_ff != GPT2_FF
        || cfg.n_layers != GPT2_LAYERS
        || cfg.n_heads != GPT2_HEADS
    {
        return false;
    }

    if w.lm_head.len() != GPT2_VOCAB * GPT2_DMODEL
        || w.wte.len() != GPT2_VOCAB * GPT2_DMODEL
        || w.wpe.len() != GPT2_CONTEXT * GPT2_DMODEL
        || w.ln_f_g.len() != GPT2_DMODEL
        || w.ln_f_b.len() != GPT2_DMODEL
    {
        return false;
    }

    for i in 0..GPT2_LAYERS {
        let l = w.layers[i];
        if l.ln1_g.len() != GPT2_DMODEL
            || l.ln1_b.len() != GPT2_DMODEL
            || l.c_attn_w.len() != GPT2_DMODEL * (3 * GPT2_DMODEL)
            || l.c_attn_b.len() != 3 * GPT2_DMODEL
            || l.c_proj_w.len() != GPT2_DMODEL * GPT2_DMODEL
            || l.c_proj_b.len() != GPT2_DMODEL
            || l.ln2_g.len() != GPT2_DMODEL
            || l.ln2_b.len() != GPT2_DMODEL
            || l.ff_w1.len() != GPT2_DMODEL * GPT2_FF
            || l.ff_b1.len() != GPT2_FF
            || l.ff_w2.len() != GPT2_FF * GPT2_DMODEL
            || l.ff_b2.len() != GPT2_DMODEL
        {
            return false;
        }
    }

    true
}

pub fn argmax(xs: &[f32]) -> usize {
    let mut best_i = 0usize;
    let mut best_v = xs[0];
    for i in 1..xs.len() {
        if xs[i] > best_v {
            best_v = xs[i];
            best_i = i;
        }
    }
    best_i
}

pub struct Tokenizer {
    tokens: [[u8; MAX_TOKEN_TEXT]; MAX_TOKENIZER_VOCAB],
    lens: [u8; MAX_TOKENIZER_VOCAB],
    n: usize,
    unk_idx: usize,
}

impl Tokenizer {
    pub fn fallback_demo() -> Self {
        let toks = [
            " ", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", ".", ",", "!", "?", "\n", "the", "to", "and", "I", "you", "it", "is", "was", "on", "in", "of", "<unk>", "<eos>",
        ];
        let mut out = Tokenizer {
            tokens: [[0u8; MAX_TOKEN_TEXT]; MAX_TOKENIZER_VOCAB],
            lens: [0u8; MAX_TOKENIZER_VOCAB],
            n: 0,
            unk_idx: 0,
        };
        for (i, s) in toks.iter().enumerate() {
            out.set_token(i, s.as_bytes());
        }
        out.n = toks.len();
        out.unk_idx = 43;
        out
    }

    pub fn from_tokenizer_txt(bytes: &[u8]) -> Option<Self> {
        let mut out = Tokenizer {
            tokens: [[0u8; MAX_TOKEN_TEXT]; MAX_TOKENIZER_VOCAB],
            lens: [0u8; MAX_TOKENIZER_VOCAB],
            n: 0,
            unk_idx: 0,
        };

        let mut start = 0usize;
        let mut idx = 0usize;
        while start < bytes.len() && idx < MAX_TOKENIZER_VOCAB {
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'\n' {
                end += 1;
            }

            let mut line_end = end;
            while line_end > start && (bytes[line_end - 1] == b'\r' || bytes[line_end - 1] == b'\n') {
                line_end -= 1;
            }

            if line_end > start {
                out.set_token(idx, &bytes[start..line_end]);
                idx += 1;
            }

            start = end.saturating_add(1);
        }

        if idx == 0 {
            return None;
        }

        out.n = idx;
        out.unk_idx = out.find_token("<unk>").unwrap_or(0);
        Some(out)
    }

    fn set_token(&mut self, idx: usize, raw: &[u8]) {
        let len = core::cmp::min(raw.len(), MAX_TOKEN_TEXT);
        self.tokens[idx][..len].copy_from_slice(&raw[..len]);
        self.lens[idx] = len as u8;
    }

    fn find_token(&self, needle: &str) -> Option<usize> {
        let n = needle.as_bytes();
        for i in 0..self.n {
            let len = self.lens[i] as usize;
            if len == n.len() && &self.tokens[i][..len] == n {
                return Some(i);
            }
        }
        None
    }

    pub fn unk_id(&self) -> u16 {
        self.unk_idx as u16
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn token_text(&self, idx: usize) -> &str {
        if idx >= self.n {
            return "?";
        }
        let len = self.lens[idx] as usize;
        core::str::from_utf8(&self.tokens[idx][..len]).unwrap_or("?")
    }

    pub fn encode_greedy(&self, prompt: &str, out: &mut [u16], unk_id: u16) -> usize {
        let bytes = prompt.as_bytes();
        let mut i = 0usize;
        let mut n = 0usize;

        while i < bytes.len() && n < out.len() {
            let mut best_id = usize::MAX;
            let mut best_len = 0usize;

            for tid in 0..self.n {
                let tl = self.lens[tid] as usize;
                if tl == 0 || i + tl > bytes.len() {
                    continue;
                }
                if &bytes[i..i + tl] == &self.tokens[tid][..tl] && tl > best_len {
                    best_len = tl;
                    best_id = tid;
                }
            }

            if best_id != usize::MAX {
                out[n] = best_id as u16;
                n += 1;
                i += best_len;
            } else {
                out[n] = unk_id;
                n += 1;
                i += 1;
            }
        }

        n
    }
}

fn aligned_f32_view(blob: &[u8]) -> Option<&[f32]> {
    if blob.len() % 4 != 0 {
        return None;
    }
    let (prefix, f32s, suffix) = unsafe { blob.align_to::<f32>() };
    if !prefix.is_empty() || !suffix.is_empty() {
        return None;
    }
    Some(f32s)
}

fn take_slice<'a>(f: &'a [f32], off: &mut usize, n: usize) -> Option<&'a [f32]> {
    if *off + n > f.len() {
        return None;
    }
    let out = &f[*off..*off + n];
    *off += n;
    Some(out)
}

pub fn load_hardcoded_gpt2_from_blob(blob: &[u8]) -> Option<(GptConfig, GptWeights<'_>)> {
    crate::println!("[gpt.model] parsing hardcoded GPT layout from blob...");
    crate::println!("[gpt.model] blob bytes: {}", blob.len());

    let flat = aligned_f32_view(blob)?;
    crate::println!("[gpt.model] blob floats: {}", flat.len());
    let mut off = 0usize;

    // Exact order must match sorted state_dict keys emitted by converter.
    let lm_head = take_slice(flat, &mut off, GPT2_VOCAB * GPT2_DMODEL)?;

    let mut layers = [LayerWeights::EMPTY; MAX_LAYERS];
    for l in 0..GPT2_LAYERS {
        let c_attn_b = take_slice(flat, &mut off, 3 * GPT2_DMODEL)?;
        let c_attn_w = take_slice(flat, &mut off, GPT2_DMODEL * (3 * GPT2_DMODEL))?;
        let c_proj_b = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let c_proj_w = take_slice(flat, &mut off, GPT2_DMODEL * GPT2_DMODEL)?;
        let ln1_b = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let ln1_g = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let ln2_b = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let ln2_g = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let ff_b1 = take_slice(flat, &mut off, GPT2_FF)?;
        let ff_w1 = take_slice(flat, &mut off, GPT2_DMODEL * GPT2_FF)?;
        let ff_b2 = take_slice(flat, &mut off, GPT2_DMODEL)?;
        let ff_w2 = take_slice(flat, &mut off, GPT2_FF * GPT2_DMODEL)?;

        layers[l] = LayerWeights {
            ln1_g,
            ln1_b,
            c_attn_w,
            c_attn_b,
            c_proj_w,
            c_proj_b,
            ln2_g,
            ln2_b,
            ff_w1,
            ff_b1,
            ff_w2,
            ff_b2,
        };
    }

    let ln_f_b = take_slice(flat, &mut off, GPT2_DMODEL)?;
    let ln_f_g = take_slice(flat, &mut off, GPT2_DMODEL)?;
    let wpe = take_slice(flat, &mut off, GPT2_CONTEXT * GPT2_DMODEL)?;
    let wte = take_slice(flat, &mut off, GPT2_VOCAB * GPT2_DMODEL)?;

    let cfg = GptConfig {
        vocab_size: GPT2_VOCAB,
        context_size: GPT2_CONTEXT,
        d_model: GPT2_DMODEL,
        d_ff: GPT2_FF,
        n_layers: GPT2_LAYERS,
        n_heads: GPT2_HEADS,
    };

    let w = GptWeights {
        lm_head,
        ln_f_g,
        ln_f_b,
        wpe,
        wte,
        layers,
    };

    if !validate_config(&cfg, &w) {
        crate::println!("[gpt.model] validate_config failed");
        return None;
    }

    let consumed_floats = off;
    let consumed_bytes = consumed_floats * core::mem::size_of::<f32>();
    let remain_floats = flat.len().saturating_sub(consumed_floats);
    crate::println!("[gpt.model] parse ok: layers={}, heads={}", cfg.n_layers, cfg.n_heads);
    crate::println!(
        "[gpt.model] consumed={} floats ({} bytes), remaining={} floats",
        consumed_floats,
        consumed_bytes,
        remain_floats
    );

    Some((cfg, w))
}

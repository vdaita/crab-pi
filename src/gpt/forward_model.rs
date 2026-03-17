use super::forward_layer::forward_layer;
use super::layernorm::layer_norm_rows;
use super::model::{validate_config, GptConfig, GptWeights, LM_HEAD_CHUNK, SCRATCH};
use super::ops::GptOps;

pub fn gpt_forward_logits(ops: &mut GptOps<'_>, cfg: &GptConfig, w: &GptWeights<'_>, tokens: &[u16]) -> Option<&'static [f32]> {
    if !validate_config(cfg, w) {
        return None;
    }
    if tokens.is_empty() || tokens.len() > cfg.context_size {
        return None;
    }

    let t = tokens.len();
    let d = cfg.d_model;
    let v = cfg.vocab_size;

    let s = unsafe { &mut *core::ptr::addr_of_mut!(SCRATCH) };

    for pos in 0..t {
        let tok = tokens[pos] as usize;
        if tok >= v {
            return None;
        }
        let xoff = pos * d;
        let teoff = tok * d;
        let peoff = pos * d;
        for c in 0..d {
            s.x[xoff + c] = w.wte[teoff + c] + w.wpe[peoff + c];
        }
    }

    for l in 0..cfg.n_layers {
        forward_layer(ops, cfg, &w.layers[l], s, t);
    }

    layer_norm_rows(&s.x[..t * d], &mut s.x_norm[..t * d], t, d, w.ln_f_g, w.ln_f_b);

    let last = (t - 1) * d;
    let mut start = 0usize;
    while start < v {
        let chunk = core::cmp::min(LM_HEAD_CHUNK, v - start);
        for j in 0..chunk {
            let tok = start + j;
            let row = tok * d;
            for c in 0..d {
                s.lm_head_chunk_t[c * chunk + j] = w.lm_head[row + c];
            }
        }
        ops.matmul(
            &s.x_norm[last..last + d],
            &s.lm_head_chunk_t[..d * chunk],
            &mut s.logits_chunk[..chunk],
            1,
            chunk,
            d,
        );
        s.logits[start..start + chunk].copy_from_slice(&s.logits_chunk[..chunk]);
        start += chunk;
    }

    Some(&s.logits[..v])
}

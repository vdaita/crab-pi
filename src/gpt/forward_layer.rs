use super::attention::causal_self_attention;
use super::layernorm::layer_norm_rows;
use super::model::{gelu_approx, GptConfig, LayerWeights, Scratch};
use super::ops::GptOps;

fn linear_rows(
    ops: &mut GptOps<'_>,
    input: &[f32],
    rows: usize,
    in_dim: usize,
    w: &[f32],
    b: &[f32],
    out_dim: usize,
    out: &mut [f32],
) {
    ops.matmul(input, w, out, rows, out_dim, in_dim);
    for r in 0..rows {
        let off = r * out_dim;
        for c in 0..out_dim {
            out[off + c] += b[c];
        }
    }
}

pub fn forward_layer(ops: &mut GptOps<'_>, cfg: &GptConfig, w: &LayerWeights<'_>, s: &mut Scratch, t: usize) {
    let d = cfg.d_model;
    let ff = cfg.d_ff;
    let three_d = 3 * d;

    layer_norm_rows(&s.x[..t * d], &mut s.x_norm[..t * d], t, d, w.ln1_g, w.ln1_b);
    linear_rows(ops, &s.x_norm[..t * d], t, d, w.c_attn_w, w.c_attn_b, three_d, &mut s.qkv[..t * three_d]);

    for r in 0..t {
        let qb = r * d;
        let qkvb = r * three_d;
        for c in 0..d {
            s.q[qb + c] = s.qkv[qkvb + c];
            s.k[qb + c] = s.qkv[qkvb + d + c];
            s.v[qb + c] = s.qkv[qkvb + 2 * d + c];
        }
    }

    causal_self_attention(
        ops,
        t,
        d,
        cfg.n_heads,
        &s.q[..t * d],
        &s.k[..t * d],
        &s.v[..t * d],
        &mut s.head_q[..t * (d / cfg.n_heads)],
        &mut s.head_k[..t * (d / cfg.n_heads)],
        &mut s.head_v[..t * (d / cfg.n_heads)],
        &mut s.head_ctx[..t * (d / cfg.n_heads)],
        &mut s.k_t[..(d / cfg.n_heads) * t],
        &mut s.att[..t * t],
        &mut s.ctx[..t * d],
    );

    linear_rows(ops, &s.ctx[..t * d], t, d, w.c_proj_w, w.c_proj_b, d, &mut s.proj[..t * d]);
    for i in 0..(t * d) {
        s.x[i] += s.proj[i];
    }

    layer_norm_rows(&s.x[..t * d], &mut s.x_norm[..t * d], t, d, w.ln2_g, w.ln2_b);
    linear_rows(ops, &s.x_norm[..t * d], t, d, w.ff_w1, w.ff_b1, ff, &mut s.ff1[..t * ff]);
    for x in s.ff1[..t * ff].iter_mut() {
        *x = gelu_approx(*x);
    }
    linear_rows(ops, &s.ff1[..t * ff], t, ff, w.ff_w2, w.ff_b2, d, &mut s.ff2[..t * d]);
    for i in 0..(t * d) {
        s.x[i] += s.ff2[i];
    }
}

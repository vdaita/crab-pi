use super::model::inv_sqrt;
use super::ops::GptOps;

pub fn causal_self_attention(
    ops: &mut GptOps<'_>,
    rows: usize,
    d: usize,
    n_heads: usize,
    q: &[f32],
    k: &[f32],
    v: &[f32],
    head_q: &mut [f32],
    head_k: &mut [f32],
    head_v: &mut [f32],
    head_ctx: &mut [f32],
    k_t: &mut [f32],
    att: &mut [f32],
    out: &mut [f32],
) {
    let head_dim = d / n_heads;
    let scale = inv_sqrt(head_dim as f32);

    for i in 0..(rows * d) {
        out[i] = 0.0;
    }

    for h in 0..n_heads {
        for r in 0..rows {
            let base = r * d + h * head_dim;
            let hb = r * head_dim;
            for c in 0..head_dim {
                head_q[hb + c] = q[base + c];
                head_k[hb + c] = k[base + c];
                head_v[hb + c] = v[base + c];
            }
        }

        for r in 0..rows {
            let kb = r * head_dim;
            for c in 0..head_dim {
                k_t[c * rows + r] = head_k[kb + c];
            }
        }

        ops.matmul(head_q, k_t, att, rows, rows, head_dim);

        for i in 0..rows {
            let row_off = i * rows;
            let mut max_score = -1e30f32;
            for j in 0..=i {
                let s = att[row_off + j] * scale;
                att[row_off + j] = s;
                if s > max_score {
                    max_score = s;
                }
            }

            ops.softmax_causal_inplace(att, rows, i, max_score);
        }

        ops.matmul(att, head_v, head_ctx, rows, head_dim, rows);

        for r in 0..rows {
            let out_base = r * d + h * head_dim;
            let hb = r * head_dim;
            for c in 0..head_dim {
                out[out_base + c] = head_ctx[hb + c];
            }
        }
    }
}

use crate::gpt::linear::Linear;
use crate::gpu::GpuKernel;
use libm::{expf, sqrtf};

pub fn causal_attention_step(
	q: &[f32],
	cache_k: &[f32],
	cache_v: &[f32],
	cache_len: usize,
	n_head: usize,
	n_embd: usize,
	ctx_out: &mut [f32],
) {
	assert!(q.len() == n_embd, "attention q shape mismatch");
	assert!(ctx_out.len() == n_embd, "attention ctx shape mismatch");
	assert!(n_embd % n_head == 0, "n_embd must be divisible by n_head");
	assert!(cache_k.len() >= (cache_len + 1) * n_embd, "cache_k too small");
	assert!(cache_v.len() >= (cache_len + 1) * n_embd, "cache_v too small");

	let head_dim = n_embd / n_head;
	let scale = 1.0 / sqrtf(head_dim as f32);

	for h in 0..n_head {
		let qh = &q[h * head_dim..(h + 1) * head_dim];

		let mut max_logit = f32::NEG_INFINITY;
		for t in 0..=cache_len {
			let kh = &cache_k[t * n_embd + h * head_dim..t * n_embd + (h + 1) * head_dim];
			let mut dot = 0.0;
			for d in 0..head_dim {
				dot += qh[d] * kh[d];
			}
			let logit = dot * scale;
			if logit > max_logit {
				max_logit = logit;
			}
		}

		let mut denom = 0.0;
		for t in 0..=cache_len {
			let kh = &cache_k[t * n_embd + h * head_dim..t * n_embd + (h + 1) * head_dim];
			let mut dot = 0.0;
			for d in 0..head_dim {
				dot += qh[d] * kh[d];
			}
			denom += expf(dot * scale - max_logit);
		}

		for d in 0..head_dim {
			let mut acc = 0.0;
			for t in 0..=cache_len {
				let kh = &cache_k[t * n_embd + h * head_dim..t * n_embd + (h + 1) * head_dim];
				let vh = &cache_v[t * n_embd + h * head_dim..t * n_embd + (h + 1) * head_dim];
				let mut dot = 0.0;
				for dd in 0..head_dim {
					dot += qh[dd] * kh[dd];
				}
				let p = expf(dot * scale - max_logit) / denom;
				acc += p * vh[d];
			}
			ctx_out[h * head_dim + d] = acc;
		}
	}
}

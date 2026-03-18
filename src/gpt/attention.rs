use crate::gpt::linear::Linear;
use crate::gpu::GpuKernel;
use libm::{expf, sqrtf};

pub struct AttentionScratch<'a> {
	pub q: &'a mut [f32],
	pub k: &'a mut [f32],
	pub v: &'a mut [f32],
	pub ctx: &'a mut [f32],
}

pub struct CausalSelfAttention<'a> {
	q_proj: Linear<'a>,
	k_proj: Linear<'a>,
	v_proj: Linear<'a>,
	o_proj: Linear<'a>,
	n_head: usize,
	n_embd: usize,
}

impl<'a> CausalSelfAttention<'a> {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		q_w: &'a [f32],
		q_b: Option<&'a [f32]>,
		k_w: &'a [f32],
		k_b: Option<&'a [f32]>,
		v_w: &'a [f32],
		v_b: Option<&'a [f32]>,
		o_w: &'a [f32],
		o_b: Option<&'a [f32]>,
		n_embd: usize,
		n_head: usize,
	) -> Self {
		assert!(n_embd % n_head == 0, "n_embd must be divisible by n_head");
		Self {
			q_proj: Linear::new(q_w, q_b, n_embd, n_embd),
			k_proj: Linear::new(k_w, k_b, n_embd, n_embd),
			v_proj: Linear::new(v_w, v_b, n_embd, n_embd),
			o_proj: Linear::new(o_w, o_b, n_embd, n_embd),
			n_head,
			n_embd,
		}
	}

	pub fn forward(
		&self,
		gpu: &mut GpuKernel,
		x: &[f32],
		out: &mut [f32],
		seq_len: usize,
		scratch: &mut AttentionScratch,
	) {
		assert!(x.len() == seq_len * self.n_embd, "attention input shape mismatch");
		assert!(out.len() == seq_len * self.n_embd, "attention output shape mismatch");
		assert!(scratch.q.len() == x.len(), "attention scratch q shape mismatch");
		assert!(scratch.k.len() == x.len(), "attention scratch k shape mismatch");
		assert!(scratch.v.len() == x.len(), "attention scratch v shape mismatch");
		assert!(scratch.ctx.len() == x.len(), "attention scratch ctx shape mismatch");

		self.q_proj.forward(gpu, x, scratch.q, seq_len);
		self.k_proj.forward(gpu, x, scratch.k, seq_len);
		self.v_proj.forward(gpu, x, scratch.v, seq_len);

		let head_dim = self.n_embd / self.n_head;
		let scale = 1.0 / sqrtf(head_dim as f32);

		for h in 0..self.n_head {
			for t in 0..seq_len {
				let mut max_logit = f32::NEG_INFINITY;

				for tp in 0..=t {
					let mut dot = 0.0;
					for d in 0..head_dim {
						let q_idx = t * self.n_embd + h * head_dim + d;
						let k_idx = tp * self.n_embd + h * head_dim + d;
						dot += scratch.q[q_idx] * scratch.k[k_idx];
					}
					let logit = dot * scale;
					if logit > max_logit {
						max_logit = logit;
					}
				}

				let mut denom = 0.0;
				for tp in 0..=t {
					let mut dot = 0.0;
					for d in 0..head_dim {
						let q_idx = t * self.n_embd + h * head_dim + d;
						let k_idx = tp * self.n_embd + h * head_dim + d;
						dot += scratch.q[q_idx] * scratch.k[k_idx];
					}
					denom += expf(dot * scale - max_logit);
				}

				for d in 0..head_dim {
					let mut accum = 0.0;
					for tp in 0..=t {
						let mut dot = 0.0;
						for dd in 0..head_dim {
							let q_idx = t * self.n_embd + h * head_dim + dd;
							let k_idx = tp * self.n_embd + h * head_dim + dd;
							dot += scratch.q[q_idx] * scratch.k[k_idx];
						}
						let w = expf(dot * scale - max_logit) / denom;
						let v_idx = tp * self.n_embd + h * head_dim + d;
						accum += w * scratch.v[v_idx];
					}
					scratch.ctx[t * self.n_embd + h * head_dim + d] = accum;
				}
			}
		}

		self.o_proj.forward(gpu, scratch.ctx, out, seq_len);
	}
}

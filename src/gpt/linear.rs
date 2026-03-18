use crate::gpu::GpuKernel;
use crate::matmul::matmul_with_gpu;

pub struct Linear<'a> {
	pub weight: &'a [f32],
	pub bias: Option<&'a [f32]>,
	pub in_dim: usize,
	pub out_dim: usize,
}

impl<'a> Linear<'a> {
	pub fn new(weight: &'a [f32], bias: Option<&'a [f32]>, in_dim: usize, out_dim: usize) -> Self {
		assert!(weight.len() == in_dim * out_dim, "Linear weight shape mismatch");
		if let Some(b) = bias {
			assert!(b.len() == out_dim, "Linear bias len must equal out_dim");
		}
		Self {
			weight,
			bias,
			in_dim,
			out_dim,
		}
	}

	pub fn forward(&self, gpu: &mut GpuKernel, input: &[f32], output: &mut [f32], rows: usize) {
		assert!(input.len() == rows * self.in_dim, "Linear input shape mismatch");
		assert!(output.len() == rows * self.out_dim, "Linear output shape mismatch");

		matmul_with_gpu(gpu, input, self.weight, output, rows, self.out_dim, self.in_dim);

		if let Some(bias) = self.bias {
			for r in 0..rows {
				let base = r * self.out_dim;
				for c in 0..self.out_dim {
					output[base + c] += bias[c];
				}
			}
		}
	}
}

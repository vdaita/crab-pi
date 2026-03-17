use crate::gpu::GpuKernel;
use crate::matmul::matmul_with_gpu;

pub struct GptOps<'a> {
    gpu: &'a mut GpuKernel,
}

impl<'a> GptOps<'a> {
    pub fn new(gpu: &'a mut GpuKernel) -> Self {
        Self { gpu }
    }

    pub fn matmul(&mut self, a: &[f32], b: &[f32], out: &mut [f32], m: usize, n: usize, k: usize) {
        matmul_with_gpu(self.gpu, a, b, out, m, n, k);
    }

    // Hook point: swap this with a QPU exp kernel implementation when available.
    pub fn exp_approx(&mut self, x: f32) -> f32 {
        let y = 1.0 + x + 0.5 * x * x + (x * x * x) * (1.0 / 6.0);
        if y > 0.0 { y } else { 0.0 }
    }

    pub fn softmax_causal_inplace(&mut self, att: &mut [f32], rows: usize, row: usize, max_score: f32) {
        let row_off = row * rows;
        let mut denom = 0.0f32;
        for j in 0..=row {
            let w = self.exp_approx(att[row_off + j] - max_score);
            att[row_off + j] = w;
            denom += w;
        }
        if denom < 1e-12 {
            denom = 1.0;
        }
        let inv = 1.0 / denom;
        for j in 0..=row {
            att[row_off + j] *= inv;
        }
        for j in (row + 1)..rows {
            att[row_off + j] = 0.0;
        }
    }
}

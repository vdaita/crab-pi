pub struct LayerNorm {
    weight: &'static [f32],
    bias: &'static [f32],
    eps: f32,
}

impl LayerNorm {
    pub fn new(weight: &'static [f32], bias: &'static [f32], eps: f32) -> Self {
        LayerNorm { weight, bias, eps }
    }

    pub fn forward(&self, x: &[f32], output: &mut [f32], dim: usize) {
        for i in (0..x.len()).step_by(dim) {
            let chunk = &x[i..i + dim];
            
            let mean: f32 = chunk.iter().sum::<f32>() / dim as f32;
            let var: f32 = chunk
                .iter()
                .map(|&v| (v - mean).powi(2))
                .sum::<f32>()
                / dim as f32;
            
            let inv_std = 1.0 / (var + self.eps).sqrt();
            for j in 0..dim {
                output[i + j] = (chunk[j] - mean) * inv_std * self.weight[j] + self.bias[j];
            }
        }
    }
}

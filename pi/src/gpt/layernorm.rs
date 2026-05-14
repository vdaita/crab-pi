use libm::sqrtf;

pub struct LayerNorm<'a> {
    weight: &'a [f32],
    bias: Option<&'a [f32]>,
    eps: f32,
}

impl<'a> LayerNorm<'a> {
    pub fn new(weight: &'a [f32], bias: Option<&'a [f32]>, eps: f32) -> Self {
        LayerNorm { weight, bias, eps }
    }

    pub fn forward(&self, x: &[f32], output: &mut [f32], dim: usize) {
        assert!(dim > 0, "LayerNorm dim must be > 0");
        assert!(x.len() == output.len(), "LayerNorm input/output len mismatch");
        assert!(x.len() % dim == 0, "LayerNorm input len must be divisible by dim");
        assert!(self.weight.len() == dim, "LayerNorm weight len must equal dim");
        if let Some(bias) = self.bias {
            assert!(bias.len() == dim, "LayerNorm bias len must equal dim");
        }

        for i in (0..x.len()).step_by(dim) {
            let chunk = &x[i..i + dim];

            let mean: f32 = chunk.iter().sum::<f32>() / dim as f32;
            let var: f32 = chunk
                .iter()
                .map(|&v| {
                    let d = v - mean;
                    d * d
                })
                .sum::<f32>()
                / dim as f32;

            let inv_std = 1.0 / sqrtf(var + self.eps);
            for j in 0..dim {
                let b = self.bias.map_or(0.0, |bias| bias[j]);
                output[i + j] = (chunk[j] - mean) * inv_std * self.weight[j] + b;
            }
        }
    }
}

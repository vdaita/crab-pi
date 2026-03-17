use super::model::inv_sqrt;

pub fn layer_norm_rows(input: &[f32], output: &mut [f32], rows: usize, cols: usize, g: &[f32], b: &[f32]) {
    for r in 0..rows {
        let row_off = r * cols;
        let mut mean = 0.0f32;
        for c in 0..cols {
            mean += input[row_off + c];
        }
        mean /= cols as f32;

        let mut var = 0.0f32;
        for c in 0..cols {
            let d = input[row_off + c] - mean;
            var += d * d;
        }
        var /= cols as f32;
        let inv = inv_sqrt(var + 1e-5);

        for c in 0..cols {
            let n = (input[row_off + c] - mean) * inv;
            output[row_off + c] = n * g[c] + b[c];
        }
    }
}

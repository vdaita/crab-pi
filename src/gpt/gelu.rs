use libm::tanhf;

// Fast tanh-based GELU approximation used by GPT-style MLP blocks.
pub fn gelu_tanh(x: f32) -> f32 {
	let c = 0.044715;
	let k = 0.7978846; // sqrt(2/pi)
	let x3 = x * x * x;
	0.5 * x * (1.0 + tanhf(k * (x + c * x3)))
}

pub fn gelu_in_place(x: &mut [f32]) {
	for v in x.iter_mut() {
		*v = gelu_tanh(*v);
	}
}

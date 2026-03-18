pub struct Embedding<'a> {
	table: &'a [f32],
	n_rows: usize,
	dim: usize,
}

impl<'a> Embedding<'a> {
	pub fn new(table: &'a [f32], n_rows: usize, dim: usize) -> Self {
		assert!(table.len() == n_rows * dim, "Embedding table shape mismatch");
		Self { table, n_rows, dim }
	}

	pub fn dim(&self) -> usize {
		self.dim
	}

	pub fn lookup(&self, idx: usize, out: &mut [f32]) {
		assert!(idx < self.n_rows, "Embedding index out of range");
		assert!(out.len() == self.dim, "Embedding output row len mismatch");
		let start = idx * self.dim;
		out.copy_from_slice(&self.table[start..start + self.dim]);
	}

	pub fn forward(&self, ids: &[usize], out: &mut [f32]) {
		assert!(out.len() == ids.len() * self.dim, "Embedding output shape mismatch");
		for (t, &id) in ids.iter().enumerate() {
			self.lookup(id, &mut out[t * self.dim..(t + 1) * self.dim]);
		}
	}
}

pub fn add_in_place(dst: &mut [f32], rhs: &[f32]) {
	assert!(dst.len() == rhs.len(), "add_in_place shape mismatch");
	for i in 0..dst.len() {
		dst[i] += rhs[i];
	}
}

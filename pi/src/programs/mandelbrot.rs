use crate::gpu::{self, GPU_BASE, GpuKernel};
use crate::{print, println};

// Must match your GPU shader's expectations
pub const RESOLUTION: usize = 4;
pub const MAX_ITERS: u32 = 16;
pub const NUM_QPUS: usize = 1; // adjust to however many QPU cores you're using

// Reciprocal helper — mirrors hex_recip / float_recip from C
fn float_recip(x: f32) -> f32 {
    1.0f32 / x
}

pub fn mandelbrot() {
    unsafe {
        // ── Initialize GPU ──────────────────────────────────────────────────
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(gpu::MANDELBROT_GPU_CODE);

        // ── Set uniforms for each QPU core ──────────────────────────────────
        // Slot layout must match what the shader expects:
        //   unif[i][0] = RESOLUTION
        //   unif[i][1] = 1.0 / RESOLUTION  (as raw f32 bits)
        //   unif[i][2] = MAX_ITERS
        //   unif[i][3] = NUM_QPUS
        //   unif[i][4] = core index i
        //   unif[i][5] = GPU bus address of output buffer (data slot 0)
        let recip_bits = float_recip(RESOLUTION as f32).to_bits();
        let output_gpu_addr = gpu.get_data_ptr(0); 

        for i in 0..NUM_QPUS {
            gpu.unif[i][0] = RESOLUTION as u32;
            gpu.unif[i][1] = recip_bits;
            gpu.unif[i][2] = MAX_ITERS;
            gpu.unif[i][3] = NUM_QPUS as u32;
            gpu.unif[i][4] = i as u32;
            gpu.unif[i][5] = output_gpu_addr;
            gpu.unif_ptr[i] = gpu.get_unif_ptr(i);
        }

        let total_pixels = (2 * RESOLUTION) * (2 * RESOLUTION);
        assert!(
            total_pixels <= crate::gpu::MAX_DATA_SIZE,
            "Output too large for data slot 0"
        );
        for px in 0..total_pixels {
            gpu.data[0][px] = 0;
        }

        println!("Running Mandelbrot on GPU...");
        gpu.execute(NUM_QPUS as u32);
        println!("GPU done!");

        println!("Running Mandelbrot on CPU...");
        let recip = float_recip(RESOLUTION as f32);
        assert!(
            total_pixels <= crate::gpu::MAX_DATA_SIZE,
            "CPU output too large for data slot 1"
        );
        for i in 0..(2 * RESOLUTION) {
            let y = -1.0f32 + recip * i as f32;
            for j in 0..(2 * RESOLUTION) {
                let x = -1.0f32 + recip * j as f32;
                let mut u = 0.0f32;
                let mut v = 0.0f32;
                let mut u2 = u * u;
                let mut v2 = v * v;
                let mut k = 1u32;
                while k < MAX_ITERS && (u2 + v2 < 4.0) {
                    v = 2.0 * u * v + y;
                    u = u2 - v2 + x;
                    u2 = u * u;
                    v2 = v * v;
                    k += 1;
                }
                let pixel = if k >= MAX_ITERS { 1u32 } else { 0u32 };
                gpu.data[1][i * 2 * RESOLUTION + j] = pixel;
            }
        }
        println!("CPU done!");

        let mut mismatches = 0usize;
        for px in 0..total_pixels {
            let gpu_val = gpu.data[0][px];
            let cpu_val = gpu.data[1][px];
            if gpu_val != cpu_val {
                mismatches += 1;
                if mismatches <= 8 {
                    // Print only the first few errors to avoid flooding
                    let i = px / (2 * RESOLUTION);
                    let j = px % (2 * RESOLUTION);
                    println!(
                        "MISMATCH at [{i}][{j}]: gpu={gpu_val}, cpu={cpu_val}"
                    );
                }
            }
        }
        if mismatches == 0 {
            println!("All {} pixels match!", total_pixels);
        } else {
            println!("{} mismatches out of {} pixels", mismatches, total_pixels);
        }

        gpu.release();
        println!("GPU released.");
    }
}
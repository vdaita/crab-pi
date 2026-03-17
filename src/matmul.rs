use crate::gpu::{GpuKernel, DMA_TEST_CODE, MAX_DATA_SIZE, MAX_VC_CORES};
use crate::timer::Timer;
use crate::{print, println};

fn pad16(x: usize) -> usize {
    ((x + 15) / 16) * 16
}

fn zero_pad_matrix(src: &[f32], dst: &mut [f32], rows: usize, cols: usize, padded_cols: usize) {
    for v in dst.iter_mut() {
        *v = 0.0;
    }

    for r in 0..rows {
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(r * cols),
                dst.as_mut_ptr().add(r * padded_cols),
                cols,
            );
        }
    }
}

fn copy_from_padded(src: &[f32], dst: &mut [f32], rows: usize, cols: usize, padded_cols: usize) {
    for r in 0..rows {
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(r * padded_cols),
                dst.as_mut_ptr().add(r * cols),
                cols,
            );
        }
    }
}

pub fn print_matrix(data: &[u32], n: usize, m: usize) {
    for i in 0..n {
        for j in 0..m {
            print!("{} ", data[i * m + j]);
        }
        print!("\n");
    }
}

pub fn print_float_matrix(data: &[f32], n: usize, m: usize) {
    for i in 0..n {
        for j in 0..m {
            print!("{} ", data[i * m + j]);
        }
        print!("\n");
    }
}

pub fn cpu_matmul(a: &[f32], b: &[f32], c: &mut [f32], m: usize, n: usize, k: usize) {
    for i in 0..m {
        for j in 0..n {
            let mut sum: f32 = 0.0;
            for kk in 0..k {
                sum += a[i * k + kk] * b[kk * n + j];
            }
            c[i * n + j] = sum;
        }
    }
}

pub fn matmul_with_gpu(gpu: &mut GpuKernel, a: &[f32], b: &[f32], out: &mut [f32], m: usize, n: usize, k: usize) {
    let m16 = pad16(m);
    let n16 = pad16(n);
    let k16 = pad16(k);

    let a_elems = m16 * k16;
    let b_elems = k16 * n16;
    let c_elems = m16 * n16;

    if a_elems > MAX_DATA_SIZE || b_elems > MAX_DATA_SIZE || c_elems > MAX_DATA_SIZE {
        panic!("matmul dimensions exceed GPU slot capacity after padding");
    }

    {
        let a_slot = gpu.data_slot_as_mut_f32(0);
        zero_pad_matrix(a, &mut a_slot[..a_elems], m, k, k16);
    }
    {
        let b_slot = gpu.data_slot_as_mut_f32(1);
        zero_pad_matrix(b, &mut b_slot[..b_elems], k, n, n16);
    }
    {
        let c_slot = gpu.data_slot_as_mut_f32(2);
        for v in c_slot[..c_elems].iter_mut() {
            *v = 0.0;
        }
    }

    let num_m_tiles = m16 / 16;
    let num_n_tiles = n16 / 16;
    let num_k_tiles = k16 / 16;

    let launch_cores = core::cmp::min(MAX_VC_CORES, num_n_tiles);
    if launch_cores == 0 {
        return;
    }
    let base_cols_per_core = num_n_tiles / launch_cores;
    let extra_cols = num_n_tiles % launch_cores;

    let mut start_col_tile = 0usize;
    for core in 0..launch_cores {
        let local_col_tiles = base_cols_per_core + if core < extra_cols { 1 } else { 0 };

        gpu.unif[core][0] = unsafe { gpu.get_data_ptr(0) };
        gpu.unif[core][1] = unsafe { gpu.get_data_ptr(1) };
        gpu.unif[core][2] = unsafe { gpu.get_data_ptr(2) };

        // ra3-ra5: mutable counters. ra3 is this core's local column-tile count.
        gpu.unif[core][3] = local_col_tiles as u32;
        gpu.unif[core][4] = 0;
        gpu.unif[core][5] = 0;

        // ra6-ra8: immutable tile counts.
        gpu.unif[core][6] = num_m_tiles as u32;
        gpu.unif[core][7] = num_n_tiles as u32;
        gpu.unif[core][8] = num_k_tiles as u32;

        // ra11: this core's starting B/C column tile.
        gpu.unif[core][9] = start_col_tile as u32;

        start_col_tile += local_col_tiles;
    }

    unsafe {
        gpu.execute(launch_cores as u32);
    }

    {
        let c_slot = gpu.data_slot_as_f32(2);
        copy_from_padded(&c_slot[..c_elems], out, m, n, n16);
    }
}

pub fn matmul(a: &[f32], b: &[f32], out: &mut [f32], m: usize, n: usize, k: usize) {
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);
        matmul_with_gpu(gpu, a, b, out, m, n, k);
        gpu.release();
    }
}

fn run_matmul_case(
    gpu: &mut GpuKernel,
    label: &str,
    m: usize,
    n: usize,
    k: usize,
    a: &[f32],
    b: &[f32],
    c: &mut [f32],
    c_cpu: &mut [f32],
) -> bool {
    for v in c.iter_mut() {
        *v = 0.0;
    }
    for v in c_cpu.iter_mut() {
        *v = 0.0;
    }

    let start_matmul = Timer::get_usec();
    matmul_with_gpu(gpu, a, b, c, m, n, k);
    let matmul_time = Timer::get_usec() - start_matmul;

    let start_cpu_matmul = Timer::get_usec();
    cpu_matmul(a, b, c_cpu, m, n, k);
    let cpu_matmul_time = Timer::get_usec() - start_cpu_matmul;

    if matmul_time == 0 {
        println!("Error! matmul_time = 0");
    } else {
        let speedup_milli = ((cpu_matmul_time as u64) * 1000) / (matmul_time as u64);
        let speedup_int = speedup_milli / 1000;
        let speedup_frac = speedup_milli % 1000;
        println!(
            "[{}] shape {}x{} * {}x{} -> {}x{}, GPU: {}, CPU: {}, speedup (CPU/GPU): {}.{:03}x",
            label,
            m,
            k,
            k,
            n,
            m,
            n,
            matmul_time,
            cpu_matmul_time,
            speedup_int,
            speedup_frac
        );
    }

    for i in 0..(m * n) {
        if (c[i] - c_cpu[i]).abs() > 1e-3 {
            println!(
                "[{}] mismatch at idx {}: expected {}, observed {}",
                label,
                i,
                c_cpu[i],
                c[i]
            );
            return false;
        }
    }

    println!("[{}] Matrix outputs match: YES", label);
    true
}



pub fn matmul_func_test() {
    let mut all_passed = true;
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);

        {
            const M: usize = 16;
            const N: usize = 16;
            const K: usize = 16;
            let a: [f32; M * K] = core::array::from_fn(|i| ((i * 13 + 7) % 23) as f32 * 0.25);
            let b: [f32; K * N] = core::array::from_fn(|i| ((i * 17 + 3) % 19) as f32 * 0.25);
            let mut c: [f32; M * N] = [0.0; M * N];
            let mut c_cpu: [f32; M * N] = [0.0; M * N];
            all_passed = run_matmul_case(gpu, "square-16", M, N, K, &a, &b, &mut c, &mut c_cpu) && all_passed;
        }

        {
            const M: usize = 32;
            const N: usize = 256;
            const K: usize = 64;
            let a: [f32; M * K] = core::array::from_fn(|i| ((i * 13 + 7) % 23) as f32 * 0.25);
            let b: [f32; K * N] = core::array::from_fn(|i| ((i * 17 + 3) % 19) as f32 * 0.25);
            let mut c: [f32; M * N] = [0.0; M * N];
            let mut c_cpu: [f32; M * N] = [0.0; M * N];
            all_passed = run_matmul_case(gpu, "wide-n-multi-qpu", M, N, K, &a, &b, &mut c, &mut c_cpu) && all_passed;
        }

        {
            const M: usize = 31;
            const N: usize = 47;
            const K: usize = 19;
            let a: [f32; M * K] = core::array::from_fn(|i| ((i * 13 + 7) % 23) as f32 * 0.25);
            let b: [f32; K * N] = core::array::from_fn(|i| ((i * 17 + 3) % 19) as f32 * 0.25);
            let mut c: [f32; M * N] = [0.0; M * N];
            let mut c_cpu: [f32; M * N] = [0.0; M * N];
            all_passed = run_matmul_case(gpu, "non-multiple-of-16", M, N, K, &a, &b, &mut c, &mut c_cpu) && all_passed;
        }

        {
            const M: usize = 48;
            const N: usize = 96;
            const K: usize = 32;
            let a: [f32; M * K] = core::array::from_fn(|i| ((i * 13 + 7) % 23) as f32 * 0.25);
            let b: [f32; K * N] = core::array::from_fn(|i| ((i * 17 + 3) % 19) as f32 * 0.25);
            let mut c: [f32; M * N] = [0.0; M * N];
            let mut c_cpu: [f32; M * N] = [0.0; M * N];
            all_passed = run_matmul_case(gpu, "rectangular", M, N, K, &a, &b, &mut c, &mut c_cpu) && all_passed;
        }

        {
            const M: usize = 7;
            const N: usize = 13;
            const K: usize = 5;
            let a: [f32; M * K] = core::array::from_fn(|i| ((i * 13 + 7) % 23) as f32 * 0.25);
            let b: [f32; K * N] = core::array::from_fn(|i| ((i * 17 + 3) % 19) as f32 * 0.25);
            let mut c: [f32; M * N] = [0.0; M * N];
            let mut c_cpu: [f32; M * N] = [0.0; M * N];
            all_passed = run_matmul_case(gpu, "tiny", M, N, K, &a, &b, &mut c, &mut c_cpu) && all_passed;
        }

        gpu.release();
    }

    if all_passed {
        println!("matmul test suite: ALL PASSED");
    } else {
        println!("matmul test suite: FAILED");
    }
}
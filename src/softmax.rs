use crate::gpu::{GpuKernel, SOFTMAX_GPU_CODE, MAX_DATA_SIZE};
use crate::matmul::print_float_matrix;
use crate::timer::Timer;
use crate::println;
use libm::expf;

fn pad_to_256(data: &mut [f32]) {
    for i in data.len()..256 {
        data[i] = f32::NEG_INFINITY;
    }
}

pub fn cpu_softmax(a: &[f32], cpu_out: &mut [f32]) {
    assert!(a.len() == cpu_out.len(), "cpu_softmax: input/output length mismatch");
    if a.is_empty() {
        return;
    }

    let mut max_val = a[0];
    for &v in a.iter().skip(1) {
        if v > max_val {
            max_val = v;
        }
    }

    let mut sum = 0.0f32;
    for i in 0..a.len() {
        let e = expf(a[i] - max_val);
        cpu_out[i] = e;
        sum += e;
    }

    if sum == 0.0 {
        return;
    }
    let inv_sum = 1.0 / sum;
    for v in cpu_out.iter_mut() {
        *v *= inv_sum;
    }

}

pub fn softmax_with_gpu(gpu: &mut GpuKernel, a: &mut [f32], n: usize) {
    assert!(n <= a.len(), "softmax_with_gpu: n exceeds slice length");
    assert!(n <= 256, "softmax_with_gpu currently supports up to 256 elements");
    assert!(256 <= MAX_DATA_SIZE, "GPU data slot is unexpectedly too small");

    {
        let slot = gpu.data_slot_as_mut_f32(0);
        for v in slot[..256].iter_mut() {
            *v = f32::NEG_INFINITY;
        }
        slot[..n].copy_from_slice(&a[..n]);
    }

    unsafe { gpu.load_code(SOFTMAX_GPU_CODE); }
    let launch_cores = 1;
    let core = 0usize;
    gpu.unif[core][0] = unsafe { gpu.get_data_ptr(0) };

    unsafe {
        gpu.execute(launch_cores as u32);
    }

    {
        let slot = gpu.data_slot_as_f32(0);
        a[..n].copy_from_slice(&slot[..n]);
    }
}

fn run_softmax_case (
    gpu: &mut GpuKernel,
    label: &str,
    input: &[f32],
) -> bool {
    const MAX_SOFTMAX_N: usize = 256;
    if input.is_empty() || input.len() > MAX_SOFTMAX_N {
        println!("[{}] invalid input length: {}", label, input.len());
        return false;
    }

    let mut gpu_out = [f32::NEG_INFINITY; MAX_SOFTMAX_N];
    let mut cpu_out = [0.0f32; MAX_SOFTMAX_N];

    gpu_out[..input.len()].copy_from_slice(input);

    let start_cpu = Timer::get_usec();
    cpu_softmax(input, &mut cpu_out[..input.len()]);
    let cpu_time = Timer::get_usec() - start_cpu;

    let start_gpu = Timer::get_usec();
    softmax_with_gpu(gpu, &mut gpu_out[..input.len()], input.len());
    let gpu_time = Timer::get_usec() - start_gpu;

    println!(
        "[{}] n={}, GPU: {} usec, CPU: {} usec",
        label,
        input.len(),
        gpu_time,
        cpu_time
    );

    let mut sum_gpu = 0.0f32;
    let mut sum_cpu = 0.0f32;

    for i in 0..input.len() {
        let g = gpu_out[i];
        let c = cpu_out[i];
        sum_gpu += g;
        sum_cpu += c;

        if (g - c).abs() > 1e-3 {
            println!(
                "[{}] mismatch at idx {}: expected {}, observed {}",
                label,
                i,
                c,
                g
            );
            return false;
        }
    }



    if (sum_gpu - 1.0).abs() > 2e-2 {
        println!("[{}] GPU softmax sum check failed: {}", label, sum_gpu);
        return false;
    }
    if (sum_cpu - 1.0).abs() > 2e-3 {
        println!("[{}] CPU softmax sum check failed: {}", label, sum_cpu);
        return false;
    }

    println!("[{}] softmax outputs match: YES", label);
    true

}

pub fn softmax_func_test() {
    let mut all_passed = true;

    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;

        {
            const N: usize = 16;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 7 + 5) % 17) as f32 * 0.25 - 1.0);
            all_passed = run_softmax_case(gpu, "small-16", &input) && all_passed;
        }

        {
            const N: usize = 64;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 13 + 3) % 23) as f32 * 0.125 - 1.5);
            all_passed = run_softmax_case(gpu, "mid-64", &input) && all_passed;
        }

        {
            const N: usize = 256;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 29 + 11) % 97) as f32 * 0.05 - 2.0);
            all_passed = run_softmax_case(gpu, "full-256", &input) && all_passed;
        }

        {
            const N: usize = 37;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 19 + 7) % 31) as f32 * 0.2 - 3.0);
            all_passed = run_softmax_case(gpu, "non-power-2", &input) && all_passed;
        }

        gpu.release();
    }

    if all_passed {
        println!("softmax test suite: ALL PASSED");
    } else {
        println!("softmax test suite: FAILED");
    }
}
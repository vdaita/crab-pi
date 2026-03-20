use crate::gpu::{GpuKernel, SOFTMAX_GPU_CODE, EXP_GPU_CODE, MAX_DATA_SIZE};
use crate::matmul::print_float_matrix;
use crate::timer::Timer;
use crate::println;
use libm::expf;

fn pad_to_256(data: &mut [f32]) {
    for i in data.len()..256 {
        data[i] = f32::NEG_INFINITY;
    }
}

pub fn cpu_exp(a: &[f32], cpu_out: &mut [f32]) {
    for i in 0..a.len() {
        cpu_out[i] = expf(a[i]);
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

    // println!("Max value, CPU softmax calculation: {}", max_val);

    let mut sum = 0.0f32;
    for i in 0..a.len() {
        let e = expf(a[i] - max_val);
        cpu_out[i] = e;
        sum += e;
    }

    // println!("Intermediate CPU expf result:\n");
    // print_float_matrix(cpu_out, 1, cpu_out.len());

    // println!("Sum value, CPU softmax calculation: {}", sum);

    if sum == 0.0 {
        return;
    }
    let inv_sum = 1.0 / sum;

   // println!("Inverse sum in CPU softmax: {}", inv_sum);
    
    for v in cpu_out.iter_mut() {
        *v *= inv_sum;
    }

}

pub fn exp_with_gpu(gpu: &mut GpuKernel, a: &mut [f32], n: usize) {
    assert!(n <= a.len(), "exp_with_gpu: n exceeds slice length");
    assert!(n <= 256, "exp_with_gpu currently supports up to 256 elements");
    assert!(256 <= MAX_DATA_SIZE, "GPU data slot is unexpectedly too small");

    {
        let slot = gpu.data_slot_as_mut_f32(0);
        for v in slot[..256].iter_mut() {
            *v = f32::NEG_INFINITY;
        }
        slot[..n].copy_from_slice(&a[..n]);
    }

    unsafe { gpu.load_code(EXP_GPU_CODE); }
    let launch_cores = 1;
    let core = 0usize;
    gpu.unif[core][0] = unsafe { gpu.get_data_ptr(0) };

    unsafe {
        gpu.execute(launch_cores as u32);
    }

    println!("Finished executing Exp successfully!");

    {
        let slot = gpu.data_slot_as_f32(0);
        a[..n].copy_from_slice(&slot[..n]);
    }
}

pub fn softmax_with_gpu(gpu: &mut GpuKernel, a: &mut [f32], n: usize) {
    assert!(n <= a.len(), "exp_with_gpu: n exceeds slice length");
    assert!(n <= 256, "exp_with_gpu currently supports up to 256 elements");
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

    println!("Finished executing Exp successfully!");

    {
        let slot = gpu.data_slot_as_f32(0);
        a[..n].copy_from_slice(&slot[..n]);
    }
}


fn run_exp_case (
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
    cpu_exp(input, &mut cpu_out[..input.len()]);
    let cpu_time = Timer::get_usec() - start_cpu;

    let start_gpu = Timer::get_usec();
    exp_with_gpu(gpu, &mut gpu_out[..input.len()], input.len());
    let gpu_time = Timer::get_usec() - start_gpu;

    // println!("Input: ");
    // print_float_matrix(input, 1, input.len());
    // println!("CPU output:");
    // print_float_matrix(&mut cpu_out[..input.len()], 1, input.len());
    // println!("GPU output:");
    // print_float_matrix(&mut gpu_out[..input.len()], 1, input.len());

    println!(
        "[{}] n={}, GPU: {} usec, CPU: {} usec",
        label,
        input.len(),
        gpu_time,
        cpu_time
    );

    for i in 0..input.len() {
        let g = gpu_out[i];
        let c = cpu_out[i];
        if (g - c).abs() > 1e-2 {
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

    println!("[{}] exp outputs match: YES", label);
    true

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

    // println!("Input: ");
    // print_float_matrix(input, 1, input.len());
    // println!("CPU output:");
    // print_float_matrix(&mut cpu_out[..input.len()], 1, input.len());
    // println!("GPU output:");
    // print_float_matrix(&mut gpu_out[..input.len()], 1, input.len());

    println!(
        "[{}] n={}, GPU: {} usec, CPU: {} usec",
        label,
        input.len(),
        gpu_time,
        cpu_time
    );

    for i in 0..input.len() {
        let g = gpu_out[i];
        let c = cpu_out[i];

        if (g - c).abs() > 1e-2 {
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

        gpu.release();
    }

    if all_passed {
        println!("softmax test suite: ALL PASSED");
    } else {
        println!("softmax test suite: FAILED");
    }
}

pub fn exp_func_test() {
    let mut all_passed = true;

    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;

        {
            const N: usize = 16;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 7 + 5) % 17) as f32 * 0.25 - 1.0);
            all_passed = run_exp_case(gpu, "small-16", &input) && all_passed;
        }

        {
            const N: usize = 64;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 13 + 3) % 23) as f32 * 0.125 - 1.5);
            all_passed = run_exp_case(gpu, "mid-64", &input) && all_passed;
        }

        {
            const N: usize = 256;
            let input: [f32; N] = core::array::from_fn(|i| ((i * 29 + 11) % 97) as f32 * 0.05 - 2.0);
            all_passed = run_exp_case(gpu, "full-256", &input) && all_passed;
        }

        gpu.release();
    }

    if all_passed {
        println!("exp test suite: ALL PASSED");
    } else {
        println!("exp test suite: FAILED");
    }
}



use crate::gpu::{GpuKernel, MATMUL_KERNEL_CODE};
use crate::timer::Timer;
use crate::{print, println};

fn pack_kernel(data: &[u32], out: &mut [u32], n: usize, m: usize, offset: usize) {
    let n16 = ((n + 15) / 16) * 16;
    let m16 = ((m + 15) / 16) * 16;

    if out.len() < n16 * m16 {
        return;
    }

    for x in out.iter_mut().take(n16 * m16) {
        *x = 0;
    }

    let num_tiles_n = (n + 15) / 16;
    let num_tiles_m = (m + 15) / 16;

    for ti in 0..num_tiles_n {
        for tj in 0..num_tiles_m {
            let packed_base_index: usize = (ti * num_tiles_m + tj) * 256;

            if tj == num_tiles_m - 1 || ti == num_tiles_n - 1 {
                for i in 0..16 {
                    for j in 0..16 {
                        let data_i = (ti * 16) + i;
                        let data_j = (tj * 16) + j;
                        if data_i < n && data_j < m {
                            let data_index = data_i * m + data_j + offset;
                            let index_offset = i * 16 + j;
                            out[packed_base_index + index_offset] = data[data_index];
                        }
                    }
                }
            } else {
                for i in 0..16 {
                    let data_i = (ti * 16) + i;
                    let data_index = data_i * m + (tj * 16) + offset;
                    let out_index = packed_base_index + i * 16;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.as_ptr().add(data_index),
                            out.as_mut_ptr().add(out_index),
                            16,
                        );
                    }
                }
            }
        }
    }
}

fn pack_transpose_kernel(data: &[u32], out: &mut [u32], n: usize, m: usize) {
    let num_tiles_n = (n + 15) / 16;
    let num_tiles_m = (m + 15) / 16;

    let n16 = num_tiles_n * 16;
    let m16 = num_tiles_m * 16;

    if out.len() < n16 * m16 {
        return;
    }

    for x in out.iter_mut().take(n16 * m16) {
        *x = 0;
    }

    let mut tile_idx = 0;
    for tj in 0..num_tiles_m {
        for ti in 0..num_tiles_n {
            let packed_base_index = tile_idx * 256;
            tile_idx += 1;

            for i in 0..16 {
                let data_i = ti * 16 + i;
                let data_j = tj * 16;
                if data_i < n && data_j < m {
                    let data_index = data_i * m + data_j;
                    let out_index = packed_base_index + i * 16;
                    let cols = (16).min(m.saturating_sub(data_j));
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            data.as_ptr().add(data_index),
                            out.as_mut_ptr().add(out_index),
                            cols,
                        );
                    }
                }
            }
        }
    }
}

fn unpack_kernel(data: &[u32], out: &mut [u32], n: usize, m: usize) {
    let num_tiles_n = n / 16;
    let num_tiles_m = m / 16;

    for ti in 0..num_tiles_n {
        for tj in 0..num_tiles_m {
            let packed_base_index = (ti * num_tiles_m + tj) * 256;
            for i in 0..16 {
                let out_i = ti * 16 + i;
                let out_index = out_i * m + tj * 16;
                let in_index = packed_base_index + i * 16;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        data.as_ptr().add(in_index),
                        out.as_mut_ptr().add(out_index),
                        16,
                    );
                }
            }
        }
    }
}

fn print_matrix(data: &[u32], n: usize, m: usize) {
    for i in 0..n {
        for j in 0..m {
            print!("{} ", data[i * m + j]);
        }
        print!("\n");
    }
}

pub fn cpu_matmul(a: &[u32], b: &[u32], c: &mut [u32], M: usize, N: usize, K: usize) {
    for i in 0..M {
        for j in 0..N {
            let mut sum: u32 = 0;
            for k in 0..K {
                sum += a[i * K + k] * b[k * N + j];
            }
            c[i * N + j] = sum;
        }
    }
}

pub fn matmul(a: &[u32], b: &[u32], out: &mut [u32], m: usize, n: usize, k: usize) {
    unsafe {
        let gpu_ptr = GpuKernel::init(MATMUL_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        let num_m_tiles = m / 16;
        let num_n_tiles = n / 16;
        let num_k_tiles = k / 16;
    
        // let mut a_slice: [u32; crate::gpu::MAX_DATA_SIZE] = [0; crate::gpu::MAX_DATA_SIZE];
        let mut b_slice: [u32; crate::gpu::MAX_DATA_SIZE] = [0; crate::gpu::MAX_DATA_SIZE];

        let tile_elems: usize = 16 * 16 as usize;
        let row_elems: usize = tile_elems * num_k_tiles as usize;

        for m_group in 0..((num_m_tiles + 11) / 12) {
            for n_tile in 0..num_n_tiles {
                // a_slice = [0; crate::gpu::MAX_DATA_SIZE];
                b_slice = [0; crate::gpu::MAX_DATA_SIZE];

                // move rows from a
                let row_start_tile = m_group * 12;
                let row_end_tile = core::cmp::min(num_m_tiles, row_start_tile + 12);
                // core::ptr::copy_nonoverlapping(
                //     a.as_ptr().add(row_start_tile * 16 * k),
                //     a_slice.as_mut_ptr(), // .add(local_tile * 16 * k),
                //     16 * k * (row_end_tile - row_start_tile)
                // );
                // for row_tile in row_start_tile..row_end_tile {
                //     let local_tile = row_tile - row_start_tile;
                //     core::ptr::copy_nonoverlapping(
                //         a.as_ptr().add(row_tile * 16 * k),         // source: actual row in a (row_tile * 16 rows * k cols)
                //         a_slice.as_mut_ptr().add(local_tile * 16 * k), // dest: local 0-based offset
                //         16 * k,                                     // 16 rows * k elements
                //     );
                // }
                
                // move column from b
                for row in 0..k {
                    core::ptr::copy_nonoverlapping(
                        b.as_ptr().add(row * n + n_tile * 16),
                        b_slice.as_mut_ptr().add(row * 16),
                        16,
                    );
                }

                pack_kernel(&a, &mut gpu.data[0], (row_end_tile - row_start_tile) * 16, k, row_start_tile * 16 * k);
                pack_transpose_kernel(&b_slice, &mut gpu.data[1], k,16);

                let mut c_unpacked: [u32; crate::gpu::MAX_DATA_SIZE] = [0; crate::gpu::MAX_DATA_SIZE];
                
                let num_cores = (row_end_tile - row_start_tile); // this is at most 12
                for core in 0..crate::gpu::MAX_VC_CORES {
                    let a_offset = core * row_elems * 4;
                    let c_offset = core * tile_elems * 4;
                    gpu.unif[core][0] = gpu.get_data_ptr(0) + a_offset as u32;
                    gpu.unif[core][1] = gpu.get_data_ptr(1);
                    gpu.unif[core][2] = gpu.get_data_ptr(2) + c_offset as u32;
                    gpu.unif[core][3] = (row_end_tile - row_start_tile) as u32; 
                    gpu.unif[core][4] = num_k_tiles as u32;
                    gpu.unif[core][5] = 1 as u32;

                    // println!("Writing to core: a_offset={}, c_offset={}, a={:x}, b={:x}, c={:x}, m_tiles={}, k_tiles={}, n_tiles={}", 
                    //     a_offset, 
                    //     c_offset,
                    //     gpu.unif[core][0], 
                    //     gpu.unif[core][1],
                    //     gpu.unif[core][2],
                    //     gpu.unif[core][3],
                    //     gpu.unif[core][4],
                    //     gpu.unif[core][5]
                    // );
                }

                gpu.execute(num_cores as u32);
                unpack_kernel(&gpu.data[2], &mut c_unpacked, (row_end_tile - row_start_tile) * 16, 16);
                
                // move tiles from c
                
                for row_tile in 0..(row_end_tile - row_start_tile) {
                    for row_i in 0..16 {
                        let abs_row = (row_start_tile + row_tile) * 16 + row_i;
                        core::ptr::copy_nonoverlapping(
                            c_unpacked.as_ptr().add((row_tile * 16 + row_i) * 16),
                            out.as_mut_ptr().add(abs_row * n + n_tile * 16),
                            16,
                        );
                    }
                }
            }
        }

        gpu.release();
    }
}

pub fn matmul_func_test() {
    const M: usize = 16 * 15;
    const N: usize = 32;
    const K: usize = 64;

    let a: [u32; M * K] = core::array::from_fn(|i| (i % 9) as u32);
    let b: [u32; K * N] = core::array::from_fn(|i| ((i % 9) as u32) + 1);
    let mut c: [u32; M * N] = [0; M * N];
    let mut c_cpu: [u32; M * N] = [0; M * N];

    let start_matmul = Timer::get_usec();
    matmul(&a, &b, &mut c, M, N, K);
    let matmul_time = Timer::get_usec() - start_matmul;

    let start_cpu_matmul = Timer::get_usec();
    cpu_matmul(&a, &b, &mut c_cpu, M, N, K);
    let cpu_matmul_time = Timer::get_usec() - start_cpu_matmul;

    // println!("CPU matmul:\n");
    // print_matrix(&c_cpu, M, N);
    // println!("\n\nGPU matmul:\n");
    // print_matrix(&c, M, N);

    println!("Amount of time taken: GPU: {}, CPU: {}\n", matmul_time, cpu_matmul_time);

    let mut matches = true;
    for i in 0..(M * N) {
        if c[i] != c_cpu[i] {
        println!(
            "Discrepancy at index {}: expected {}, observed {}",
            i, c_cpu[i], c[i] 
        );
        matches = false;
        break;
        }
    }

    if matches {
        println!("Matrix outputs match: YES");
    } else {
        println!("Matrix outputs match: NO");
    }
}
 
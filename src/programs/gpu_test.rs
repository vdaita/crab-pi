use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, MATMUL_KERNEL_CODE};
use crate::{print, println};

pub fn deadbeef_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(DEADBEEF_GPU_CODE);
        let gpu = &mut *gpu_ptr;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);
        gpu.execute(1);
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}

fn matrix_transpose(data: &[u32], out: &mut [u32], n: usize, m: usize) {
    if data.len() != n * m || out.len() != n * m {
        return;
    }

    for i in 0..n {
        for j in 0..m {
            out[j * n + i] = data[i * m + j];
        }
    }
}

fn pack_kernel(data: &[u32], out: &mut [u32], n: usize, m: usize) {
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
                            let data_index = data_i * m + data_j;
                            let index_offset = i * 16 + j;
                            out[packed_base_index + index_offset] = data[data_index];
                        }
                    }
                }
            } else {
                for i in 0..16 {
                    let data_i = (ti * 16) + i;
                    let data_index = data_i * m + (tj * 16);
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

pub fn add_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(ADD_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        let a: [u32; 128] = core::array::from_fn(|i| i as u32);
        let b: [u32; 128] = core::array::from_fn(|i| (i as u32) + 6);
        let n = a.len(); // copy_nonoverlapping expects element count

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[1].as_mut_ptr(), n);
        gpu.unif[0][3] = n as u32;

        print!("A: ");
        print_matrix(&a, 1, 128);

        print!("B: ");
        print_matrix(&b, 1, 128);

        print!("Before: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[2][i]);
        }
        println!("");

        gpu.execute(1);

        print!("After: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[2][i]);
        }
        println!("");

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}

pub fn print_row(addr: *const u32) {
    unsafe {
        for i in 0..16 {
            print!("{} ", *addr.add(i));
        }
        println!("");
    }
}

pub fn matmul_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(MATMUL_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        // MNK: M rows of A, N columns of B, K shared dimension
        const M: usize = 32; // rows of A
        const N: usize = 16; // columns of B
        const K: usize = 64; // columns of A / rows of B

        let a: [u32; M * K] = core::array::from_fn(|i| (i % 11) as u32);
        let b: [u32; K * N] = core::array::from_fn(|i| ((i % 11) as u32) + 1);

        let mut a_packed: [u32; M * K] = [0; M * K];
        let mut b_packed: [u32; K * N] = [0; K * N];
        pack_kernel(&a, &mut a_packed, M, K);
        pack_transpose_kernel(&b, &mut b_packed, K, N);

        println!("Matrix a ({} x {}):", M, K);
        print_matrix(&a, M, K);
        println!("Matrix b ({} x {}):", K, N);
        print_matrix(&b, K, N);
        println!("Matrix a_packed ({} x {}):", K * (M / 16), 16);
        print_matrix(&a_packed,K * (M / 16), 16);
        println!("Matrix b_packed ({} x {}):", N * (K / 16), 16);
        print_matrix(&b_packed, N * (K / 16), 16);

        let mut c: [u32; M * N] = [0; M * N];
        for i in 0..M {
            for j in 0..N {
                let mut sum: u32 = 0;
                for k in 0..K {
                    sum += a[i * K + k] * b[k * N + j];
                }
                c[i * N + j] = sum;
            }
        }
        println!("Expected matmul result ({} x {}):", M, N);
        print_matrix(&c, M, N);

        // per-tile matrix multiplication debug
        // for tile in 0..(K / 16) {
        //     let mut tile_c = [0u32; M * N];
        //     for i in 0..M {
        //         for j in 0..N {
        //             let mut sum = 0u32;
        //             for k in (tile * 16)..((tile + 1) * 16) {
        //                 sum += a[i * K + k] * b[k * N + j];
        //             }
        //             tile_c[i * N + j] = sum;
        //         }
        //     }
        //     println!("Tile {} partial product ({} x {}):", tile, M, N);
        //     print_matrix(&tile_c, M, N);
        // }

        core::ptr::copy_nonoverlapping(a_packed.as_ptr(), gpu.data[0].as_mut_ptr(), M * K);
        core::ptr::copy_nonoverlapping(b_packed.as_ptr(), gpu.data[1].as_mut_ptr(), K * N);
        
        let num_m_tiles = (M as u32) / 16;
        let num_k_tiles = (K as u32) / 16;
        let num_n_tiles = (N as u32) / 16;
        let num_tasks = num_m_tiles * num_n_tiles;
        // let num_cores = core::cmp::min(num_tasks, 12);
        let num_cores = 1;
        
        gpu.unif[0][3] = num_m_tiles;
        gpu.unif[0][4] = num_k_tiles;
        gpu.unif[0][5] = num_n_tiles;
        gpu.unif[0][6] = 1;
        let tile_bytes = 256 * 4;

        // for core in 0..(num_cores as usize) {
        //     let mut slot = 0;
        //     // let mut task = core;
        //     let mut task = 0;

        //     gpu.unif[core][0] = num_m_tiles;
        //     gpu.unif[core][1] = num_k_tiles;
        //     gpu.unif[core][2] = num_n_tiles;

        //     while task < num_tasks as usize{
        //         let ti = task / num_n_tiles as usize;
        //         let tj = task % num_n_tiles as usize;

        //         let a_offset = (ti * num_k_tiles as usize * tile_bytes);
        //         let b_offset = (tj * num_k_tiles as usize * tile_bytes);
        //         let c_offset = (ti * num_n_tiles as usize + tj) * tile_bytes;

        //         let a_addr = gpu.get_data_ptr(0) + a_offset as u32;
        //         let b_addr = gpu.get_data_ptr(1) + b_offset as u32;
        //         let c_addr = gpu.get_data_ptr(2) + c_offset as u32;

        //         gpu.unif[core][4 + slot * 3]     = a_addr;
        //         gpu.unif[core][4 + slot * 3 + 1] = b_addr;
        //         gpu.unif[core][4 + slot * 3 + 2] = c_addr;

        //         println!("Assigning task {} to core {}, a={:x}, b={:x}, c={:x}", task, core, a_addr, b_addr, c_addr);
        //         println!("ti={}, tj={}, a_offset={}, b_offset={}, c_offset={}", ti, tj, a_offset, b_offset, c_offset);
        //         println!("A pointer: ");
        //         print_row(gpu.data[0].as_ptr().add(a_offset / 4));
        //         println!("B pointer: ");
        //         print_row(gpu.data[1].as_ptr().add(b_offset / 4));

        //         slot += 1;
        //         task += num_cores as usize;
        //     }

        //     println!("Writing slot {} for core {}", slot, core);
        //     gpu.unif[core][3] = slot as u32;
        // }

        print!("Before out:\n");
        print_matrix(&gpu.data[2], M, N);

        println!("Executing kernel on {} cores", num_cores);
        gpu.execute(num_cores);

        print!("After out:\n");
        print_matrix(&gpu.data[2], 16, (M * N) / 16);

        print!("After unpacking: \n");
        let mut c_unpacked: [u32; M * N] = [0; M * N];
        unpack_kernel(&gpu.data[2], &mut c_unpacked, M, N);
        print_matrix(&c_unpacked, M, N);

        let mut matches = true;
        for i in 0..(M * N) {
            if gpu.data[2][i] != c[i] {
                matches = false;
                break;
            }
        }
        if matches {
            println!("Matrix outputs match: YES");
        } else {
            println!("Matrix outputs match: NO");
        }

        gpu.release();
        println!("Finished releasing matmul test (MNK: {} {} {})", M, N, K);
    }
}

pub fn test_gpu() {    
    // add_kernel();
    matmul_kernel();
}
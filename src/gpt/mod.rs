#![allow(static_mut_refs)]

mod attention;
mod forward_layer;
mod forward_model;
mod layernorm;
mod loader;
mod model;
mod ops;

use crate::gpu::{GpuKernel, DMA_TEST_CODE};
use crate::{print, println};
use forward_model::gpt_forward_logits;
use loader::load_from_fat32;
use model::{argmax, load_hardcoded_gpt2_from_blob, GptConfig, GptWeights, Tokenizer, MAX_SEQ};
use ops::GptOps;

fn gpt_generate(
    ops: &mut GptOps<'_>,
    cfg: &GptConfig,
    w: &GptWeights<'_>,
    tok: &Tokenizer,
    prompt: &str,
    max_new_tokens: usize,
    unk_id: u16,
) {
    let mut toks = [0u16; MAX_SEQ];
    let mut n = tok.encode_greedy(prompt, &mut toks, unk_id);
    if n == 0 {
        println!("gpt_generate: prompt tokenized to empty sequence");
        return;
    }
    if n > MAX_SEQ {
        n = MAX_SEQ;
    }

    println!("GPT prompt: {}", prompt);
    print!("GPT completion: ");

    let mut generated = 0usize;
    while generated < max_new_tokens && n < MAX_SEQ {
        let logits = match gpt_forward_logits(ops, cfg, w, &toks[..n]) {
            Some(l) => l,
            None => {
                println!("\nforward pass failed");
                return;
            }
        };

        let next = argmax(logits) as u16;
        toks[n] = next;
        n += 1;
        generated += 1;

        print!("{}", tok.token_text(next as usize));
    }
    println!("");
}

pub fn gpt_demo() {
    println!("[gpt] begin demo");
    let loaded = match load_from_fat32() {
        Some(l) => l,
        None => {
            println!("FAT32 model files not found");
            return;
        }
    };

    println!(
        "Loaded from FAT32: weights={} bytes, tokenizer={} bytes",
        loaded.weights_len,
        loaded.tokenizer_len
    );

    let (cfg, weights) = match load_hardcoded_gpt2_from_blob(loaded.weights) {
        Some(v) => v,
        None => {
            println!("failed to parse GPTW.BIN using hardcoded GPT-2 layout");
            return;
        }
    };

    println!(
        "[gpt] config: vocab={}, ctx={}, d_model={}, d_ff={}, layers={}, heads={}",
        cfg.vocab_size,
        cfg.context_size,
        cfg.d_model,
        cfg.d_ff,
        cfg.n_layers,
        cfg.n_heads
    );
    println!("[gpt] tokenizer loaded entries: {}", loaded.tokenizer.len());

    println!("Running full 8-layer forward pass...");
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);
        let mut ops = GptOps::new(gpu);
        gpt_generate(
            &mut ops,
            &cfg,
            &weights,
            &loaded.tokenizer,
            "the cat",
            16,
            loaded.tokenizer.unk_id(),
        );
        gpu.release();
    }

    println!(
        "FAT32 files consumed: {} + {} bytes",
        loaded.weights_len,
        loaded.tokenizer_len
    );
}

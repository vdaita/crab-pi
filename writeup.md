# Dummy LLM Inference on Raspberry Pi

## QPU Matrix Multiplication
This was the most challenging part of the project. To build up to this, I ported parts of the 240LX lab (up to Mandelbrot) over to Rust. The current algorithm loads a tile of the P matrix row by row, the Q matrix as a tile into registers, and sums over producing the output tile. To implement this, I used special register functions: specifically, replicating and moving data across the entire core by using r5 to make sure that each of my output rows were neatly accumulated into.

Some of the issues I faced while doing the matmul implementation were:
- single-core implementation needing to be redone for multiple cores due to running out of VPM memory
- data hazards leading to incorrect results on loop
- nested loop behavior not working within macros
- register restrictions leading to some silent fails
- adjusting the algorithm for matrix multiplication to something I would write in CUDA to something that would make the most sense for the QPU. 
- bad base for exponentials
- weird memory behavior for when data becomes available to the register for the SFU

With the QPU, there are an associated set of helper functions and test functions that could be useful when writing other matrix-based programs for the Raspberry Pi. 

## Rust
Implementing this project in Rust made debugging non-QPU code more straightforward and convenient. This also forced me to go through the original code again when debugging the re-implementation of the FAT32 filesystem and the EMMC drivers. 

## Model
Larger TinyStories models took a very long time to load on the Pi Zero. To get initial tests working, I trained a minimal character-level model and wrote a simple encoding. Then, I loaded these weights from the filesystem into memory, and immediately moved them into GPU memory. I tried to have as many operations work in the buffers I had registered with my GPU object to minimize data movement. 

The rest of the model implementation was mostly implemented on the CPU. It seems that running the operations for softmax on the QPU's SFU might be of comparable speed of doing the same operation on the CPU, but there are still some correctness checks I am failing for the softmax implementation.


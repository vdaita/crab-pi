.include "vc4.qinc"

.macro load_a_row, a
    mov vr_setup, vpm_setup(1, 1, h32(a))
    mov ra0, vpm
    mov -, vr_wait
.endm

.macro load_b_row, b
    mov vr_setup, vpm_setup(1, 1, h32(16 + b))
    mov rb0, vpm
    mov -, vr_wait
.endm

.macro load_a_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, ra4 # launch dma load
    mov -, vr_wait
.endm

.macro load_b_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    mov vr_addr, ra5
    mov -, vr_wait
.endm

.macro store_c_row, c
    mov vw_setup, vpm_setup(1, 1, h32(32 + c))
    mov vpm, r3
    mov -, vw_wait
.endm

.macro store_c_tile
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    mov vw_addr, ra6
    mov -, vw_wait
.endm

mov ra4, unif # ra4 <- src addresses
mov ra5, unif # ra5 <- src address 2
mov ra6, unif # ra6 <- output address

.macro row_mul, i
    mov r5rep, r1 << i
    mov ra10, r3
    mul24 r3, r5rep, rb0
    add r3, r3, ra10
    
    # mov r3, r5rep
    # mov r3, rb0
.endm

.macro load_then_row_mul_4, i
    load_b_row i
    row_mul i
    
    load_b_row i + 1
    row_mul i + 1

    load_b_row i + 2
    row_mul i + 2

    load_b_row i + 3
    row_mul i + 3
.endm

.macro process_row, i
    load_a_row i
    mov r1, ra0
    mov r3, 0
    
    load_then_row_mul_4 0
    load_then_row_mul_4 4
    load_then_row_mul_4 8
    load_then_row_mul_4 12

    store_c_row i
.endm

.macro process_row_4, i
    process_row i
    process_row i + 1
    process_row i + 2
    process_row i + 3
.endm

load_a_tile
load_b_tile

process_row_4 0 
process_row_4 4 
process_row_4 8
process_row_4 12


store_c_tile

nop
thrend
nop
nop
nop
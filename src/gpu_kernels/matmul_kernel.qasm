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
    mov vr_setup, vdr_setup_1(256)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, r0 # launch dma load
    mov -, vr_wait
.endm

.macro load_b_tile
    mov vr_setup, vdr_setup_1(256)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    mov vr_addr, r1
    mov -, vr_wait
.endm

.macro store_c_row, c
    mov vw_setup, vpm_setup(1, 1, h32(32 + c))
    mov vpm, r3
    mov -, vw_wait
.endm

.macro store_c_tile
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    mov vw_addr, r2
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov r1, unif # r1 <- src address 2
mov r2, unif # r2 <- output address

load_a_tile
load_b_tile

load_a_row 0
load_b_row 0
add r3, ra0, rb0
store_c_row 0

store_c_tile

nop
thrend
nop
nop
nop
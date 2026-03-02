.include "vc4.qinc"

.macro load_a_row, a
    mov vr_setup, vpm_setup(1, 1, h32(a))
    mov ra10 + a, vpm
    mov -, vr_wait
.endm

.macro load_a_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, ra4 # launch dma load
    mov -, vr_wait
.endm

.macro store_c_row, c
    mov vw_setup, vpm_setup(1, 1, h32(32 + c))
    mov vpm, rb10 + c
    mov -, vw_wait
.endm

.macro store_c_tile
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    mov vw_addr, ra6
    mov -, vw_wait
.endm

.macro row_exp, a_row 
    mov r3, 1.0
    mov r4, ra10 + a_row
    nop; nop; nop;
    fadd r3, r3, r4
    nop; nop; nop;
    
    mov r2, r2, 0.5 # get the first one ready
    fmul r4, r4, r4 # get this one ready
    nop; nop; nop;
    fmul r1, r4, r2
    nop; nop; nop;
    fadd r3, r3, r1
    nop; nop; nop;
    
    # fmul r2, r2, 3.0 # get the first one ready
    # fmul r4, r4, r4 # get this one ready
    # nop; nop; nop;
    # fdiv r1, r4, r2
    # nop; nop; nop;
    # fadd r3, r3, r1
    # nop; nop; nop;
    
    mov rb10 + a_row, r3
    nop; nop; nop;
.endm

mov ra4, unif # ra4 <- src addresses
mov ra6, unif # ra6 <- output addresses
load_a_tile
nop; nop; nop;
load_a_row 0
nop; nop; nop;
row_exp 0
nop; nop; nop;
store_c_row 0
nop; nop; nop;
store_c_tile

nop;
thrend;
nop;
nop;
nop;
.include "vc4.qinc"

.macro write_16, a, c
    mov vr_setup, vpm_setup(1, 1, h32(a))
    mov ra0, vpm
    mov -, vr_wait
    nop;nop;nop;

    mov r3, 1.0
    nop; nop; nop;
    fadd r3, r3, ra0
    nop; nop; nop;
    
    mov r2, 0.5 # get the first one ready
    fmul r4, ra0, ra0 # get this one ready
    nop; nop; nop;
    fmul r1, r4, r2
    # nop; nop; nop;
    # fadd r3, r3, r1
    # nop; nop; nop;

    mov vw_setup, vpm_setup(1, 1, h32(c))
    mov vpm, r3
    mov -, vw_wait
.endm

.macro do_round
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 0, 0))
    mov vr_addr, r0 # launch dma load
    mov -, vr_wait

    # ----

    write_16 0, 4
    write_16 1, 5
    write_16 2, 6
    write_16 3, 7

    # ---
    mov vw_setup, vdw_setup_0(4, 16, dma_h32(4, 0))
    mov vw_addr, rb0 # write to the destination address
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov rb0, unif # rb0 <- output address
nop;nop;nop;
do_round

nop
thrend 
nop
nop
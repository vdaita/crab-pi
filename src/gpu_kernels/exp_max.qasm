.include "vc4.qinc"

.macro write_16, a, c
    mov vr_setup, vpm_setup(1, 1, h32(a))
    mov ra0, vpm
    mov -, vr_wait

    add r3, ra0, 1

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
    mov vw_addr, r2 # write to the destination address
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov r2, unif # r2 <- output address
nop;nop;nop;
do_round

nop
thrend 
nop
nop
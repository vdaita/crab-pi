.include "vc4.qinc"

.macro write_16, a, b
    mov vr_setup, vpm_setup(1, 1, h32(a)) # read 16 rows, increment by 1 after each read, start at vpm coord 0,0
    mov ra0, vpm
    mov -, vr_wait

    mov vr_setup, vpm_setup(1, 1, h32(b)) # read 16 rows, increment by 1 after each write, start at vpm coord 0,0
    mov rb0, vpm
    mov -, vr_wait
 
    mov ra6, r3
    add r3, ra0, rb0
    add r3, r3, ra6 # make sure that accumulator registers are properly used, as this might silently fail
.endm

.macro do_round
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 0, 0))
    mov vr_addr, r0 # launch dma load
    mov -, vr_wait

    mov vr_setup, vdr_setup_1(256)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    mov vr_addr, r1 # launch dma load
    mov -, vr_wait

    mov r3, 0

    # ----

    write_16 0, 16
    write_16 1, 17
    write_16 2, 18
    write_16 3, 19

    # ----

    mov vw_setup, vpm_setup(1, 1, h32(32))
    mov vpm, r3
    mov -, vw_wait

    # ---
    mov vw_setup, vdw_setup_0(1, 16, dma_h32(32, 0))

    mov vw_addr, r2 # write to the destination address
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov r1, unif # r1 <- src address 2
mov r2, unif # r2 <- output address
mov ra2, unif # number of elements to process
ldi ra1, 256 # <- increment amount
ldi rb2, 64 # number of elements

:loop
    do_round
    add r0, r0, ra1
    add r1, r1, ra1
    add r2, r2, ra1

    sub.setf ra2, ra2, rb2
    brr.anynn -, :loop

    nop
    nop
    nop
:end

nop
thrend 
nop
nop
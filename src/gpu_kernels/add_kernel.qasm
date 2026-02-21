.include "vc4.qinc"

.macro write_16, a, b, c
    mov vr_setup, vpm_setup(1, 1, h32(a)) # read 16 rows, increment by 1 after each read, start at vpm coord 0,0
    mov ra0, vpm
    mov -, vr_wait

    mov vr_setup, vpm_setup(1, 1, h32(b)) # read 16 rows, increment by 1 after each write, start at vpm coord 0,0
    mov rb0, vpm
    mov -, vr_wait

    mov vw_setup, vpm_setup(1, 1, h32(c))
    add r3, ra0, rb0
    mov vpm, r3
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov r1, unif # r1 <- src address 2
mov r2, unif # r2 <- output address


mov vr_setup, vdr_setup_1(64)
mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 0, 0))
mov vr_addr, r0 # launch dma load
mov -, vr_wait

mov vr_setup, vdr_setup_1(64)
mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 4, 0))
mov vr_addr, r1 # launch dma load
mov -, vr_wait

# ----

write_16 0, 4, 8
write_16 1, 5, 9
write_16 2, 6, 10
write_16 3, 7, 11

# ---

mov vw_setup, vdw_setup_0(4, 16, dma_h32(8, 0))
mov vw_addr, r2 # write to the destination address
mov -, vw_wait

nop
thrend 
nop
nop
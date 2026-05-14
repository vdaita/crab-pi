.include "vc4.qinc"

# Read uniforms into registers
mov   ra0, unif # RESOLUTION    
mov   ra1, unif # 1/RESOLUTION
mov   ra2, unif # MAX_ITER
mov   ra3, unif # NUM_QPU
mov   ra4, unif # QPU_NUM
mov   ra5, unif # ADDRESS

mov ra10, ra4
mov r1, ra0
shl ra6, r1, 1      # width,height = 2*RESOLUTION

:row_loop

mov ra11, 0
mov ra14, 0x40800000    # float 4.0

shl r1, ra0, 3
mov r2, ra10
mul24 ra12, r1, r2

itof r1, ra10
itof r2, -1
fmul r1, r1, ra1
fadd rb9, r1, r2        # y = -1 + i/RESOLUTION

:column_loop

    mov r0, ra11
    add r0, r0, elem_num

    itof r1, r0
    itof r2, -1
    fmul r1, r1, ra1
    fadd rb8, r1, r2        # x = -1 + j/RESOLUTION

    itof r1, 0
    mov rb0, r1     # u
    mov ra8, r1     # v
    mov rb2, r1     # u2
    mov ra9, r1     # v2

    mov ra7, ra2    # iter counter = MAX_ITERS
    mov rb7, 1      # result = 1 (converged by default)

:inner_loop

    mov r0, rb0     # r0 = u
    mov r1, ra8     # r1 = v
    mov r2, rb2     # r2 = u2
    mov r3, ra9     # r3 = v2

    # v = 2*u*v + y
    fmul r0, r0, r1
    nop
    nop
    nop
    fadd r0, r0, r0
    nop
    nop
    nop
    mov r4, rb9
    nop
    nop
    nop
    fadd r1, r0, r4         # r1 = new v

    # u = u2 - v2 + x
    fsub r0, r2, r3
    nop
    nop
    nop
    mov r4, rb8
    nop
    nop
    nop
    fadd r0, r0, r4         # r0 = new u

    # store new u, v
    mov rb0, r0
    mov ra8, r1

    # u2 = u*u
    fmul r2, r0, r0
    nop
    nop
    nop

    # v2 = v*v
    fmul r3, r1, r1
    nop
    nop
    nop

    # store new u2, v2
    mov rb2, r2
    mov ra9, r3

    # divergence check: (u2+v2) - 4.0
    fadd r0, r2, r3
    nop
    nop
    nop
    mov r1, ra14
    nop
    nop
    nop
    fsub.setf -, r0, r1
    nop
    nop
    nop

    mov.ifnn rb7, 0

    brr.allz -, :exit
    nop
    nop
    nop

    sub.setf ra7, ra7, 1
    brr.anynz -, :inner_loop
    nop
    nop
    nop

:exit

    mov r2, vpm_setup(1, 1, h32(0))
    add vw_setup, ra4, r2
    mov vpm, rb7
    mov -, vw_wait

    shl r1, ra4, 7
    mov r2, vdw_setup_0(1, 16, dma_h32(0,0))
    add vw_setup, r1, r2

    mov r1, ra11
    shl r1, r1, 2
    add r1, ra12, r1
    add vw_addr, ra5, r1
    mov -, vw_wait

    add ra11, ra11, 16

    mov r1, ra6
    sub.setf r1, ra11, r1
    brr.anyc -, :column_loop
    nop
    nop
    nop

    mov r1, ra3
    add ra10, ra10, r1

    mov r1, ra6
    sub.setf r1, ra10, r1
    brr.anyc -, :row_loop
    nop
    nop
    nop

:end
thrend
mov interrupt, 1
nop
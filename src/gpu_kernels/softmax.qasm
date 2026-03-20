.include "vc4.qinc"

# ra0: read/write address for a
# ra1: current a row
# ra2: current sum of values
# ra3: number of elements to process

.macro load_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, ra0
    mov -, vr_wait

    .rep row, 16
        mov vr_setup, vpm_setup(1, 1, h32(row))
        mov rb0 + row, vpm
        mov -, vr_wait
    .endr
.endm

.macro max_helper
    .rep elem, 16
        mov r5rep, r1 << elem
        fmax r2, r2, r5rep
    .endr
.endm

.macro add_helper
    .rep elem, 16
        mov r5rep, r1 << elem
        sub r0, r5rep, r3
        fadd r2, r2, r0 # add the sum here
    .endr
.endm

.macro update_values
    .rep row, 16
        mov ra54, rb0 + row # take the exponent of the value in ra1
        nop; nop; nop;
        mov rb0 + row, r4 # store this back in the old value
    .endr

    # we must maximize this now
    mov r2, 0 # sums
    .rep row, 16
        mov r1, rb0 + row
        max_helper
    .endr

    mov r3, r2 # pliz store this in r3
    .rep row, 16
        mov r1, rb0 + row
        add_helper
    .endr

    # now we must recip this sum
    mov ra52, r2
    nop; nop; nop;
    mov r2, r4

    # now we must multiply everything by this reciprocal
    .rep row, 16
        fmul r0, rb0 + row, r2
        nop; nop; nop;
        mov rb0 + row, 1
    .endr
.endm

.macro store_tile
    .rep row, 16
        mov vw_setup, vpm_setup(16, 16, h32(row))
        mov vpm, rb0 + row
        mov -, vr_wait
    .endr

    mov vw_setup, vdw_setup_0(16, 16, dma_h32(0, 0))
    mov vw_addr, ra0
    mov -, vw_wait
.endm

mov ra0, unif # ra0 <- src addresses

load_tile
update_values
store_tile

nop
thrend 
nop
nop
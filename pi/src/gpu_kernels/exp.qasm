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
        nop; nop; nop
        sub r0, r5rep, r3
        nop; nop; nop;
        fadd r2, r2, r0 # add the sum here
        nop; nop; nop;
    .endr
.endm

.macro update_values
    ldi r1, 0x3fb8aa3b        # log2(e)
    nop; nop; nop;

    # mov r0, rb0
    # nop; nop; nop;
    # fmul r0, r0, r1       # x * log2(e)
    # nop; nop; nop;
    # mov ra54, r0 # i guess bruh
    # nop; nop; nop;
    # mov ra54, r0          # sfu does exp2!
    # nop; nop; nop;
    # mov rb0, r4     # result
    # nop; nop; nop;

    .rep row, 16
        mov r0, rb0 + row
        nop; nop; nop;
        fmul r0, r0, r1       # x * log2(e)
        nop; nop; nop;
        mov ra54, r0 # i guess bruh
        nop; nop; nop;
        mov ra54, r0          # sfu does exp2!
        nop; nop; nop;
        mov rb0 + row, r4     # result
        nop; nop; nop;
    .endr

    #.rep row, 16
    #    mov r0, rb0 + row
    #    nop; nop; nop;
    #    mov rb54, r0 # take the exponent of the value in ra1
    #    nop; nop; nop;
    #    mov r0, r4
    #    nop; nop; nop;
    #    mov rb0 + row, r0 # store this back in the old value
    #    nop; nop; nop; 
    # .endr

    # we must maximize this now
    # mov r2, 0 # sums
    # .rep row, 16
    #   mov r1, rb0 + row
    #    max_helper
    #.endr

    # mov r3, r2 # pliz store this in r3
    # mov r2, 0
    # .rep row, 16
    #    mov r1, rb0 + row
    #    add_helper
    #.endr

    # now we must recip this sum
    # mov ra52, r2
    nop; nop; nop;
    # mov r2, r4

    # now we must multiply everything by this reciprocal
    # .rep row, 16
    #    fmul r0, rb0 + row, r2
    #    nop; nop; nop;
    #    mov rb0 + row, r0
    #    nop; nop; nop;
    # .endr
.endm

.macro store_tile
    .rep row, 16
        mov vw_setup, vpm_setup(1, 1, h32(row))
        mov vpm, rb0 + row
        mov -, vw_wait
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
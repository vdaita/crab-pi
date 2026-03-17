.include "vc4.qinc"

# Variable assignments:

# clobber-able registers: these registers are loaded into and rewritten for computaion
# r0, r1, r2, r3

# ra0: address for a
# ra1: address for b
# ra2: address for c
# ra3: number of bytes in row for a
# ra4: number of bytes in row for b
# ra5: number of bytes in row for c

# ra6: loop range start along b dimension
# ra7: loop range end along b dimension

# rb0-15: loaded tile a
# ra16-32: loaded tile b
# rb16-32: stored tile a

.macro load_a_row, a_row
    mov vr_setup, vpm_setup(1, 1, h32(a_row))
    mov rb0 + a_row, vpm
    mov -, vr_wait
.endm

.macro load_a_tile
    mov r0, ra3
    mov r1, 0x90000000
    or r0, r0, r1
    mov vr_setup, r0
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, ra0
    mov -, vr_wait
.endm

.macro load_b_row, b_row
    mov vr_setup, vpm_setup(1, 1, h32(16 + b_row))
    mov ra16 + b_row, vpm
    mov -, vr_wait
.endm

.macro load_b_tile
    mov r0, ra4
    mov r1, 0x90000000
    or r0, r0, r1
    mov vr_setup, r0
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    mov vr_addr, ra1
    mov -, vr_wait
.endm

.macro store_c_row, c_row
    mov vw_setup, vpm_setup(1, 1, h32(32 + c_row))
    mov vpm, rb16 + c_row
    mov -, vw_wait
.endm

.macro store_c_tile
    mov r0, ra5
    mov r1, 64
    sub r0, r0, r1
    mov r1, 0xc0000000
    or r0, r0, r1
    mov vw_setup, r0
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    mov vw_addr, ra2
    mov -, vw_wait
.endm


mov ra0, unif
mov ra1, unif
mov ra2, unif
mov ra3, unif
mov ra4, unif
mov ra5, unif
mov ra6, unif

# load_a_tile

# Loop through the given columns
load_b_tile

# loop downwards, through the rows
.rep i, 16
    nop
    nop
    # load_a_row i
    load_b_row i
    mov rb16 + i, ra16 + i
    store_c_row i
.endr

store_c_tile
nop
thrend
nop
nop
.include "vc4.qinc"

# ra0: base address for a
# ra1: base address for b
# ra2: base address for c
# ra3: number of bytes in row for a
# ra4: number of bytes in row for b
# ra5: number of bytes in row for c
# ra6: number of columns (in tiles)
# ra7: number of rows (in tiles)
# ra15: saved copy of ra7

# tile offsets (in tiles, not bytes):
# ra9:  a row offset  (vertical)
# ra10: a col offset  (horizontal)
# ra11: b row offset  (vertical)
# ra12: b col offset  (horizontal)
# ra13: c row offset  (vertical)
# ra14: c col offset  (horizontal)

# rb0-15:  loaded tile a
# ra16-31: loaded tile b
# rb16-31: accumulator tile c

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

    mov r0, ra0         # base of A
    mov r1, ra9         # A row offset (in tiles)
    mov r2, ra3         # A row stride in bytes
    shl r2, r2, 4
    nop; nop; nop;
    mul24 r1, r1, r2    # row_tile * stride = byte offset to that row-of-tiles
    nop; nop; nop;
    add r0, r0, r1

    mov r1, ra10        # A col offset (in tiles)
    shl r1, r1, 6       # * 64 bytes per tile (16 elements * 4 bytes)
    add r0, r0, r1

    mov vr_addr, r0
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

    mov r0, ra1         # base of B
    mov r1, ra11        # B row offset (in tiles)
    mov r2, ra4         # B row stride in bytes
    shl r2, r2, 4  # stride has 16 rows per tile
    nop; nop; nop;
    mul24 r1, r1, r2    # row_tile * stride
    nop; nop; nop;
    add r0, r0, r1

    mov r1, ra12        # B col offset (in tiles)
    shl r1, r1, 6       # * 64 bytes per tile
    add r0, r0, r1

    mov vr_addr, r0
    mov -, vr_wait
.endm

.macro store_c_row, c_row
    mov rb16 + c_row, 1
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

    mov r0, ra2         # base of C

    
    # mov r1, ra13        # C row offset (in tiles)
    # mov r2, ra5         # C row stride in bytes
    # shl r2, r2, 4
    # nop; nop; nop;
    # mul24 r1, r1, r2    # row_tile * stride
    # nop; nop; nop;
    # add r0, r0, r1

    mov r1, ra14        # C col offset (in tiles)
    nop; nop; nop;
    mov r2, 64
    mul24 r1, r1, r2
    # shl r1, r1, 6       # * 64 bytes per tile
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;
    # add r0, r0, r2
    
    
    mov vw_addr, r0
    mov -, vw_wait
.endm

# --- moves: all operate on tile-index offsets, not byte pointers ---

.macro move_a_right
    mov r0, ra10
    add r0, r0, 1
    mov ra10, r0
.endm

.macro move_a_down
    mov r0, ra9
    add r0, r0, 1
    mov ra9, r0
.endm

.macro move_b_right
    mov r0, ra12
    add r0, r0, 1
    mov ra12, r0
.endm

.macro move_b_down
    mov r0, ra11
    add r0, r0, 1
    mov ra11, r0
.endm

.macro move_c_right
    mov r0, ra14
    nop; nop; nop;
    add r0, r0, 1
    nop; nop; nop;
    mov ra14, r0
.endm

.macro move_c_down
    mov r0, ra13
    add r0, r0, 1
    mov ra13, r0
.endm

# --- setup ---

mov ra0, unif
mov ra1, unif
mov ra2, unif
mov ra3, unif
mov ra4, unif
mov ra5, unif
mov ra6, unif
mov ra7, unif

mov ra15, ra7 # save row count

# zero all offsets
mov ra9,  0
mov ra10, 0
mov ra11, 0
mov ra12, 0
mov ra13, 0
mov ra14, 0

# --- loops ---

# :col_loop
    mov ra9,  0     # reset A to top of its column (col stays at ra10)
    mov ra11, 0     # reset B to top of current column
    mov ra13, 0     # reset C to top of current column
    mov ra7, ra15   # reset row counter
    

    :row_loop
        load_b_tile

        .rep i, 16
            load_b_row i
            mov rb16 + i, ra16 + i
            store_c_row i
        .endr
        store_c_tile

        move_b_down
        move_c_down
        move_a_down

        mov r0, ra7
        sub.setf r0, r0, 1
        mov ra7, r0
        brr.anynz -, :row_loop
        nop; nop; nop;
    :end_row

    move_b_right
    move_c_right
    move_a_right

    # mov r0, ra6
    # sub.setf r0, r0, 1
    # mov ra6, r0
    # brr.anynz -, :col_loop
    # nop; nop; nop;
# :end_col


mov ra9,  0     # reset A to top of its column (col stays at ra10)
mov ra11, 0     # reset B to top of current column
mov ra13, 0     # reset C to top of current column
mov ra7, ra15   # reset row counter


:row_loop2
    load_b_tile

    .rep i, 16
        load_b_row i
        mov rb16 + i, ra16 + i
        store_c_row i
    .endr
    store_c_tile

    move_b_down
    move_c_down
    move_a_down

    mov r0, ra7
    sub.setf r0, r0, 1
    mov ra7, r0
    brr.anynz -, :row_loop2
    nop; nop; nop;
:end_row2

move_b_right
move_c_right
move_a_right

# mov r0, ra6
# sub.setf r0, r0, 1
# mov ra6, r0
# brr.anynz -, :col_loop
# nop; nop; nop;

nop
thrend
nop
nop
.include "vc4.qinc"

# Variable assignments:

# clobber-able registers: these registers are loaded into and rewritten for computaion
# r0, r1, r2, r3

# ra0: address for a
# ra1: address for b
# ra2: address for c
# ra3: active counter (outer/column tiles)
# ra4: active counter (inner/row tiles)
# ra5: active counter (reserved for k/inner-dimension tiles)
# ra6: number of a vertical tiles (immutable storage)
# ra7: number of b horizontal tiles (immutable storage)
# ra8: number of inner dimension tiles (immutable storage)
# ra11: per-QPU starting b/c horizontal tile index


# ra9 -> current a row (vertical)
# ra10 -> current a column (horizontal)

# ra12 -> current b row (vertical)
# ra13 -> current b column (horizontal)

# ra14 -> current c row (vertical)
# ra15 -> current c column (horizontal)

# rb0-15: loaded tile a
# ra16-32: loaded tile b
# rb16-32: stored tile a

.macro load_a_row, a_row
    mov vr_setup, vpm_setup(1, 1, h32(a_row))
    mov rb0 + a_row, vpm
    mov -, vr_wait
.endm

.macro load_a_tile
    mov r0, ra8 # take the inner dimension (rows of a) and shift
    nop; nop; nop;
    shl r0, r0, 6 # 4 bytes, 16 elements
    
    mov r1, 0x90000000
    or r0, r0, r1
    mov vr_setup, r0
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    # mov vr_addr, ra8

    mov r0, ra0
    # add vertical offset
    nop; nop; nop;
    mov r1, ra9
    
    mov r2, ra8; # take the inner dimension (rows of a) and shift
    nop; nop; nop;
    shl r2, r2, 6;
    
    nop; nop; nop;
    shl r1, r1, 4
    nop; nop; nop;
    mul24 r1, r1, r2;
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;


    # add horizontal offset
    mov r1, ra10
    nop; nop; nop;
    shl r1, r1, 6
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;

    mov vr_addr, r0

    mov -, vr_wait
.endm

.macro load_b_row, b_row
    # mov vr_setup, vpm_setup(1, 1, v32(0, 16 + b_row))
    mov vr_setup, vpm_setup(1, 1, h32(16 + b_row))
    mov ra16 + b_row, vpm
    mov -, vr_wait
.endm

.macro load_b_tile
    mov r0, ra7
    nop; nop; nop;
    shl r0, r0, 6
    
    mov r1, 0x90000000
    or r0, r0, r1
    mov vr_setup, r0
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    # mov vr_addr, ra9

    # mov r0, ra9
    mov r0, ra1
    # add vertical offset
    nop; nop; nop;
    mov r1, ra12

    mov r2, ra7
    nop; nop; nop;
    shl r2, r2, 6
    
    nop; nop; nop;
    shl r1, r1, 4
    nop; nop; nop;
    mul24 r1, r1, r2;
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;


    # add horizontal offset
    mov r1, ra13
    nop; nop; nop;
    shl r1, r1, 6
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;

    mov vr_addr, r0

    mov -, vr_wait
.endm

.macro store_c_row, c_row
    mov vw_setup, vpm_setup(1, 1, h32(32 + c_row))
    mov vpm, rb16 + c_row
    mov -, vw_wait
.endm

.macro store_c_tile
    mov r0, ra7
    nop; nop; nop;
    shl r0, r0, 6
    
    mov r1, 64
    sub r0, r0, r1
    mov r1, 0xc0000000
    or r0, r0, r1
    mov vw_setup, r0
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    # mov vw_addr, ra10

    # mov r0, ra9
    mov r0, ra2
    # # add vertical offset
    nop; nop; nop;
    mov r1, ra14
    # mov r2, ra5
    
    mov r2, ra7
    nop; nop; nop;
    shl r2, r2, 6
    
    # 
    nop; nop; nop;
    shl r1, r1, 4
    nop; nop; nop;
    mul24 r1, r1, r2;
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;


    # # add horizontal offset
    mov r1, ra15
    nop; nop; nop;
    shl r1, r1, 6
    nop; nop; nop;
    add r0, r0, r1
    nop; nop; nop;

    mov vw_addr, r0

    mov -, vw_wait
.endm

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

.macro a_go_left
    mov ra10, 0
.endm

.macro a_go_top
    mov ra9, 0
.endm

.macro move_b_right
    mov r0, ra13
    add r0, r0, 1
    mov ra13, r0
.endm

.macro move_b_down
    mov r0, ra12
    add r0, r0, 1
    mov ra12, r0
.endm

.macro b_go_left
    mov ra13, 0
.endm

.macro b_go_top
    mov ra12, 0
.endm

.macro move_c_right
    mov r0, ra15
    add r0, r0, 1
    mov ra15, r0
.endm

.macro move_c_down
    mov r0, ra14
    add r0, r0, 1
    mov ra14, r0
.endm

.macro c_go_left
    mov ra15, 0
.endm

.macro c_go_top
    mov ra14, 0
.endm

.macro mac_tile_helper, a_row
    mov r1, rb0 + a_row
    nop; nop; nop;
    .rep b_row, 16
        mov r5rep, r1 << b_row
        nop; nop; nop;
        fmul r3, r5rep, ra16 + b_row
        nop; nop; nop;
        fadd r3, r3, rb16 + a_row
        nop; nop; nop;
        mov rb16 + a_row, r3
    .endr
.endm

.macro mac_tile
    .rep a_row, 16
        mac_tile_helper a_row
    .endr
.endm

.macro load_all_a
    .rep i, 16
        load_a_row i
    .endr
.endm

.macro load_all_b
    .rep i, 16
        load_b_row i
    .endr
.endm

.macro store_all_c
    .rep i, 16
        store_c_row i
    .endr
.endm

.macro clear_acc
    .rep i, 16
        mov rb16 + i, 0
    .endr
.endm

mov ra0, unif
mov ra1, unif
mov ra2, unif
mov ra3, unif
mov ra4, unif
mov ra5, unif
mov ra6, unif
mov ra7, unif
mov ra8, unif
mov ra11, unif

# counters live in ra3-ra5; ra6-ra8 remain immutable tile-count storage
# ra3 is provided as local per-QPU outer column count
mov ra4, ra6
mov ra5, ra8

mov ra9, 0
mov ra10, 0
mov ra12, 0
mov ra13, ra11
mov ra14, 0
mov ra15, ra11

:hor_loop
    a_go_top
    c_go_top

    mov ra4, ra6
    :ver_loop
        a_go_left
        b_go_top

        clear_acc
        mov ra5, ra8
        :innerloop
            load_a_tile
            load_b_tile

            load_all_a
            load_all_b

            mac_tile

            move_a_right
            move_b_down

            mov r0, ra5
            sub.setf r0, r0, 1
            mov ra5, r0
            brr.anynz -, :innerloop
            nop
            nop
            nop
        :endinner

        store_all_c
        store_c_tile

        move_a_down
        move_c_down

        mov r0, ra4
        sub.setf r0, r0, 1
        mov ra4, r0
        brr.anynz -, :ver_loop
        nop
        nop
        nop
    :end_ver

    move_b_right
    move_c_right

    mov r0, ra3
    sub.setf r0, r0, 1
    mov ra3, r0
    brr.anynz -, :hor_loop
    nop
    nop
    nop
:end


nop
thrend
nop
nop

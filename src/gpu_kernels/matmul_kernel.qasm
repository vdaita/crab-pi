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
# ra6: number of columns
# ra7: number of rows
#
#

# used values for:
# ra0 -> ra8
# ra1 -> ra9
# ra2 -> ra10
# ra7 -> ra11

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
    mov r0, ra3
    mov r1, 0x90000000
    or r0, r0, r1
    mov vr_setup, r0
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    # mov vr_addr, ra8

    mov r0, ra0
    # add vertical offset
    nop; nop; nop;
    mov r1, ra9
    mov r2, ra3
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
    mov r0, ra4
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
    mov r2, ra4
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
    mov r0, ra5
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
    mov r2, ra5
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

.macro mac_tile_helper, a_row
    mov r1, rb0 + a_row
    nop; nop; nop;
    .rep b_row, 16
        mov r5rep, r1 << b_row
        nop; nop; nop;
        mul24 r3, r5rep, ra16 + b_row
        nop; nop; nop;
        add r3, r3, rb16 + a_row
        nop; nop; nop;
        mov rb16 + a_row, r3
    .endr
.endm

.macro mac_tile
    .rep a_row, 16
        mac_tile_helper a_row
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
mov ra11, ra7

mov ra9, 0
mov ra10, 0
mov ra12, 0
mov ra13, 0
mov ra14, 0
mov ra15, 0

load_a_tile
load_b_tile

.rep i, 16
    load_a_row i
    load_b_row i
    mov rb16 + i, 0
.endr

mac_tile

.rep i, 16
    store_c_row i
.endr

store_c_tile

# load_a_tile
# Loop through the given columns

# :col_loop
#     # mov ra8, ra0
#     # mov ra9, ra1
#     # mov ra10, ra2
#     mov ra7, ra11

#     mov ra9, 0
#     mov ra12, 0
#     mov ra14, 0

#     :row_loop
#         load_b_tile
#         load_a_tile
#         # loop downwards, through the rows
#         .rep i, 16
#             nop
#             nop
#             # load_a_row i
#             # mov rb16 + i, rb0 + i
#             load_b_row i
#             mov rb16 + i, ra16 + i
#             store_c_row i
#         .endr
#         store_c_tile
        
#         move_a_down
#         move_b_down
#         move_c_down


#         # subtract 1 to keep going
#         mov r0, ra7
#         sub.setf r0, r0, 1
#         mov ra7, r0
#         brr.anynz -, :row_loop
#         nop
#         nop
#         nop
#     :end_rl
    
#     move_a_right
#     move_b_right
#     move_c_right


#     # subtract 1 from r0
#     mov r0, ra6
#     sub.setf r0, r0, 1
#     mov ra6, r0
#     brr.anynz -, :col_loop
#     nop
#     nop
#     nop
# :end


nop
thrend
nop
nop

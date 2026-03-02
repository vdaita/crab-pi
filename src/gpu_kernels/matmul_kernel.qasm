.include "vc4.qinc"

.macro load_a_row, a
    mov vr_setup, vpm_setup(1, 1, h32(a))
    mov ra10 + a, vpm
    mov -, vr_wait
.endm

.macro load_b_row, b
    mov vr_setup, vpm_setup(1, 1, h32(16 + b))
    mov rb0, vpm
    mov -, vr_wait
.endm


.macro load_a_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 0, 0))
    mov vr_addr, ra4 # launch dma load
    mov -, vr_wait
.endm

.macro load_b_tile
    mov vr_setup, vdr_setup_1(64)
    mov vr_setup, vdr_setup_0(0, 16, 16, vdr_h32(1, 16, 0))
    mov vr_addr, ra5
    mov -, vr_wait
.endm


.macro store_c_row, c
    mov vw_setup, vpm_setup(1, 1, h32(32 + c))
    mov vpm, rb10 + c
    mov -, vw_wait
.endm

.macro store_c_tile
    mov vw_setup, vdw_setup_0(16, 16, dma_h32(32, 0))
    mov vw_addr, ra6
    mov -, vw_wait
.endm

.macro row_mul, a_row, b_row # need nops because of dependent operations. can't parallelize further because this needs r5rep
    mov r3, 0
    nop
    nop
    nop
    mov r1, ra10 + a_row
    nop
    nop
    nop
    mov r5rep, r1 << b_row
    nop
    nop
    nop
    mul24 r3, r5rep, rb0
    nop
    nop
    nop
    add r3, r3, rb10 + a_row
    nop
    nop
    nop
    # mov rb10 + a_row, r1
    # mov rb10 + a_row, rb0
    mov rb10 + a_row, r3
    nop
    nop
    nop
.endm

.macro row_mul_list, b_row
    row_mul 0, b_row
    row_mul 1, b_row
    row_mul 2, b_row
    row_mul 3, b_row

    row_mul 4, b_row
    row_mul 5, b_row
    row_mul 6, b_row
    row_mul 7, b_row
    
    row_mul 8, b_row
    row_mul 9, b_row
    row_mul 10, b_row
    row_mul 11, b_row
    
    row_mul 12, b_row
    row_mul 13, b_row
    row_mul 14, b_row
    row_mul 15, b_row
.endm


.macro process_group
    load_a_row 0
    load_a_row 1
    load_a_row 2
    load_a_row 3

    load_a_row 4
    load_a_row 5
    load_a_row 6
    load_a_row 7

    load_a_row 8
    load_a_row 9
    load_a_row 10
    load_a_row 11

    load_a_row 12
    load_a_row 13
    load_a_row 14
    load_a_row 15

    load_b_row 0
    row_mul_list 0

    load_b_row 1
    row_mul_list 1

    load_b_row 2
    row_mul_list 2

    load_b_row 3
    row_mul_list 3

    load_b_row 4
    row_mul_list 4

    load_b_row 5
    row_mul_list 5

    load_b_row 6
    row_mul_list 6

    load_b_row 7
    row_mul_list 7

    load_b_row 8
    row_mul_list 8

    load_b_row 9
    row_mul_list 9

    load_b_row 10
    row_mul_list 10

    load_b_row 11
    row_mul_list 11

    load_b_row 12
    row_mul_list 12

    load_b_row 13
    row_mul_list 13

    load_b_row 14
    row_mul_list 14

    load_b_row 15
    row_mul_list 15

    store_c_row 0
    store_c_row 1
    store_c_row 2
    store_c_row 3

    store_c_row 4
    store_c_row 5
    store_c_row 6
    store_c_row 7

    store_c_row 8
    store_c_row 9
    store_c_row 10
    store_c_row 11

    store_c_row 12
    store_c_row 13
    store_c_row 14
    store_c_row 15
.endm

mov ra4, unif # ra4 <- src addresses (a matrix)
mov ra5, unif # ra5 <- src address 2 (b matrix)
mov ra6, unif # ra6 <- output address

mov r4, unif # ra4 <- height, which this will iterate through
mov r2, unif # r2 <- number of tiles in b (k dimension)
mov ra3, unif # ra2 <- width of b matrix (in tiles)

mov rb31, 1024 # save this to the highest possible register position

# :outer_loop
    

#      sub.setf r4, r4, 1
#     brr.anynz -, :outer_looop
# :end

mov r3, 0
mov rb10 + 0, 0
mov rb10 + 1, 0
mov rb10 + 2, 0
mov rb10 + 3, 0

mov rb10 + 4, 0
mov rb10 + 5, 0
mov rb10 + 6, 0
mov rb10 + 7, 0

mov rb10 + 8, 0
mov rb10 + 9, 0
mov rb10 + 10, 0
mov rb10 + 11, 0

mov rb10 + 12, 0
mov rb10 + 13, 0
mov rb10 + 14, 0
mov rb10 + 15, 0

:innerloop
    load_a_tile 
    nop; nop; 
    load_b_tile
    nop; nop;
    process_group
    nop; nop;

    add r0, ra4, rb31; nop; nop; nop # shift right
    mov ra4, r0; nop; nop; nop;

    mul24 r0, rb31, ra3; nop; nop; nop# width of b matrix * the index of the elements
    add r0, r0, ra5; nop; nop; nop # add this to the current value to shift it down
    mov ra5, r0; nop; nop; nop # move this there

    sub.setf r2, r2, 1
    brr.anynz -, :innerloop
    nop; nop; nop
:end

store_c_tile

# load_a_tile
# load_b_tile
# process_group
# store_c_tile

nop
thrend
nop
nop
nop
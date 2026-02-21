.include "vc4.qinc"

.macro load_next_global_a
    mov vr_setup, vdr_setup_1(16)
    mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 0, 0))
    mov vr_addr, r0 # launch dma load
    mov -, vr_wait

    add r0, r0, ra1
.endm 

.macro load_next_local_a, a
    mov vr_setup, vpm_setup(1, 1, h32(a)) # read 16 rows, increment by 1 after each read, start at vpm coord 0,0
    mov ra0, vpm
    mov -, vr_wait
.endm

.macro load_next_global_b
    mov vr_setup, vdr_setup_1(16)
    mov vr_setup, vdr_setup_0(0, 16, 4, vdr_h32(1, 4, 0))
    mov vr_addr, r1 # launch dma load
    mov -, vr_wait

    add r1, r1, ra1
.endm

.macro load_next_local_b, b
    mov vr_setup, vpm_setup(1, 1, h32(b)) # read 16 rows, increment by 1 after each write, start at vpm coord 0,0
    mov rb0, vpm
    mov -, vr_wait
.endm

.macro compute_row, i
    mov ra0, ra0 << i; nop
    mov r5rep, ra0; nop
    mul24 rb3, r5rep, rb0; 
    nop; 
    nop; 
    nop;
    add r3, r3, rb3; 
    nop; 
    nop;
.endm

.macro do_step
    mov r3, 0

    load_next_global_a
    load_next_local_a 0

    load_next_global_b
    
        load_next_local_b 0
        compute_row 0

        load_next_local_b 1
        compute_row 1

        load_next_local_b 2
        compute_row 2

        load_next_local_b 3
        compute_row 3

    load_next_global_b

        load_next_local_b 4
        compute_row 4

        load_next_local_b 5
        compute_row 5

        load_next_local_b 6
        compute_row 6

        load_next_local_b 7
        compute_row 7

    load_next_global_b

        load_next_local_b 8
        compute_row 8

        load_next_local_b 9
        compute_row 9

        load_next_local_b 10
        compute_row 10

        load_next_local_b 11
        compute_row 11

    load_next_global_b

        load_next_local_b 12
        compute_row 12

        load_next_local_b 13
        compute_row 13

        load_next_local_b 14
        compute_row 14

        load_next_local_b 15
        compute_row 15

    mov vw_setup, vpm_setup(1, 1, h32(8))
    mov vpm, r3
    mov -, vw_wait
.endm

.macro do_round    

    # ----
    do_step

    # ---
    mov vw_setup, vdw_setup_0(4, 16, dma_h32(8, 0))
    mov vw_addr, r2 # write to the destination address
    mov -, vw_wait
.endm

mov r0, unif # r0 <- src addresses
mov r1, unif # r1 <- src address 2
mov r2, unif # r2 <- output address
mov ra2, unif # number of elements to process
ldi ra1, 256 # <- increment amount
ldi rb2, 64 # number of elements

do_round

nop
thrend 
nop
nop
nop
thrend 
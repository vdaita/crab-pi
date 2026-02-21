.func vpm_read_setup(num_vectors, stride, start_offset)
    (0 << 30) | (num_vectors << 20) | (stride << 12) | (2 << 8) | (start_offset)
.endf

.func vpm_write_setup(num, stride, start_offset)
    (2 << 30) | (num << 20) | (stride << 12) | (2 << 8) | (start_offset)
.endf

# addresses
mov ra0, unif
mov ra1, unif
mov ra2, unif

# ram -> vpm
mov vr_setup, vpm_read_setup(16, 1, 0)
mov vr_addr, ra0
mov -, vr_wait

# ram -> vpm
mov vr_setup, vpm_read_setup(16, 1, 16)
mov vr_addr, ra1
mov -, vr_wait

# vpm -> reg
mov vr_setup, vpm_read_setup(1, 1, 0)
mov r1, vpm

# vpm -> reg
mov vr_setup, vpm_read_setup(1, 1, 16)
mov r2, vpm

# add values
add r3, r1, r2

# register
mov vr_setup, vpm_write_setup(1, 1, 32)
mov vpm, r3

# vpm -> ram
mov vw_setup, vpm_write_setup(16, 1, 32)
mov vw_addr, ra2
mov -, vw_wait

:end
thrend
nop
nop
nop
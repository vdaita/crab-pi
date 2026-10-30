# rust os

mac/ - contains the mac-side code that sends code over to the pi. difference between the this and the default is that it recognizes logs from the pi that are from busybox and presents them to the user while putting everything else in a file

pi/ - everything from 140e/240lx, as one bare-metal kernel. `pi/src/`:
- main.rs, start.rs, panic_infra.rs, llvm_infra.rs - boot and runtime glue
- arch/ - barriers, CPSR helpers, MMIO (`mem.rs`)
- devices/ - gpio, uart, timer, watchdog
- fs/fat32/ - SD/EMMC driver and FAT32
- mm/ - kmalloc
- util/ - printing, crc/hash (SD checksums)
- os/ - the os: runs static ARM (musl) linux binaries off the SD card
- programs/ - device demos (dma)

pi/src/os/:
- holder.rs - program slots, MMU mapping, `setup_elf`/`exec_elf` (loads an ELF and builds the linux initial stack: argv/envp/auxv), `run_busybox`
- elf_file.rs - validates a static ELF and copies its segments into a program's ELF region
- file.rs - open files (disk/dir/console/pipe/null), shared open file descriptions, and the per-program fd table
- vfs.rs - path resolution (cwd, `.`/`..`) and create/remove/rename on top of fat32
- pipe.rs - kernel pipe buffers
- errno.rs - errno values and `SysResult`
- syscalls/ - the linux syscall layer
  - mod.rs - svc entry/exit, context switching, and the syscall table (`handler`); set `TRACE` for an strace-style log
  - nr.rs - ARM EABI syscall numbers
  - uaccess.rs - all reads/writes of user memory (the one place that knows how user VAs map)
  - fs.rs, proc.rs, mem.rs, time.rs, sys.rs - handlers, grouped by area

main boots into a busybox shell (`os::run_busybox()`, expects /BUSYBOX on the card); swap in `programs::dma::dma_test()` to run the DMA demo instead.

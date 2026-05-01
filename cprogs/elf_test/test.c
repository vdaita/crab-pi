void _start() {
    const char *msg = "Hello, World!\n";

    register int r0 asm("r0") = 1;
    register const char *r1 asm("r1") = msg;
    register int r2 asm("r2") = 14;
    register int r7 asm("r7") = 4;

    asm volatile("svc 0");

    while (1);
}
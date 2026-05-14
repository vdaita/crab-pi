#include "unistd.h"

int main() {
    write(1, "Hello from musl!\n", 17);
    return 0;
}
#include <stdio.h>
#include "sysinfo.h"

unsigned long long to_mib(unsigned long long bytes) {
    return bytes / 1024 / 1024;
}

void print_architecture(WORD arch) {
    if (arch == 9) {
        printf("    Architecture: x64 (AMD64)\n");
    } else if (arch == 5) {
        printf("    Architecture: ARM\n");
    } else if (arch == 12) {
        printf("    Architecture: ARM64\n");
    } else if (arch == 0) {
        printf("    Architecture: x86\n");
    } else {
        printf("    Architecture: Unknown (%d)\n", arch);
    }
}

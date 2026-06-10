#include <stdio.h>
#include <time.h>
#include <windows.h>
#include "sysinfo.h"

int main() {
    char computer[256];
    char user[256];
    DWORD computer_size = 256;
    DWORD user_size = 256;
    MEMORYSTATUSEX mem;
    SYSTEM_INFO sys;
    unsigned long long free_to_user = 0;
    unsigned long long disk_total = 0;
    unsigned long long disk_free = 0;

    printf("==================================================\n");
    printf("        System Information Diagnostic              \n");
    printf("==================================================\n\n");

    time_t now = time(0);
    struct tm* lt = localtime(&now);
    printf("[*] Local System Time:\n");
    printf("    Date: %04d-%02d-%02d (YYYY-MM-DD)\n", lt->tm_year + 1900, lt->tm_mon + 1, lt->tm_mday);
    printf("    Time: %02d:%02d:%02d\n\n", lt->tm_hour, lt->tm_min, lt->tm_sec);

    printf("[*] Host Identity:\n");
    if (GetComputerNameA(&computer[0], &computer_size)) {
        printf("    Host Computer Name: %s\n", &computer[0]);
    } else {
        printf("    Host Computer Name: unavailable\n");
    }

    if (GetUserNameA(&user[0], &user_size)) {
        printf("    Current User: %s\n\n", &user[0]);
    } else {
        printf("    Current User: unavailable\n\n");
    }

    mem.dwLength = sizeof(MEMORYSTATUSEX);
    printf("[*] Physical Memory (RAM):\n");
    if (GlobalMemoryStatusEx(&mem)) {
        printf("    Total RAM: %llu MiB\n", to_mib(mem.ullTotalPhys));
        printf("    Active RAM: %llu MiB\n", to_mib(mem.ullTotalPhys - mem.ullAvailPhys));
        printf("    Avail RAM: %llu MiB\n", to_mib(mem.ullAvailPhys));
        printf("    Memory Load: %lu%%\n\n", mem.dwMemoryLoad);
    } else {
        printf("    Memory status unavailable\n\n");
    }

    GetSystemInfo(&sys);
    printf("[*] CPU and Architecture:\n");
    print_architecture(sys.u.s.wProcessorArchitecture);
    printf("    Processor Cores: %lu\n", sys.dwNumberOfProcessors);
    printf("    System Page Size: %lu bytes\n\n", sys.dwPageSize);

    printf("[*] Storage Status (C:\\):\n");
    if (GetDiskFreeSpaceExA("C:\\", &free_to_user, &disk_total, &disk_free)) {
        printf("    Total Capacity: %llu MiB\n", to_mib(disk_total));
        printf("    Free Disk Space: %llu MiB\n", to_mib(disk_free));
        printf("    Available To User: %llu MiB\n\n", to_mib(free_to_user));
    } else {
        printf("    Disk status unavailable\n\n");
    }

    printf("==================================================\n");
    printf(" Diagnostics Completed.\n");
    printf("==================================================\n");

    return 0;
}

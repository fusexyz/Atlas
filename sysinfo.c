#include <stdio.h>
#include <time.h>

typedef int BOOL;
typedef unsigned short WORD;
typedef unsigned int DWORD;
typedef unsigned long long DWORDLONG;
typedef unsigned long long DWORD_PTR;

struct MEMORYSTATUSEX {
    DWORD dwLength;
    DWORD dwMemoryLoad;
    DWORDLONG ullTotalPhys;
    DWORDLONG ullAvailPhys;
    DWORDLONG ullTotalPageFile;
    DWORDLONG ullAvailPageFile;
    DWORDLONG ullTotalVirtual;
    DWORDLONG ullAvailVirtual;
    DWORDLONG ullAvailExtendedVirtual;
};

struct SYSTEM_INFO {
    WORD wProcessorArchitecture;
    WORD wReserved;
    DWORD dwPageSize;
    void* lpMinimumApplicationAddress;
    void* lpMaximumApplicationAddress;
    DWORD_PTR dwActiveProcessorMask;
    DWORD dwNumberOfProcessors;
    DWORD dwProcessorType;
    DWORD dwAllocationGranularity;
    WORD wProcessorLevel;
    WORD wProcessorRevision;
};

extern BOOL GetComputerNameA(char* lpBuffer, DWORD* nSize);
extern BOOL GetUserNameA(char* lpBuffer, DWORD* pcbBuffer);
extern void GetSystemInfo(struct SYSTEM_INFO* lpSystemInfo);
extern BOOL GlobalMemoryStatusEx(struct MEMORYSTATUSEX* lpBuffer);
extern BOOL GetDiskFreeSpaceExA(
    const char* lpDirectoryName,
    unsigned long long* lpFreeBytesAvailableToCaller,
    unsigned long long* lpTotalNumberOfBytes,
    unsigned long long* lpTotalNumberOfFreeBytes
);

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

int main() {
    char computer[256];
    char user[256];
    DWORD computer_size = 256;
    DWORD user_size = 256;
    struct MEMORYSTATUSEX mem;
    struct SYSTEM_INFO sys;
    unsigned long long free_to_user = 0;
    unsigned long long disk_total = 0;
    unsigned long long disk_free = 0;


    time_t now = time(NULL);
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

    mem.dwLength = sizeof(struct MEMORYSTATUSEX);
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
    print_architecture(sys.wProcessorArchitecture);
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


    return 0;
}

# Atlas

![Language](https://img.shields.io/badge/language-Rust-orange?style=flat-square)
![Target](https://img.shields.io/badge/target-x86--64%20Windows-blue?style=flat-square)
![Output](https://img.shields.io/badge/output-PE32%2B-green?style=flat-square)
![Dependencies](https://img.shields.io/badge/dependencies-zero-brightgreen?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)

A standalone C compiler written from scratch in Rust that targets 64-bit Windows. Atlas takes `.c` source files and produces native PE32+ executables — no LLVM, no external assembler, no system linker, no third-party crates. Every stage of the pipeline, from preprocessor to binary PE writer, lives inside a single self-contained binary.

---

## Pipeline

```
 ┌─────────────────┐
 │   .c source(s)  │
 └────────┬────────┘
          │
          ▼
 ┌─────────────────────────────────────┐
 │  Preprocessor                       │
 │  macros · #include · #ifdef · pack  │
 │  variadic macros · SDK discovery    │
 └────────┬────────────────────────────┘
          │  preprocessed token stream
          ▼
 ┌─────────────────────────────────────┐
 │  Lexer                              │
 │  keywords · literals · operators   │
 └────────┬────────────────────────────┘
          │  token list
          ▼
 ┌─────────────────────────────────────┐
 │  Parser                             │
 │  recursive descent · full AST       │
 │  MSVC extensions · PCH support      │
 └────────┬────────────────────────────┘
          │  AST
          ▼
 ┌─────────────────────────────────────┐
 │  Semantic Analysis                  │
 │  type checking · scope resolution  │
 │  function signatures · lvalue check │
 └────────┬────────────────────────────┘
          │  validated AST
          ▼
 ┌─────────────────────────────────────┐
 │  IR Lowering                        │
 │  typed IR · struct layout           │
 │  const folding · string pooling     │
 └────────┬────────────────────────────┘
          │  IR module
          ▼
 ┌─────────────────────────────────────┐
 │  Codegen                            │
 │  Microsoft x64 ABI                  │
 │  sized memory ops · peephole opt    │
 └────────┬────────────────────────────┘
          │  machine instructions
          ▼
 ┌─────────────────────────────────────┐
 │  Encoder                            │
 │  direct x86-64 byte emission        │
 │  REX · ModR/M · SIB · relocs        │
 └────────┬────────────────────────────┘
          │
     ┌────┴─────────────────────┐
     │  -c mode                 │  full link mode
     ▼                          ▼
 ┌──────────┐       ┌────────────────────────────┐
 │  .obj    │──────▶│  Linker                    │
 └──────────┘       │  merge · dead code elim    │
                    │  import resolve · PE write  │
                    └────────────┬───────────────┘
                                 │
                                 ▼
                            ┌─────────┐
                            │  .exe   │
                            └─────────┘
```

---

## Why Atlas?

Most hobby compilers stop at either generating C or targeting a custom VM. Atlas goes further: it produces real native Windows PE32+ binaries that run without any runtime support, CRT startup, or external toolchain. A few things that set it apart:

- **Genuinely zero dependencies.** The `[dependencies]` section in `Cargo.toml` is empty. No `inkwell`, no `object`, no `goblin`. Every data structure, every encoding, every binary layout is hand-written.
- **Real preprocessor.** Full macro expansion, recursive `#include`, conditional compilation, `#pragma pack`, `__VA_ARGS__`, token pasting, and automatic discovery of Windows SDK and MSVC include paths — so `#include <stdio.h>` actually works.
- **Multi-file compilation.** Atlas can compile multiple `.c` files separately with `-c`, producing custom `.obj` files, then link them together into a single executable — including dead code elimination across the merged object.
- **No text assembly step.** The encoder writes x86-64 bytes directly from the machine IR. There is no intermediate `.asm` file and no call to NASM or MASM.
- **Understands the Windows ABI.** Shadow space, argument registers (RCX/RDX/R8/R9), 16-byte stack alignment before every `call`, sized memory operations, RIP-relative imports via `FF 15` — all handled correctly.
- **Import resolution from `.lib` files.** Atlas parses COFF import libraries from your installed Windows SDK and MSVC toolchain to map symbols to their DLLs automatically, instead of maintaining a hardcoded lookup table.

---

## Features

- Full C preprocessor with MSVC compatibility macros
- Hand-written recursive-descent parser covering most of C99
- Semantic type checker with scope resolution and lvalue validation
- Typed IR with explicit basic blocks and correct struct layout computation
- Microsoft x64 calling convention with correct stack alignment
- Sized memory operations: 8/16/32/64-bit load/store/extend
- Peephole optimizer: redundant load elimination via stack value tracking
- Direct x86-64 machine code emission — no text assembly stage
- Custom binary object file format (`CCOBJ001`) for separate compilation
- Multi-file linking with dead code elimination (reachability from `main`)
- PE32+ binary writer: DOS header, COFF, optional header, section table, IAT
- Automatic symbol-to-DLL resolution by scanning installed `.lib` archives
- Precompiled header support (binary serialization of full AST + macro state)
- Anonymous struct/union field flattening
- Variadic function support (`__va_start` intrinsic)
- `#pragma pack` push/pop stack
- `sizeof` and `alignof` with correct struct alignment semantics
- Function pointers and indirect calls

---

## Quick Start

```bash
# Build the compiler
cargo build --release

# Compile and link multiple files in one step
./target/release/compiler examples/sysinfo.c examples/sysinfo_format.c -o sysinfo.exe

# Run it
./sysinfo.exe
```

**Separate compilation:**
```bash
# Compile each file to a .obj
./target/release/compiler -c examples/sysinfo.c -o sysinfo.obj
./target/release/compiler -c examples/sysinfo_format.c -o sysinfo_format.obj

# Link them together
./target/release/compiler sysinfo.obj sysinfo_format.obj -o sysinfo.exe
```

**Precompiled headers:**
```bash
# Build a PCH from a header-heavy file
./target/release/compiler --make-pch stdafx.pch stdafx.c

# Use it when compiling other files
./target/release/compiler -c main.c --use-pch stdafx.pch -o main.obj
```

**All options:**
```
compiler [-c] <file.c|file.obj>... [-o output]
         [--import DLL:FUNC ...]
         [--make-pch FILE] [--use-pch FILE]
```

---

## Example

`examples/sysinfo.c` + `examples/sysinfo_format.c` — a two-file demo that calls real Win32 APIs. It uses `#include <windows.h>` resolved against your installed SDK, exercises structs and unions from the Windows headers (`MEMORYSTATUSEX`, `SYSTEM_INFO`), and splits helper functions into a separate translation unit to demonstrate multi-file compilation.

`examples/sysinfo.c`:
```c
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
    unsigned long long free_to_user = 0, disk_total = 0, disk_free = 0;

    time_t now = time(0);
    struct tm* lt = localtime(&now);
    printf("[*] Local System Time:\n");
    printf("    Date: %04d-%02d-%02d (YYYY-MM-DD)\n", lt->tm_year + 1900, lt->tm_mon + 1, lt->tm_mday);
    printf("    Time: %02d:%02d:%02d\n\n", lt->tm_hour, lt->tm_min, lt->tm_sec);

    printf("[*] Host Identity:\n");
    if (GetComputerNameA(&computer[0], &computer_size))
        printf("    Host Computer Name: %s\n", &computer[0]);
    if (GetUserNameA(&user[0], &user_size))
        printf("    Current User: %s\n\n", &user[0]);

    mem.dwLength = sizeof(MEMORYSTATUSEX);
    printf("[*] Physical Memory (RAM):\n");
    if (GlobalMemoryStatusEx(&mem)) {
        printf("    Total RAM: %llu MiB\n", to_mib(mem.ullTotalPhys));
        printf("    Avail RAM: %llu MiB\n", to_mib(mem.ullAvailPhys));
        printf("    Memory Load: %lu%%\n\n", mem.dwMemoryLoad);
    }

    GetSystemInfo(&sys);
    printf("[*] CPU and Architecture:\n");
    print_architecture(sys.u.s.wProcessorArchitecture);
    printf("    Processor Cores: %lu\n\n", sys.dwNumberOfProcessors);

    printf("[*] Storage Status (C:\\):\n");
    if (GetDiskFreeSpaceExA("C:\\", &free_to_user, &disk_total, &disk_free)) {
        printf("    Total Capacity: %llu MiB\n", to_mib(disk_total));
        printf("    Free Disk Space: %llu MiB\n\n", to_mib(disk_free));
    }
    return 0;
}
```

`examples/sysinfo_format.c`:
```c
#include <stdio.h>
#include "sysinfo.h"

unsigned long long to_mib(unsigned long long bytes) {
    return bytes / 1024 / 1024;
}

void print_architecture(WORD arch) {
    if (arch == 9)       printf("    Architecture: x64 (AMD64)\n");
    else if (arch == 5)  printf("    Architecture: ARM\n");
    else if (arch == 12) printf("    Architecture: ARM64\n");
    else if (arch == 0)  printf("    Architecture: x86\n");
    else                 printf("    Architecture: Unknown (%d)\n", arch);
}
```

```
[*] Local System Time:
    Date: 2026-06-10 (YYYY-MM-DD)
    Time: 14:23:07

[*] Host Identity:
    Host Computer Name: DESKTOP-XYZ
    Current User: fuse

[*] Physical Memory (RAM):
    Total RAM: 32678 MiB
    Avail RAM: 18475 MiB
    Memory Load: 43%

[*] CPU and Architecture:
    Architecture: x64 (AMD64)
    Processor Cores: 16

[*] Storage Status (C:\):
    Total Capacity: 476837 MiB
    Free Disk Space: 123456 MiB
```

---

## Architecture

### Preprocessor (`src/preprocessor/`)

The preprocessor runs before the lexer and produces a clean token stream with all directives resolved. It handles:

- **Macro expansion** — object-like and function-like macros with correct rescanning, argument substitution, and expansion guards to prevent infinite recursion
- **Token pasting and stringification** — `##` and `#` operators inside macro bodies
- **Variadic macros** — `__VA_ARGS__` with correct expansion and stringification
- **`#include` resolution** — recursive with cycle detection; searches the source file's directory first, then system paths discovered at runtime
- **Conditional compilation** — `#ifdef`, `#ifndef`, `#if`, `#elif`, `#else`, `#endif` backed by a full expression evaluator supporting `defined()`, arithmetic, bitwise, and logical operators
- **`#pragma pack`** — push/pop stack; emits sentinel tokens (`__pragma_pack_push_N`, `__pragma_pack_pop`) consumed by the parser
- **Predefined macros** — `_WIN32`, `_WIN64`, `_MSC_VER` (1930), `_M_AMD64`, `__STDC__`, `__cdecl`, `__declspec(x)`, `NULL`, and others that Windows SDK headers depend on
- **SDK/MSVC discovery** — reads `%INCLUDE%`, walks `Program Files (x86)/Windows Kits/10/Include/<latest>/ucrt|shared|um|winrt` and Visual Studio MSVC include paths to find system headers automatically
- **Line continuation** — backslash-newline splicing before tokenization

### Lexer (`src/lexer/`)

Hand-written tokenizer with full line/column tracking for error messages. Handles all C tokens including hex literals (`0x...`), integer/float suffixes (`ULL`, `f`, `L`), all escape sequences, the full set of compound-assignment and bitwise operators, `->`, and `...` for variadic declarations.

### Parser (`src/parser/`)

Recursive-descent parser that builds a typed AST. Coverage includes:

- All declaration forms: functions, local and global variables, structs, unions, enums, typedefs
- All statement forms: `if/else`, `while`, `do-while`, `for`, `switch/case/default`, `break`, `continue`, `goto`, labeled statements, `return`
- Expressions with correct precedence via precedence climbing: binary, unary, ternary, casts, `sizeof`, `alignof`, address-of, dereference, struct member access (`.` and `->`), array indexing, function calls
- All assignment operators (`=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`)
- MSVC extension keywords: `__declspec`, `__stdcall`, `__cdecl`, `__fastcall`, `__int8/16/32/64`, `__forceinline`, `__pragma`, `__unaligned`
- Typedef-name tracking for correct type vs. identifier disambiguation during parsing
- Enum constant evaluation with compile-time integer expression folding
- **Precompiled header (PCH)** — binary serialization and deserialization of the full parsed AST, all active macro definitions, typedef names, and enum constants; enables fast re-compilation of large header-heavy translation units

### Semantic Analysis (`src/sema.rs`)

A dedicated semantic pass runs after parsing and before IR lowering. It:

- Collects all function signatures, struct field maps, typedefs, global declarations, and enum constants
- Checks all function bodies with scoped variable tracking
- Validates types on binary/unary expressions, assignments, and function calls
- Verifies that lvalue-required contexts (assignment left-hand sides, address-of) receive actual lvalues
- Reports errors with source location (line:col from the span attached to each AST node)

### IR (`src/ir/`)

A typed, flat intermediate representation with explicit basic blocks.

**Types:** `void`, `i8`, `i16`, `i32`, `i64`, `f32`, `f64`, `ptr(T)`, `array(T, N)`

**Instructions:** `alloca`, `load`, `store`, `binop`, `unaryop`, `call`, `gep` (pointer arithmetic), `cast`, `copy`

**Terminators:** `ret`, `br`, `condbr`, `unreachable`

The lowering pass computes struct field offsets and sizes with correct natural alignment and `#pragma pack` overrides. Union fields all map to offset 0 with the struct size set to the largest field. Anonymous struct/union fields are flattened into the parent's field namespace. Typedef cycles are detected. String literals are pooled with automatic null termination. Global variables with constant initializers are emitted to the `.data` section.

### Codegen (`src/codegen/`)

Translates IR into machine instructions following the Microsoft x64 ABI:

- First four integer/pointer arguments in `RCX`, `RDX`, `R8`, `R9`; remaining arguments spilled to the stack
- 32-byte shadow space allocated before every `call`
- Return values in `RAX`; stack kept 16-byte aligned before every `call`
- Every virtual register gets a fixed `[RBP - N]` slot — no register allocator, spill-everything layout
- **Sized memory operations** — 8-bit (`Mov8`/`Movzx8`), 16-bit (`Mov16`/`Movzx16`), 32-bit (`Mov32`), 64-bit (`Mov`) — selected based on the IR type of the load/store
- **Peephole optimizer** — tracks which stack slots hold known values and eliminates redundant load-after-store sequences; runs to a fixed point
- RIP-relative addressing for string literals, global variables, and IAT import slots
- `__va_start` intrinsic: spills the four argument registers into the shadow space area so `va_arg` can walk them sequentially
- Division and modulo via `CQO` + `IDIV`; modulo copies `RDX` to `RAX`
- Comparisons via `CMP` + `SETcc` + `MOVZX` producing a clean 0/1 integer result
- Function pointers via `CALL RAX` (indirect register call)
- External DLL calls via `FF 15 rel32` (RIP-relative indirect through the IAT slot)
- Internal function calls via `E8 rel32` (direct relative call)

### Machine Code Encoder (`src/codegen/encode.rs`)

Emits raw x86-64 bytes from the machine instruction list — no textual assembly intermediate:

- **REX prefixes** — `REX.W` for 64-bit operands, `REX.R`/`REX.B` for registers R8–R15; 8-bit and 16-bit operations emit the correct REX or operand-size prefix
- **ModR/M** — register-register (`mod=11`), register-memory with base register, RIP-relative (`mod=00 rm=101`)
- **SIB** — emitted when the base register is `RSP`
- **Displacement** — 8-bit when the value fits in a signed byte, 32-bit otherwise
- **RIP-relative relocations** — `(offset, symbol)` pairs patched by the PE writer once section layout is finalized
- **Label fixups** — intra-function branch targets resolved within the encoder after all instructions are emitted

Instructions covered: `MOV`/`MOV8`/`MOV16`/`MOV32`, `LEA`, `ADD`, `SUB`, `IMUL`, `AND`, `OR`, `XOR`, `SHL`, `SAR`, `NEG`, `NOT`, `CQO`, `IDIV`, `CMP`, `SETcc`, `MOVZX8`/`MOVZX16`, `PUSH`, `POP`, `CALL`, `JMP`, `Jcc`, `RET`.

### Object File Format (`src/linker/object.rs`)

Atlas uses a custom binary object file format (magic `CCOBJ001`) rather than COFF. Each `.obj` stores:

- Encoded function bytes and their names
- Relocation records: `(function_name, [(offset, symbol)])` pairs
- String literal table: `(name, bytes)`
- Global variable table with type and optional initializer

`write_object` / `read_object` serialize and deserialize this format. When multiple objects are merged, string literal names are scoped to avoid collisions, and duplicate symbol definitions are rejected with a diagnostic.

### Linker (`src/linker/`)

**Dead code elimination** — `retain_reachable_symbols` performs a reachability analysis from `main` over the merged relocation graph, then drops all unreachable functions, string literals, and globals before writing the PE. The number of pruned symbols is reported.

**PE Writer (`pe.rs`)** constructs a valid PE32+ executable from scratch:

- **DOS stub** — minimal header with correct `e_lfanew`
- **COFF header** — machine type `0x8664` (AMD64), section count, characteristics
- **Optional header (PE32+)** — `ImageBase` (`0x140000000`), `SectionAlignment` (0x1000), `FileAlignment` (0x200), subsystem (console), stack/heap sizes, data directory entries for the import table and IAT
- **Sections** — `.text` (code), `.rdata` (string literals), `.data` (mutable globals), `.idata` (import structures)
- **Import Directory Table** — one entry per DLL, null-terminated, with correct `OriginalFirstThunk`/`FirstThunk`/`Name` RVAs
- **IAT** — hint/name entries for each imported symbol
- **Entry trampoline:**
  ```asm
  and  rsp, -16        ; align (Windows entry RSP ≡ 8 mod 16)
  call main
  mov  rcx, rax        ; pass return value to ExitProcess
  sub  rsp, 32         ; shadow space
  call [rip+ExitProcess]
  ```

### Import Resolver (`src/linker/import_lib.rs`)

Instead of a hardcoded function-to-DLL table, Atlas discovers the mapping dynamically by scanning the installed toolchain:

- Searches `%INCLUDE%` / `%LIB%` environment variables, `Program Files (x86)/Windows Kits/10/lib`, and detected Visual Studio installation paths
- Opens `.lib` files as COFF archives, reads the archive member headers
- Parses **COFF Short Import Objects** (identified by `Sig1=0x0000, Sig2=0xFFFF`) to extract the symbol name and its source DLL
- Handles the `__imp_` prefix convention automatically, registering both variants
- Any unresolved extern symbol triggers a warning with a hint to use `--import DLL:FUNC`

---

## Supported C Subset

| Feature | Status |
|---|---|
| `int`, `char`, `short`, `long`, `long long` | ✅ |
| `unsigned` variants | ✅ |
| `float`, `double` (parsing + IR) | ✅ |
| Float/double arithmetic codegen | ❌ |
| Pointers and pointer arithmetic | ✅ |
| Arrays (fixed size) | ✅ |
| `struct` and `union` | ✅ |
| Anonymous struct/union fields | ✅ |
| `enum` with constant expressions | ✅ |
| `typedef` | ✅ |
| `if / else` | ✅ |
| `while`, `do-while`, `for` | ✅ |
| `switch / case / default` | 🔧 Parsed, not yet lowered |
| `break`, `continue` | ✅ |
| `goto` | 🔧 Parsed, not yet lowered |
| `return` | ✅ |
| Function definitions and calls | ✅ |
| Variadic functions (`...`, `va_list`) | ✅ |
| Function pointers | ✅ |
| `sizeof`, `alignof` | ✅ |
| `#define` macros (object + function) | ✅ |
| `#include` (local + system) | ✅ |
| `#ifdef / #ifndef / #if / #elif` | ✅ |
| `#pragma pack` | ✅ |
| Variadic macros (`__VA_ARGS__`) | ✅ |
| Token pasting (`##`) and stringification (`#`) | ✅ |
| Precompiled headers (`.pch`) | ✅ |
| MSVC extension keywords | ✅ |
| Windows SDK headers (`<stdio.h>`, `<windows.h>`, etc.) | ✅ |
| Multiple source files + separate compilation | ✅ |
| Dead code elimination | ✅ |
| Peephole optimization | ✅ |
| Struct/array initializer lists | ❌ |
| Float/double codegen (SSE2) | ❌ |
| Bitfields | ❌ |
| VLAs | ❌ |
| Inline assembly | ❌ |
| Register allocator | ❌ |

---

## Building

**Requirements:**
- Rust stable (edition 2024)
- Windows 10/11 x64
- Windows SDK and/or MSVC toolchain if you want `#include <windows.h>` to resolve

```bash
git clone https://github.com/fusexyz/Atlas
cd Atlas
cargo build --release
```

The binary is at `target/release/compiler.exe`. No build scripts, no code generation, no external tools involved.

Demo examples are in `examples/` — `sysinfo.c` and `sysinfo_format.c` demonstrate a real two-file Windows program compiled with Atlas.

---

## Roadmap

- [ ] `switch` statement lowering to conditional branches
- [ ] `goto` and labeled `break`/`continue`
- [ ] Struct and array initializer lists (`{1, 2, 3}`)
- [ ] Floating-point codegen (SSE2)
- [ ] Bitfield support
- [ ] A proper register allocator (linear scan)
- [ ] Stronger constant propagation and dead-store elimination
- [ ] Debug info (CodeView / PDB)
- [ ] Linux ELF target (System V AMD64 ABI)
- [ ] A test suite with expected-output `.c` files

---

## License

MIT. See [LICENSE](LICENSE).

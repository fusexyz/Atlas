# Hand-Rolled C to PE Compiler

A self-contained C compiler written in Rust that compiles C source directly into native 64-bit Windows PE executables.

There is no LLVM, no NASM, and no external linker in the compile path. The compiler owns the pipeline from source text to PE32+ bytes:

```text
C source -> preprocessor -> lexer -> parser -> IR lowering -> x86-64 codegen -> PE writer -> runnable EXE
```

## What It Can Do

- Compile C directly to a Windows x64 `.exe`
- Emit PE32+ files with `.text`, `.rdata`, `.data`, and `.idata` sections
- Generate x86-64 machine code for the Microsoft x64 ABI
- Build import tables and call external DLL functions through the IAT
- Discover imports from installed Windows SDK/MSVC COFF import libraries
- Preprocess local files and Windows/MSVC headers in memory
- Handle structs, unions, typedefs, enums, arrays, pointers, globals, string literals, and common control flow
- Compile and run the included `sysinfo.c` Windows diagnostic CLI

The compiler no longer writes a debug `preprocessed.c` side file during normal builds.

## Quick Start

```powershell
cargo run -- sysinfo.c out.exe
.\out.exe
```

Expected output includes host name, current user, RAM usage, CPU architecture/core count, page size, and C: drive capacity.

To compile another C file:

```powershell
cargo run -- input.c output.exe
```

For unusual imports that are not present in discovered SDK/MSVC import libraries:

```powershell
cargo run -- input.c output.exe --import MyDll.dll:MyFunction
```

## Example

```c
extern int MessageBoxA(void* hwnd, const char* text, const char* caption, int type);

int main() {
    MessageBoxA(0, "Hello from my compiler!", "My Compiler", 0);
    return 0;
}
```

```powershell
cargo run -- test_msgbox.c msgbox.exe
.\msgbox.exe
```

## Architecture

### Preprocessor

`src/preprocessor/` expands macros, resolves includes, predefines a small MSVC/Windows environment, and discovers installed SDK/MSVC include directories.

### Lexer

`src/lexer/` tokenizes C source into keywords, identifiers, literals, operators, punctuation, and preprocessor-ready token streams.

### Parser

`src/parser/` is a hand-written recursive-descent parser. It builds the AST for declarations, functions, types, expressions, statements, structs/unions, enums, typedefs, and common MSVC syntax.

### IR

`src/ir/` lowers the AST into a simpler intermediate representation with explicit blocks, virtual registers, loads/stores, calls, branches, globals, and string literals.

### Codegen

`src/codegen/` lowers IR into x86-64 machine instructions using the Microsoft x64 calling convention:

- First four integer/pointer args in `RCX`, `RDX`, `R8`, `R9`
- Stack shadow space for calls
- Return values in `RAX`
- RIP-relative access for strings, globals, and imports
- Support for MSVC `__va_start` used by CRT inline varargs wrappers

### Linker / PE Writer

`src/linker/` writes PE32+ binaries directly. It builds headers, sections, import descriptors, hint/name tables, and an entry trampoline that calls `main` and exits through `ExitProcess`.

The import resolver scans installed Windows SDK/MSVC `.lib` files, reads COFF short import objects, and maps referenced symbols to their DLLs. This replaces the old hard-coded function-to-DLL table.

## Current Limits

Known gaps include:

- Incomplete C standard coverage
- Limited diagnostics and type checking
- No object-file output or multi-file linker
- No debug info
- Limited floating-point support
- Partial ABI support for complex aggregate passing/returning
- Limited optimizer behavior
- Partial MSVC/GNU extension coverage

## Project Layout

- `src/main.rs`: CLI and pipeline driver
- `src/preprocessor/`: macro expansion and include handling
- `src/lexer/`: tokenization
- `src/parser/`: AST parser and PCH serialization
- `src/ir/`: IR definitions and lowering
- `src/codegen/`: machine instruction selection and encoding
- `src/linker/`: PE writer and SDK/MSVC import-library resolver
- `sysinfo.c`: Windows system information CLI compiled by this compiler

## Verification

Useful smoke test:

```powershell
cargo check
cargo run -- sysinfo.c out.exe
.\out.exe
```

# Hand-Rolled C to x86-64 PE Compiler

A self-contained C compiler written in Rust that compiles directly to native 64-bit Windows executables. 

This project operates without external toolchain dependencies: there is **no LLVM, no NASM, and no external linker**. Every phase—from scanning to writing PE32+ binaries and generating x86-64 machine instructions—is hand-coded from scratch.

```
C source → Lexer → Parser → IR Lowering → x86-64 Codegen → PE32+ Writer → Runnable EXE
```

---

## Compiler Pipeline Architecture

### 1. Lexical Analysis (`src/lexer/`)
Tokenizes standard C code. It parses numeric constants, identifier keywords, operators, character literals, and string literals, while filtering comments and whitespace.

### 2. Recursive-Descent Parsing (`src/parser/`)
Parses tokens into a typed Abstract Syntax Tree (AST). Handled syntax includes:
* Declarations (functions, locals, pointers, arrays, externs)
* Control flow constructs (`if/else`, `while`, `for`, `return`)
* Expressions with operator precedence climbing

### 3. IR Lowering (`src/ir/`)
Lowers AST nodes into a structured, flat Intermediate Representation (IR) consisting of explicit control-flow blocks and virtual instructions. During this pass, complex statements are normalized and functions are prepared for machine mapping.

### 4. Code Generation & x86-64 Encoding (`src/codegen/`)
Translates IR into concrete x86-64 instructions:
* **Calling Convention**: Adheres to the **Microsoft x64 ABI**, passing the first four arguments in `RCX`, `RDX`, `R8`, and `R9`, allocating 32 bytes of shadow space on the stack, and returning values in `RAX`.
* **String Allocation**: Places string literals in `.rdata` and addresses them using RIP-relative `LEA` instructions.
* **x86-64 Instruction Encoding**: Encodes instructions into binary payloads (handling REX prefixes, ModR/M bytes, and SIB bytes).

### 5. Linker & PE32+ Writer (`src/linker/`)
Directly constructs the Windows Portable Executable (PE32+) format in memory:
* Generates the DOS header, PE signature, COFF header, and PE32+ Optional Header.
* Configures sections:
  * `.text`: Contains executable code and the entry point trampoline.
  * `.rdata`: Contains read-only data, such as static string literals.
  * `.idata`: Houses the Import Directory, Import Address Table (IAT), and Hint/Name tables.
* Implements a custom entry point trampoline that:
  1. Aligns the stack to 16 bytes: `and rsp, -16` (satisfying Windows ABI requirements).
  2. Invokes `main`: `call main`.
  3. Moves the result code: `mov rcx, rax` (passing it as the first parameter to `ExitProcess`).
  4. Reserves shadow stack: `sub rsp, 0x20`.
  5. Dynamically terminates: `call [ExitProcess]` via the IAT.

---

## Features & Supported Syntax

* **Expressions**: Complete arithmetic operations (`+`, `-`, `*`, `/`, `%`), prefix/postfix increments, comparisons, and logical operators.
* **Control Flow**: `if` / `else` conditionals, `while` and `for` loops, and function returns.
* **Data Types**: Integers (`int`, `char`), explicit pointers (e.g. `int*`), and single-dimension arrays.
* **Memory Addressing**: RIP-relative addressing for read-only string data and local stack offset allocation.
* **Dynamic Linking / Imports**: Calls to external Win32 / DLL APIs through the Import Address Table.
* **Built-in DLL Mapping**: Auto-resolves common standard functions (like `printf`, `malloc`, `MessageBoxA`, `ExitProcess`) to their respective Windows libraries (`ucrtbase.dll`, `KERNEL32.dll`, `USER32.dll`).

---

## Usage

Compile a source file directly to a Windows executable:
```powershell
cargo run -- <input.c> <output.exe>
```

### Specifying Custom Imports
If you reference external functions that are not recognized by the built-in library table, map them explicitly via the `--import` flag:
```powershell
cargo run -- input.c output.exe --import MyCustomLib.dll:MyFunctionName
```

---

## Example: Native Windows MessageBox Dialog

The following C program declares `MessageBoxA` as an external symbol and invokes it:

```c
extern int MessageBoxA(void* hwnd, const char* text, const char* caption, int type);

int main() {
    MessageBoxA(0, "Hello from my compiler!", "My Compiler", 0);
    return 0;
}
```

### Compiling and Running
1. Save the snippet as `test_msgbox.c`.
2. Compile the binary:
   ```powershell
   cargo run -- test_msgbox.c msgbox.exe
   ```
3. Run the executable:
   ```powershell
   .\msgbox.exe
   ```

---

## Current Scope Limits

To keep the compiler logic tight and self-contained, the following features are not currently implemented:
* Structures (`struct`) and unions (`union`)
* Type definitions (`typedef` resolution)
* C Preprocessor (`#include`, `#define`, etc.)
* Function signatures with more than 4 parameters (requiring stack-spilled arguments)
* Floating-point calculations (`float` / `double`)
* Global variables

---

## Directory Structure

* [src/main.rs](file:///c:/Users/Ben/Desktop/Github/Compiler/src/main.rs): Entry point, argument parsing, and overall pipeline driver.
* [src/lexer/](file:///c:/Users/Ben/Desktop/Github/Compiler/src/lexer/): Lexer scanner splitting input source into discrete C tokens.
* [src/parser/](file:///c:/Users/Ben/Desktop/Github/Compiler/src/parser/): Recursive-descent parser mapping tokens to AST representations.
* [src/ir/](file:///c:/Users/Ben/Desktop/Github/Compiler/src/ir/): Flat intermediate representation definitions and AST flattening logic.
* [src/codegen/](file:///c:/Users/Ben/Desktop/Github/Compiler/src/codegen/): x86-64 machine instruction selection, register assignment, and REX/ModR/M byte encoding.
* [src/linker/](file:///c:/Users/Ben/Desktop/Github/Compiler/src/linker/): PE32+ header generator, import table builder, and binary assembler.

---

## Project Status & Contributions

This is an educational research project exploring direct machine-code encoding, and link-free PE executable construction.

Contributions, feature extensions, and bug fixes are very welcome! If you would like to help expand the language support (e.g. adding struct handling or global variables), feel free to open an issue or submit a pull request.


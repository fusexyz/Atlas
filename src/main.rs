mod codegen;
mod ir;
mod lexer;
mod linker;
mod parser;

use codegen::{compile_module, encode_function};
use ir::lower_module;
use lexer::Lexer;
use linker::write_pe;
use parser::Parser;

fn builtin_dll(name: &str) -> Option<&'static str> {
    match name {
        "ExitProcess" | "CreateFileA" | "CreateFileW" | "WriteFile" | "ReadFile"
        | "CloseHandle" | "GetLastError" | "GetStdHandle" | "WriteConsoleA" | "WriteConsoleW"
        | "VirtualAlloc" | "VirtualFree" | "GetProcessHeap" | "HeapAlloc" | "HeapFree" => {
            Some("KERNEL32.dll")
        }

        "MessageBoxA" | "MessageBoxW" | "CreateWindowExA" | "CreateWindowExW"
        | "DefWindowProcA" | "DefWindowProcW" | "DispatchMessageA" | "DispatchMessageW"
        | "GetMessageA" | "GetMessageW" | "PostQuitMessage" | "RegisterClassExA"
        | "RegisterClassExW" | "ShowWindow" | "UpdateWindow" | "DestroyWindow" | "LoadIconA"
        | "LoadCursorA" => Some("USER32.dll"),

        "printf" | "puts" | "putchar" | "gets" | "scanf" | "malloc" | "free" | "calloc"
        | "realloc" | "memcpy" | "memset" | "memmove" | "memcmp" | "strlen" | "strcmp"
        | "strcpy" | "strcat" | "strncmp" | "strncpy" | "sprintf" | "snprintf" | "fprintf"
        | "fopen" | "fclose" | "fwrite" | "fread" | "exit" | "abort" => Some("ucrtbase.dll"),
        _ => None,
    }
}

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() < 2 {
        eprintln!("usage: compiler <file.c> [output.exe] [--import DLL:FUNC ...]");
        std::process::exit(1);
    }

    let source = std::fs::read_to_string(&raw_args[1]).unwrap_or_else(|e| {
        eprintln!("error reading '{}': {e}", raw_args[1]);
        std::process::exit(1);
    });

    let mut out_path = "out.exe".to_string();
    let mut cli_imports: Vec<(String, String)> = Vec::new();
    let mut i = 2;
    while i < raw_args.len() {
        if raw_args[i] == "--import" {
            i += 1;
            if i >= raw_args.len() {
                eprintln!("--import requires DLL:FUNC argument");
                std::process::exit(1);
            }
            let spec = &raw_args[i];
            if let Some(colon) = spec.find(':') {
                let dll = spec[..colon].to_string();
                let func = spec[colon + 1..].to_string();
                cli_imports.push((dll, func));
            } else {
                eprintln!("--import expects DLL:FUNC, got '{spec}'");
                std::process::exit(1);
            }
        } else if !raw_args[i].starts_with('-') && out_path == "out.exe" {
            out_path = raw_args[i].clone();
        }
        i += 1;
    }

    let tokens = Lexer::new(&source).tokenize().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let ast = Parser::new(tokens).parse().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let module = lower_module(&ast).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let mut extra_imports: Vec<(String, String)> = cli_imports;
    for func in &module.functions {
        if !func.is_extern {
            continue;
        }
        let already = extra_imports.iter().any(|(_, f)| f == &func.name);
        if already {
            continue;
        }
        if let Some(dll) = builtin_dll(&func.name) {
            extra_imports.push((dll.to_string(), func.name.clone()));
        } else {
            eprintln!(
                "warning: extern '{}' has no known DLL — use --import DLL:{}",
                func.name, func.name
            );
        }
    }
    let extra_imports_ref: Vec<(&str, &str)> = extra_imports
        .iter()
        .map(|(d, f)| (d.as_str(), f.as_str()))
        .collect();

    let machine_funcs = compile_module(&module).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    let mut encoded = Vec::new();
    let mut all_relocs = Vec::new();
    for mf in &machine_funcs {
        let (ef, relocs) = encode_function(&mf.name, &mf.insts).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        all_relocs.push((ef.name.clone(), relocs));
        encoded.push(ef);
    }

    let pe_bytes = write_pe(
        &encoded,
        &all_relocs,
        "main",
        &extra_imports_ref,
        &module.string_lits,
    )
    .unwrap_or_else(|e| {
        eprintln!("pe error: {e}");
        std::process::exit(1);
    });

    std::fs::write(&out_path, &pe_bytes).unwrap_or_else(|e| {
        eprintln!("error writing '{out_path}': {e}");
        std::process::exit(1);
    });

    println!("wrote {} ({} bytes)", out_path, pe_bytes.len());
}

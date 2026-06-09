mod codegen;
mod ir;
mod lexer;
mod linker;
mod parser;
mod preprocessor;

use codegen::{compile_module, encode_function};
use ir::lower_module;
use lexer::Lexer;
use linker::{ImportResolver, write_pe};
use parser::Parser;
use preprocessor::preprocess;

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();
    let mut input_file = None;
    let mut out_path = "out.exe".to_string();
    let mut make_pch_path = None;
    let mut use_pch_path = None;
    let mut cli_imports: Vec<(String, String)> = Vec::new();

    let mut i = 1;
    while i < raw_args.len() {
        if raw_args[i] == "--make-pch" {
            i += 1;
            if i >= raw_args.len() {
                eprintln!("--make-pch requires a file path");
                std::process::exit(1);
            }
            make_pch_path = Some(raw_args[i].clone());
        } else if raw_args[i] == "--use-pch" {
            i += 1;
            if i >= raw_args.len() {
                eprintln!("--use-pch requires a file path");
                std::process::exit(1);
            }
            use_pch_path = Some(raw_args[i].clone());
        } else if raw_args[i] == "--import" {
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
        } else if raw_args[i].starts_with('-') {
            eprintln!("unknown option '{}'", raw_args[i]);
            std::process::exit(1);
        } else {
            if input_file.is_none() {
                input_file = Some(raw_args[i].clone());
            } else if out_path == "out.exe" {
                out_path = raw_args[i].clone();
            }
        }
        i += 1;
    }

    let input_file_str = match input_file {
        Some(f) => f,
        None => {
            eprintln!(
                "usage: compiler <file.c> [output.exe] [--import DLL:FUNC ...] [--make-pch FILE] [--use-pch FILE]"
            );
            std::process::exit(1);
        }
    };

    let source = std::fs::read_to_string(&input_file_str).unwrap_or_else(|e| {
        eprintln!("error reading '{}': {e}", input_file_str);
        std::process::exit(1);
    });

    let filepath = std::path::Path::new(&input_file_str);
    let mut include_dirs = Vec::new();
    if let Some(parent) = filepath.parent() {
        include_dirs.push(parent.to_path_buf());
    }
    include_dirs.extend(preprocessor::discover_system_includes());

    let mut defines = std::collections::HashMap::new();
    let mut typedef_names = std::collections::HashSet::new();
    let mut pch_ast = None;

    if let Some(ref pch_path) = use_pch_path {
        println!("loading pch: {}", pch_path);
        let loaded = parser::pch::load_pch(
            std::path::Path::new(pch_path),
            &mut defines,
            &mut typedef_names,
        )
        .unwrap_or_else(|e| {
            eprintln!("pch load error: {e}");
            std::process::exit(1);
        });
        pch_ast = Some(loaded);
    } else {
        preprocessor::predefine_system_macros(&mut defines);
    }

    let mut skip_headers = std::collections::HashSet::new();
    if let Some(ref pch_path) = use_pch_path {
        let path = std::path::Path::new(pch_path);
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            skip_headers.insert(format!("{}.h", stem));
            skip_headers.insert(format!("{}.gch", stem));
        }
    }

    let mut active_includes = std::collections::HashSet::new();
    let preprocessed_source = preprocess(
        &source,
        filepath,
        &include_dirs,
        &mut defines,
        &mut active_includes,
        &skip_headers,
    )
    .unwrap_or_else(|e| {
        eprintln!("preprocessor error: {e}");
        std::process::exit(1);
    });

    let tokens = Lexer::new(&preprocessed_source)
        .tokenize()
        .unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });

    let mut parser = Parser::new(tokens);
    if !typedef_names.is_empty() {
        parser.typedef_names = typedef_names.clone();
    }

    let mut ast = parser.parse().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    if let Some(ref pch_path) = make_pch_path {
        println!("creating pch: {}", pch_path);
        parser::pch::save_pch(
            std::path::Path::new(pch_path),
            &defines,
            &parser.typedef_names,
            &ast,
        )
        .unwrap_or_else(|e| {
            eprintln!("pch save error: {e}");
            std::process::exit(1);
        });
        println!("wrote pch to {}", pch_path);
        return;
    }

    if let Some(mut pch) = pch_ast {
        pch.items.extend(ast.items);
        pch.enum_constants.extend(ast.enum_constants);
        ast = pch;
    }

    let module = lower_module(&ast).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

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

    let import_resolver = ImportResolver::discover();
    let mut local_symbols: std::collections::HashSet<&str> =
        encoded.iter().map(|f| f.name.as_str()).collect();
    local_symbols.extend(module.string_lits.iter().map(|(name, _)| name.as_str()));
    local_symbols.extend(
        module
            .globals
            .iter()
            .filter(|g| !g.is_extern)
            .map(|g| g.name.as_str()),
    );
    let mut extra_imports: Vec<(String, String)> = cli_imports;
    for (_, relocs) in &all_relocs {
        for reloc in relocs {
            if local_symbols.contains(reloc.symbol.as_str()) {
                continue;
            }
            let already = extra_imports.iter().any(|(_, f)| f == &reloc.symbol);
            if already {
                continue;
            }
            if let Some(dll) = import_resolver.resolve(&reloc.symbol) {
                extra_imports.push((dll.to_string(), reloc.symbol.clone()));
            } else {
                eprintln!(
                    "warning: referenced extern '{}' was not found in discovered import libraries; use --import DLL:{}",
                    reloc.symbol, reloc.symbol
                );
            }
        }
    }
    let extra_imports_ref: Vec<(&str, &str)> = extra_imports
        .iter()
        .map(|(d, f)| (d.as_str(), f.as_str()))
        .collect();

    let pe_bytes = write_pe(
        &encoded,
        &all_relocs,
        "main",
        &extra_imports_ref,
        &module.string_lits,
        &module.globals,
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

mod codegen;
mod ir;
mod lexer;
mod linker;
mod parser;
mod preprocessor;
mod sema;

use codegen::{compile_module, encode_function};
use ir::lower_module;
use lexer::Lexer;
use linker::{
    ImportResolver, ObjectFile, merge_objects, read_object, retain_reachable_symbols, write_object,
    write_pe,
};
use parser::Parser;
use preprocessor::preprocess;
use sema::check_translation_unit;
use std::collections::HashSet;
use std::path::Path;

struct Args {
    inputs: Vec<String>,
    out_path: Option<String>,
    compile_only: bool,
    make_pch_path: Option<String>,
    use_pch_path: Option<String>,
    cli_imports: Vec<(String, String)>,
}

fn main() {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    if let Some(pch_path) = args.make_pch_path.as_deref() {
        if args.inputs.len() != 1 {
            eprintln!("--make-pch requires exactly one C input");
            std::process::exit(1);
        }
        create_pch(&args.inputs[0], pch_path).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        println!("wrote pch to {pch_path}");
        return;
    }

    if args.compile_only {
        if args.inputs.len() != 1 || !is_c_file(&args.inputs[0]) {
            eprintln!("-c requires exactly one C input");
            std::process::exit(1);
        }
        let out_path = args
            .out_path
            .clone()
            .unwrap_or_else(|| default_obj_path(&args.inputs[0]));
        let obj = compile_c_to_object(&args.inputs[0], args.use_pch_path.as_deref())
            .unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            });
        write_object(&out_path, &obj).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1);
        });
        println!("wrote {} ({} functions)", out_path, obj.funcs.len());
        return;
    }

    let out_path = args.out_path.as_deref().unwrap_or("out.exe");
    let mut objects = Vec::new();
    for input in &args.inputs {
        if is_obj_file(input) {
            objects.push(read_object(input).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            }));
        } else if is_c_file(input) {
            objects.push(
                compile_c_to_object(input, args.use_pch_path.as_deref()).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(1);
                }),
            );
        } else {
            eprintln!("unsupported input '{input}'");
            std::process::exit(1);
        }
    }

    let mut obj = merge_objects(objects).unwrap_or_else(|e| {
        eprintln!("link error: {e}");
        std::process::exit(1);
    });
    let pruned = retain_reachable_symbols(&mut obj, "main").unwrap_or_else(|e| {
        eprintln!("link error: {e}");
        std::process::exit(1);
    });
    if pruned.functions > 0 || pruned.strings > 0 || pruned.globals > 0 {
        println!(
            "link: removed {} functions, {} strings, {} globals",
            pruned.functions, pruned.strings, pruned.globals
        );
    }
    let pe = link_object_to_pe(&obj, &args.cli_imports).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    std::fs::write(out_path, &pe).unwrap_or_else(|e| {
        eprintln!("error writing '{out_path}': {e}");
        std::process::exit(1);
    });
    println!("wrote {} ({} bytes)", out_path, pe.len());
}

fn parse_args() -> Result<Args, String> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut inputs = Vec::new();
    let mut out_path = None;
    let mut compile_only = false;
    let mut make_pch_path = None;
    let mut use_pch_path = None;
    let mut cli_imports = Vec::new();

    let mut i = 0;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "-c" => compile_only = true,
            "-o" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("-o requires an output path".into());
                }
                out_path = Some(raw_args[i].clone());
            }
            "--make-pch" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--make-pch requires a file path".into());
                }
                make_pch_path = Some(raw_args[i].clone());
            }
            "--use-pch" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--use-pch requires a file path".into());
                }
                use_pch_path = Some(raw_args[i].clone());
            }
            "--import" => {
                i += 1;
                if i >= raw_args.len() {
                    return Err("--import requires DLL:FUNC argument".into());
                }
                let spec = &raw_args[i];
                let colon = spec
                    .find(':')
                    .ok_or_else(|| format!("--import expects DLL:FUNC, got '{spec}'"))?;
                cli_imports.push((spec[..colon].to_string(), spec[colon + 1..].to_string()));
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option '{flag}'")),
            input => inputs.push(input.to_string()),
        }
        i += 1;
    }

    if inputs.is_empty() {
        return Err(usage());
    }

    if out_path.is_none() && inputs.len() > 1 {
        let last = inputs.last().unwrap();
        if (!compile_only && looks_like_output_exe(last)) || (compile_only && is_obj_file(last)) {
            out_path = inputs.pop();
        }
    }

    Ok(Args {
        inputs,
        out_path,
        compile_only,
        make_pch_path,
        use_pch_path,
        cli_imports,
    })
}

fn usage() -> String {
    "usage: compiler [-c] <file.c|file.obj>... [-o output] [--import DLL:FUNC ...] [--make-pch FILE] [--use-pch FILE]".into()
}

fn create_pch(input: &str, pch_path: &str) -> Result<(), String> {
    let source =
        std::fs::read_to_string(input).map_err(|e| format!("error reading '{input}': {e}"))?;
    let filepath = Path::new(input);
    let mut include_dirs = include_dirs_for(filepath);
    include_dirs.extend(preprocessor::discover_system_includes());
    let mut defines = std::collections::HashMap::new();
    preprocessor::predefine_system_macros(&mut defines);
    let mut active_includes = HashSet::new();
    let skip_headers = HashSet::new();
    let preprocessed = preprocess(
        &source,
        filepath,
        &include_dirs,
        &mut defines,
        &mut active_includes,
        &skip_headers,
    )
    .map_err(|e| format!("preprocessor error: {e}"))?;
    let tokens = Lexer::new(&preprocessed)
        .tokenize()
        .map_err(|e| e.to_string())?;
    let mut parser = Parser::new(tokens);
    let ast = parser.parse().map_err(|e| e.to_string())?;
    parser::pch::save_pch(Path::new(pch_path), &defines, &parser.typedef_names, &ast)
        .map_err(|e| format!("pch save error: {e}"))
}

fn compile_c_to_object(input: &str, use_pch_path: Option<&str>) -> Result<ObjectFile, String> {
    let ast = parse_c(input, use_pch_path)?;
    check_translation_unit(&ast).map_err(|e| e.to_string())?;
    let module = lower_module(&ast).map_err(|e| e.to_string())?;
    let machine_funcs = compile_module(&module).map_err(|e| e.to_string())?;

    let mut funcs = Vec::new();
    let mut relocs = Vec::new();
    for mf in &machine_funcs {
        let (ef, rs) = encode_function(&mf.name, &mf.insts).map_err(|e| e.to_string())?;
        relocs.push((ef.name.clone(), rs));
        funcs.push(ef);
    }

    Ok(ObjectFile {
        funcs,
        relocs,
        string_lits: module.string_lits,
        globals: module.globals,
    })
}

fn parse_c(
    input: &str,
    use_pch_path: Option<&str>,
) -> Result<parser::ast::TranslationUnit, String> {
    let source =
        std::fs::read_to_string(input).map_err(|e| format!("error reading '{input}': {e}"))?;
    let filepath = Path::new(input);
    let mut include_dirs = include_dirs_for(filepath);
    include_dirs.extend(preprocessor::discover_system_includes());
    let mut defines = std::collections::HashMap::new();
    let mut typedef_names = std::collections::HashSet::new();
    let mut pch_ast = None;

    if let Some(pch_path) = use_pch_path {
        let loaded = parser::pch::load_pch(Path::new(pch_path), &mut defines, &mut typedef_names)
            .map_err(|e| format!("pch load error: {e}"))?;
        pch_ast = Some(loaded);
    } else {
        preprocessor::predefine_system_macros(&mut defines);
    }

    let mut skip_headers = std::collections::HashSet::new();
    if let Some(pch_path) = use_pch_path {
        let path = Path::new(pch_path);
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
    .map_err(|e| format!("preprocessor error: {e}"))?;

    let tokens = Lexer::new(&preprocessed_source)
        .tokenize()
        .map_err(|e| e.to_string())?;
    let mut parser = Parser::new(tokens);
    if !typedef_names.is_empty() {
        parser.typedef_names = typedef_names;
    }
    let mut ast = parser.parse().map_err(|e| e.to_string())?;

    if let Some(mut pch) = pch_ast {
        pch.items.extend(ast.items);
        pch.enum_constants.extend(ast.enum_constants);
        ast = pch;
    }
    Ok(ast)
}

fn link_object_to_pe(
    obj: &ObjectFile,
    cli_imports: &[(String, String)],
) -> Result<Vec<u8>, String> {
    let import_resolver = ImportResolver::discover();
    let mut local_symbols: HashSet<&str> = obj.funcs.iter().map(|f| f.name.as_str()).collect();
    local_symbols.extend(obj.string_lits.iter().map(|(name, _)| name.as_str()));
    local_symbols.extend(
        obj.globals
            .iter()
            .filter(|g| !g.is_extern)
            .map(|g| g.name.as_str()),
    );

    let mut extra_imports = cli_imports.to_vec();
    for (_, relocs) in &obj.relocs {
        for reloc in relocs {
            if local_symbols.contains(reloc.symbol.as_str()) {
                continue;
            }
            if extra_imports.iter().any(|(_, f)| f == &reloc.symbol) {
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

    write_pe(
        &obj.funcs,
        &obj.relocs,
        "main",
        &extra_imports_ref,
        &obj.string_lits,
        &obj.globals,
    )
    .map_err(|e| format!("pe error: {e}"))
}

fn include_dirs_for(path: &Path) -> Vec<std::path::PathBuf> {
    path.parent().into_iter().map(|p| p.to_path_buf()).collect()
}

fn default_obj_path(input: &str) -> String {
    let path = Path::new(input);
    path.with_extension("obj").to_string_lossy().into_owned()
}

fn is_c_file(path: &str) -> bool {
    extension_eq(path, "c")
}

fn is_obj_file(path: &str) -> bool {
    extension_eq(path, "obj")
}

fn looks_like_output_exe(path: &str) -> bool {
    extension_eq(path, "exe")
}

fn extension_eq(path: &str, ext: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(ext))
}

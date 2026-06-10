use crate::codegen::{EncodedFunction, Reloc};
use crate::ir::ir::{Global, IrType, Value};
use std::io::{Read, Write};

const MAGIC: &[u8; 8] = b"CCOBJ001";

#[derive(Debug, Default)]
pub struct ObjectFile {
    pub funcs: Vec<EncodedFunction>,
    pub relocs: Vec<(String, Vec<Reloc>)>,
    pub string_lits: Vec<(String, Vec<u8>)>,
    pub globals: Vec<Global>,
}

pub fn write_object(path: &str, obj: &ObjectFile) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("error creating '{path}': {e}"))?;
    let mut w = std::io::BufWriter::new(file);
    write_obj(&mut w, obj).map_err(|e| format!("error writing '{path}': {e}"))
}

pub fn read_object(path: &str) -> Result<ObjectFile, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("error opening '{path}': {e}"))?;
    let mut r = std::io::BufReader::new(file);
    read_obj(&mut r).map_err(|e| format!("error reading '{path}': {e}"))
}

pub fn merge_objects(mut objects: Vec<ObjectFile>) -> Result<ObjectFile, String> {
    let mut merged = ObjectFile::default();
    for (idx, obj) in objects.iter_mut().enumerate() {
        let string_names: std::collections::HashSet<String> = obj
            .string_lits
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        for (name, _) in &mut obj.string_lits {
            *name = object_local_name(idx, name);
        }
        for (_, relocs) in &mut obj.relocs {
            for reloc in relocs {
                if string_names.contains(&reloc.symbol) {
                    reloc.symbol = object_local_name(idx, &reloc.symbol);
                }
            }
        }
        merged.funcs.append(&mut obj.funcs);
        merged.relocs.append(&mut obj.relocs);
        merged.string_lits.append(&mut obj.string_lits);
        merged.globals.append(&mut obj.globals);
    }
    reject_duplicate_symbols(&merged)?;
    Ok(merged)
}

pub struct RetainStats {
    pub functions: usize,
    pub strings: usize,
    pub globals: usize,
}

pub fn retain_reachable_symbols(obj: &mut ObjectFile, entry: &str) -> Result<RetainStats, String> {
    let funcs: std::collections::HashSet<String> =
        obj.funcs.iter().map(|f| f.name.clone()).collect();
    if !funcs.contains(entry) {
        return Err(format!("entry point '{entry}' not found"));
    }

    let reloc_map: std::collections::HashMap<String, Vec<String>> = obj
        .relocs
        .iter()
        .map(|(name, relocs)| {
            (
                name.clone(),
                relocs.iter().map(|r| r.symbol.clone()).collect::<Vec<_>>(),
            )
        })
        .collect();

    let mut reachable = std::collections::HashSet::new();
    let mut stack = vec![entry.to_string()];
    while let Some(name) = stack.pop() {
        if !reachable.insert(name.clone()) {
            continue;
        }
        if let Some(symbols) = reloc_map.get(&name) {
            for symbol in symbols {
                if funcs.contains(symbol) && !reachable.contains(symbol) {
                    stack.push(symbol.clone());
                }
            }
        }
    }

    let used_symbols: std::collections::HashSet<String> = obj
        .relocs
        .iter()
        .filter(|(name, _)| reachable.contains(name))
        .flat_map(|(_, relocs)| relocs.iter().map(|r| r.symbol.clone()))
        .collect();

    let before_funcs = obj.funcs.len();
    let before_strings = obj.string_lits.len();
    let before_globals = obj.globals.len();

    obj.funcs.retain(|f| reachable.contains(&f.name));
    obj.relocs.retain(|(name, _)| reachable.contains(name));
    obj.string_lits
        .retain(|(name, _)| used_symbols.contains(name));
    obj.globals
        .retain(|g| g.is_extern || used_symbols.contains(&g.name));

    Ok(RetainStats {
        functions: before_funcs - obj.funcs.len(),
        strings: before_strings - obj.string_lits.len(),
        globals: before_globals - obj.globals.len(),
    })
}

fn object_local_name(idx: usize, name: &str) -> String {
    format!(".Lobj{idx}_{name}")
}

fn reject_duplicate_symbols(obj: &ObjectFile) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for f in &obj.funcs {
        if !seen.insert(f.name.clone()) {
            return Err(format!("duplicate function symbol '{}'", f.name));
        }
    }
    for g in obj.globals.iter().filter(|g| !g.is_extern) {
        if !seen.insert(g.name.clone()) {
            return Err(format!("duplicate global symbol '{}'", g.name));
        }
    }
    Ok(())
}

fn write_obj<W: Write>(w: &mut W, obj: &ObjectFile) -> std::io::Result<()> {
    w.write_all(MAGIC)?;
    write_u32(w, obj.funcs.len() as u32)?;
    for f in &obj.funcs {
        write_str(w, &f.name)?;
        write_bytes(w, &f.bytes)?;
    }
    write_u32(w, obj.relocs.len() as u32)?;
    for (name, relocs) in &obj.relocs {
        write_str(w, name)?;
        write_u32(w, relocs.len() as u32)?;
        for r in relocs {
            write_u64(w, r.offset as u64)?;
            write_str(w, &r.symbol)?;
        }
    }
    write_u32(w, obj.string_lits.len() as u32)?;
    for (name, bytes) in &obj.string_lits {
        write_str(w, name)?;
        write_bytes(w, bytes)?;
    }
    write_u32(w, obj.globals.len() as u32)?;
    for g in &obj.globals {
        write_str(w, &g.name)?;
        write_ir_type(w, &g.ty)?;
        write_bool(w, g.is_extern)?;
        match &g.init {
            Some(v) => {
                write_bool(w, true)?;
                write_value(w, v)?;
            }
            None => write_bool(w, false)?,
        }
    }
    Ok(())
}

fn read_obj<R: Read>(r: &mut R) -> std::io::Result<ObjectFile> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid object file magic",
        ));
    }
    let mut obj = ObjectFile::default();
    for _ in 0..read_u32(r)? {
        obj.funcs.push(EncodedFunction {
            name: read_str(r)?,
            bytes: read_bytes(r)?,
        });
    }
    for _ in 0..read_u32(r)? {
        let name = read_str(r)?;
        let mut relocs = Vec::new();
        for _ in 0..read_u32(r)? {
            relocs.push(Reloc {
                offset: read_u64(r)? as usize,
                symbol: read_str(r)?,
            });
        }
        obj.relocs.push((name, relocs));
    }
    for _ in 0..read_u32(r)? {
        obj.string_lits.push((read_str(r)?, read_bytes(r)?));
    }
    for _ in 0..read_u32(r)? {
        let name = read_str(r)?;
        let ty = read_ir_type(r)?;
        let is_extern = read_bool(r)?;
        let init = if read_bool(r)? {
            Some(read_value(r)?)
        } else {
            None
        };
        obj.globals.push(Global {
            name,
            ty,
            init,
            is_extern,
        });
    }
    Ok(obj)
}

fn write_ir_type<W: Write>(w: &mut W, ty: &IrType) -> std::io::Result<()> {
    match ty {
        IrType::Void => write_u8(w, 0),
        IrType::I8 => write_u8(w, 1),
        IrType::I16 => write_u8(w, 2),
        IrType::I32 => write_u8(w, 3),
        IrType::I64 => write_u8(w, 4),
        IrType::F32 => write_u8(w, 5),
        IrType::F64 => write_u8(w, 6),
        IrType::Ptr(inner) => {
            write_u8(w, 7)?;
            write_ir_type(w, inner)
        }
        IrType::Array(inner, n) => {
            write_u8(w, 8)?;
            write_ir_type(w, inner)?;
            write_u64(w, *n)
        }
    }
}

fn read_ir_type<R: Read>(r: &mut R) -> std::io::Result<IrType> {
    Ok(match read_u8(r)? {
        0 => IrType::Void,
        1 => IrType::I8,
        2 => IrType::I16,
        3 => IrType::I32,
        4 => IrType::I64,
        5 => IrType::F32,
        6 => IrType::F64,
        7 => IrType::Ptr(Box::new(read_ir_type(r)?)),
        8 => {
            let inner = read_ir_type(r)?;
            let n = read_u64(r)?;
            IrType::Array(Box::new(inner), n)
        }
        tag => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid IR type tag {tag}"),
            ));
        }
    })
}

fn write_value<W: Write>(w: &mut W, value: &Value) -> std::io::Result<()> {
    match value {
        Value::ImmI(v) => {
            write_u8(w, 0)?;
            write_i64(w, *v)
        }
        Value::ImmF(v) => {
            write_u8(w, 1)?;
            write_u64(w, v.to_bits())
        }
        Value::Global(name) => {
            write_u8(w, 2)?;
            write_str(w, name)
        }
        Value::Reg(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "cannot serialize register value in object global",
        )),
    }
}

fn read_value<R: Read>(r: &mut R) -> std::io::Result<Value> {
    Ok(match read_u8(r)? {
        0 => Value::ImmI(read_i64(r)?),
        1 => Value::ImmF(f64::from_bits(read_u64(r)?)),
        2 => Value::Global(read_str(r)?),
        tag => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid value tag {tag}"),
            ));
        }
    })
}

fn write_bool<W: Write>(w: &mut W, v: bool) -> std::io::Result<()> {
    write_u8(w, if v { 1 } else { 0 })
}

fn read_bool<R: Read>(r: &mut R) -> std::io::Result<bool> {
    Ok(read_u8(r)? != 0)
}

fn write_bytes<W: Write>(w: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    write_u32(w, bytes.len() as u32)?;
    w.write_all(bytes)
}

fn read_bytes<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let len = read_u32(r)? as usize;
    let mut bytes = vec![0u8; len];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_str<W: Write>(w: &mut W, s: &str) -> std::io::Result<()> {
    write_bytes(w, s.as_bytes())
}

fn read_str<R: Read>(r: &mut R) -> std::io::Result<String> {
    String::from_utf8(read_bytes(r)?)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn write_u8<W: Write>(w: &mut W, v: u8) -> std::io::Result<()> {
    w.write_all(&[v])
}

fn read_u8<R: Read>(r: &mut R) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn write_u32<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_u32<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn write_i64<W: Write>(w: &mut W, v: i64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_i64<R: Read>(r: &mut R) -> std::io::Result<i64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(i64::from_le_bytes(b))
}

fn write_u64<W: Write>(w: &mut W, v: u64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn read_u64<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

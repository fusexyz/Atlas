use crate::codegen::{EncodedFunction, Reloc};
use crate::ir::ir::{Global, Value};
use std::collections::HashMap;

const FILE_ALIGN: u32 = 0x200;
const SECT_ALIGN: u32 = 0x1000;
const IMAGE_BASE: u64 = 0x0000_0001_4000_0000;

fn align_up(v: u32, a: u32) -> u32 {
    (v + a - 1) / a * a
}

fn put_u16(b: &mut [u8], off: usize, v: u16) {
    b[off..off + 2].copy_from_slice(&v.to_le_bytes());
}
fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}
fn put_i32(b: &mut [u8], off: usize, v: i32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn write_pe(
    funcs: &[EncodedFunction],
    func_relocs: &[(String, Vec<Reloc>)],
    entry: &str,
    extra_imports: &[(&str, &str)],
    string_lits: &[(String, Vec<u8>)],
    globals: &[Global],
) -> Result<Vec<u8>, String> {
    let mut all_imports: Vec<(&str, &str)> = extra_imports.to_vec();
    if !all_imports.iter().any(|(_, s)| *s == "ExitProcess") {
        all_imports.push(("KERNEL32.dll", "ExitProcess"));
    }
    let mut by_dll: Vec<(&str, Vec<&str>)> = Vec::new();
    for &(dll, sym) in &all_imports {
        match by_dll.iter_mut().find(|(d, _)| *d == dll) {
            Some((_, syms)) => {
                if !syms.contains(&sym) {
                    syms.push(sym);
                }
            }
            None => by_dll.push((dll, vec![sym])),
        }
    }

    let text_va: u32 = SECT_ALIGN;
    let text_raw: u32 = 0x200;

    let mut text: Vec<u8> = Vec::new();
    let mut func_base: HashMap<String, usize> = HashMap::new();

    for f in funcs {
        let base = text.len();
        func_base.insert(f.name.clone(), base);
        text.extend_from_slice(&f.bytes);
    }

    let main_base = *func_base
        .get(entry)
        .ok_or_else(|| format!("entry point '{entry}' not found"))?;

    let text_end_va: u32 = text_va + align_up(text.len() as u32 + 64, SECT_ALIGN);
    let has_rdata = !string_lits.is_empty();
    let rdata_va: u32 = text_end_va;
    let mut rdata_syms: HashMap<String, u32> = HashMap::new();
    let mut rdata_bytes: Vec<u8> = Vec::new();
    for (name, data) in string_lits {
        let sym_rva = rdata_va + rdata_bytes.len() as u32;
        rdata_syms.insert(name.clone(), sym_rva);
        rdata_bytes.extend_from_slice(data);
    }
    let rdata_vsize = rdata_bytes.len() as u32;
    while rdata_bytes.len() % FILE_ALIGN as usize != 0 {
        rdata_bytes.push(0);
    }
    let rdata_raw_size = rdata_bytes.len() as u32;

    let has_data = globals.iter().any(|g| !g.is_extern);
    let data_va: u32 = if has_rdata {
        rdata_va + align_up(rdata_vsize, SECT_ALIGN)
    } else {
        text_end_va
    };

    let mut data_bytes: Vec<u8> = Vec::new();
    let mut data_syms: HashMap<String, u32> = HashMap::new();
    if has_data {
        for g in globals {
            if g.is_extern {
                continue;
            }
            let size = g.ty.size_bytes();
            let align = match size {
                0 => 1,
                1 => 1,
                2 => 2,
                3 | 4 => 4,
                _ => 8,
            };
            while data_bytes.len() as u64 % align != 0 {
                data_bytes.push(0);
            }
            let sym_rva = data_va + data_bytes.len() as u32;
            data_syms.insert(g.name.clone(), sym_rva);

            let start_offset = data_bytes.len();
            data_bytes.resize(start_offset + size as usize, 0);

            if let Some(init_val) = &g.init {
                match init_val {
                    Value::ImmI(val) => match size {
                        1 => data_bytes[start_offset] = *val as u8,
                        2 => data_bytes[start_offset..start_offset + 2]
                            .copy_from_slice(&(*val as u16).to_le_bytes()),
                        4 => data_bytes[start_offset..start_offset + 4]
                            .copy_from_slice(&(*val as u32).to_le_bytes()),
                        8 => data_bytes[start_offset..start_offset + 8]
                            .copy_from_slice(&(*val as u64).to_le_bytes()),
                        _ => {
                            let write_len = size.min(8) as usize;
                            let val_bytes = val.to_le_bytes();
                            data_bytes[start_offset..start_offset + write_len]
                                .copy_from_slice(&val_bytes[..write_len]);
                        }
                    },
                    Value::ImmF(val) => {
                        if size == 4 {
                            data_bytes[start_offset..start_offset + 4]
                                .copy_from_slice(&((*val as f32).to_bits()).to_le_bytes());
                        } else if size == 8 {
                            data_bytes[start_offset..start_offset + 8]
                                .copy_from_slice(&(val.to_bits()).to_le_bytes());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let data_vsize = data_bytes.len() as u32;
    while data_bytes.len() % FILE_ALIGN as usize != 0 {
        data_bytes.push(0);
    }
    let data_raw_size = data_bytes.len() as u32;

    let idata_va: u32 = if has_data {
        data_va + align_up(data_vsize, SECT_ALIGN)
    } else if has_rdata {
        rdata_va + align_up(rdata_vsize, SECT_ALIGN)
    } else {
        text_end_va
    };

    let mut iat_slots: HashMap<String, u32> = HashMap::new();
    {
        let idt_size = (by_dll.len() + 1) as u32 * 20;
        let mut cur = idata_va + idt_size;
        for (_, syms) in &by_dll {
            for &sym in syms {
                iat_slots.insert(sym.to_string(), cur);
                cur += 8;
            }
            cur += 8;
        }
    }

    for (fname, relocs) in func_relocs {
        let fbase = *func_base
            .get(fname.as_str())
            .ok_or_else(|| format!("reloc: unknown function '{fname}'"))?;
        for r in relocs {
            let slot_abs = fbase + r.offset;

            let sym_rva: u64 = if let Some(&iat_rva) = iat_slots.get(&r.symbol) {
                iat_rva as u64
            } else if let Some(&f_off) = func_base.get(&r.symbol) {
                text_va as u64 + f_off as u64
            } else if let Some(&rdata_rva) = rdata_syms.get(&r.symbol) {
                rdata_rva as u64
            } else if let Some(&data_rva) = data_syms.get(&r.symbol) {
                data_rva as u64
            } else {
                return Err(format!("reloc: unknown symbol '{}'", r.symbol));
            };

            let instr_end_rva = text_va as u64 + slot_abs as u64 + 4;
            let rel32 = (sym_rva as i64 - instr_end_rva as i64) as i32;
            put_i32(&mut text, slot_abs, rel32);
        }
    }

    let tramp_off = text.len();
    let tramp_rva = text_va as u64 + tramp_off as u64;
    let ep_rva = tramp_rva as u32;

    text.extend_from_slice(&[0x48, 0x83, 0xE4, 0xF0]);

    {
        let after_va = IMAGE_BASE + tramp_rva + 4 + 5;
        let target_va = IMAGE_BASE + text_va as u64 + main_base as u64;
        let rel = (target_va as i64 - after_va as i64) as i32;
        text.push(0xE8);
        text.extend_from_slice(&rel.to_le_bytes());
    }

    text.extend_from_slice(&[0x48, 0x89, 0xC1]);

    text.extend_from_slice(&[0x48, 0x83, 0xEC, 0x20]);

    {
        let cur_off = text.len();

        let instr_end_rva = text_va as u64 + cur_off as u64 + 6;
        let ep_iat_rva = *iat_slots
            .get("ExitProcess")
            .ok_or("ExitProcess not in IAT")?;
        let rel = (ep_iat_rva as i64 - instr_end_rva as i64) as i32;
        text.extend_from_slice(&[0xFF, 0x15]);
        text.extend_from_slice(&rel.to_le_bytes());
    }

    text.push(0xCC);

    let text_vsize = text.len() as u32;
    while text.len() % FILE_ALIGN as usize != 0 {
        text.push(0);
    }
    let text_raw_size = text.len() as u32;

    let idata_bytes = build_idata(idata_va, &by_dll);
    let idata_vsize = idata_bytes.len() as u32;
    let rdata_raw = text_raw + text_raw_size;
    let data_raw = if has_rdata {
        rdata_raw + rdata_raw_size
    } else {
        rdata_raw
    };
    let idata_raw = if has_data {
        data_raw + data_raw_size
    } else {
        data_raw
    };

    let mut idata_padded = idata_bytes;
    while idata_padded.len() % FILE_ALIGN as usize != 0 {
        idata_padded.push(0);
    }
    let idata_raw_size = idata_padded.len() as u32;

    let size_of_image = align_up(idata_va + align_up(idata_vsize, SECT_ALIGN), SECT_ALIGN);

    let idt_size = (by_dll.len() + 1) as u32 * 20;
    let iat_total = by_dll
        .iter()
        .map(|(_, s)| (s.len() + 1) as u32 * 8)
        .sum::<u32>();
    let iat_start_va = idata_va + idt_size;

    let mut pe: Vec<u8> = Vec::new();

    let mut dos = [0u8; 64];
    dos[0] = 0x4D;
    dos[1] = 0x5A;
    dos[60] = 64;
    pe.extend_from_slice(&dos);

    pe.extend_from_slice(b"PE\0\0");

    let nsections: u16 = {
        let mut n = 2;
        if has_rdata {
            n += 1;
        }
        if has_data {
            n += 1;
        }
        n
    };

    let mut coff = [0u8; 20];
    put_u16(&mut coff, 0, 0x8664);
    put_u16(&mut coff, 2, nsections);
    put_u16(&mut coff, 16, 240);
    put_u16(&mut coff, 18, 0x0022);
    pe.extend_from_slice(&coff);

    let mut opt = vec![0u8; 240];
    put_u16(&mut opt, 0, 0x020B);
    opt[2] = 14;
    put_u32(&mut opt, 4, text_vsize);
    let mut init_data_size = idata_vsize;
    if has_rdata {
        init_data_size += rdata_vsize;
    }
    if has_data {
        init_data_size += data_vsize;
    }
    put_u32(&mut opt, 8, init_data_size);
    put_u32(&mut opt, 16, ep_rva);
    put_u32(&mut opt, 20, text_va);
    put_u64(&mut opt, 24, IMAGE_BASE);
    put_u32(&mut opt, 32, SECT_ALIGN);
    put_u32(&mut opt, 36, FILE_ALIGN);
    put_u16(&mut opt, 40, 6);
    put_u16(&mut opt, 48, 6);
    put_u32(&mut opt, 56, size_of_image);
    put_u32(&mut opt, 60, 0x200);
    put_u16(&mut opt, 68, 3);
    put_u16(&mut opt, 70, 0x0140);
    put_u64(&mut opt, 72, 0x100000);
    put_u64(&mut opt, 80, 0x1000);
    put_u64(&mut opt, 88, 0x100000);
    put_u64(&mut opt, 96, 0x1000);
    put_u32(&mut opt, 108, 16);

    put_u32(&mut opt, 120, idata_va);
    put_u32(&mut opt, 124, idata_vsize);

    put_u32(&mut opt, 208, iat_start_va);
    put_u32(&mut opt, 212, iat_total);
    pe.extend_from_slice(&opt);

    pe.extend_from_slice(&section_entry(
        b".text\0\0\0",
        text_vsize,
        text_va,
        text_raw_size,
        text_raw,
        0x60000020,
    ));
    if has_rdata {
        pe.extend_from_slice(&section_entry(
            b".rdata\0\0",
            rdata_vsize,
            rdata_va,
            rdata_raw_size,
            rdata_raw,
            0x40000040,
        ));
    }
    if has_data {
        pe.extend_from_slice(&section_entry(
            b".data\0\0\0",
            data_vsize,
            data_va,
            data_raw_size,
            data_raw,
            0xC0000040,
        ));
    }
    pe.extend_from_slice(&section_entry(
        b".idata\0\0",
        idata_vsize,
        idata_va,
        idata_raw_size,
        idata_raw,
        0xC0000040,
    ));

    while pe.len() < text_raw as usize {
        pe.push(0);
    }

    pe.extend_from_slice(&text);
    if has_rdata {
        pe.extend_from_slice(&rdata_bytes);
    }
    if has_data {
        pe.extend_from_slice(&data_bytes);
    }
    pe.extend_from_slice(&idata_padded);

    Ok(pe)
}

fn build_idata(idata_va: u32, by_dll: &[(&str, Vec<&str>)]) -> Vec<u8> {
    let idt_size: u32 = (by_dll.len() + 1) as u32 * 20;
    let iat_size: u32 = by_dll.iter().map(|(_, s)| (s.len() + 1) as u32 * 8).sum();

    let mut hint_data: Vec<u8> = Vec::new();
    let mut hint_offs: Vec<Vec<usize>> = Vec::new();

    for (_, syms) in by_dll {
        let mut row = Vec::new();
        for &sym in syms {
            let off = hint_data.len();
            hint_data.push(0);
            hint_data.push(0);
            hint_data.extend_from_slice(sym.as_bytes());
            hint_data.push(0);
            if hint_data.len() % 2 != 0 {
                hint_data.push(0);
            }
            row.push(off);
        }
        hint_offs.push(row);
    }

    let mut dll_data: Vec<u8> = Vec::new();
    let mut dll_offs: Vec<usize> = Vec::new();
    for (dll, _) in by_dll {
        let off = dll_data.len();
        dll_data.extend_from_slice(dll.as_bytes());
        dll_data.push(0);
        if dll_data.len() % 2 != 0 {
            dll_data.push(0);
        }
        dll_offs.push(off);
    }

    let hint_base_va = idata_va + idt_size + iat_size;
    let dll_base_va = hint_base_va + hint_data.len() as u32;
    let iat_base_va = idata_va + idt_size;

    let mut iat_bytes: Vec<u8> = Vec::new();
    for (di, (_, syms)) in by_dll.iter().enumerate() {
        for (si, _) in syms.iter().enumerate() {
            let va = hint_base_va + hint_offs[di][si] as u32;
            iat_bytes.extend_from_slice(&(va as u64).to_le_bytes());
        }
        iat_bytes.extend_from_slice(&0u64.to_le_bytes());
    }

    let mut idt_bytes: Vec<u8> = Vec::new();
    let mut iat_off: u32 = 0;
    for (di, (_, syms)) in by_dll.iter().enumerate() {
        let iat_va = iat_base_va + iat_off;
        let dname_va = dll_base_va + dll_offs[di] as u32;
        let mut entry = [0u8; 20];
        put_u32(&mut entry, 0, iat_va);
        put_u32(&mut entry, 12, dname_va);
        put_u32(&mut entry, 16, iat_va);
        idt_bytes.extend_from_slice(&entry);
        iat_off += (syms.len() + 1) as u32 * 8;
    }
    idt_bytes.extend_from_slice(&[0u8; 20]);

    let mut out = Vec::new();
    out.extend_from_slice(&idt_bytes);
    out.extend_from_slice(&iat_bytes);
    out.extend_from_slice(&hint_data);
    out.extend_from_slice(&dll_data);
    out
}

fn section_entry(
    name: &[u8; 8],
    vsize: u32,
    va: u32,
    raw_size: u32,
    raw_off: u32,
    chars: u32,
) -> [u8; 40] {
    let mut b = [0u8; 40];
    b[0..8].copy_from_slice(name);
    put_u32(&mut b, 8, vsize);
    put_u32(&mut b, 12, va);
    put_u32(&mut b, 16, raw_size);
    put_u32(&mut b, 20, raw_off);
    put_u32(&mut b, 36, chars);
    b
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct ImportResolver {
    symbols: HashMap<String, String>,
}

impl ImportResolver {
    pub fn discover() -> Self {
        let mut resolver = Self::default();
        for dir in discover_import_lib_dirs() {
            resolver.load_dir(&dir);
        }
        resolver.load_system_dll_exports();
        resolver
    }

    pub fn resolve(&self, symbol: &str) -> Option<&str> {
        self.symbols.get(symbol).map(String::as_str)
    }

    fn load_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("lib"))
            {
                self.load_lib(&path);
            }
        }
    }

    fn load_lib(&mut self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        for member in archive_members(&bytes) {
            if let Some((symbol, dll)) = parse_short_import(member) {
                self.add_symbol(&symbol, &dll);
            }
        }
    }

    fn add_symbol(&mut self, symbol: &str, dll: &str) {
        let dll = normalize_dll_name(dll);
        self.symbols
            .entry(symbol.to_string())
            .or_insert_with(|| dll.clone());
        if let Some(stripped) = symbol.strip_prefix("__imp_") {
            self.symbols
                .entry(stripped.to_string())
                .or_insert_with(|| dll.clone());
        } else {
            self.symbols.entry(format!("__imp_{symbol}")).or_insert(dll);
        }
    }

    fn load_system_dll_exports(&mut self) {
        let Some(system_root) = std::env::var_os("SystemRoot") else {
            return;
        };
        let system32 = PathBuf::from(system_root).join("System32");
        for dll in ["msvcrt.dll", "ucrtbase.dll"] {
            let path = system32.join(dll);
            if let Ok(bytes) = std::fs::read(&path) {
                for symbol in parse_pe_exports(&bytes) {
                    self.add_symbol(&symbol, dll);
                }
            }
        }
    }
}

fn discover_import_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(lib) = std::env::var_os("LIB") {
        dirs.extend(std::env::split_paths(&lib));
    }

    if let Some(sdk) = std::env::var_os("WindowsSdkDir") {
        let sdk = PathBuf::from(sdk);
        if let Some(ver) = std::env::var_os("WindowsSDKLibVersion") {
            add_sdk_lib_version(&mut dirs, &sdk, &PathBuf::from(ver));
        }
        add_latest_sdk_libs(&mut dirs, &sdk);
    }

    if let Some(pf86) = std::env::var_os("ProgramFiles(x86)") {
        let pf86 = PathBuf::from(pf86);
        add_latest_sdk_libs(&mut dirs, &pf86.join("Windows Kits").join("10"));
        add_msvc_libs(&mut dirs, &pf86.join("Microsoft Visual Studio"));
    }

    if let Some(pf) = std::env::var_os("ProgramFiles") {
        add_msvc_libs(
            &mut dirs,
            &PathBuf::from(pf).join("Microsoft Visual Studio"),
        );
    }

    dirs.sort();
    dirs.dedup();
    dirs.into_iter().filter(|d| d.is_dir()).collect()
}

fn add_latest_sdk_libs(dirs: &mut Vec<PathBuf>, sdk: &Path) {
    let lib_root = sdk.join("Lib");
    let Ok(entries) = std::fs::read_dir(&lib_root) else {
        return;
    };
    for entry in entries.flatten() {
        let version = entry.path();
        if version.is_dir() {
            if let Some(version_name) = version.file_name() {
                add_sdk_lib_version(dirs, sdk, Path::new(version_name));
            }
        }
    }
}

fn add_sdk_lib_version(dirs: &mut Vec<PathBuf>, sdk: &Path, version: &Path) {
    for family in ["ucrt", "um"] {
        dirs.push(sdk.join("Lib").join(version).join(family).join("x64"));
    }
}

fn add_msvc_libs(dirs: &mut Vec<PathBuf>, vs_root: &Path) {
    let Ok(years) = std::fs::read_dir(vs_root) else {
        return;
    };
    for year in years.flatten() {
        let Ok(editions) = std::fs::read_dir(year.path()) else {
            continue;
        };
        for edition in editions.flatten() {
            let tools = edition.path().join("VC").join("Tools").join("MSVC");
            let Ok(versions) = std::fs::read_dir(tools) else {
                continue;
            };
            for version in versions.flatten() {
                dirs.push(version.path().join("lib").join("x64"));
            }
        }
    }
}

fn archive_members(bytes: &[u8]) -> Vec<&[u8]> {
    if !bytes.starts_with(b"!<arch>\n") {
        return Vec::new();
    }
    let mut members = Vec::new();
    let mut off = 8usize;
    while off + 60 <= bytes.len() {
        let header = &bytes[off..off + 60];
        if &header[58..60] != b"`\n" {
            break;
        }
        let size_text = std::str::from_utf8(&header[48..58]).unwrap_or("").trim();
        let Ok(size) = size_text.parse::<usize>() else {
            break;
        };
        off += 60;
        if off + size > bytes.len() {
            break;
        }
        members.push(&bytes[off..off + size]);
        off += size;
        if off % 2 != 0 {
            off += 1;
        }
    }
    members
}

fn parse_short_import(data: &[u8]) -> Option<(String, String)> {
    if data.len() < 20 {
        return None;
    }
    let sig1 = u16::from_le_bytes([data[0], data[1]]);
    let sig2 = u16::from_le_bytes([data[2], data[3]]);
    if sig1 != 0 || sig2 != 0xffff {
        return None;
    }
    let string_size = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    if data.len() < 20 + string_size {
        return None;
    }
    let strings = &data[20..20 + string_size];
    let mut parts = strings.split(|b| *b == 0);
    let symbol = std::str::from_utf8(parts.next()?).ok()?.trim();
    let dll = std::str::from_utf8(parts.next()?).ok()?.trim();
    if symbol.is_empty() || dll.is_empty() {
        return None;
    }
    Some((symbol.to_string(), dll.to_string()))
}

fn normalize_dll_name(dll: &str) -> String {
    if dll.to_ascii_lowercase().ends_with(".dll") {
        dll.to_string()
    } else {
        format!("{dll}.dll")
    }
}

fn parse_pe_exports(bytes: &[u8]) -> Vec<String> {
    let Some(pe_off) = read_u32(bytes, 0x3c).map(|v| v as usize) else {
        return Vec::new();
    };
    if bytes.get(0..2) != Some(b"MZ") || bytes.get(pe_off..pe_off + 4) != Some(b"PE\0\0") {
        return Vec::new();
    }
    let Some(section_count) = read_u16(bytes, pe_off + 6).map(|v| v as usize) else {
        return Vec::new();
    };
    let Some(opt_size) = read_u16(bytes, pe_off + 20).map(|v| v as usize) else {
        return Vec::new();
    };
    let opt_off = pe_off + 24;
    let Some(magic) = read_u16(bytes, opt_off) else {
        return Vec::new();
    };
    let data_dir_off = match magic {
        0x10b => opt_off + 96,
        0x20b => opt_off + 112,
        _ => return Vec::new(),
    };
    let Some(export_rva) = read_u32(bytes, data_dir_off) else {
        return Vec::new();
    };
    if export_rva == 0 {
        return Vec::new();
    }
    let section_off = opt_off + opt_size;
    let mut sections = Vec::new();
    for i in 0..section_count {
        let off = section_off + i * 40;
        let Some(vsize) = read_u32(bytes, off + 8) else {
            return Vec::new();
        };
        let Some(va) = read_u32(bytes, off + 12) else {
            return Vec::new();
        };
        let Some(raw_size) = read_u32(bytes, off + 16) else {
            return Vec::new();
        };
        let Some(raw_ptr) = read_u32(bytes, off + 20) else {
            return Vec::new();
        };
        sections.push((va, vsize.max(raw_size), raw_ptr));
    }
    let Some(export_off) = rva_to_off(export_rva, &sections) else {
        return Vec::new();
    };
    let Some(name_count) = read_u32(bytes, export_off + 24) else {
        return Vec::new();
    };
    let Some(names_rva) = read_u32(bytes, export_off + 32) else {
        return Vec::new();
    };
    let Some(names_off) = rva_to_off(names_rva, &sections) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..name_count as usize {
        let Some(name_rva) = read_u32(bytes, names_off + i * 4) else {
            break;
        };
        let Some(name_off) = rva_to_off(name_rva, &sections) else {
            continue;
        };
        if let Some(name) = read_cstr(bytes, name_off as usize) {
            out.push(name);
        }
    }
    out
}

fn rva_to_off(rva: u32, sections: &[(u32, u32, u32)]) -> Option<usize> {
    sections.iter().find_map(|(va, size, raw)| {
        if *va <= rva && rva < va.saturating_add(*size) {
            Some((raw + (rva - va)) as usize)
        } else {
            None
        }
    })
}

fn read_cstr(bytes: &[u8], off: usize) -> Option<String> {
    let end = bytes.get(off..)?.iter().position(|b| *b == 0)? + off;
    std::str::from_utf8(bytes.get(off..end)?)
        .ok()
        .map(str::to_string)
}

fn read_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let b = bytes.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

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

//! Compose a compact post-link module image for the SDK hazard tools.
//! The module is appended after the resident PS-X EXE, while its declared
//! `--module` placement tells the tools where that suffix runs in the linker.
use std::io::{self, Read, Write};

const HEADER: usize = 0x800;
const LOAD: u32 = 0x8001_0000;
const RAM_END: u32 = 0x8020_0000;
const MAGIC: u32 = 0x4841_5A54;

fn resident_size(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < HEADER || !bytes.starts_with(b"PS-X EXE") {
        return Err("resident input is not a PS-X EXE".into());
    }
    let load = u32::from_le_bytes(bytes[0x18..0x1c].try_into().unwrap());
    if load != LOAD {
        return Err(format!(
            "resident load address {load:#x} differs from HK {LOAD:#x}"
        ));
    }
    let payload = u32::from_le_bytes(bytes[0x1c..0x20].try_into().unwrap()) as usize;
    if bytes.len()
        != HEADER
            .checked_add(payload)
            .ok_or("resident size overflows")?
    {
        return Err("resident file length disagrees with EXE header payload size".into());
    }
    if !bytes.len().is_multiple_of(4) || !payload.is_multiple_of(4) {
        return Err("resident payload must be word aligned".into());
    }
    Ok(payload)
}

fn compose(resident: &[u8], module: &[u8]) -> Result<(Vec<u8>, u32), String> {
    let payload = resident_size(resident)?;
    if module.is_empty() || !module.len().is_multiple_of(4) {
        return Err("module image must be nonempty and word aligned".into());
    }
    let at = LOAD
        .checked_add(u32::try_from(payload).map_err(|_| "resident too large")?)
        .ok_or("placement overflows")?;
    let end = at
        .checked_add(u32::try_from(module.len()).map_err(|_| "module too large")?)
        .ok_or("placement end overflows")?;
    if end > RAM_END {
        return Err(format!("module placement ends beyond RAM at {end:#x}"));
    }
    let mut result = Vec::with_capacity(resident.len() + module.len());
    result.extend_from_slice(resident);
    result.extend_from_slice(module);
    Ok((result, at))
}

fn extract(resident: &[u8], composite: &[u8], module_len: usize) -> Result<Vec<u8>, String> {
    resident_size(resident)?;
    let end = resident
        .len()
        .checked_add(module_len)
        .ok_or("module length overflows")?;
    if module_len == 0 || !module_len.is_multiple_of(4) || composite.len() < end {
        return Err("patched image does not contain the complete module".into());
    }
    if !composite.starts_with(resident) {
        return Err("hazard patch changed resident bytes while patching a module".into());
    }
    if composite[end..].iter().any(|&byte| byte != 0) {
        return Err("patched image has nonzero bytes after the module".into());
    }
    Ok(composite[resident.len()..end].to_vec())
}

fn clean_capacity(
    composite: &[u8],
    link: (u32, u32),
    at: u32,
    tramp: (u32, u32),
) -> Result<u32, String> {
    if composite.len() < HEADER {
        return Err("compact image has no PS-X EXE header".into());
    }
    let payload = u32::from_le_bytes(composite[0x1c..0x20].try_into().unwrap()) as usize;
    let resident_end = HEADER
        .checked_add(payload)
        .ok_or("resident size overflows")?;
    if composite.len() < resident_end {
        return Err("compact image truncates resident bytes".into());
    }
    resident_size(&composite[..resident_end])?;
    let module_len = link.1.checked_sub(link.0).ok_or("bad module link span")? as usize;
    let expected_at = LOAD
        .checked_add(u32::try_from(payload).map_err(|_| "resident payload too large")?)
        .ok_or("module placement overflows")?;
    if module_len == 0 || at != expected_at {
        return Err("module does not start after the resident payload".into());
    }
    let module_end = resident_end
        .checked_add(module_len)
        .ok_or("module size overflows")?;
    if composite.len() < module_end {
        return Err("compact image truncates module bytes".into());
    }
    if !(link.0 <= tramp.0 && tramp.0 < tramp.1 && tramp.1 <= link.1) {
        return Err("trampoline span is outside the module".into());
    }
    let start = resident_end + (tramp.0 - link.0) as usize;
    let end = resident_end + (tramp.1 - link.0) as usize;
    let bytes = &composite[start..end];
    if bytes.len() < 8 || !bytes.len().is_multiple_of(4) {
        return Err("trampoline span is not a word-aligned header and array".into());
    }
    if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != MAGIC {
        return Err("clean module has no trampoline magic at its declared span".into());
    }
    let capacity = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if capacity == 0
        || bytes.len() != 8 + 4 * capacity as usize
        || bytes[8..].iter().any(|&b| b != 0)
    {
        return Err("clean module trampoline capacity or contents disagree with its span".into());
    }
    Ok(capacity)
}

fn hex(value: &str) -> Result<u32, String> {
    u32::from_str_radix(value.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

fn run(args: &[String], input: &[u8]) -> Result<Vec<u8>, String> {
    match args {
        [action, resident, composite] if action == "compose" => {
            let resident = std::fs::read(resident).map_err(|e| e.to_string())?;
            let (bytes, at) = compose(&resident, input)?;
            std::fs::write(composite, bytes).map_err(|e| e.to_string())?;
            Ok(format!("{at:x}\n").into_bytes())
        }
        [action, resident, composite, length] if action == "extract" => {
            let resident = std::fs::read(resident).map_err(|e| e.to_string())?;
            let composite = std::fs::read(composite).map_err(|e| e.to_string())?;
            let length = length.parse::<usize>().map_err(|e| e.to_string())?;
            extract(&resident, &composite, length)
        }
        [action, composite, lo, hi, at, tramp_lo, tramp_hi] if action == "clean-capacity" => {
            let composite = std::fs::read(composite).map_err(|e| e.to_string())?;
            let capacity = clean_capacity(
                &composite,
                (hex(lo)?, hex(hi)?),
                hex(at)?,
                (hex(tramp_lo)?, hex(tramp_hi)?),
            )?;
            Ok(format!("{capacity}\n").into_bytes())
        }
        _ => Err("usage: hk-module-image compose RESIDENT COMPOSITE < IMAGE; hk-module-image extract RESIDENT COMPOSITE LENGTH > IMAGE; hk-module-image clean-capacity COMPOSITE LINK_LO LINK_HI AT TRAMP_LO TRAMP_HI".into()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = Vec::new();
    if args.first().is_some_and(|arg| arg == "compose") {
        io::stdin().read_to_end(&mut input).unwrap_or_else(|e| {
            eprintln!("hk-module-image: {e}");
            std::process::exit(1);
        });
    }
    match run(&args, &input) {
        Ok(output) => io::stdout().write_all(&output).unwrap_or_else(|e| {
            eprintln!("hk-module-image: {e}");
            std::process::exit(1);
        }),
        Err(error) => {
            eprintln!("hk-module-image: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resident(payload: usize) -> Vec<u8> {
        let mut exe = vec![0x5a; HEADER + payload];
        exe[..8].copy_from_slice(b"PS-X EXE");
        exe[0x18..0x1c].copy_from_slice(&LOAD.to_le_bytes());
        exe[0x1c..0x20].copy_from_slice(&(payload as u32).to_le_bytes());
        exe
    }

    #[test]
    fn compact_suffix_has_exact_placement_and_preserves_magic() {
        let mut exe = resident(0x1000);
        exe[HEADER + 0x20..HEADER + 0x24].copy_from_slice(&0x4841_5a54u32.to_le_bytes());
        let module = [1, 2, 3, 4, 5, 6, 7, 8];
        let (image, at) = compose(&exe, &module).unwrap();
        assert_eq!(at, LOAD + 0x1000);
        assert_eq!(image.len(), exe.len() + module.len());
        assert_eq!(&image[..exe.len()], exe);
        assert_eq!(&image[exe.len()..], module);
        assert_eq!(extract(&exe, &image, module.len()).unwrap(), module);
    }

    #[test]
    fn extraction_rejects_resident_mutation_and_truncation() {
        let exe = resident(0x1000);
        let (mut image, _) = compose(&exe, &[1, 2, 3, 4]).unwrap();
        image[HEADER] ^= 1;
        assert!(extract(&exe, &image, 4)
            .unwrap_err()
            .contains("resident bytes"));
        image.truncate(exe.len() + 2);
        assert!(extract(&exe, &image, 4).is_err());
    }

    #[test]
    fn extraction_returns_only_patched_module_and_accepts_sector_padding() {
        let exe = resident(0x1000);
        let (mut image, _) = compose(&exe, &[1, 2, 3, 4]).unwrap();
        image[exe.len()..exe.len() + 4].copy_from_slice(&[9, 8, 7, 6]);
        image.extend_from_slice(&[0; 8]);
        assert_eq!(extract(&exe, &image, 4).unwrap(), [9, 8, 7, 6]);
        image[exe.len() + 4] = 1;
        assert!(extract(&exe, &image, 4)
            .unwrap_err()
            .contains("nonzero bytes after the module"));
    }

    #[test]
    fn malformed_header_and_out_of_ram_fail() {
        let mut exe = resident(0x1000);
        exe[0x1c..0x20].copy_from_slice(&0x2000u32.to_le_bytes());
        assert!(compose(&exe, &[1, 2, 3, 4]).is_err());
        let mut exe = resident(0x1000);
        exe[0x18..0x1c].copy_from_slice(&0x8002_0000u32.to_le_bytes());
        assert!(compose(&exe, &[1, 2, 3, 4])
            .unwrap_err()
            .contains("load address"));
        let exe = resident((RAM_END - LOAD) as usize - 4);
        assert!(compose(&exe, &[1, 2, 3, 4, 5, 6, 7, 8]).is_err());
    }

    #[test]
    fn clean_module_reports_its_declared_capacity() {
        let exe = resident(0x1000);
        let mut module = vec![0; 0x80];
        module[0x40..0x44].copy_from_slice(&MAGIC.to_le_bytes());
        module[0x44..0x48].copy_from_slice(&14u32.to_le_bytes());
        let (image, at) = compose(&exe, &module).unwrap();
        assert_eq!(
            clean_capacity(
                &image,
                (0x8040_8000, 0x8040_8080),
                at,
                (0x8040_8040, 0x8040_8080)
            ),
            Ok(14)
        );
    }

    #[test]
    fn clean_module_rejects_missing_or_corrupt_trampoline_header() {
        let exe = resident(0x1000);
        let mut module = vec![0; 0x80];
        module[0x40..0x44].copy_from_slice(&MAGIC.to_le_bytes());
        module[0x44..0x48].copy_from_slice(&14u32.to_le_bytes());
        let (mut image, at) = compose(&exe, &module).unwrap();
        let link = (0x8040_8000, 0x8040_8080);
        let tramp = (0x8040_8040, 0x8040_8080);
        image[exe.len() + 0x40] = 0;
        assert!(clean_capacity(&image, link, at, tramp).is_err());
        image[exe.len() + 0x40..exe.len() + 0x44].copy_from_slice(&MAGIC.to_le_bytes());
        image[exe.len() + 0x44..exe.len() + 0x48].copy_from_slice(&13u32.to_le_bytes());
        assert!(clean_capacity(&image, link, at, tramp).is_err());
        image[exe.len() + 0x44..exe.len() + 0x48].copy_from_slice(&14u32.to_le_bytes());
        image[exe.len() + 0x48] = 1;
        assert!(clean_capacity(&image, link, at, tramp).is_err());
        assert!(clean_capacity(&image, link, at - 4, tramp).is_err());
    }
}

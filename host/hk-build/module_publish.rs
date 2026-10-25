//! HK declarations for the shared same-link publisher. Extraction, relocation,
//! composition and hardened finalization remain in the SDK.
use psoxide_hazard::overlay_elf::{Elf, Symbol};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
mod disc;
use std::{collections::BTreeSet, fs, path::Path, process::Command};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn owner<'a>(symbols: &[Symbol<'a>], section: usize, site: u32) -> Result<Symbol<'a>> {
    let mut found = symbols.iter().filter(|s| {
        s.section as usize == section
            && s.bytes != 0
            && site >= s.address
            && site
                .checked_add(4)
                .is_some_and(|end| end <= s.address.saturating_add(s.bytes))
    });
    let result = *found.next().ok_or("unowned resident import")?;
    if found.any(|s| s.name != result.name) {
        return Err("ambiguous resident import owner".into());
    }
    Ok(result)
}
fn read_word(bytes: &[u8], offset: u32) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset as usize..offset as usize + 4)
            .ok_or("import word outside section")?
            .try_into()?,
    ))
}
fn describe(elf_path: &Path, declaration: &Path, output: &Path) -> Result<()> {
    let bytes = fs::read(elf_path)?;
    let elf = Elf::parse(&bytes)?;
    let symbols = elf.symbols()?;
    let declarations: Value = serde_json::from_slice(&fs::read(declaration)?)?;
    let modules = declarations["modules"]
        .as_array()
        .ok_or("module declarations")?;
    let mut images = Vec::new();
    // Every edge from one cold module into another, so one build lists them all.
    let mut cross: Vec<String> = Vec::new();
    for module in modules {
        let name = module["name"].as_str().ok_or("module name")?;
        let id = module["id"].as_u64().ok_or("module id")?;
        let code_name = format!(".mod_{name}");
        let data_name = format!(".mod_{name}_rodata");
        let Ok((code_index, code)) = elf.section(&code_name) else {
            continue;
        };
        if code.size == 0 {
            continue;
        }
        let data = elf.section(&data_name).ok();
        let mut calls = BTreeSet::new();
        let pinned: Vec<Value> = Vec::new();
        for (index, section) in elf.sections().filter(|(_, s)| s.flags & 2 != 0) {
            if index == code_index || data.is_some_and(|(i, _)| i == index) {
                continue;
            }
            let relocations = elf.relocations(index)?;
            for relocation in &relocations {
                let is_code = relocation.target_section as usize == code_index;
                let is_data = data.is_some_and(|(i, _)| relocation.target_section as usize == i);
                if !is_code && !is_data {
                    continue;
                }
                // Any cross-cold edge is a real residency bug, never a resident import.
                if section.name.starts_with(".mod_") {
                    let caller = owner(&symbols, index, relocation.address)?;
                    let word = read_word(section.bytes, relocation.address.saturating_sub(section.address)).unwrap_or(0);
                    let target = ((relocation.address + 4) & 0xf0000000) | ((word & 0x03ffffff) << 2);
                    let callee = symbols
                        .iter()
                        .find(|s| s.address <= target && target < s.address + s.bytes.max(1) && s.bytes != 0)
                        .map_or("?", |s| s.name);
                    cross.push(format!("{} in {} reaches {callee} of module {name}", caller.name, section.name));
                    continue;
                }
                let caller = owner(&symbols, index, relocation.address)?;
                let offset = relocation
                    .address
                    .checked_sub(section.address)
                    .ok_or("site address")?;
                let instruction = read_word(section.bytes, offset)?;
                if relocation.kind == 4 && is_code && section.flags & 4 != 0 {
                    if ![
                        "4fsm3",
                        "7enemies",
                        "14blocker_roller",
                        "5frame8simulate",
                        "5frame6render",
                        "8dialogue",
                        "5frame",
                        "10boss_hooks",
                    ]
                    .iter()
                    .any(|p| caller.name.contains(p))
                    {
                        return Err(format!("unapproved caller {}", caller.name).into());
                    }
                    let target =
                        ((relocation.address + 4) & 0xf0000000) | ((instruction & 0x03ffffff) << 2);
                    let export = symbols
                        .iter()
                        .find(|s| {
                            s.section as usize == code_index
                                && s.kind == 2
                                && s.address == target
                                && s.bytes != 0
                        })
                        .ok_or("unowned cold export")?;
                    calls.insert((caller.name, export.name));
                    continue;
                }
                return Err(format!(
                    "unsupported resident binding {} at {:#x}",
                    caller.name, relocation.address
                )
                .into());
            }
        }
        images.push(json!({"name":name,"id":id,"code":code_name,"data":data.map(|_|vec![data_name]).unwrap_or_default(),"calls":calls,"pinned":pinned}));
    }
    if !cross.is_empty() {
        return Err(format!("cross-cold dependency: {}", cross.join("; ")).into());
    }

    let result = json!({"header":".ps_exe_head","resident":[".text",".data",".bss"],"compact_sites":"HK_MODULE_IMPORTS","compact_ranges":"HK_MODULE_IMPORT_RANGE","compact_trap":"__hk_module_trap","images":images});
    if output.exists() {
        return Err("publication declaration already exists".into());
    }

    fs::write(output, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
fn publish(
    composer: &Path,
    elf: &Path,
    config: &Path,
    output: &Path,
    final_guard: Option<&Path>,
) -> Result<()> {
    let config: Value = serde_json::from_slice(&fs::read(config)?)?;
    let string = |value: &Value| -> Result<String> {
        Ok(value
            .as_str()
            .ok_or("publisher string declaration")?
            .to_owned())
    };
    let mut args = vec![
        elf.to_string_lossy().into_owned(),
        output.to_string_lossy().into_owned(),
        "--header".into(),
        string(&config["header"])?,
    ];
    for section in config["resident"]
        .as_array()
        .ok_or("resident declarations")?
    {
        args.extend(["--resident".into(), string(section)?]);
    }
    for (key, flag) in [
        ("compact_sites", "--compact-sites"),
        ("compact_ranges", "--compact-ranges"),
        ("compact_trap", "--compact-trap"),
    ] {
        args.extend([flag.into(), string(&config[key])?]);
    }
    for image in config["images"].as_array().ok_or("image declarations")? {
        args.extend([
            "--image".into(),
            string(&image["name"])?,
            "--id".into(),
            image["id"].as_u64().ok_or("image identity")?.to_string(),
            "--code".into(),
            string(&image["code"])?,
        ]);
        for section in image["data"].as_array().ok_or("readonly declarations")? {
            args.extend(["--data".into(), string(section)?]);
        }
        for call in image["calls"].as_array().ok_or("call declarations")? {
            let call = call.as_array().ok_or("call pair")?;
            if call.len() != 2 {
                return Err("call pair length".into());
            }
            args.extend([
                "--call".into(),
                format!("{}:{}", string(&call[0])?, string(&call[1])?),
            ]);
        }
        if !image["pinned"]
            .as_array()
            .ok_or("pinned declarations")?
            .is_empty()
        {
            return Err("HK publication forbids escaped pinned pointers".into());
        }
    }
    if let Some(guard) = final_guard {
        args.extend(["--final-guard".into(), guard.to_string_lossy().into_owned()]);
    }
    if !std::process::Command::new(composer)
        .args(args)
        .status()?
        .success()
    {
        return Err("shared publisher refused generation".into());
    }
    Ok(())
}
fn guarded(tool: &Path, arguments: &[String], log: &Path) -> Result<String> {
    let result = Command::new(tool).args(arguments).output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(log, &text)?;
    if !result.status.success() {
        return Err(format!("{} refused; see {}", tool.display(), log.display()).into());
    }
    Ok(text)
}
fn pipeline(elf: &Path, map: &Path, declarations: &Path, work: &Path, tools: &Path) -> Result<()> {
    if work.exists() {
        return Err("new publication directory required".into());
    }
    fs::create_dir_all(work)?;
    let config = work.join("declarations.json");
    describe(elf, declarations, &config)?;
    let raw = work.join("raw");
    publish(&tools.join("overlay-compose"), elf, &config, &raw, None)?;
    let hardened = work.join("hardened.exe");
    fs::copy(raw.join("guard.exe"), &hardened)?;
    let guard: Vec<String> = fs::read_to_string(raw.join("guard-args"))?
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let mut modules = Vec::new();
    for pair in guard.chunks_exact(2) {
        if pair[0] == "--module" {
            modules.extend_from_slice(pair);
        }
    }
    let image_args = |image: &Path, check: bool| {
        let mut args = vec![
            image.to_string_lossy().into_owned(),
            "--map".into(),
            map.to_string_lossy().into_owned(),
        ];
        if check {
            args.push("--check".into());
        }
        args.extend(guard.clone());
        args
    };
    let stack_args = |image: &Path| {
        let mut args = vec![
            image.to_string_lossy().into_owned(),
            map.to_string_lossy().into_owned(),
        ];
        args.extend(modules.clone());
        args.extend(["--forbid".into(), "__psx_rt_flush_i_cache".into()]);
        args
    };
    let patch = guarded(
        &tools.join("hazard-patch"),
        &image_args(&hardened, false),
        &work.join("patch.log"),
    )?;
    guarded(
        &tools.join("hazard-scan"),
        &image_args(&hardened, false),
        &work.join("scan.log"),
    )?;
    guarded(
        &tools.join("stack-guard"),
        &stack_args(&hardened),
        &work.join("stack.log"),
    )?;
    let final_dir = work.join("final");
    publish(
        &tools.join("overlay-compose"),
        elf,
        &config,
        &final_dir,
        Some(&hardened),
    )?;
    let final_guard = final_dir.join("guard.exe");
    guarded(
        &tools.join("hazard-patch"),
        &image_args(&final_guard, true),
        &work.join("final-check.log"),
    )?;
    guarded(
        &tools.join("hazard-scan"),
        &image_args(&final_guard, false),
        &work.join("final-scan.log"),
    )?;
    guarded(
        &tools.join("stack-guard"),
        &stack_args(&final_guard),
        &work.join("final-stack.log"),
    )?;
    let config: Value = serde_json::from_slice(&fs::read(config)?)?;
    let declarations: Value = serde_json::from_slice(&fs::read(declarations)?)?;
    let mut report = serde_json::Map::new();
    let final_runtime = fs::read(final_dir.join("runtime.exe"))?;
    let raw_elf = fs::read(elf)?;
    let parsed_elf = Elf::parse(&raw_elf)?;
    let final_symbols = parsed_elf.symbols()?;
    let ranges = final_symbols
        .iter()
        .find(|s| s.name == "HK_MODULE_IMPORT_RANGE")
        .ok_or("import range table")?;
    let load = read_word(&final_runtime, 24)?;
    let mut imports = 0;
    for module in declarations["modules"]
        .as_array()
        .ok_or("module declarations")?
    {
        let name = module["name"].as_str().ok_or("module name")?;
        let path = final_dir.join(format!("{name}.pak"));
        if !path.exists() {
            report.insert(name.to_owned(), json!({"image_bytes":0,"code_bytes":0,"fixups":0,"hazard_sites":0,"trampoline_words":0,"empty":true,"import_sites":[]}));
            continue;
        }
        let bytes = fs::read(&path)?;
        if bytes.len() % 4 != 0 {
            return Err("word-aligned final package required".into());
        }
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect();
        let _package = psx_residency::overlay::Package::parse(&words)
            .map_err(|e| format!("final package: {e:?}"))?;
        let image = config["images"]
            .as_array()
            .ok_or("images")?
            .iter()
            .find(|i| i["name"] == name)
            .ok_or("image declaration")?;
        let id = module["id"].as_u64().ok_or("module identity")? as u32;
        let range = read_word(
            &final_runtime,
            2048 + ranges.address.checked_sub(load).ok_or("range address")?
                + id.checked_mul(4).ok_or("range identity")?,
        )?;
        let count = range >> 16;
        let _ = image;
        imports += count;
        report.insert(name.to_owned(), json!({"image_bytes":words[2]*4,"code_bytes":words[4],"fixups":words[3],"package":path,"link_address":module["linked"],"import_sites_count":count}));
    }
    fs::write(
        work.join("publication-report.json"),
        serde_json::to_vec_pretty(
            &json!({"modules":report,"import_sites":imports,"patch_log":patch,"runtime":final_dir.join("runtime.exe"),"final_guard":final_guard}),
        )?,
    )?;
    Ok(())
}
// Bind transport bytes only after the SDK has finalized code and imports.
fn assemble(elf_path: &Path, config_path: &Path, publication: &Path, output: &Path) -> Result<()> {
    let elf_bytes = fs::read(elf_path)?;
    let elf = Elf::parse(&elf_bytes)?;
    let config: Value = serde_json::from_slice(&fs::read(config_path)?)?;
    let names = config["names"].as_array().ok_or("transport names")?;
    let publication_report: Value = serde_json::from_slice(&fs::read(
        publication
            .parent()
            .ok_or("publication parent")?
            .join("publication-report.json"),
    )?)?;
    let code_count = config["code_count"].as_u64().ok_or("code count")? as usize;
    let mut packages = Vec::new();
    let mut sizes = Vec::new();
    for (id, name) in names.iter().enumerate() {
        let name = name.as_str().ok_or("module name")?;
        let bytes = if id < code_count {
            let path = publication.join(format!("{name}.pak"));
            if path.exists() {
                fs::read(path)?
            } else {
                if publication_report["modules"][name]["empty"] != true {
                    return Err("missing final package without explicit empty declaration".into());
                }
                if elf
                    .section(&format!(".mod_{name}"))
                    .is_ok_and(|(_, section)| section.size != 0)
                    || elf
                        .section(&format!(".mod_{name}_rodata"))
                        .is_ok_and(|(_, section)| section.size != 0)
                {
                    return Err("missing finalized nonempty code package".into());
                }
                [0x444d4b48u32, id as u32, 0, 0, 0]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect()
            }
        } else {
            let path = config["data"][name].as_str().ok_or("data blob path")?;
            let mut data = fs::read(path)?;
            data.resize((data.len() + 3) & !3, 0);
            let mut bytes: Vec<u8> = [0x444d4b48u32, id as u32, (data.len() / 4) as u32, 0, 0]
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect();
            bytes.extend(data);
            bytes
        };
        if bytes.len() % 4 != 0 {
            return Err("unaligned transport package".into());
        }
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect();
        let _ = psx_residency::overlay::Package::parse(&words)
            .map_err(|e| format!("transport package: {e:?}"))?;
        if words[1] != id as u32 {
            return Err("transport module identity".into());
        }
        sizes.push(words[2] * 4);
        packages.push(bytes);
    }
    fs::create_dir_all(output.join("chunks"))?;
    let mut runtime = fs::read(publication.join("runtime.exe"))?;
    let load = read_word(&runtime, 24)?;
    let symbols = elf.symbols()?;
    let mut chunks = Vec::new();
    for (index, chunk) in config["chunks"]
        .as_array()
        .ok_or("chunks")?
        .iter()
        .enumerate()
    {
        let kind = chunk["kind"].as_str().ok_or("chunk kind")?;
        let ids = chunk["modules"].as_array().ok_or("chunk modules")?;
        let mut raw = Vec::new();
        for id in ids {
            raw.extend_from_slice(
                packages
                    .get(id.as_u64().ok_or("chunk id")? as usize)
                    .ok_or("chunk module bounds")?,
            );
        }
        let stem = chunk["stem"].as_str().ok_or("chunk stem")?;
        if stem.contains('/') || stem.contains("..") {
            return Err("chunk path".into());
        }
        let raw_path = output.join("chunks").join(format!("{stem}.raw"));
        fs::write(&raw_path, &raw)?;
        let stored_path = output.join("chunks").join(format!("{stem}.stored"));
        let stored = if kind != "code" && stored_path.exists() {
            Some(fs::read(&stored_path)?)
        } else {
            None
        };
        if let Some(bytes) = &stored {
            let binding: Value =
                serde_json::from_slice(&fs::read(stored_path.with_extension("binding.json"))?)?;
            let digest = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
            if binding["raw_sha256"] != digest(&raw) || binding["stored_sha256"] != digest(bytes) {
                return Err("stored chunk does not bind exact raw bytes".into());
            }
            if bytes.get(..4) != Some(b"HLZC") || read_word(bytes, 4)? as usize != raw.len() {
                return Err("compressed chunk identity".into());
            }
        }
        let hash_bytes = |bytes: &[u8]| {
            bytes.iter().fold(0x811c9dc5u32, |h, b| {
                (h ^ *b as u32).wrapping_mul(0x01000193)
            })
        };
        let hash_words = raw.chunks_exact(4).fold(0x811c9dc5u32, |h, b| {
            (h ^ u32::from_le_bytes(b.try_into().unwrap())).wrapping_mul(0x01000193)
        });
        let values = [
            ("HK_CODE_LEN", raw.len() as u32),
            ("HK_CODE_FNV", hash_bytes(&raw)),
            ("HK_CODE_HASH", hash_words),
            (
                "HK_CODE_STORED_LEN",
                stored.as_ref().map_or(0, |b| b.len() as u32),
            ),
            (
                "HK_CODE_STORED_FNV",
                stored.as_ref().map_or(0, |b| hash_bytes(b)),
            ),
        ];
        for (name, value) in values {
            let symbol = symbols
                .iter()
                .find(|s| s.name == name)
                .ok_or("chunk table symbol")?;
            if (index + 1) * 4 > symbol.bytes as usize {
                return Err("chunk table capacity".into());
            }
            let offset = 2048usize
                + symbol.address.checked_sub(load).ok_or("chunk table load")? as usize
                + index * 4;
            runtime
                .get_mut(offset..offset + 4)
                .ok_or("chunk table bounds")?
                .copy_from_slice(&value.to_le_bytes());
        }
        let path = output.join("chunks").join(format!("{stem}.hkmd"));
        fs::write(&path, stored.as_deref().unwrap_or(&raw))?;
        chunks.push(json!({"kind":kind,"scene_index":chunk["scene_index"],"modules":ids,"bytes":raw.len(),"stored_bytes":stored.as_ref().map_or(raw.len(),|b|b.len()),"path":path}));
    }
    // The three largest carried packages can be in the pool at once (a package nobody asks for any more is
    // evicted for the next one), each read in whole sectors after its 20-byte header. Greenpath's Hornet's room carries
    // no Shade (game/src/shade.rs `waits_for_hornet`): her module, her trees and her state leave no room for 30 KB of it.
    let names = config["names"].as_array().ok_or("names")?;
    let carried_of = |without_shade: bool| -> std::result::Result<u32, Box<dyn std::error::Error>> {
        let mut sizes_of = Vec::new();
        for id in config["carried"].as_array().ok_or("carried")? {
            let id = id.as_u64().unwrap_or(u64::MAX) as usize;
            if without_shade && names.get(id).and_then(|n| n.as_str()) == Some("art_shade") {
                continue;
            }
            sizes_of.push((sizes.get(id).ok_or("carried id")? + 20).div_ceil(2048) * 2048);
        }
        sizes_of.sort_unstable_by(|a, b| b.cmp(a));
        Ok(sizes_of.iter().take(3).sum())
    };
    let carried = carried_of(false)?;
    let carried_without_shade = carried_of(true)?;
    let hornet_id = names.iter().position(|n| n.as_str() == Some("hornet"));
    // Every room's own modules plus the carried packages in the pool together must fit the pool less one 4 KiB window fence.
    let pool = config["pool"].as_u64().unwrap_or(0x17000) as u32;
    let limit = pool - 0x1000;
    // A code module never straddles a 64 KiB `lui` window (game/src/modules.rs `gap_in`), so it needs a window's worth of pool
    // between the pool's start (or a window boundary) and the next boundary or the pool's end. The sum above does not say that.
    let base = config["pool_base"].as_u64().ok_or("pool base")? as u32;
    let widest = (0u32..)
        .map(|k| ((base.wrapping_add(0x8000) >> 16) + k) << 16)
        .map(|w| w.wrapping_sub(0x8000))
        .take_while(|&w| w < base + pool)
        .map(|w| (w + 0x1_0000).min(base + pool) - w.max(base))
        .max()
        .unwrap_or(0);
    let code_count = config["code_count"].as_u64().ok_or("code count")? as usize;
    for (id, size) in sizes.iter().take(code_count).enumerate() {
        if *size > widest {
            return Err(format!(
                "code module {} is {size} bytes; the widest 64 KiB window the pool (base {base:#x}, {pool:#x} bytes) holds is {widest}",
                config["names"][id].as_str().unwrap_or("?")
            )
            .into());
        }
    }
    let mut table = Vec::new();
    for (index, room) in config["rooms"].as_array().ok_or("rooms")?.iter().enumerate() {
        let ids = room.as_array().ok_or("room modules")?;
        let mut need = 0u32;
        let mut listed = Vec::new();
        let mut shadeless = false;
        for id in ids {
            let id = id.as_u64().ok_or("room id")? as usize;
            shadeless |= Some(id) == hornet_id;
            need = need
                .checked_add(*sizes.get(id).ok_or("room module")?)
                .ok_or("room sum")?;
            listed.push(format!("{}={}", names[id].as_str().unwrap_or("?"), sizes[id]));
        }
        table.push((need, index, listed, shadeless));
    }
    table.sort_by(|a, b| b.cmp(a));
    let mut report_rooms = Vec::new();
    let carried_in = |shadeless: bool| if shadeless { carried_without_shade } else { carried };
    for (need, index, listed, shadeless) in &table {
        report_rooms.push(json!({"room":index,"bytes":need,"with_carried":need + carried_in(*shadeless),"modules":listed}));
    }
    // Largest total first, whatever the room carries.
    table.sort_by_key(|(need, _, _, shadeless)| std::cmp::Reverse(need + carried_in(*shadeless)));
    for (need, index, listed, shadeless) in table.iter().take(6) {
        let carried = carried_in(*shadeless);
        eprintln!(
            "pool fit: room {index} {need} + carried {carried} = {} of {limit} ({:+}): {}",
            need + carried,
            (limit as i64) - ((need + carried) as i64),
            listed.join(" ")
        );
    }
    if let Some((need, _, _, shadeless)) = table.first() {
        let total = need.checked_add(carried_in(*shadeless)).ok_or("carried sum")?;
        if total > limit {
            return Err(format!("whole room plus carried package exceeds fixed pool: {total} of {limit}").into());
        }
    }
    fs::write(output.join("runtime.exe"), runtime)?;
    let report = serde_json::to_vec_pretty(
        &json!({"chunks":chunks,"module_sizes":sizes,"carried_bytes":carried,"pool_bytes":pool,"pool_limit":limit,"rooms":report_rooms}),
    )?;
    fs::write(output.join("transport.json"), &report)?;
    fs::write(output.join("modules.json"), &report)?;
    Ok(())
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.as_slice() {
        [command, root, work, library] if command == "pack" => disc::package(Path::new(root), &Path::new(work).join("runtime.exe"), Path::new(work), Path::new(library), &disc::Songs { xa:vec![Path::new(root).join("data/music.xa")], cdda:Vec::new() }, None),
        [command, elf, config, publication, output] if command == "assemble" => assemble(Path::new(elf),Path::new(config),Path::new(publication),Path::new(output)),
        [command, elf, declaration, output] if command == "describe" => describe(Path::new(elf), Path::new(declaration), Path::new(output)),
        [command, composer, elf, config, output] if command == "publish" => publish(Path::new(composer), Path::new(elf), Path::new(config), Path::new(output), None),
        [command, composer, elf, config, output, guard] if command == "publish" => publish(Path::new(composer), Path::new(elf), Path::new(config), Path::new(output), Some(Path::new(guard))),
        [command, elf, map, declarations, work, tools] if command == "pipeline" => pipeline(Path::new(elf), Path::new(map), Path::new(declarations), Path::new(work), Path::new(tools)),
        _ => Err("usage: hk-module-publish describe ELF MODULES_JSON NEW_DECLARATIONS_JSON; hk-module-publish publish COMPOSER ELF DECLARATIONS_JSON NEW_DIRECTORY [HARDENED_GUARD]".into()),
    };
    if let Err(error) = result {
        eprintln!("hk-module-publish: {error}");
        std::process::exit(1);
    }
}

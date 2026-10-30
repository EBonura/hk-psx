//! A settled frame of a journey is not nearly black.
//!
//! A scene whose gameplay data was wrong once drew its rooms almost black at full frame rate, and no pin
//! noticed: the counters and the outcome of the replay were the ones of a healthy run. The replay saves a
//! screenshot every 400 ticks; for each one taken in gameplay, a room the Knight has stood in for
//! `SETTLED_TICKS` already, the share of its pixels that are not black has to reach `FLOOR_PERCENT`.
use super::Result;
use serde_json::Value;
use std::path::Path;

/// Ticks in the same room, in gameplay, before a screenshot counts (a fade, a bind, a respawn are over).
pub const SETTLED_TICKS: usize = 120;
/// Percent of pixels with a channel above `BLACK_LEVEL` that a settled frame must have. The darkest settled frames
/// of a healthy build are 19 to 30 percent (the False Knight's rooms); the rooms of the broken one were 8 to 14.
pub const FLOOR_PERCENT: u64 = 14;
/// A channel value at or under this is black.
pub const BLACK_LEVEL: u8 = 24;

/// Percent of the pixels of a binary PPM that are not black.
pub fn lit_percent(ppm: &[u8]) -> Option<u64> {
    let mut fields = 0;
    let mut at = 0;
    // "P6", width, height, maxval, each followed by whitespace; the pixels start after the last.
    while fields < 4 {
        while at < ppm.len() && ppm[at].is_ascii_whitespace() {
            at += 1;
        }
        while at < ppm.len() && !ppm[at].is_ascii_whitespace() {
            at += 1;
        }
        fields += 1;
    }
    let pixels = ppm.get(at + 1..)?;
    let count = pixels.len() / 3;
    if count == 0 {
        return None;
    }
    let lit = pixels
        .chunks_exact(3)
        .filter(|p| p.iter().any(|&c| c > BLACK_LEVEL))
        .count();
    Some((lit * 100 / count) as u64)
}

/// The settled screenshots of a replay with their lit percent: (tick, region, percent).
pub fn settled(output: &Path) -> Result<Vec<(usize, String, u64)>> {
    let command: Value = serde_json::from_slice(&std::fs::read(output.join("command.json"))?)?;
    let column = |name: &str| {
        command["watches"][name]
            .as_str()
            .map(|a| format!("ram_{:0>8}", a.trim_start_matches("0x")))
    };
    let text = std::fs::read_to_string(output.join("route.csv"))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("route.csv is empty")?.split(',').collect();
    let at = |name: &str| header.iter().position(|h| *h == name);
    let (Some(game), Some(region)) = (
        column("HK_GAME_MODE").and_then(|c| at(&c)),
        column("HK_REGION_ID").and_then(|c| at(&c)),
    ) else {
        return Err("route.csv does not watch HK_GAME_MODE and HK_REGION_ID".into());
    };
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(output.join("screenshots")) else {
        return Ok(found);
    };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    for path in files {
        let Some(tick) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("tick-"))
            .and_then(|s| s.parse::<usize>().ok())
        else {
            continue;
        };
        if tick < SETTLED_TICKS || tick >= rows.len() {
            continue;
        }
        let window = &rows[tick - SETTLED_TICKS..=tick];
        if !window.iter().all(|r| r[game] == "1" && r[region] == rows[tick][region]) {
            continue;
        }
        if let Some(percent) = lit_percent(&std::fs::read(&path)?) {
            found.push((tick, rows[tick][region].to_string(), percent));
        }
    }
    Ok(found)
}

/// What the settled frames say, for the report: how many and the darkest.
pub fn summary(output: &Path) -> Result<String> {
    let frames = settled(output)?;
    Ok(match frames.iter().min_by_key(|f| f.2) {
        Some((tick, region, percent)) => format!(
            "brightness: {} settled frames, darkest {percent}% lit (tick {tick}, region {region}), floor {FLOOR_PERCENT}%",
            frames.len()
        ),
        None => "brightness: no settled frame".to_string(),
    })
}

/// The settled frames under the floor, as report lines.
pub fn failures(output: &Path) -> Result<Vec<String>> {
    Ok(settled(output)?
        .into_iter()
        .filter(|&(_, _, percent)| percent < FLOOR_PERCENT)
        .map(|(tick, region, percent)| {
            format!("tick {tick} in region {region}: {percent}% of the frame is lit, under {FLOOR_PERCENT}%")
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppm(pixels: &[[u8; 3]]) -> Vec<u8> {
        let mut out = format!("P6\n{} 1\n255\n", pixels.len()).into_bytes();
        for p in pixels {
            out.extend_from_slice(p);
        }
        out
    }

    #[test]
    fn black_and_lit_pixels_are_told_apart() {
        assert_eq!(lit_percent(&ppm(&[[0, 0, 0]; 4])), Some(0));
        assert_eq!(lit_percent(&ppm(&[[0, 0, 0], [0, 0, 0], [30, 0, 0], [0, 200, 0]])), Some(50));
        assert_eq!(lit_percent(&ppm(&[[24, 24, 24]])), Some(0));
        assert_eq!(lit_percent(b"P6\n1 1\n255\n"), None);
    }

    #[test]
    fn a_settled_dark_room_fails_and_a_fresh_one_does_not() {
        let dir = std::env::temp_dir().join(format!("hk-brightness-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("screenshots")).unwrap();
        std::fs::write(
            dir.join("command.json"),
            r#"{"watches":{"HK_GAME_MODE":"0x80000010","HK_REGION_ID":"0x80000014"}}"#,
        )
        .unwrap();
        // 450 ticks in gameplay; the region changes at tick 300, so the frame at 450 is settled in region 7 and
        // the one at 350 is not.
        let mut csv = String::from("route_tick,ram_80000010,ram_80000014\n");
        for tick in 0..=450 {
            csv += &format!("{tick},1,{}\n", if tick < 300 { 3 } else { 7 });
        }
        std::fs::write(dir.join("route.csv"), csv).unwrap();
        let black = ppm(&[[0, 0, 0]; 100]);
        let lit = ppm(&[[200, 200, 200]; 100]);
        std::fs::write(dir.join("screenshots/tick-000250.ppm"), &lit).unwrap();
        std::fs::write(dir.join("screenshots/tick-000350.ppm"), &black).unwrap();
        std::fs::write(dir.join("screenshots/tick-000450.ppm"), &black).unwrap();
        let dark = failures(&dir).unwrap();
        assert_eq!(dark.len(), 1, "{dark:?}");
        assert!(dark[0].contains("tick 450 in region 7"), "{dark:?}");
        std::fs::write(dir.join("screenshots/tick-000450.ppm"), &lit).unwrap();
        assert!(failures(&dir).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

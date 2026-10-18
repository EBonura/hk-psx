//! The game's localization sheets (the Rust twin of host/language.py).
//!
//! `Language.GetLanguageFileContents` loads `Languages/<code>_<sheet>` and hands it to
//! `Encryption.Decrypt`: base64, then AES-256 in ECB mode with PKCS7 padding. The key is a 32 byte
//! literal in `Encryption`'s static constructor, so it is read there and never written down here, and
//! `openssl` does the block cipher the way the other cooking tools lean on ffmpeg.
use crate::common::{err, Result};
use hk_dotnet::il::{self, Operand};
use hk_dotnet::Assembly;
use hk_unity::{Obj, Source, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// The language key, read from `Encryption`'s static constructor.
pub fn encryption_key(managed: &Path) -> Result<String> {
    let asm = Assembly::open(&managed.join("TeamCherry.SharedUtils.dll")).map_err(|e| e.0)?;
    let (_, _, _, rva) = asm
        .methods_of("Encryption")
        .into_iter()
        .find(|(_, _, name, rva)| name == ".cctor" && *rva != 0)
        .ok_or("Encryption has no static constructor")?;
    let code = il::body(&asm, rva).map_err(|e| e.0)?;
    let mut keys = Vec::new();
    for ins in il::decode(code).map_err(|e| e.0)? {
        if let ("ldstr", Operand::Token(t)) = (ins.name, ins.operand) {
            keys.push(asm.user_string(t & 0x00ff_ffff));
        }
    }
    if keys.len() != 1 || keys[0].len() != 32 {
        return err("the language key is no longer one 32 byte literal");
    }
    Ok(keys.remove(0))
}

/// `html.unescape` for the entities the sheets use.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `<entry name="K">V</entry>` pairs in file order.
pub fn entries(plain: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = plain;
    while let Some(at) = rest.find("<entry name=\"") {
        rest = &rest[at + 13..];
        let Some(q) = rest.find("\">") else { break };
        let name = &rest[..q];
        rest = &rest[q + 2..];
        let Some(end) = rest.find("</entry>") else { break };
        out.push((name.to_owned(), unescape(&rest[..end]).trim().to_owned()));
        rest = &rest[end + 8..];
    }
    out
}

/// One language sheet, decrypted and parsed into key/text pairs.
pub fn sheet(source: &Source, name: &str, language: &str) -> Result<Vec<(String, String)>> {
    let key = encryption_key(&source.directory.join("Managed"))?;
    let wanted = format!("{language}_{name}");
    let file = source.file("resources.assets").map_err(|e| e.to_string())?;
    for info in file.objects.iter() {
        if info.class_id != 49 {
            continue;
        }
        let obj = Obj { file: file.clone(), info: *info };
        let asset = source.read(&obj).map_err(|e| e.to_string())?;
        if asset.get("m_Name").and_then(Value::str).as_deref() != Some(wanted.as_str()) {
            continue;
        }
        let cipher: Vec<u8> = match asset.get("m_Script") {
            Some(Value::Str(b)) => b.clone(),
            Some(Value::List(l)) => l.iter().map(|x| x.int().unwrap_or(0) as u8).collect(),
            _ => return err("TextAsset without a script"),
        };
        let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
        let mut child = Command::new("openssl")
            .args(["enc", "-d", "-aes-256-ecb", "-K", &hex, "-base64", "-A"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("openssl: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("openssl stdin")?;
        let writer = std::thread::spawn(move || stdin.write_all(&cipher));
        let done = child.wait_with_output().map_err(|e| e.to_string())?;
        let _ = writer.join();
        if !done.status.success() {
            return err("openssl failed to decrypt a language sheet");
        }
        let plain = String::from_utf8(done.stdout).map_err(|_| "a language sheet is not UTF-8")?;
        let found = entries(&plain);
        if found.is_empty() {
            return err(format!("{wanted} decrypted without entries"));
        }
        return Ok(found);
    }
    err(format!("no {wanted} language sheet"))
}

/// `hk-cook language SHEET [KEY...]`: print a sheet, or the keys asked for.
pub fn main(root: &Path, args: &[String]) -> Result<()> {
    let source = Source::from_doctor(root).map_err(|e| e.to_string())?;
    let Some(name) = args.first() else { return err("usage: hk-cook language SHEET [KEY...]") };
    let all = sheet(&source, name, "EN")?;
    for (key, text) in &all {
        if args.len() == 1 || args[1..].iter().any(|k| key.contains(k.as_str())) {
            println!("{key}\t{text:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_unescaped_and_trimmed() {
        let plain = "<entry name=\"A\"> one &amp; two &#39;x&#x27; </entry><entry name=\"B\">3</entry>";
        assert_eq!(entries(plain), [("A".to_owned(), "one & two 'x'".to_owned()), ("B".to_owned(), "3".to_owned())]);
    }

    #[test]
    fn a_bare_ampersand_stays() {
        assert_eq!(unescape("a & b &unknown; c"), "a & b &unknown; c");
    }
}

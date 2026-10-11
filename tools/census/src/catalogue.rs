//! Stream the generated catalogue; gzip failures must never look like complete input.
use std::{
    io::{self, BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
};
pub struct Catalogue {
    input: Option<Box<dyn BufRead>>,
    child: Option<Child>,
    done: bool,
}
impl Catalogue {
    pub fn open(directory: &Path) -> io::Result<Self> {
        let plain = directory.join("source-objects.jsonl");
        if plain.exists() {
            return Ok(Self {
                input: Some(Box::new(BufReader::new(std::fs::File::open(plain)?))),
                child: None,
                done: false,
            });
        }
        Self::gzip(&directory.join("source-objects.jsonl.gz"))
    }
    fn gzip(path: &Path) -> io::Result<Self> {
        let mut child = Command::new("/usr/bin/gzip")
            .arg("-dc")
            .arg("--")
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("gzip stdout unavailable"))?;
        Ok(Self {
            input: Some(Box::new(BufReader::new(stdout))),
            child: Some(child),
            done: false,
        })
    }
}
impl Iterator for Catalogue {
    type Item = io::Result<String>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut line = String::new();
        match self.input.as_mut().unwrap().read_line(&mut line) {
            Ok(0) => {
                self.done = true;
                self.input.take();
                if let Some(child) = self.child.take() {
                    match child.wait_with_output() {
                        Ok(output) if output.status.success() => {}
                        Ok(output) => {
                            return Some(Err(io::Error::other(format!(
                                "gzip catalogue failed: {}: {}",
                                output.status,
                                String::from_utf8_lossy(&output.stderr)
                            ))))
                        }
                        Err(error) => return Some(Err(error)),
                    }
                }
                None
            }
            Ok(_) => Some(Ok(line)),
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}
impl Drop for Catalogue {
    fn drop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture() -> (std::path::PathBuf, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!(
            "hk-catalogue-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut child = Command::new("/usr/bin/gzip")
            .arg("-c")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"{\"row\":1}\n{\"row\":2}\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        (dir, output.stdout)
    }
    #[test]
    fn valid_gzip_reaches_checked_eof() {
        let (dir, bytes) = fixture();
        std::fs::write(dir.join("source-objects.jsonl.gz"), bytes).unwrap();
        let rows = Catalogue::open(&dir)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_gzip_cannot_report_success_after_valid_rows() {
        let (dir, mut bytes) = fixture();
        let tail = bytes.len() - 8;
        bytes[tail] ^= 1;
        std::fs::write(dir.join("source-objects.jsonl.gz"), bytes).unwrap();
        let result = Catalogue::open(&dir)
            .unwrap()
            .collect::<io::Result<Vec<_>>>();
        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

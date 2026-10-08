use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::time::{Duration, Instant};

pub fn append_pid(work: &Path, label: &str, pid: u32) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(work.join("PIDS")) { let _ = writeln!(f, "{label} {pid}"); }
}

/// Wait for exit; on timeout kill exactly this child. True on a zero exit.
pub fn wait_with_timeout(mut child: Child, secs: u64) -> Result<bool, String> {
    let t0 = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(st) => return Ok(st.success()),
            None => {
                if t0.elapsed() > Duration::from_secs(secs) { let _ = child.kill(); let _ = child.wait(); return Ok(false); }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

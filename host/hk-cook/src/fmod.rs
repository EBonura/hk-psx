//! AudioClip samples as UnityPy hands them to the cookers: the clip's FSB5
//! bank decoded by FMOD into a 16-bit WAV (fmod_toolkit `raw_to_wav`).
//!
//! The decoder is the FMOD library itself, the same build fmod_toolkit ships
//! (`HK_FMOD_LIB`, or the project venv's `fmod_toolkit/libfmod`), loaded at
//! run time. A decoder of our own would not give the same PCM, and every
//! cooked clip downstream depends on it bit for bit.

use crate::common::{err, Result};
use hk_unity::{Source, Value};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type Handle = *mut c_void;
struct Api {
    system_create: unsafe extern "C" fn(*mut Handle, c_uint) -> c_int,
    system_set_output: unsafe extern "C" fn(Handle, c_int) -> c_int,
    system_init: unsafe extern "C" fn(Handle, c_int, c_uint, *mut c_void) -> c_int,
    create_sound:
        unsafe extern "C" fn(Handle, *const c_char, c_uint, *mut c_void, *mut Handle) -> c_int,
    num_subsounds: unsafe extern "C" fn(Handle, *mut c_int) -> c_int,
    get_subsound: unsafe extern "C" fn(Handle, c_int, *mut Handle) -> c_int,
    get_format:
        unsafe extern "C" fn(Handle, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> c_int,
    get_length: unsafe extern "C" fn(Handle, *mut c_uint, c_uint) -> c_int,
    get_defaults: unsafe extern "C" fn(Handle, *mut f32, *mut c_int) -> c_int,
    lock: unsafe extern "C" fn(
        Handle,
        c_uint,
        c_uint,
        *mut *mut c_void,
        *mut *mut c_void,
        *mut c_uint,
        *mut c_uint,
    ) -> c_int,
    unlock: unsafe extern "C" fn(Handle, *mut c_void, *mut c_void, c_uint, c_uint) -> c_int,
    release: unsafe extern "C" fn(Handle) -> c_int,
}
// SAFETY: the function pointers are plain C entry points of a loaded library.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

/// pyfmodex's `header_version`.
const HEADER_VERSION: c_uint = 0x0002_0230;
/// FMOD_OUTPUTTYPE_NOSOUND.
const OUTPUT_NOSOUND: c_int = 2;
const MODE_OPENMEMORY: c_uint = 0x0000_0800;
const TIMEUNIT_PCMBYTES: c_uint = 4;
const FORMAT_PCMFLOAT: c_int = 5;
/// sizeof(FMOD_CREATESOUNDEXINFO) as pyfmodex declares it.
const EXINFO_BYTES: usize = 224;

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
/// One FMOD system per channel count, as fmod_toolkit keeps them.
static SYSTEMS: OnceLock<Mutex<HashMap<i32, usize>>> = OnceLock::new();

/// Where fmod_toolkit's library is: `HK_FMOD_LIB`, else the venv beside `root`.
fn library_path(root: &Path) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("HK_FMOD_LIB") {
        return Ok(PathBuf::from(p));
    }
    let lib = root.join(".venv/lib");
    for entry in std::fs::read_dir(&lib).map_err(|e| format!("{}: {e}", lib.display()))? {
        let dir = entry
            .map_err(|e| e.to_string())?
            .path()
            .join("site-packages/fmod_toolkit/libfmod");
        let system = if cfg!(target_os = "macos") {
            "Darwin"
        } else {
            "Linux"
        };
        let candidate = dir.join(system).join(if cfg!(target_os = "macos") {
            "libfmod.dylib"
        } else {
            "libfmod.so"
        });
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    err("no FMOD library: set HK_FMOD_LIB or install the project venv (fmod_toolkit)")
}

/// A resolved symbol as the C function type its field declares.
unsafe fn entry<T: Copy>(p: *mut c_void) -> T {
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
    // SAFETY: the caller names the documented signature of this FMOD entry point.
    unsafe { std::mem::transmute_copy(&p) }
}

fn api(root: &Path) -> Result<&'static Api> {
    API.get_or_init(|| {
        let path = library_path(root)?;
        let c = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: loading a C library and resolving its documented entry points.
        unsafe {
            let handle = dlopen(c.as_ptr(), 2);
            if handle.is_null() {
                return Err(format!("cannot load {}", path.display()));
            }
            let sym = |name: &str| -> std::result::Result<*mut c_void, String> {
                let n = CString::new(name).unwrap();
                let p = dlsym(handle, n.as_ptr());
                if p.is_null() {
                    Err(format!("FMOD has no {name}"))
                } else {
                    Ok(p)
                }
            };
            Ok(Api {
                system_create: entry(sym("FMOD_System_Create")?),
                system_set_output: entry(sym("FMOD_System_SetOutput")?),
                system_init: entry(sym("FMOD_System_Init")?),
                create_sound: entry(sym("FMOD_System_CreateSound")?),
                num_subsounds: entry(sym("FMOD_Sound_GetNumSubSounds")?),
                get_subsound: entry(sym("FMOD_Sound_GetSubSound")?),
                get_format: entry(sym("FMOD_Sound_GetFormat")?),
                get_length: entry(sym("FMOD_Sound_GetLength")?),
                get_defaults: entry(sym("FMOD_Sound_GetDefaults")?),
                lock: entry(sym("FMOD_Sound_Lock")?),
                unlock: entry(sym("FMOD_Sound_Unlock")?),
                release: entry(sym("FMOD_Sound_Release")?),
            })
        }
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn check(what: &str, result: c_int) -> Result<()> {
    if result == 0 {
        Ok(())
    } else {
        err(format!("FMOD {what} failed ({result})"))
    }
}

/// fmod_toolkit `raw_to_wav` for the first subsound: the WAV bytes of
/// `data` (an FSB bank) decoded at `channels` and `frequency`, float PCM
/// converted to 16-bit as numpy does it.
pub fn raw_to_wav(root: &Path, data: &[u8], channels: i32, frequency: i32) -> Result<Vec<u8>> {
    let api = api(root)?;
    let systems = SYSTEMS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut systems = systems.lock().unwrap();
    // SAFETY: FMOD calls on handles FMOD created, serialised by the lock above.
    unsafe {
        let system = match systems.get(&channels) {
            Some(&s) => s as Handle,
            None => {
                let mut s: Handle = std::ptr::null_mut();
                check("System_Create", (api.system_create)(&mut s, HEADER_VERSION))?;
                // Offline decoding never plays anything: mix to NOSOUND so a host
                // without an audio device decodes too.
                check(
                    "System_SetOutput",
                    (api.system_set_output)(s, OUTPUT_NOSOUND),
                )?;
                check(
                    "System_Init",
                    (api.system_init)(s, channels, 0, std::ptr::null_mut()),
                )?;
                systems.insert(channels, s as usize);
                s
            }
        };
        let mut exinfo = [0u8; EXINFO_BYTES];
        exinfo[0..4].copy_from_slice(&(EXINFO_BYTES as i32).to_le_bytes());
        exinfo[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        exinfo[12..16].copy_from_slice(&channels.to_le_bytes());
        exinfo[16..20].copy_from_slice(&frequency.to_le_bytes());
        let mut sound: Handle = std::ptr::null_mut();
        check(
            "CreateSound",
            (api.create_sound)(
                system,
                data.as_ptr() as *const c_char,
                MODE_OPENMEMORY,
                exinfo.as_mut_ptr() as *mut c_void,
                &mut sound,
            ),
        )?;
        let mut count = 0;
        check("GetNumSubSounds", (api.num_subsounds)(sound, &mut count))?;
        if count < 1 {
            (api.release)(sound);
            return err("FSB bank without a subsound");
        }
        let mut sub: Handle = std::ptr::null_mut();
        check("GetSubSound", (api.get_subsound)(sound, 0, &mut sub))?;
        let (mut kind, mut format, mut chans, mut bits) = (0, 0, 0, 0);
        check(
            "GetFormat",
            (api.get_format)(sub, &mut kind, &mut format, &mut chans, &mut bits),
        )?;
        let mut length: c_uint = 0;
        check(
            "GetLength",
            (api.get_length)(sub, &mut length, TIMEUNIT_PCMBYTES),
        )?;
        let (mut freq, mut priority) = (0f32, 0);
        check(
            "GetDefaults",
            (api.get_defaults)(sub, &mut freq, &mut priority),
        )?;
        let rate = freq as i32;
        let (audio_format, bits, data_len, convert) = match format {
            1..=4 => (1i16, bits, length as usize, false),
            FORMAT_PCMFLOAT => (1, 16, length as usize / 2, true),
            other => return err(format!("Sound format {other} is not supported.")),
        };
        // fmod_toolkit sizes its buffer at data + 40 and writes the samples
        // from byte 44 on, which grows it to data + 44.
        let mut wav = vec![0u8; 44];
        wav[0..4].copy_from_slice(b"RIFF");
        wav[4..8].copy_from_slice(&((data_len + 36) as i32).to_le_bytes());
        wav[8..12].copy_from_slice(b"WAVE");
        wav[12..16].copy_from_slice(b"fmt ");
        wav[16..20].copy_from_slice(&16i32.to_le_bytes());
        wav[20..22].copy_from_slice(&audio_format.to_le_bytes());
        wav[22..24].copy_from_slice(&(chans as i16).to_le_bytes());
        wav[24..28].copy_from_slice(&rate.to_le_bytes());
        wav[28..32].copy_from_slice(&(rate * chans * bits / 8).to_le_bytes());
        wav[32..34].copy_from_slice(&((chans * bits / 8) as i16).to_le_bytes());
        wav[34..36].copy_from_slice(&(bits as i16).to_le_bytes());
        wav[36..40].copy_from_slice(b"data");
        wav[40..44].copy_from_slice(&(data_len as i32).to_le_bytes());
        let (mut p1, mut p2) = (std::ptr::null_mut(), std::ptr::null_mut());
        let (mut l1, mut l2) = (0, 0);
        check(
            "Lock",
            (api.lock)(sub, 0, length, &mut p1, &mut p2, &mut l1, &mut l2),
        )?;
        for (p, l) in [(p1, l1), (p2, l2)] {
            if p.is_null() || l == 0 {
                continue;
            }
            let bytes = std::slice::from_raw_parts(p as *const u8, l as usize);
            if convert {
                for b in bytes.chunks_exact(4) {
                    let v = f32::from_le_bytes(b.try_into().unwrap()) * 32768.0f32;
                    wav.extend_from_slice(&(v.clamp(-32768.0, 32767.0) as i16).to_le_bytes());
                }
            } else {
                wav.extend_from_slice(bytes);
            }
        }
        if wav.len() < data_len + 40 {
            wav.resize(data_len + 40, 0);
        }
        (api.unlock)(sub, p1, p2, l1, l2);
        (api.release)(sub);
        (api.release)(sound);
        Ok(wav)
    }
}

/// UnityPy's `AudioClip.samples` for a clip whose tree `t` was read: the WAV
/// of its one sound. `m_AudioData` is the FSB bank, or, when empty, the slice
/// of the `.resS` file `m_Resource` names; a RIFF payload is taken as is.
pub fn clip_wav(root: &Path, source: &Source, t: &Value) -> Result<Vec<u8>> {
    let data = match t.get("m_AudioData") {
        Some(Value::Bytes(b)) if !b.is_empty() => b.clone(),
        _ => {
            let r = t
                .get("m_Resource")
                .ok_or("AudioClip with neither m_AudioData nor m_Resource")?;
            let path = r.get("m_Source").and_then(Value::str).unwrap_or_default();
            let base = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            let offset = r.get("m_Offset").and_then(Value::int).unwrap_or(0) as usize;
            let size = r.get("m_Size").and_then(Value::int).unwrap_or(0) as usize;
            let bytes = source
                .resource(&source.directory.join(&base))
                .map_err(|x| x.to_string())?;
            bytes
                .get(offset..offset + size)
                .ok_or("audio resource out of range")?
                .to_vec()
        }
    };
    if data.starts_with(b"RIFF") {
        return Ok(data);
    }
    let channels = t
        .get("m_Channels")
        .and_then(Value::int)
        .filter(|&c| c != 0)
        .unwrap_or(2) as i32;
    let frequency = t
        .get("m_Frequency")
        .and_then(Value::int)
        .filter(|&c| c != 0)
        .unwrap_or(44100) as i32;
    raw_to_wav(root, &data, channels, frequency)
}

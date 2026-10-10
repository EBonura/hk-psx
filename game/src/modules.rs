//! Streaming code modules (host/code_modules.py): bosses and enemy families
//! whose code travels with the rooms that use them.
//!
//! A room's disc group starts with its code chunk: the packages of the modules
//! its actors need. The scene streamer's predictive prefetch reads the group
//! of the gate the Knight is approaching, chunk first, and `service` installs
//! the modules from those bytes a slice per frame while he walks; the gate
//! then finds them resident. Whatever is still missing at the gate (no
//! prefetch, no room in the pool, a prefetch that lost the race) is installed
//! from the staged chunk during the admission, the old gate-time path.
//!
//! The pool is a fixed region below the stack reserve, inside one 64 KiB
//! `lui` window. A module lands at any free offset and is relocated: J26
//! jumps and W32 words shift by the distance moved, every HI16 becomes the
//! window, every LO16 shifts by one constant. Resident code reaches a module
//! only through the import sites the build lists; installing binds them,
//! evicting points them back at `__hk_module_trap`. The current room's
//! modules are pinned; anything else is evicted least recently used first.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/modules.rs"));

const J26: u32 = 1;
const HI16: u32 = 2;
const LO16: u32 = 3;
const W32: u32 = 4;
const PACKAGE_MAGIC: u32 = 0x444D_4B48;

/// Per code chunk: byte length, byte FNV-1a (the pack directory's) and word
/// hash (what an install checks), written into the EXE by the build.
#[no_mangle]
pub static mut HK_CODE_LEN: [u32; PACKAGE_CHUNKS] = [u32::MAX; PACKAGE_CHUNKS];
#[no_mangle]
pub static mut HK_CODE_FNV: [u32; PACKAGE_CHUNKS] = [u32::MAX; PACKAGE_CHUNKS];
#[no_mangle]
pub static mut HK_CODE_HASH: [u32; PACKAGE_CHUNKS] = [u32::MAX; PACKAGE_CHUNKS];
/// A chunk shipped as HLZC (art only, host/code_modules.py `art_stored`): its
/// stored length and FNV-1a; zero length when it ships raw.
#[no_mangle]
pub static mut HK_CODE_STORED_LEN: [u32; PACKAGE_CHUNKS] = [u32::MAX; PACKAGE_CHUNKS];
#[no_mangle]
pub static mut HK_CODE_STORED_FNV: [u32; PACKAGE_CHUNKS] = [u32::MAX; PACKAGE_CHUNKS];
/// Resident import sites: (address, kind << 28 | module << 20 | target
/// offset), and each module's (first << 0 | count << 16) range. Build-written.
#[no_mangle]
pub static mut HK_MODULE_IMPORTS: [[u32; 2]; MAX_IMPORTS] = [[u32::MAX; 2]; MAX_IMPORTS];
#[no_mangle]
pub static mut HK_MODULE_IMPORT_RANGE: [u32; MODULE_COUNT] = [u32::MAX; MODULE_COUNT];

/// Bitmask of resident modules, installs (all, in the background, at a
/// gate), evictions, gates into rooms with code that found it all resident
/// (hits) or not (misses), boss/family frames refused for a missing module,
/// background installs abandoned (prefetch moved on) or skipped (no room
/// without evicting the current room's code), pool bytes in use and peak,
/// the last gate install's check, copy, fixups and binding in HBlanks.
#[no_mangle]
pub static mut HK_MODULE_RESIDENT: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_INSTALLS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_BG_INSTALLS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_GATE_INSTALLS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_EVICTIONS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_GATE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_GATE_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_MISSING: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_BG_ABORTS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_BG_NOROOM: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_POOL_USED: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_POOL_PEAK: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_GATE_HBLANKS: u32 = 0;
/// The last gate install's wait for its chunk's sectors, in HBlanks.
#[no_mangle]
pub static mut HK_MODULE_GATE_WAIT_HBLANKS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_BG_FRAMES: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_TRAPS: u32 = 0;
/// Chunks read straight into the pool (code ahead of a gate, art after the
/// scene data or after a gate), and prop draws skipped because their room's
/// art had not arrived yet.
#[no_mangle]
pub static mut HK_MODULE_POOL_FETCHES: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_ART_LATE: u32 = 0;
/// Gates that had to read their room's art themselves, and the last one's cost.
#[no_mangle]
pub static mut HK_MODULE_GATE_ART_READS: u32 = 0;
#[no_mangle]
pub static mut HK_MODULE_GATE_ART_HBLANKS: u32 = 0;

extern "C" {
    static POOL_BASE: u8;
}

/// Where resident code lands when it calls a module that is not resident.
/// Nothing should: the accessors refuse first. Count it and stop loudly.
#[no_mangle]
pub extern "C" fn __hk_module_trap() -> ! {
    unsafe {
        HK_MODULE_TRAPS = HK_MODULE_TRAPS.saturating_add(1);
    }
    panic!("call into a code module that is not resident");
}

#[derive(Clone, Copy)]
struct Slot {
    offset: u32,
    bytes: u32,
    used: u32,
}
const EMPTY: Slot = Slot {
    offset: 0,
    bytes: 0,
    used: 0,
};
static mut SLOTS: [Slot; MODULE_COUNT] = [EMPTY; MODULE_COUNT];
/// Modules the resident room needs: never evicted.
static mut PINNED: u32 = 0;
/// The resident room (manifest index), for its art.
static mut CURRENT: usize = usize::MAX;
/// Pool bytes a chunk read is landing in: (offset, bytes), bytes 0 when none.
static mut RESERVE: (u32, u32) = (0, 0);
static mut CLOCK: u32 = 0;

fn pool() -> usize {
    unsafe { &raw const POOL_BASE as usize }
}
fn resident() -> u32 {
    unsafe { HK_MODULE_RESIDENT }
}
fn slots() -> &'static mut [Slot; MODULE_COUNT] {
    unsafe { &mut *(&raw mut SLOTS) }
}
fn hblanks() -> u16 {
    psx_io::timers::counter(psx_io::timers::Timer::Timer1)
}

/// Room logic module `m` is resident: guard its entry points with this
/// where nothing should happen outside its room anyway.
pub fn loaded(m: usize) -> bool {
    (resident() | ALWAYS) & (1 << m) != 0
}
/// The code for actors of module `m` may run now.
pub fn require(m: usize) -> bool {
    if (resident() | ALWAYS) & (1 << m) != 0 {
        return true;
    }
    unsafe {
        HK_MODULE_MISSING = HK_MODULE_MISSING.saturating_add(1);
    }
    false
}
pub const fn scene_modules(scene: usize) -> u32 {
    SCENE_MODULES[scene]
}
/// Carried data: (module, manifest scene) pairs gameplay asked for, which the
/// scene's gate loads like its own art (`carry`).
static mut CARRIED: [(u8, u16); 4] = [(u8::MAX, u16::MAX); 4];
/// Ask for carried data module `m` (one with its own chunk, host/code_modules.py
/// `CARRIED`) to be resident whenever manifest scene `scene` is, or for no scene
/// (`None`). A gate into that scene reads it with the scene's art; it is pinned
/// while the scene is current and evictable otherwise. Cheap: call it whenever
/// the answer may have changed.
pub fn carry(m: usize, scene: Option<usize>) {
    let slots = unsafe { &mut *(&raw mut CARRIED) };
    let s = scene.map_or(u16::MAX, |s| s as u16);
    if let Some(e) = slots.iter_mut().find(|e| e.0 as usize == m) {
        e.1 = s;
        return;
    }
    if let Some(e) = slots.iter_mut().find(|e| e.0 == u8::MAX) {
        *e = (m as u8, s);
    }
}
/// Every module `scene` needs: its rooms' own and what is carried into it.
fn wanted(scene: usize) -> u32 {
    let mut w = scene_modules(scene);
    for &(m, s) in unsafe { &*(&raw const CARRIED) } {
        if m != u8::MAX && s as usize == scene {
            w |= 1 << m;
        }
    }
    w
}
/// The chunk that brings `scene` a missing data module: its own art chunk
/// first, then a carried module's.
fn missing_art_chunk(scene: usize) -> Option<usize> {
    let missing = wanted(scene) & !CODE_MASK & !resident();
    if missing == 0 {
        return None;
    }
    if missing & scene_modules(scene) != 0 {
        if let Some(c) = art_chunk(scene) {
            return Some(c);
        }
    }
    (0..MODULE_COUNT)
        .find(|&m| missing & (1 << m) != 0 && MODULE_CHUNK[m] != u8::MAX)
        .map(|m| MODULE_CHUNK[m] as usize)
}
/// A resident data package's bytes (room art), or None while it is not.
pub fn data(m: usize) -> Option<&'static [u8]> {
    if resident() & (1 << m) == 0 {
        unsafe {
            HK_MODULE_ART_LATE = HK_MODULE_ART_LATE.saturating_add(1);
        }
        return None;
    }
    let s = slots()[m];
    Some(unsafe {
        core::slice::from_raw_parts(
            core::ptr::with_exposed_provenance::<u8>(pool() + s.offset as usize),
            s.bytes as usize,
        )
    })
}
/// Every module `scene`'s actors need is resident.
pub fn all_resident(scene: usize) -> bool {
    SCENE_MODULES[scene] & !resident() == 0
}
/// Every code module `scene`'s actors need is resident.
pub fn all_code_resident(scene: usize) -> bool {
    SCENE_MODULES[scene] & CODE_MASK & !resident() == 0
}
pub const fn code_chunk(scene: usize) -> Option<usize> {
    let c = SCENE_CODE[scene];
    if c == u8::MAX {
        None
    } else {
        Some(c as usize)
    }
}
/// The art chunk ending `scene`'s group (a package chunk id).
pub const fn art_chunk(scene: usize) -> Option<usize> {
    let c = SCENE_ART[scene];
    if c == u8::MAX {
        None
    } else {
        Some(c as usize)
    }
}
pub fn chunk_len(c: usize) -> usize {
    unsafe { core::ptr::read_volatile((&raw const HK_CODE_LEN).cast::<u32>().add(c)) as usize }
}
pub fn chunk_fnv(c: usize) -> u32 {
    unsafe { core::ptr::read_volatile((&raw const HK_CODE_FNV).cast::<u32>().add(c)) }
}
fn chunk_hash(c: usize) -> u32 {
    unsafe { core::ptr::read_volatile((&raw const HK_CODE_HASH).cast::<u32>().add(c)) }
}
/// Chunk `c` as the disc holds it: (bytes, FNV-1a), stored form if compressed.
pub fn disc_spec(c: usize) -> (usize, u32) {
    match stored_len(c) {
        0 => (chunk_len(c), chunk_fnv(c)),
        s => (s, stored_fnv(c)),
    }
}
fn stored_len(c: usize) -> usize {
    unsafe {
        core::ptr::read_volatile((&raw const HK_CODE_STORED_LEN).cast::<u32>().add(c)) as usize
    }
}
fn stored_fnv(c: usize) -> u32 {
    unsafe { core::ptr::read_volatile((&raw const HK_CODE_STORED_FNV).cast::<u32>().add(c)) }
}
/// Pool bytes a fetch of chunk `c` reserves: its packages, plus LZ4's
/// in-place margin when it ships compressed (host/code_modules.py
/// `art_reserve`, which checked the decode fits).
fn reserve_bytes(c: usize) -> u32 {
    let (raw, stored) = (chunk_len(c), stored_len(c));
    if stored == 0 {
        return (raw.div_ceil(2048) * 2048) as u32;
    }
    ((raw + (stored >> 8) + 64).div_ceil(2048) * 2048).max(stored.div_ceil(2048) * 2048) as u32
}

/// A package inside a code chunk: module id, image words, fixup words.
struct Package {
    module: usize,
    image: *const u32,
    words: usize,
    fixups: *const u32,
    count: usize,
}
/// The package for module `m` in `chunk` (word aligned).
fn find(chunk: &[u32], m: usize) -> Option<Package> {
    let mut at = 0;
    while at + 5 <= chunk.len() {
        let (magic, id, words, count) = (
            chunk[at],
            chunk[at + 1] as usize,
            chunk[at + 2] as usize,
            chunk[at + 3] as usize,
        );
        if magic != PACKAGE_MAGIC || at + 5 + words + count > chunk.len() {
            return None;
        }
        if id == m {
            let p = chunk.as_ptr();
            return Some(Package {
                module: id,
                image: unsafe { p.add(at + 5) },
                words,
                fixups: unsafe { p.add(at + 5 + words) },
                count,
            });
        }
        at += 5 + words + count;
    }
    None
}

/// Centre of the 64 KiB `lui` window holding `address`. A module never
/// straddles two (see `gap`), so one HI16 value serves all of it.
const fn window_of(address: u32) -> u32 {
    address.wrapping_add(0x8000) & 0xffff_0000
}
/// Lowest free offset that holds `bytes`, between the resident slots and
/// inside one `lui` window.
fn gap(bytes: u32) -> Option<u32> {
    gap_in(bytes, true)
}
/// `gap`, with `fenced` false for data, which no `lui` window binds.
fn gap_in(bytes: u32, fenced: bool) -> Option<u32> {
    if bytes == 0 {
        return Some(0);
    }
    let mut at = 0u32;
    loop {
        let mut moved = false;
        let start = pool() as u32 + at;
        if fenced && window_of(start) != window_of(start + bytes - 1) {
            // Move to the next window's first byte.
            at = window_of(start)
                .wrapping_add(0x8000)
                .wrapping_sub(pool() as u32);
            moved = true;
        }
        for (m, s) in slots().iter().enumerate() {
            if resident() & (1 << m) != 0 && s.offset < at + bytes && at < s.offset + s.bytes {
                at = s.offset + s.bytes;
                moved = true;
            }
        }
        let (r, rb) = unsafe { RESERVE };
        if rb != 0 && r < at + bytes && at < r + rb {
            at = r + rb;
            moved = true;
        }
        if !moved {
            break;
        }
    }
    (at + bytes <= POOL_BYTES as u32).then_some(at)
}
/// Room for `bytes`, evicting least recently used modules outside `keep`.
#[inline(never)]
fn allocate(bytes: u32, keep: u32) -> Option<u32> {
    allocate_in(bytes, keep, true)
}
/// `allocate`, unfenced for data (`gap_in`).
fn allocate_in(bytes: u32, keep: u32, fenced: bool) -> Option<u32> {
    if let Some(at) = gap_in(bytes, fenced) {
        return Some(at);
    }
    // Evict only the modules in one place that fits: one that keeps every
    // art package if there is such a place (art is shared by many rooms, grub
    // jars most of all, and costs a read at a gate), then the place whose most
    // recently used module is oldest, then the fewest modules. Evicting in
    // plain LRU order until a gap opens also drops modules nowhere near it.
    let mut best = Place {
        art: true,
        newest: u32::MAX,
        count: usize::MAX,
        victims: 0,
        at: u32::MAX,
        fenced,
    };
    best.consider(0, bytes, keep);
    for m in 0..MODULE_COUNT {
        if resident() & (1 << m) != 0 {
            best.consider(slots()[m].offset, bytes, keep);
            best.consider(slots()[m].offset + slots()[m].bytes, bytes, keep);
        }
    }
    let pool = pool() as u32;
    let mut w = window_of(pool).wrapping_add(0x8000);
    while w < pool + POOL_BYTES as u32 {
        best.consider(w - pool, bytes, keep);
        w = w.wrapping_add(0x1_0000);
    }
    let (r, rb) = unsafe { RESERVE };
    if rb != 0 {
        best.consider(r + rb, bytes, keep);
    }
    if best.at == u32::MAX {
        return None;
    }
    for m in 0..MODULE_COUNT {
        if best.victims & (1 << m) != 0 {
            evict(m);
        }
    }
    Some(best.at)
}
/// A candidate place for `allocate`: whether it evicts art, the newest module
/// it would evict, how many, which, and where.
struct Place {
    art: bool,
    newest: u32,
    count: usize,
    victims: u32,
    at: u32,
    fenced: bool,
}
impl Place {
    #[inline(never)]
    fn consider(&mut self, at: u32, bytes: u32, keep: u32) {
        let start = pool() as u32 + at;
        if bytes == 0
            || at + bytes > POOL_BYTES as u32
            || (self.fenced && window_of(start) != window_of(start + bytes - 1))
        {
            return;
        }
        let (r, rb) = unsafe { RESERVE };
        if rb != 0 && r < at + bytes && at < r + rb {
            return;
        }
        let (mut victims, mut newest, mut count) = (0u32, 0u32, 0usize);
        for (m, s) in slots().iter().enumerate() {
            if resident() & (1 << m) != 0 && s.offset < at + bytes && at < s.offset + s.bytes {
                if keep & (1 << m) != 0 {
                    return;
                }
                victims |= 1 << m;
                newest = newest.max(s.used);
                count += 1;
            }
        }
        let art = victims & !CODE_MASK != 0;
        if (art, newest, count) < (self.art, self.newest, self.count) {
            *self = Place {
                art,
                newest,
                count,
                victims,
                at,
                fenced: self.fenced,
            };
        }
    }
}
fn bind(m: usize, address: Option<u32>) {
    let range = unsafe {
        core::ptr::read_volatile((&raw const HK_MODULE_IMPORT_RANGE).cast::<u32>().add(m))
    };
    let (first, count) = ((range & 0xffff) as usize, (range >> 16) as usize);
    let trap = __hk_module_trap as usize as u32;
    for i in first..first + count {
        let [site, info] = unsafe {
            core::ptr::read_volatile((&raw const HK_MODULE_IMPORTS).cast::<[u32; 2]>().add(i))
        };
        let target = address.map_or(trap, |a| a + (info & 0xfffff));
        // Bound, every target is in the pool's window; unbound, the trap's.
        let window = window_of(address.unwrap_or(trap));
        let p = core::ptr::with_exposed_provenance_mut::<u32>(site as usize);
        unsafe {
            let w = p.read_volatile();
            p.write_volatile(match info >> 28 {
                J26 => (w & 0xfc00_0000) | ((target >> 2) & 0x03ff_ffff),
                HI16 => (w & 0xffff_0000) | (window >> 16),
                LO16 => (w & 0xffff_0000) | (target.wrapping_sub(window) & 0xffff),
                _ => target,
            });
        }
    }
}
fn evict(m: usize) {
    bind(m, None);
    psx_rt::cache::flush_i_cache();
    unsafe {
        HK_MODULE_RESIDENT &= !(1 << m);
        HK_MODULE_POOL_USED -= slots()[m].bytes;
        HK_MODULE_EVICTIONS = HK_MODULE_EVICTIONS.saturating_add(1);
    }
}

/// One install, run in steps so the background path can spread it over
/// frames: copy, fix up, bind.
struct Install {
    module: usize,
    at: u32,
    step: usize,
    pkg: Package,
}
impl Install {
    /// Up to `budget` words of work; true when the module is resident.
    fn run(&mut self, budget: usize) -> bool {
        let dst = core::ptr::with_exposed_provenance_mut::<u32>(pool() + self.at as usize);
        let words = self.pkg.words;
        let mut left = budget;
        while left > 0 && self.step < words {
            let n = left.min(words - self.step);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.pkg.image.add(self.step),
                    dst.add(self.step),
                    n,
                );
            }
            self.step += n;
            left -= n;
        }
        let address = (pool() as u32) + self.at;
        let delta = address.wrapping_sub(LINK_BASE + LINK_STRIDE * self.module as u32);
        while left > 0 && self.step < words + self.pkg.count {
            let f = unsafe { self.pkg.fixups.add(self.step - words).read() };
            let p = unsafe { dst.add(((f & 0x0fff_ffff) / 4) as usize) };
            unsafe {
                let w = p.read();
                p.write(match f >> 28 {
                    J26 => {
                        let target = 0x8000_0000 | ((w & 0x03ff_ffff) << 2);
                        (w & 0xfc00_0000) | ((target.wrapping_add(delta) >> 2) & 0x03ff_ffff)
                    }
                    HI16 => (w & 0xffff_0000) | (window_of(address) >> 16),
                    LO16 => {
                        let offset = ((w & 0xffff) as i16 as i32 + 0x8000) as u32;
                        (w & 0xffff_0000)
                            | (address
                                .wrapping_add(offset)
                                .wrapping_sub(window_of(address))
                                & 0xffff)
                    }
                    W32 => w.wrapping_add(delta),
                    _ => w,
                });
            }
            self.step += 1;
            left -= 1;
        }
        if self.step < words + self.pkg.count {
            return false;
        }
        bind(self.module, Some(address));
        psx_rt::cache::flush_i_cache();
        unsafe {
            CLOCK += 1;
            slots()[self.module] = Slot {
                offset: self.at,
                bytes: (words * 4) as u32,
                used: CLOCK,
            };
            HK_MODULE_RESIDENT |= 1 << self.module;
            HK_MODULE_POOL_USED += (words * 4) as u32;
            HK_MODULE_POOL_PEAK = HK_MODULE_POOL_PEAK.max(HK_MODULE_POOL_USED);
            HK_MODULE_INSTALLS = HK_MODULE_INSTALLS.saturating_add(1);
        }
        true
    }
}
fn begin(pkg: Package, keep: u32) -> Option<Install> {
    let at = allocate((pkg.words * 4) as u32, keep)?;
    // Reserve the bytes now: mark nothing resident until bound, but keep the
    // slot so a second allocation does not reuse them.
    Some(Install {
        module: pkg.module,
        at,
        step: 0,
        pkg,
    })
}

/// Word FNV-1a, as host/code_modules.py `word_hash`.
fn hash(words: &[u32], h: u32) -> u32 {
    words
        .iter()
        .fold(h, |h, &w| (h ^ w).wrapping_mul(0x0100_0193))
}

// ---- background: install the predicted room's code from the prefetch.

enum Bg {
    Idle,
    Verify {
        scene: usize,
        gen: u32,
        done: usize,
        h: u32,
    },
    Install {
        scene: usize,
        gen: u32,
        job: Install,
    },
}
static mut BG: Bg = Bg::Idle;
static mut BG_DONE: (usize, u32) = (usize::MAX, 0);
/// A room whose background install gave up (no room, bad chunk): its
/// prefetch reads the whole group and the gate installs.
static mut GAVE_UP: usize = usize::MAX;
pub fn gave_up(scene: usize) -> bool {
    unsafe { GAVE_UP == scene }
}
const VERIFY_WORDS: usize = 2048;
const INSTALL_WORDS: usize = 1024;

fn bg() -> &'static mut Bg {
    unsafe { &mut *(&raw mut BG) }
}
fn abort_background() {
    if let Bg::Install { .. } | Bg::Verify { .. } = bg() {
        unsafe {
            HK_MODULE_BG_ABORTS = HK_MODULE_BG_ABORTS.saturating_add(1);
        }
    }
    *bg() = Bg::Idle;
}

/// Once per frame. While the drive prefetches the group of the gate the
/// Knight is approaching, check its code chunk as it lands and install the
/// modules it holds that are not resident, a slice of work per frame.
pub fn service() {
    pool_service(POOL_WORDS);
    let Some((scene, gen, base, landed)) = crate::disc::code_prefetch() else {
        if !matches!(bg(), Bg::Idle) {
            abort_background();
        }
        return;
    };
    let Some(c) = code_chunk(scene) else { return };
    let len = chunk_len(c);
    if matches!(bg(), Bg::Idle) {
        let missing = scene_modules(scene) & CODE_MASK & !resident();
        if missing == 0 || unsafe { BG_DONE } == (scene, gen) || landed < len {
            return;
        }
        *bg() = Bg::Verify {
            scene,
            gen,
            done: 0,
            h: 0x811c_9dc5,
        };
    }
    let chunk = unsafe { core::slice::from_raw_parts(base.cast::<u32>(), len / 4) };
    unsafe {
        HK_MODULE_BG_FRAMES = HK_MODULE_BG_FRAMES.saturating_add(1);
    }
    match bg() {
        Bg::Idle => {}
        Bg::Verify {
            scene: s,
            gen: g,
            done,
            h,
        } => {
            if (*s, *g) != (scene, gen) {
                abort_background();
                return;
            }
            let n = VERIFY_WORDS.min(chunk.len() - *done);
            *h = hash(&chunk[*done..*done + n], *h);
            *done += n;
            if *done == chunk.len() {
                if *h != chunk_hash(c) {
                    unsafe {
                        BG_DONE = (scene, gen);
                        GAVE_UP = scene;
                    }
                    abort_background();
                    return;
                }
                next_background(scene, gen, chunk);
            }
        }
        Bg::Install {
            scene: s,
            gen: g,
            job,
        } => {
            if (*s, *g) != (scene, gen) {
                abort_background();
                return;
            }
            if job.run(INSTALL_WORDS) {
                unsafe {
                    HK_MODULE_BG_INSTALLS = HK_MODULE_BG_INSTALLS.saturating_add(1);
                }
                next_background(scene, gen, chunk);
            }
        }
    }
}
fn next_background(scene: usize, gen: u32, chunk: &[u32]) {
    let want = scene_modules(scene) & CODE_MASK;
    let missing = want & !resident();
    let Some(m) = (0..MODULE_COUNT).find(|&m| missing & (1 << m) != 0) else {
        *bg() = Bg::Idle;
        unsafe {
            BG_DONE = (scene, gen);
        }
        return;
    };
    // Never evict the room the Knight is in, nor what this room needs.
    match find(chunk, m).and_then(|p| begin(p, unsafe { PINNED } | want)) {
        Some(job) => *bg() = Bg::Install { scene, gen, job },
        None => {
            unsafe {
                HK_MODULE_BG_NOROOM = HK_MODULE_BG_NOROOM.saturating_add(1);
                BG_DONE = (scene, gen);
                GAVE_UP = scene;
            }
            *bg() = Bg::Idle;
        }
    }
}

/// The start of a gate admission into `scene`, before anything moves the
/// prefetched bytes: finish a background install for this room, drop any
/// other one.
pub fn gate_begin(scene: usize) {
    pool_gate(scene);
    loop {
        match bg() {
            Bg::Idle => return,
            Bg::Verify { scene: s, .. } | Bg::Install { scene: s, .. } if *s != scene => {
                abort_background();
                return;
            }
            _ => {}
        }
        let before = resident();
        service();
        if matches!(bg(), Bg::Idle) && resident() == before {
            return;
        }
    }
}

/// The admission of `scene`: its modules resident and pinned. `chunk` stages
/// the scene's code chunk when something is missing (the gate-time path).
pub fn admit(scene: usize, chunk: impl FnOnce() -> Option<&'static [u8]>) -> bool {
    // Code only: the room's art is read at the end of the gate (admit_art).
    let want = scene_modules(scene) & CODE_MASK;
    unsafe {
        PINNED = wanted(scene);
        CURRENT = scene;
        CLOCK += 1;
        GAVE_UP = usize::MAX;
    }
    for m in 0..MODULE_COUNT {
        if wanted(scene) & resident() & (1 << m) != 0 {
            slots()[m].used = unsafe { CLOCK };
        }
    }
    if want == 0 {
        return true;
    }
    if want & !resident() == 0 {
        unsafe {
            HK_MODULE_GATE_HITS = HK_MODULE_GATE_HITS.saturating_add(1);
        }
        return true;
    }
    unsafe {
        HK_MODULE_GATE_MISSES = HK_MODULE_GATE_MISSES.saturating_add(1);
    }
    let t0 = hblanks();
    let Some(bytes) = chunk() else { return false };
    let t1 = hblanks();
    unsafe {
        HK_MODULE_GATE_WAIT_HBLANKS = t1.wrapping_sub(t0) as u32;
    }
    let Some(c) = code_chunk(scene) else {
        return false;
    };
    if bytes.len() != chunk_len(c) || bytes.as_ptr() as usize % 4 != 0 {
        return false;
    }
    let words =
        unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u32>(), bytes.len() / 4) };
    if hash(words, 0x811c_9dc5) != chunk_hash(c) {
        return false;
    }
    // A fragmented restart may drop modules installed earlier in the pass;
    // the second pass fills them.
    for _ in 0..2 {
        if !install_missing(want, words) {
            return false;
        }
        if want & !resident() == 0 {
            break;
        }
    }
    unsafe {
        HK_MODULE_GATE_HBLANKS = hblanks().wrapping_sub(t1) as u32;
    }
    want & !resident() == 0
}
fn install_missing(want: u32, words: &[u32]) -> bool {
    for m in 0..MODULE_COUNT {
        if want & !resident() & (1 << m) == 0 {
            continue;
        }
        let Some(pkg) = find(words, m) else {
            return false;
        };
        let mut job = match begin(pkg, want) {
            Some(job) => job,
            None => {
                // Fragmented: start over with only this room's code.
                for r in 0..MODULE_COUNT {
                    if resident() & (1 << r) != 0 {
                        evict(r);
                    }
                }
                let Some(job) = find(words, m).and_then(|p| begin(p, want)) else {
                    return false;
                };
                job
            }
        };
        while !job.run(usize::MAX) {}
        unsafe {
            HK_MODULE_GATE_INSTALLS = HK_MODULE_GATE_INSTALLS.saturating_add(1);
        }
        crate::input::checkpoint();
    }
    true
}

// ---- pool fetch: a room's code chunk (ahead of its gate) or art chunk
// (after the scene data, or after its gate) read by the drive straight into
// the pool, checked, then installed where it landed: no arena slack used,
// nothing copied unless an image would cross a `lui` window.

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Code,
    Art,
}
#[derive(Clone, Copy, PartialEq)]
enum Pf {
    Idle,
    Want,
    Reading,
    Landed,
    Decode,
    Verify { done: usize, h: u32 },
    Install { at: usize },
}
/// The in-place decode of a compressed chunk the pool fetch landed.
static mut PF_DECODER: Option<crate::room_decode::Decoder> = None;
struct PoolFetch {
    state: Pf,
    kind: Kind,
    scene: usize,
    chunk: usize,
    job: Option<Install>,
}
static mut PF: PoolFetch = PoolFetch {
    state: Pf::Idle,
    kind: Kind::Code,
    scene: usize::MAX,
    chunk: 0,
    job: None,
};
/// The last (kind, scene) a fetch gave up on, so it is not tried again until
/// the next admission.
static mut PF_GAVE_UP: (u8, usize) = (0, usize::MAX);
const POOL_WORDS: usize = 1024;
fn pf() -> &'static mut PoolFetch {
    unsafe { &mut *(&raw mut PF) }
}
fn release() {
    unsafe {
        RESERVE = (0, 0);
    }
}
fn pf_idle() {
    release();
    let p = pf();
    p.state = Pf::Idle;
    p.job = None;
    unsafe {
        PF_DECODER = None;
    }
}

/// The code of `scene` is resident or on its way into the pool, so neither
/// the prefetch nor the gate needs its code chunk's sectors.
pub fn code_coming(scene: usize) -> bool {
    let p = pf();
    p.kind == Kind::Code && p.scene == scene && p.state != Pf::Idle
}
/// Plan a fetch of `scene`'s code if it is not resident (called with the
/// prefetch's predicted room, before the prefetch is planned).
pub fn plan_code(scene: usize) {
    if pf().state != Pf::Idle || scene_modules(scene) & CODE_MASK & !resident() == 0 {
        return;
    }
    if let Some(c) = code_chunk(scene) {
        plan(Kind::Code, scene, c);
    }
}
fn plan(kind: Kind, scene: usize, c: usize) {
    if unsafe { PF_GAVE_UP } == (kind as u8, scene) {
        return;
    }
    let bytes = reserve_bytes(c);
    let keep = unsafe { PINNED } | wanted(scene);
    match allocate_in(bytes, keep, kind == Kind::Code) {
        Some(at) => {
            unsafe {
                RESERVE = (at, bytes);
            }
            let p = pf();
            *p = PoolFetch {
                state: Pf::Want,
                kind,
                scene,
                chunk: c,
                job: None,
            };
        }
        None => unsafe {
            PF_GAVE_UP = (kind as u8, scene);
            if kind == Kind::Code {
                GAVE_UP = scene;
            }
        },
    }
}
/// The drive asks for a pool read when it is free (code first, then art,
/// both before the room prefetch).
pub fn pool_want(code: bool) -> Option<(*mut u32, usize, usize)> {
    let p = pf();
    if p.state != Pf::Want || (p.kind == Kind::Code) != code {
        return None;
    }
    p.state = Pf::Reading;
    let (at, bytes) = unsafe { RESERVE };
    unsafe {
        HK_MODULE_POOL_FETCHES = HK_MODULE_POOL_FETCHES.saturating_add(1);
    }
    // A compressed chunk is read into the reservation's last sectors: its
    // stored bytes then end where the in-place decode wants them.
    let sectors = match stored_len(p.chunk) {
        0 => bytes as usize / 2048,
        s => s.div_ceil(2048),
    };
    let skip = bytes as usize - sectors * 2048;
    Some((
        core::ptr::with_exposed_provenance_mut::<u32>(pool() + at as usize + skip),
        sectors,
        p.chunk,
    ))
}
pub fn pool_landed(ok: bool) {
    let p = pf();
    if p.state != Pf::Reading {
        return;
    }
    if ok {
        p.state = Pf::Landed;
    } else {
        let (k, s) = (p.kind, p.scene);
        pf_idle();
        unsafe {
            PF_GAVE_UP = (k as u8, s);
        }
    }
}
/// Check and install a landed chunk, up to `budget` words of work.
#[inline(never)]
fn pool_service(budget: usize) {
    let p = pf();
    if p.state == Pf::Idle {
        // After a gate, the room's own art; ahead of one, the next room's.
        let current = unsafe { CURRENT };
        if current != usize::MAX && art_missing(current) {
            if let Some(c) = missing_art_chunk(current) {
                plan(Kind::Art, current, c);
            }
        } else if let Some(next) = crate::disc::predicted_scene().filter(|&n| n != current) {
            if art_missing(next)
                && (scene_modules(next) & CODE_MASK & !resident() == 0
                    || code_chunk(next).is_none())
            {
                if let Some(c) = missing_art_chunk(next) {
                    plan(Kind::Art, next, c);
                }
            }
        }
        return;
    }
    let (at, _) = unsafe { RESERVE };
    let len = chunk_len(p.chunk);
    let words = unsafe {
        core::slice::from_raw_parts(
            core::ptr::with_exposed_provenance::<u32>(pool() + at as usize),
            len / 4,
        )
    };
    let mut left = budget;
    while left > 0 {
        match p.state {
            Pf::Idle | Pf::Want | Pf::Reading => return,
            Pf::Landed if stored_len(p.chunk) != 0 => {
                // Decode in place: the stored bytes end the reservation's
                // slice; the decoder checks their hash, and the raw packages
                // land at the reservation's front.
                let (stored, bytes) = (stored_len(p.chunk), unsafe { RESERVE.1 } as usize);
                let start = bytes - stored.div_ceil(2048) * 2048;
                unsafe {
                    PF_DECODER = Some(
                        crate::room_decode::Decoder::new_bytes(
                            stored,
                            stored_fnv(p.chunk),
                            len,
                            chunk_fnv(p.chunk),
                        )
                        .at(start),
                    );
                }
                p.state = Pf::Decode;
            }
            Pf::Decode => {
                let (stored, bytes) = (stored_len(p.chunk), unsafe { RESERVE.1 } as usize);
                let end = bytes - stored.div_ceil(2048) * 2048 + stored;
                let arena = unsafe {
                    core::slice::from_raw_parts_mut(
                        core::ptr::with_exposed_provenance_mut::<u8>(pool() + at as usize),
                        end,
                    )
                };
                let step = unsafe {
                    (*(&raw mut PF_DECODER))
                        .as_mut()
                        .map(|d| d.step(arena, left))
                };
                left = 0;
                match step {
                    Some(Ok(None)) => return,
                    Some(Ok(Some(n))) if n == len => {
                        unsafe {
                            PF_DECODER = None;
                        }
                        p.state = Pf::Install { at: 0 };
                        if budget != usize::MAX {
                            return;
                        }
                        left = usize::MAX;
                    }
                    _ => {
                        unsafe {
                            PF_DECODER = None;
                        }
                        let (k, s) = (p.kind, p.scene);
                        pf_idle();
                        unsafe {
                            PF_GAVE_UP = (k as u8, s);
                        }
                        return;
                    }
                }
            }
            Pf::Landed => {
                p.state = Pf::Verify {
                    done: 0,
                    h: 0x811c_9dc5,
                }
            }
            Pf::Verify { done, h } => {
                let n = left.min(words.len() - done);
                let h = hash(&words[done..done + n], h);
                left -= n;
                if done + n < words.len() {
                    p.state = Pf::Verify { done: done + n, h };
                    continue;
                }
                if h != chunk_hash(p.chunk) {
                    let (k, s) = (p.kind, p.scene);
                    pf_idle();
                    unsafe {
                        PF_GAVE_UP = (k as u8, s);
                    }
                    return;
                }
                p.state = Pf::Install { at: 0 };
            }
            Pf::Install { at: w } => {
                if let Some(job) = p.job.as_mut() {
                    let done = job.run(left);
                    left = left.saturating_sub(left.min(1024));
                    if !done {
                        return;
                    }
                    unsafe {
                        HK_MODULE_BG_INSTALLS = HK_MODULE_BG_INSTALLS.saturating_add(1);
                    }
                    p.job = None;
                    continue;
                }
                // The next package that is not resident.
                if w + 5 > words.len() {
                    pf_idle();
                    return;
                }
                let (m, n, f) = (
                    words[w + 1] as usize,
                    words[w + 2] as usize,
                    words[w + 3] as usize,
                );
                p.state = Pf::Install { at: w + 5 + n + f };
                if words[w] != PACKAGE_MAGIC || m >= MODULE_COUNT || resident() & (1 << m) != 0 {
                    continue;
                }
                let image = pool() as u32 + at + 4 * (w as u32 + 5);
                let pkg = Package {
                    module: m,
                    image: words[w + 5..].as_ptr(),
                    words: n,
                    fixups: words[w + 5 + n..].as_ptr(),
                    count: f,
                };
                p.job = if n == 0
                    || m >= CODE_MODULES
                    || window_of(image) == window_of(image + 4 * n as u32 - 1)
                {
                    // In place: the image is already where it will run.
                    Some(Install {
                        module: m,
                        at: image - pool() as u32,
                        step: n,
                        pkg,
                    })
                } else {
                    begin(pkg, unsafe { PINNED } | wanted(p.scene))
                };
                if p.job.is_none() {
                    let (k, s) = (p.kind, p.scene);
                    pf_idle();
                    unsafe {
                        PF_GAVE_UP = (k as u8, s);
                    }
                    return;
                }
            }
        }
    }
}
/// A gate into `scene` begins: a fetch for this room finishes now, any other
/// one is dropped (its read is waited for first: the drive owns the bytes).
fn pool_gate(scene: usize) {
    // This room's code or art is worth waiting for (a room's art is ready on
    // its first faded-in frame: no pop-in); another room's read is not.
    let mine = pf().scene == scene;
    if pf().state == Pf::Reading {
        if mine {
            crate::disc::pool_wait();
        } else {
            crate::disc::pool_cancel();
        }
    }
    let p = pf();
    match p.state {
        Pf::Idle => {}
        Pf::Want => pf_idle(),
        _ if !mine => pf_idle(),
        _ => {
            while pf().state != Pf::Idle {
                pool_service(usize::MAX);
            }
        }
    }
    unsafe {
        PF_GAVE_UP = (0, usize::MAX);
    }
}
/// A pool fetch still needs the drive (planned, or reading).
pub fn fetch_pending() -> bool {
    matches!(pf().state, Pf::Want | Pf::Reading)
}
/// Some of `scene`'s art is not resident.
pub fn art_missing(scene: usize) -> bool {
    wanted(scene) & !CODE_MASK & !resident() != 0
}
/// Start reading `scene`'s art into the pool if it is missing and the drive's
/// pool fetch is free: called by the gate once the group's last sector has
/// landed, so the read overlaps the scene decode. The art chunk follows the
/// group on the disc, so it starts where the gate's own read ended.
pub fn art_begin(scene: usize) -> bool {
    if !art_missing(scene) || pf().state != Pf::Idle {
        return false;
    }
    let Some(c) = missing_art_chunk(scene) else {
        return false;
    };
    unsafe {
        PF_GAVE_UP = (0, usize::MAX);
    }
    plan(Kind::Art, scene, c);
    if pf().state != Pf::Want {
        return false;
    }
    crate::disc::pool_read_begin(unsafe { RESERVE.1 } as usize / 2048);
    true
}
/// The end of an admission: the room's art, if any is not resident yet,
/// landed (or read now) and installed, so the props are there on the first
/// faded-in frame. False when it could not be installed.
#[inline(never)]
pub fn admit_art(scene: usize) -> bool {
    if !art_missing(scene) {
        return true;
    }
    let t0 = hblanks();
    // The room's art chunk, then each carried package, one fetch at a time.
    for _ in 0..1 + 4 {
        if !art_missing(scene) {
            break;
        }
        let mine = pf().kind == Kind::Art && pf().scene == scene && pf().state != Pf::Idle;
        if !mine && !art_begin(scene) {
            return false;
        }
        crate::disc::pool_wait();
        while pf().state != Pf::Idle {
            pool_service(usize::MAX);
        }
    }
    unsafe {
        HK_MODULE_GATE_ART_READS = HK_MODULE_GATE_ART_READS.saturating_add(1);
        HK_MODULE_GATE_ART_HBLANKS = hblanks().wrapping_sub(t0) as u32;
    }
    !art_missing(scene)
}

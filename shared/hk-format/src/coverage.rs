//! HKOCSC01: scene-owned opaque coverage certificates. All offsets are bytes
//! except CoverageCert::offset, which addresses bits in its own bitmap section.
//! Section order/alignment and owner identity are checked before incremental
//! record validation. Native slices are exposed only from validated, aligned,
//! little-endian storage; no allocation or pointer-bearing cooked records.
use crate::{u16_at, u32_at, Error};

pub const NO_CERT: u16 = u16::MAX;
pub const GRID_SHIFT: u32 = 2;
const HEADER: usize = 80;
const SIZES: [usize; 6] = [12, 4, 2, 12, 4, 10];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    pub scene_id: u32,
    pub scene_raw_fnv: u32,
    /// FNV-1a over owner-order little-endian u32 tuples:
    /// (kind, first, count, raw_byte_length, raw_fnv).
    pub atlas_fnv: u32,
    pub draw_pool_count: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CoverageCert {
    pub gx: i16,
    pub gy: i16,
    pub width: u16,
    pub height: u16,
    pub offset: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CoverageGroup {
    pub certificate: u16,
    pub members: [u16; 4],
}
const _: () = assert!(core::mem::size_of::<CoverageCert>() == 12);
const _: () = assert!(core::mem::align_of::<CoverageCert>() == 4);
const _: () = assert!(core::mem::size_of::<CoverageGroup>() == 10);
const _: () = assert!(core::mem::align_of::<CoverageGroup>() == 2);

/// A cheap borrowed owner. Copying this view never extends the arena lifetime.
#[derive(Clone, Copy, Debug)]
pub struct CoverageView<'a> {
    bytes: &'a [u8],
}
impl<'a> CoverageView<'a> {
    pub fn parse(bytes: &'a [u8], expected: Expected) -> Result<Self, Error> {
        let mut check = CoverageValidation::new(bytes, expected)?;
        check.step(usize::MAX)?;
        check.finish()
    }
    fn header(bytes: &'a [u8], expected: Expected) -> Result<Self, Error> {
        if !cfg!(target_endian = "little")
            || bytes.len() < HEADER
            || bytes.as_ptr() as usize & 3 != 0
            || &bytes[..8] != b"HKOCSC01"
            || u32_at(bytes, 24) != GRID_SHIFT
            || u32_at(bytes, 28) != 0
        {
            return Err(Error::Header);
        }
        if expected.draw_pool_count > NO_CERT as u32 {
            return Err(Error::Limit);
        }
        if [
            u32_at(bytes, 8),
            u32_at(bytes, 12),
            u32_at(bytes, 16),
            u32_at(bytes, 20),
        ] != [
            expected.scene_id,
            expected.scene_raw_fnv,
            expected.atlas_fnv,
            expected.draw_pool_count,
        ] {
            return Err(Error::Reference);
        }
        let view = Self { bytes };
        if view.count(2) != expected.draw_pool_count as usize {
            return Err(Error::Reference);
        }
        for section in [0, 3, 5] {
            if view.count(section) > NO_CERT as usize {
                return Err(Error::Limit);
            }
        }
        let mut end = HEADER;
        // Exactly six sections and at most three padding bytes per boundary.
        for (section, size) in SIZES.iter().enumerate() {
            let aligned = end.checked_add(3).ok_or(Error::Truncated)? & !3;
            if view.offset(section) != aligned
                || aligned > bytes.len()
                || bytes[end..aligned].iter().any(|&v| v != 0)
            {
                return Err(Error::Header);
            }
            end = aligned
                .checked_add(
                    view.count(section)
                        .checked_mul(*size)
                        .ok_or(Error::Truncated)?,
                )
                .ok_or(Error::Truncated)?;
            if end > bytes.len() {
                return Err(Error::Truncated);
            }
        }
        if end.checked_add(3).ok_or(Error::Truncated)? & !3 != bytes.len()
            || bytes[end..].iter().any(|&v| v != 0)
        {
            return Err(Error::Truncated);
        }
        Ok(view)
    }
    /// # Safety
    /// These exact bytes must have completed CoverageValidation or parse with
    /// the intended owner's Expected identity. They must remain unchanged and
    /// four-byte aligned, on a little-endian target, for the returned lifetime.
    /// Do not reuse admission of an old arena owner after replacement.
    #[inline]
    pub unsafe fn validated_view(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    #[inline]
    fn offset(&self, section: usize) -> usize {
        u32_at(self.bytes, 32 + section * 8) as usize
    }
    #[inline]
    fn count(&self, section: usize) -> usize {
        u32_at(self.bytes, 36 + section * 8) as usize
    }
    #[inline]
    pub fn scene_id(&self) -> u32 {
        u32_at(self.bytes, 8)
    }
    #[inline]
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
    #[inline]
    pub fn draw_pool_count(&self) -> usize {
        self.count(2)
    }
    #[inline]
    pub fn grid_shift(&self) -> u32 {
        GRID_SHIFT
    }
    #[inline]
    pub fn draw_certificate(&self, pool: usize) -> Option<u16> {
        self.pool_map()
            .get(pool)
            .copied()
            .filter(|&id| id != NO_CERT)
    }
    #[inline]
    pub fn tile_cert(&self, id: u16) -> Option<&'a CoverageCert> {
        self.tile_certs().get(id as usize)
    }
    #[inline]
    pub fn group_cert(&self, id: u16) -> Option<&'a CoverageCert> {
        self.group_certs().get(id as usize)
    }
    #[inline]
    pub fn tile_certs(&self) -> &'a [CoverageCert] {
        unsafe { self.section(0) }
    }
    #[inline]
    pub fn tile_bits(&self) -> &'a [u32] {
        unsafe { self.section(1) }
    }
    #[inline]
    pub fn pool_map(&self) -> &'a [u16] {
        unsafe { self.section(2) }
    }
    #[inline]
    pub fn group_certs(&self) -> &'a [CoverageCert] {
        unsafe { self.section(3) }
    }
    #[inline]
    pub fn group_bits(&self) -> &'a [u32] {
        unsafe { self.section(4) }
    }
    #[inline]
    pub fn groups(&self) -> &'a [CoverageGroup] {
        unsafe { self.section(5) }
    }
    /// Only used with the fixed section's matching integer-only repr(C) type.
    #[inline]
    unsafe fn section<T>(&self, section: usize) -> &'a [T] {
        core::slice::from_raw_parts(
            self.bytes.as_ptr().add(self.offset(section)).cast::<T>(),
            self.count(section),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Tiles,
    PoolMap,
    GroupCerts,
    Groups,
    Done,
}
/// Immutable arena borrow enforces that bytes cannot change between steps.
/// Each work unit checks one certificate, pool reference, group, or phase end;
/// it never walks a whole bitmap, scene, or pool. Budget zero does no work.
/// finish consumes the validator and publishes only a completely checked view.
pub struct CoverageValidation<'a> {
    view: CoverageView<'a>,
    phase: Phase,
    index: usize,
    next_bit: u32,
    failed: bool,
}
impl<'a> CoverageValidation<'a> {
    pub fn new(bytes: &'a [u8], expected: Expected) -> Result<Self, Error> {
        Ok(Self {
            view: CoverageView::header(bytes, expected)?,
            phase: Phase::Tiles,
            index: 0,
            next_bit: 0,
            failed: false,
        })
    }
    pub fn step(&mut self, mut work_budget: usize) -> Result<bool, Error> {
        if self.failed {
            return Err(Error::Reference);
        }
        while work_budget != 0 && self.phase != Phase::Done {
            work_budget -= 1;
            if let Err(error) = self.unit() {
                self.failed = true;
                return Err(error);
            }
        }
        Ok(self.phase == Phase::Done)
    }
    pub fn finish(self) -> Result<CoverageView<'a>, Error> {
        if self.failed || self.phase != Phase::Done {
            Err(Error::Reference)
        } else {
            Ok(self.view)
        }
    }
    fn unit(&mut self) -> Result<(), Error> {
        match self.phase {
            Phase::Tiles | Phase::GroupCerts => {
                let section = if self.phase == Phase::Tiles { 0 } else { 3 };
                if self.index == self.view.count(section) {
                    if self.next_bit as usize / 32 != self.view.count(section + 1) {
                        return Err(Error::Reference);
                    }
                    self.phase = if section == 0 {
                        Phase::PoolMap
                    } else {
                        Phase::Groups
                    };
                    self.index = 0;
                    self.next_bit = 0;
                } else {
                    // No native references are constructed until the entire
                    // payload validates; scalar loads also work on host tests.
                    let p = self.view.offset(section) + self.index * 12;
                    let w = u16_at(self.view.bytes, p + 4) as u32;
                    let h = u16_at(self.view.bytes, p + 6) as u32;
                    let bit = u32_at(self.view.bytes, p + 8);
                    if w == 0 || h == 0 {
                        return Err(Error::Geometry);
                    }
                    if bit != self.next_bit {
                        return Err(Error::Reference);
                    }
                    let end = bit
                        .checked_add(w.checked_mul(h).ok_or(Error::Limit)?)
                        .ok_or(Error::Limit)?;
                    let aligned = end.checked_add(31).ok_or(Error::Limit)? & !31;
                    if aligned as usize / 32 > self.view.count(section + 1) {
                        return Err(Error::Truncated);
                    }
                    if end & 31 != 0 {
                        let last = u32_at(
                            self.view.bytes,
                            self.view.offset(section + 1) + (end as usize / 32) * 4,
                        );
                        if last >> (end & 31) != 0 {
                            return Err(Error::Geometry);
                        }
                    }
                    self.next_bit = aligned;
                    self.index += 1;
                }
            }
            Phase::PoolMap => {
                if self.index == self.view.count(2) {
                    self.phase = Phase::GroupCerts;
                    self.index = 0;
                } else {
                    let id = u16_at(self.view.bytes, self.view.offset(2) + self.index * 2);
                    if id != NO_CERT && id as usize >= self.view.count(0) {
                        return Err(Error::Reference);
                    }
                    self.index += 1;
                }
            }
            Phase::Groups => {
                if self.index == self.view.count(5) {
                    self.phase = Phase::Done;
                } else {
                    let p = self.view.offset(5) + self.index * 10;
                    if u16_at(self.view.bytes, p) as usize >= self.view.count(3) {
                        return Err(Error::Reference);
                    }
                    let members: [u16; 4] =
                        core::array::from_fn(|i| u16_at(self.view.bytes, p + 2 + i * 2));
                    let mut n = 0;
                    for (i, &member) in members.iter().enumerate() {
                        if member == NO_CERT {
                            continue;
                        }
                        if member as usize >= self.view.count(2)
                            || i != n
                            || (i != 0 && members[i - 1] >= member)
                        {
                            return Err(Error::Reference);
                        }
                        n += 1;
                    }
                    if n < 2 {
                        return Err(Error::Reference);
                    }
                    if self.index != 0 {
                        let previous: [u16; 4] =
                            core::array::from_fn(|i| u16_at(self.view.bytes, p - 8 + i * 2));
                        // Tuple order treats a shorter membership prefix as earlier;
                        // serialized NO_CERT padding therefore sorts before IDs.
                        let key = |row: [u16; 4]| {
                            row.map(|id| if id == NO_CERT { 0 } else { id as u32 + 1 })
                        };
                        if key(previous) >= key(members) {
                            return Err(Error::Reference);
                        }
                    }
                    self.index += 1;
                }
            }
            Phase::Done => {}
        }
        Ok(())
    }
}

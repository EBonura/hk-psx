//! Authored grass Slash Impact sprites; bounded transient instances, no particles.
use hk_format::{i32_at, u32_at, Room};
pub const CAPACITY: usize = 32;
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub clips: [u16; 2],
    pub ticks: u16,
}
#[derive(Clone, Copy, Debug)]
struct Impact {
    scene: u8,
    x: i32,
    y: i32,
    sign: i8,
    age: u16,
    variant: u8,
}
pub struct Pool {
    slots: [Option<Impact>; CAPACITY],
    pub dropped: u32,
}
impl Pool {
    pub const fn new() -> Self {
        Self {
            slots: [None; CAPACITY],
            dropped: 0,
        }
    }
    pub fn spawn(
        &mut self,
        scene: usize,
        source: usize,
        body: [i32; 4],
        nail: [i32; 4],
        sign: i32,
    ) -> bool {
        assert!(scene < 256 && (sign == 1 || sign == -1));
        let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        };
        // GrassCut uses the midpoint of the two collider bounds centers.
        let x = ((body[0] as i64 + body[2] as i64 + nail[0] as i64 + nail[2] as i64) / 4) as i32;
        let y = ((body[1] as i64 + body[3] as i64 + nail[1] as i64 + nail[3] as i64) / 4) as i32;
        *slot = Some(Impact {
            scene: scene as u8,
            x,
            y,
            sign: sign as i8,
            age: 0,
            variant: (source & 1) as u8,
        });
        true
    }
    pub fn tick(&mut self) {
        for slot in &mut self.slots {
            if let Some(effect) = slot {
                effect.age += 1;
                if effect.age >= 10 {
                    *slot = None;
                }
            }
        }
    }
    pub fn clear_scene(&mut self, scene: usize) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|s| s.scene as usize == scene) {
                *slot = None;
            }
        }
    }
    pub fn active(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
    pub fn draw(&self, scene: usize, spec: Option<Spec>, room: &Room, camera: (i32, i32)) -> u32 {
        self.draw_limited(scene, spec, room, camera, CAPACITY as u32)
            .0
    }
    pub fn draw_limited(
        &self,
        scene: usize,
        spec: Option<Spec>,
        room: &Room,
        camera: (i32, i32),
        limit: u32,
    ) -> (u32, u32) {
        let Some(spec) = spec else {
            return (0, 0);
        };
        assert_eq!(spec.ticks, 10);
        let mut count = 0;
        let mut skipped = 0;
        for effect in self
            .slots
            .iter()
            .flatten()
            .filter(|e| e.scene as usize == scene)
        {
            let clip = room.clip(spec.clips[effect.variant as usize] as usize);
            let offset = (effect.age as u32 * clip[2] / (60 * 65536)).min(clip[1] - 1);
            let f = room.frame((clip[0] + offset) as usize);
            let texture = u32_at(f, 0) as usize;
            assert!(
                !room.texture(texture).is_streamed(),
                "impact must not exhaust actor texture slots"
            );
            let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
            let mut verts = [(0i16, 0i16); 4];
            for (v, (x, y)) in
                verts
                    .iter_mut()
                    .zip([(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])])
            {
                let x = effect.x - x * effect.sign as i32 - camera.0;
                let y = effect.y + y - camera.1;
                *v = (
                    (160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                    (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                );
            }
            if verts.iter().all(|v| v.0 < 0)
                || verts.iter().all(|v| v.0 >= 320)
                || verts.iter().all(|v| v.1 < 0)
                || verts.iter().all(|v| v.1 >= 240)
            {
                continue;
            }
            if count >= limit {
                skipped += 1;
                continue;
            }
            crate::render::texture(texture, verts, (128, 128, 128));
            count += 1;
        }
        (count, skipped)
    }
}

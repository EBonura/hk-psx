//! Bounded source grass/death particle systems. Source art/curves are cooked;
//! 60Hz integration, finite RNG precision and native damping remain approximations.
use hk_format::{u32_at, Room};
use hk_sim::ONE;
#[path="break_effects.rs"]pub mod break_effects;
pub const CAPACITY: usize = 128;
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub size: i32,
    pub alpha: [u8; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub life: [u8; 2],
    pub speed: [i32; 2],
    pub size: [i32; 2],
    pub force: [i32; 2],
    pub dampen: i32,
    pub rotation: i32,
    pub count: u8,
    pub duration: u8,
    pub uv_scale: i32,
    pub colors: [[u8; 3]; 2],
    pub curves: &'static [Sample],
}
#[derive(Clone, Copy, Debug)]
pub struct EmitterSpec {
    pub state: u16,
    pub source: u32,
    pub origin: [i32; 3],
    pub basis: [[i32; 3]; 3],
    pub direction: [i32; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Bank {
    pub frames: [&'static [[u16; 4]]; 2],
    pub styles: &'static [Style],
    pub death_offset: [i32; 3],
}
#[derive(Clone, Copy, Debug)]
struct Particle {
    position: [i32; 3],
    velocity: [i32; 3],
    size: i32,
    force: i32,
    angle: i32,
    scene: u8,
    kind: u8,
    life: u16,
    age: u16,
    delay: u8,
    cell: u8,
    gradient: u8,
    color: [u8; 3],
}
pub struct Pool {
    particles: [Option<Particle>; CAPACITY],
    pub spawned: u32,
    pub dropped: u32,
    pub drawn: u32,
}
#[no_mangle]
pub static mut HK_PARTICLES_ACTIVE: u32 = 0;
#[no_mangle]
pub static mut HK_PARTICLES_SPAWNED: u32 = 0;
#[no_mangle]
pub static mut HK_PARTICLES_DROPPED: u32 = 0;
#[no_mangle]
pub static mut HK_PARTICLES_DRAWN: u32 = 0;
impl Pool {
    pub const fn new() -> Self {
        Self {
            particles: [None; CAPACITY],
            spawned: 0,
            dropped: 0,
            drawn: 0,
        }
    }
    pub fn active(&self) -> usize {
        self.particles.iter().flatten().count()
    }
    fn publish(&self) {
        #[cfg(target_arch = "mips")]
        unsafe {
            core::ptr::write_volatile(
                core::ptr::addr_of_mut!(HK_PARTICLES_ACTIVE),
                self.active() as u32,
            );
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_PARTICLES_SPAWNED), self.spawned);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_PARTICLES_DROPPED), self.dropped);
            core::ptr::write_volatile(core::ptr::addr_of_mut!(HK_PARTICLES_DRAWN), self.drawn);
        }
    }
    pub fn clear_scene(&mut self, scene: usize) {
        for p in &mut self.particles {
            if p.as_ref().is_some_and(|p| p.scene as usize == scene) {
                *p = None;
            }
        }
        self.publish();
    }
    pub fn spawn_break(&mut self,scene:usize,owner:usize,kind:u8,facing:i32) {
        break_effects::spawn_specs(self,scene,owner,kind,facing,break_effects::EMITTERS,break_effects::STYLES);
    }
    pub fn needs_break_collision(&self,scene:usize,bounds:[i32;4])->bool {
        self.particles.iter().flatten().any(|p|p.scene as usize==scene&&p.kind>=2&&p.delay==0&&p.age+1<p.life
            &&break_effects::STYLES[(p.kind-2)as usize].collision&&p.position[0]>=bounds[0]&&p.position[0]<=bounds[2]
            &&p.position[1]>=bounds[1]&&p.position[1]<=bounds[3])
    }
    pub fn tick_break(&mut self,scene:usize,coverage:[i32;4],count:usize,edge:impl Fn(usize)->[i32;4]) {
        break_effects::tick_specs(self,scene,break_effects::STYLES,coverage,count,edge);
    }
    pub fn spawn_grass(&mut self, scene: usize, emitter: EmitterSpec, bank: Bank) {
        self.spawn(scene, 0, emitter, bank);
    }
    pub fn spawn_death(&mut self, scene: usize, source: u32, position: [i32; 3], bank: Bank) {
        let origin = core::array::from_fn(|i| position[i] + bank.death_offset[i]);
        self.spawn(
            scene,
            1,
            EmitterSpec {
                state: 0,
                source,
                origin,
                basis: [[ONE, 0, 0], [0, ONE, 0], [0, 0, ONE]],
                direction: [0, 0, ONE],
            },
            bank,
        );
    }
    #[inline(never)]
    fn spawn(&mut self, scene: usize, kind: usize, emitter: EmitterSpec, bank: Bank) {
        assert!(scene < 256);
        let style = bank.styles[kind];
        assert!(style.duration > 0 && style.curves.len() == 65);
        let mut rng = emitter.source ^ 0xb5297a4d;
        let mut search = 0;
        for i in 0..style.count {
            while search < CAPACITY && self.particles[search].is_some() {
                search += 1;
            }
            if search == CAPACITY {
                self.dropped = self.dropped.saturating_add((style.count - i) as u32);
                break;
            }
            let life = range(&mut rng, [style.life[0] as i32, style.life[1] as i32]) as u16;
            let speed = range(&mut rng, style.speed);
            let size = range(&mut rng, style.size);
            let force = range(&mut rng, style.force);
            let angle = range(&mut rng, [0, 360 * ONE - 1]);
            let color_seed = (random(&mut rng) >> 24) as i32;
            let color = core::array::from_fn(|j| {
                (style.colors[0][j] as i32
                    + (style.colors[1][j] as i32 - style.colors[0][j] as i32) * color_seed / 255)
                    as u8
            });
            let gradient = (random(&mut rng) >> 24) as u8;
            let (offset, direction) = if kind == 0 {
                let p = core::array::from_fn::<_, 3, _>(|_| range(&mut rng, [-ONE / 2, ONE / 2]));
                (
                    core::array::from_fn(|j| dot3(emitter.basis[j], p)),
                    emitter.direction,
                )
            } else {
                // Uniform sphere volume via bounded rejection. Explicit rare fallback
                // reports no false source-RNG parity; all positions stay in source radius1.
                let mut p = [0, 0, ONE / 2];
                for _ in 0..16 {
                    let trial = core::array::from_fn::<_, 3, _>(|_| range(&mut rng, [-ONE, ONE]));
                    let n = length3(trial);
                    if n > 0 && n <= ONE {
                        p = trial;
                        break;
                    }
                }
                let n = length3(p);
                (p, core::array::from_fn(|j| divq(p[j], n)))
            };
            self.particles[search] = Some(Particle {
                position: core::array::from_fn(|j| emitter.origin[j] + offset[j]),
                velocity: core::array::from_fn(|j| mulq(direction[j], speed)),
                size,
                force,
                angle,
                scene: scene as u8,
                kind: kind as u8,
                life,
                age: 0,
                delay: (((i as u16 + 1) * style.duration as u16 - 1) / style.count as u16) as u8,
                cell: if kind == 0 {
                    (random(&mut rng) % 3) as u8
                } else {
                    0
                },
                gradient,
                color,
            });
            self.spawned = self.spawned.saturating_add(1);
            search += 1;
        }
        self.publish();
    }
    /// Tick through the existing fixed simulation clock. Metadata is scene-wide;
    /// crossing grid cells never restarts an emitter or swaps a particle's cell.
    #[inline(never)]
    pub fn tick(&mut self, scene: usize, bank: Option<Bank>) {
        let Some(bank) = bank else {
            return;
        };
        for slot in &mut self.particles {
            let Some(p) = slot else {
                continue;
            };
            if p.scene as usize != scene || p.kind>=2 {
                continue;
            }
            if p.delay > 0 {
                p.delay -= 1;
                continue;
            }
            p.age += 1;
            if p.age >= p.life {
                *slot = None;
                continue;
            }
            let style = bank.styles[p.kind as usize];
            p.velocity[1] += p.force / 60;
            let speed = length3(p.velocity);
            if speed > ONE {
                // Source LimitVelocity.dampen is a fractional reduction above speed1;
                // native substep/delta scaling is not exposed in the serialized data.
                let limited = speed - mulq(speed - ONE, style.dampen);
                for v in &mut p.velocity {
                    *v = (((*v as i64) * limited as i64) / speed as i64) as i32;
                }
            }
            for j in 0..3 {
                p.position[j] += p.velocity[j] / 60;
            }
            p.angle = (p.angle + style.rotation / 60).rem_euclid(360 * ONE);
        }
        self.publish();
    }
    #[inline(never)]
    pub fn draw(
        &mut self,
        scene: usize,
        bank: Option<Bank>,
        room: &Room,
        camera: (i32, i32),
    ) -> u32 {
        self.drawn = 0;
        for p in self.particles.iter().flatten().filter(|p|p.scene as usize==scene&&p.kind>=2&&p.delay==0) {
            self.drawn+=u32::from(break_effects::draw_particle(p,camera));
        }
        let Some(bank) = bank else {
            self.publish();
            return self.drawn;
        };
        for p in self
            .particles
            .iter()
            .flatten()
            .filter(|p| p.scene as usize == scene && p.delay == 0 && p.kind<2)
        {
            let kind = p.kind as usize;
            let style = bank.styles[kind];
            let phase = (p.age as usize * 64 / p.life as usize).min(64);
            let sample = style.curves[phase];
            let alpha = sample.alpha[0] as i32
                + (sample.alpha[1] as i32 - sample.alpha[0] as i32) * p.gradient as i32 / 255;
            let level = (alpha * 4 + 127) / 255;
            if level == 0 {
                continue;
            }
            let cell = if kind == 0 {
                p.cell as usize
            } else {
                ((p.age as u32 * 9 * style.uv_scale as u32) / (p.life as u32 * 65536)).min(8)
                    as usize
            };
            let frame = bank.frames[kind][cell][level as usize - 1];
            let texture = u32_at(room.frame(frame as usize), 0) as usize;
            assert!(!room.texture(texture).is_streamed());
            let half = mulq(p.size, sample.size) / 2;
            let mut verts = [(0i16, 0i16); 4];
            // Same perspective as the source room camera; source particles retain Z.
            let depth = 2496922 + p.position[2];
            if depth <= ONE {
                continue;
            }
            let scale = (crate::KNIGHT_SCALE as i64 * 2496922 / depth as i64) as i32;
            for (dst, corner) in
                verts
                    .iter_mut()
                    .zip([[-half, half], [half, half], [-half, -half], [half, -half]])
            {
                let v = crate::world::debris::rotate(corner, p.angle);
                let x = p.position[0] + v[0] - camera.0;
                let y = p.position[1] + v[1] - camera.1;
                *dst = (
                    (160 + (((x as i64 >> 8) * scale as i64) >> 20)) as i16,
                    (120 - (((y as i64 >> 8) * scale as i64) >> 20)) as i16,
                );
            }
            if verts.iter().all(|v| v.0 < 0)
                || verts.iter().all(|v| v.0 >= 320)
                || verts.iter().all(|v| v.1 < 0)
                || verts.iter().all(|v| v.1 >= 240)
            {
                continue;
            }
            crate::render::texture_material_alpha(
                texture,
                verts,
                (p.color[0], p.color[1], p.color[2]),
                crate::render::SOURCE_ALPHA_COVERAGE,
            );
            self.drawn += 1;
        }
        self.publish();
        self.drawn
    }
}
fn random(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed
}
fn range(seed: &mut u32, b: [i32; 2]) -> i32 {
    b[0] + ((random(seed) as u64 * (b[1] as i64 - b[0] as i64 + 1) as u64) >> 32) as i32
}
fn mulq(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) / ONE as i64) as i32
}
fn divq(a: i32, b: i32) -> i32 {
    ((a as i64 * ONE as i64) / b as i64) as i32
}
fn dot3(a: [i32; 3], b: [i32; 3]) -> i32 {
    mulq(a[0], b[0]) + mulq(a[1], b[1]) + mulq(a[2], b[2])
}
fn length_squared(v:[i32;3])->u64 {
    v.iter().map(|x|*x as i64 * *x as i64).sum::<i64>()as u64
}
/// Exact integer norm. Candidate squares use the R3000's native32x32 MULTU;
/// the prior restoring loop manipulated several64-bit values each iteration.
fn length3(v:[i32;3])->i32 {
    let n=length_squared(v);if n==0{return 0;}
    let mut bit=1u32<<((63-n.leading_zeros())/2);let mut root=0u32;
    while bit!=0 {let candidate=root|bit;if u64::from(candidate)*u64::from(candidate)<=n{root=candidate;}bit>>=1;}
    root as i32
}

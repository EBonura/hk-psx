//! Bounded source grass/death particle systems. Source art/curves are cooked;
//! 60Hz integration, finite RNG precision and native damping remain approximations.
use hk_format::{u32_at, Room};
use hk_sim::ONE;
use core::num::NonZeroU16;
#[path="break_effects.rs"]pub mod break_effects;
#[path="particle_perspective.rs"]mod perspective;
#[path="particle_division.rs"]mod division;
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
    // Authored lifetimes are strictly positive. The zero niche makes
    // Option<Particle> the same size as Particle, saving four bytes per slot.
    life: NonZeroU16,
    age: u16,
    delay: u8,
    cell: u8,
    gradient: u8,
    color: [u8; 3],
}
pub struct Pool {
    particles: [Option<Particle>; CAPACITY],
    // One-based immutable track IDs; zero means scalar/no particle.
    // Empty slots therefore use the all-zero scalar sentinel.
    tracks:[u16;CAPACITY],
    active_count: u32,
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
            tracks:[0;CAPACITY],
            active_count: 0,
            spawned: 0,
            dropped: 0,
            drawn: 0,
        }
    }
    pub fn active(&self) -> usize {
        self.active_count as usize
    }
    fn publish(&self) {
        // Native tests independently recount every published mutation batch;
        // the guest reads only the exact cached count, even for empty pools.
        #[cfg(test)]
        assert_eq!(self.active(),self.particles.iter().flatten().count());
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
        for (i,p) in self.particles.iter_mut().enumerate() {
            if p.as_ref().is_some_and(|p| p.scene as usize == scene) {
                *p = None;self.tracks[i]=0;
                self.active_count -= 1;
            }
        }
        self.publish();
    }
    pub fn spawn_break(&mut self,scene:usize,owner:usize,kind:u8,facing:i32) {
        break_effects::spawn_specs(self,scene,owner,kind,facing,break_effects::scene_emitters(scene),break_effects::scene_styles(scene));
    }
    pub fn needs_break_collision(&self,scene:usize,bounds:[i32;4])->bool {
        self.particles.iter().flatten().any(|p|p.scene as usize==scene&&p.kind>=2&&p.delay==0&&p.age+1<p.life.get()
            &&break_effects::scene_styles(p.scene as usize)[(p.kind-2)as usize].collision&&p.position[0]>=bounds[0]&&p.position[0]<=bounds[2]
            &&p.position[1]>=bounds[1]&&p.position[1]<=bounds[3])
    }
    pub fn tick_break(&mut self,scene:usize,coverage:[i32;4],count:usize,edge:impl Fn(usize)->[i32;4]) {
        break_effects::tick_specs(self,scene,break_effects::scene_styles(scene),coverage,count,edge);
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
            self.tracks[search]=0;
            self.particles[search] = Some(Particle {
                position: core::array::from_fn(|j| emitter.origin[j] + offset[j]),
                velocity: core::array::from_fn(|j| mulq(direction[j], speed)),
                size,
                force,
                angle,
                scene: scene as u8,
                kind: kind as u8,
                life: NonZeroU16::new(life).expect("positive particle lifetime"),
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
            self.active_count += 1;
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
            #[cfg(target_arch="mips")]
            crate::input::checkpoint();
            if p.delay > 0 {
                p.delay -= 1;
                continue;
            }
            p.age += 1;
            if p.age >= p.life.get() {
                *slot = None;
                self.active_count -= 1;
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
                    *v = break_effects::scale_velocity(*v, limited, speed);
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
        for (i,slot) in self.particles.iter().enumerate() {
            let Some(p)=slot.as_ref() else{continue;};
            if p.scene as usize==scene&&p.kind>=2&&p.delay==0 {self.drawn+=u32::from(break_effects::draw_particle(p,self.tracks[i],camera));}
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
            let phase = (p.age as usize * 64 / p.life.get() as usize).min(64);
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
                ((p.age as u32 * 9 * style.uv_scale as u32) / (p.life.get() as u32 * 65536)).min(8)
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
            let scale = perspective::scale(depth);
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
const fn root8_table()->[u8;256] {
    let mut out=[0;256];let mut n=0;
    while n<256 {let mut r=0;while (r+1)*(r+1)<=n {r+=1;}out[n]=r as u8;n+=1;}
    out
}
static ROOT8:[u8;256]=root8_table();
/// General wide fallback, retaining the previous complete-u64 root semantics.
fn root_wide(n:u64)->u32 {
    if n==0 {return 0;}
    let mut bit=1u32<<((63-n.leading_zeros())/2);let mut root=0u32;
    while bit!=0 {let candidate=root|bit;if u64::from(candidate)*u64::from(candidate)<=n{root=candidate;}bit>>=1;}
    root
}
/// Exact floor square root. Authored speeds fit below sqrt(2^48). Seed the
/// upper word with a tiny integer table, then consume four input bits per step.
/// The invariant is prefix=root²+remainder, 0<=remainder<2*root+1. A nibble
/// selects 4*root+[0..3]: test the +2 threshold (16*root+4), then +1 against
/// the updated root. Before any nibble root<2^22; the expanded remainder is
/// <2^27 and the threshold<2^26. Their wrapping difference therefore has the
/// exact comparison in its sign bit, avoiding LLVM's duplicated select branches.
/// Every intermediate fits u32; the full-u64 fallback remains unchanged.
/// Outline once: both legacy and break particles use this hot path.
#[inline(never)]
fn integer_root(n:u64)->u32 {
    if n==0 {return 0;}
    if n>>48!=0 {return root_wide(n);}
    let high=(n>>32)as u32;
    let mut root=if high<256 {ROOT8[high as u8 as usize]as u32}else {
        let mut r=ROOT8[(high>>8)as u8 as usize]as u32;
        let mut remainder=(high>>8)-r*r;
        for shift in [6,4,2,0] {
            remainder=(remainder<<2)|((high>>shift)&3);
            let trial=(r<<2)|1;let take=u32::from(remainder>=trial);
            remainder-=trial&take.wrapping_neg();r=(r<<1)|take;
        }
        r
    };
    let mut remainder=high-root*root;let mut low=n as u32;
    for _ in 0..8 {
        remainder=(remainder<<4)|(low>>28);low<<=4;
        let mid=(root<<4)|4;let upper=(remainder.wrapping_sub(mid)>>31)^1;
        remainder-=mid&upper.wrapping_neg();root=(root<<2)|(upper<<1);
        let trial=(root<<1)|1;let lower=u32::from(remainder>=trial);
        remainder-=trial&lower.wrapping_neg();root|=lower;
    }
    root
}
fn length3(v:[i32;3])->i32 {integer_root(length_squared(v))as i32}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digit_root_matches_wide_squares_and_complete_u64_domain() {
        let mut state=0xd1b54a32d192ed03u64;
        for i in 0..200000 {
            state=state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n=match i%3 {0=>state,1=>state&((1<<48)-1),_=>state&((1<<40)-1)};
            assert_eq!(integer_root(n),root_wide(n),"{n}");
        }
        for r in (0..(1u64<<24)).step_by(4093) {
            let n=r*r;
            for v in [n.saturating_sub(1),n,n+1] {assert_eq!(integer_root(v),root_wide(v),"{v}");}
        }
        for n in [0,1,2,3,4,(1<<32)-1,1<<32,(1<<40)-1,1<<40,(1<<48)-1,1<<48,
            (1<<60)-1,1<<60,(1<<63)-1,1<<63,u64::MAX] {
            assert_eq!(integer_root(n),root_wide(n),"{n}");
        }
    }
    #[test]
    fn paired_root_digits_preserve_remainder_at_all_fast_domain_boundaries() {
        for root in [0u32,1,2,3,15,16,255,256,4095,65535,(1<<20)-1,(1<<22)-1] {
            for prior_remainder in [0,root,root*2] {
                for nibble in 0..16 {
                    let expanded=(prior_remainder<<4)|nibble;
                    let mid=(root<<4)|4;
                    let upper=(expanded.wrapping_sub(mid)>>31)^1;
                    assert_eq!(upper,u32::from(expanded>=mid));
                    let remainder=expanded-(mid&upper.wrapping_neg());
                    let partial=(root<<2)|(upper<<1);
                    let trial=(partial<<1)|1;
                    let lower=u32::from(remainder>=trial);
                    let next=partial|lower;
                    let remaining=remainder-(trial&lower.wrapping_neg());
                    let value=(root as u64*root as u64+prior_remainder as u64)*16+nibble as u64;
                    assert_eq!(next,root_wide(value));
                    assert_eq!(value,next as u64*next as u64+remaining as u64);
                    assert!(remaining<2*next+1);
                }
            }
        }
        for root in 0u64..65536 {
            let square=root*root;
            for n in [square.saturating_sub(1),square,square+1] {
                assert_eq!(integer_root(n),root_wide(n));
            }
        }
    }
    /// Keep the prior full legacy update as the differential oracle: compare
    /// every surviving particle and lifecycle field after every fixed tick.
    fn legacy_reference(pool:&mut Pool,scene:usize,bank:Option<Bank>,paths:&mut [usize;2]) {
        let Some(bank)=bank else{return;};
        for slot in &mut pool.particles {
            let Some(p)=slot else{continue;};
            if p.scene as usize!=scene||p.kind>=2 {continue;}
            if p.delay>0 {p.delay-=1;continue;}
            p.age+=1;if p.age>=p.life.get() {*slot=None;continue;}
            let style=bank.styles[p.kind as usize];
            p.velocity[1]+=p.force/60;
            let speed=length3(p.velocity);
            if speed>ONE {
                let limited=speed-mulq(speed-ONE,style.dampen);
                for v in &mut p.velocity {
                    paths[usize::from(v.checked_mul(limited).is_none())]+=1;
                    *v=(*v as i64*limited as i64/speed as i64)as i32;
                }
            }
            for j in 0..3 {p.position[j]+=p.velocity[j]/60;}
            p.angle=(p.angle+style.rotation/60).rem_euclid(360*ONE);
        }
        pool.active_count=pool.particles.iter().flatten().count()as u32;
    }
    #[test]
    fn legacy_full_pool_matches_wide_reference_through_expiry_and_scene_changes() {
        static CURVES:[Sample;1]=[Sample{size:ONE,alpha:[255;2]}];
        const STYLE:Style=Style{life:[80,96],speed:[0,ONE],size:[ONE;2],force:[0;2],
            dampen:19661,rotation:-231*ONE,count:1,duration:1,uv_scale:ONE,
            colors:[[128;3];2],curves:&CURVES};
        static STYLES:[Style;2]=[STYLE,Style{dampen:0,rotation:377*ONE,..STYLE}];
        let bank=Bank{frames:[&[],&[]],styles:&STYLES,death_offset:[0;3]};
        let mut fast=Pool::new();
        for i in 0..CAPACITY {
            let velocity=match i%7 {
                0=>[0;3],1=>[-1,7,0],2=>[ONE,0,0],3=>[2*ONE,-ONE,1],
                4=>[1_000_000_000,-500_000_000,0],
                5=>[-123_456_789,987_654,31],_=>[7*ONE,-9*ONE,3*ONE],
            };
            fast.particles[i]=Some(Particle{position:[i as i32,-12345,765],velocity,size:ONE,
                force:if i%7==0 {0}else{-137*ONE},angle:if i%2==0 {-1}else{360*ONE+1},
                scene:(i%3)as u8,kind:(i%5%3)as u8,life:NonZeroU16::new(80+(i%17)as u16).unwrap(),
                age:(i%11)as u16,delay:(i%4)as u8,cell:(i%9)as u8,gradient:i as u8,color:[37,89,123]});
        }
        fast.active_count=128;fast.spawned=128;fast.dropped=11;fast.drawn=17;
        let mut reference=Pool{particles:fast.particles,tracks:[0;CAPACITY],active_count:128,spawned:128,dropped:11,drawn:17};
        let mut paths=[0;2];
        for tick in 0..384 {
            let scene=tick/17%3;let selected=if tick%19==0 {None}else{Some(bank)};
            fast.tick(scene,selected);legacy_reference(&mut reference,scene,selected,&mut paths);
            assert_eq!(fast.active(),reference.active(),"tick {tick}");
            for (index,(actual,expected)) in fast.particles.iter().zip(&reference.particles).enumerate() {
                match (actual,expected) {
                    (None,None)=>{},
                    (Some(a),Some(b))=>{
                        perspective::assert_vertices(a.position,a.angle,a.size,(tick as i32*7919,-(tick as i32)*3571));
                        assert_eq!((a.position,a.velocity,a.angle,a.force,a.age,a.delay),
                            (b.position,b.velocity,b.angle,b.force,b.age,b.delay),"tick {tick},slot {index}");
                        assert_eq!((a.size,a.scene,a.kind,a.life,a.cell,a.gradient,a.color),
                            (b.size,b.scene,b.kind,b.life,b.cell,b.gradient,b.color));
                    },_=>panic!("lifecycle mismatch at tick {tick},slot {index}"),
                }
            }
            assert_eq!((fast.spawned,fast.dropped,fast.drawn),(reference.spawned,reference.dropped,reference.drawn));
        }
        assert!(paths[0]>100&&paths[1]>100,"exercise native and wide paths: {paths:?}");
        assert!(fast.particles.iter().flatten().all(|p|p.kind>=2),"all legacy particles expire");
    }
}

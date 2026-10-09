//! `Shockwave Wave` and the `Shockwave Spurt`s it leaves: the False Knight's
//! slam wave, as `S Attack Recover` spawns it.
//!
//! The wave itself is an invisible trigger. `Start Move` sets its speed to the
//! written 22 times 0.025 and `Move` adds twice 22 every second, so it creeps
//! out of the slam and crosses the arena in a little over a second, until its
//! trigger meets terrain or its ground ray finds none. Each frame it moves it
//! spawns a spurt where it is; the spurts are what is drawn and what hurts,
//! each for the same short window of its own life (`Damage timing` arms the
//! DamageHero after one wait and disarms it after the next), and each recycles
//! when its clip has played once.
//!
//! Every number comes from the two prefabs through host/false_knight_art.py,
//! which is why they arrive as [`Params`] rather than constants here: the
//! guest reads them out of `data/false_knight_art.rs` and tools/boss_sim.py out
//! of the same file.
//!
//! One departure, at the end: `End Pause` keeps spawning for 0.15 s with the
//! body still coasting into whatever stopped it, which in this port would draw
//! spurts on top of the wall (actor art is not depth sorted against scenery);
//! this stops at the contact instead.

/// Spurts a wave remembers: one a tick for as long as a spurt lives.
pub const SPURTS: usize = 18;

/// The cooked numbers, Q16 world units and 60 Hz ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// `Start Move`: Speed times its factor, then twice Speed added a second.
    pub start_speed: i32,
    pub accel: i32,
    /// The wave's terrain trigger, in its rightward frame, from its transform.
    pub wave_box: [i32; 4],
    /// `Move`'s RayCast2d, straight down.
    pub ground_ray: i32,
    /// A spurt's DamageHero box, rightward frame, from the spurt's transform.
    pub spurt_box: [i32; 4],
    /// The spurt's DamageHero is armed for ticks `damage_from..damage_to` of its life.
    pub damage_from: u16,
    pub damage_to: u16,
    pub damage: u16,
    /// The spurt's clip, played once.
    pub spurt_ticks: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Wave {
    /// +1 travelling right, -1 left.
    pub dir: i8,
    pub moving: bool,
    pub x: i32,
    pub y: i32,
    speed: i32,
    /// Where it spawned: the spurts are kept as Q8 offsets from here, which
    /// holds an arena's width in an i16 and halves the ring.
    x0: i32,
    tick: u16,
    /// Spurts spawned, one a tick from tick 0, so spurt `i` is `tick - i` old.
    born: u16,
    spurts: [i16; SPURTS],
}

impl Wave {
    pub fn new(position: [i32; 2], dir: i32, p: &Params) -> Self {
        Self {
            dir: if dir > 0 { 1 } else { -1 },
            moving: true,
            x: position[0],
            y: position[1],
            speed: p.start_speed,
            x0: position[0],
            tick: 0,
            born: 0,
            spurts: [0; SPURTS],
        }
    }
    pub fn age(&self) -> u16 {
        self.tick
    }
    fn spurt_x(&self, i: u16) -> i32 {
        self.x0 + ((self.spurts[i as usize % SPURTS] as i32) << 8)
    }
    /// One 60 Hz step of `Move`. `hits` answers whether a segment crosses
    /// terrain; returns true once the wave and every spurt it left are gone.
    pub fn step(&mut self, p: &Params, hits: impl Fn([i32; 2], [i32; 2]) -> bool) -> bool {
        let dir = self.dir as i32;
        if self.moving {
            // FloatAdd perSecond, then SetVelocity2d every frame.
            self.speed += p.accel / 60;
            let x = self.x + dir * (self.speed / 60);
            // `Trigger2dEventLayer` on layer 8: the trigger box's leading side
            // swept across the step, just inside its bottom and top.
            let lead = |at: i32| {
                at + if dir > 0 {
                    p.wave_box[2]
                } else {
                    -p.wave_box[0]
                }
            };
            let inset = crate::ONE / 16;
            let wall = [p.wave_box[1] + inset, p.wave_box[3] - inset]
                .iter()
                .any(|&h| hits([lead(self.x), self.y + h], [lead(x), self.y + h]));
            // The 1.6-unit ground ray. The spawn point sits a fifth of a unit
            // inside the floor, where Physics2D reports the collider the ray
            // starts in; an edge world only sees surfaces it crosses, so the ray
            // starts at the trigger's own base.
            let ground = hits([x, self.y + p.wave_box[1]], [x, self.y - p.ground_ray]);
            if wall || !ground {
                self.moving = false;
            } else {
                self.x = x;
                self.spurts[self.born as usize % SPURTS] = ((x - self.x0) >> 8) as i16;
                self.born += 1;
            }
        }
        self.tick += 1;
        !self.moving && self.tick >= self.born + p.spurt_ticks
    }
    /// Does an armed spurt overlap `hero`? Checked after `step`, so spurt `i`
    /// is `tick - 1 - i` ticks old here, which is its age on the tick it hurts.
    pub fn hurts(&self, p: &Params, hero: [i32; 4]) -> bool {
        let now = self.tick.saturating_sub(1);
        let first = self.born.saturating_sub(p.damage_to);
        (first..self.born).any(|i| {
            let age = now - i;
            if !(p.damage_from..p.damage_to).contains(&age) {
                return false;
            }
            let (x, b) = (self.spurt_x(i), p.spurt_box);
            let bounds = if self.dir > 0 {
                [x + b[0], self.y + b[1], x + b[2], self.y + b[3]]
            } else {
                [x - b[2], self.y + b[1], x - b[0], self.y + b[3]]
            };
            bounds[0] <= hero[2]
                && bounds[2] >= hero[0]
                && bounds[1] <= hero[3]
                && bounds[3] >= hero[1]
        })
    }
    /// Live spurts as (x, age in ticks), every `stride`th one, newest last.
    pub fn spurts(&self, p: &Params, stride: u16) -> impl Iterator<Item = (i32, u16)> + '_ {
        let first = self.born.saturating_sub(p.spurt_ticks);
        let now = self.tick;
        let life = p.spurt_ticks;
        (first..self.born)
            .filter(move |i| i % stride == 0)
            .map(move |i| (self.spurt_x(i), now - i))
            .filter(move |&(_, age)| age < life)
    }
}

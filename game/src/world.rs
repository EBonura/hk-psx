//! Cooked region indices and bounded, persistent-in-session authored state.
use hk_format::Room;
#[path = "debris.rs"]
pub mod debris;
#[path = "particles.rs"]
pub mod particles;
use hk_sim::{AttackParams, Grass, Nail, Params, Player, ONE};

pub const SCENES: usize = 3;
pub const BREAKABLES_PER_SCENE: usize = 128;
pub const GRASS_PER_SCENE: usize = 1024;
#[derive(Clone, Copy)]
pub struct MaskFade {
    pub draws: &'static [u16],
    pub ticks: u16,
}
#[derive(Clone, Copy)]
pub struct MaskBinding {
    pub owner: usize,
    pub fade_ticks: u16,
    pub fade: MaskFade,
}
#[derive(Clone, Copy)]
pub struct Breakable {
    pub id: usize,
    pub bounds: [i32; 4],
    pub polygons: &'static [&'static [[i32; 2]]],
    pub off: &'static [u16],
    pub on: &'static [u16],
    pub edges: &'static [u16],
    pub fades: &'static [MaskFade],
    pub fade_ticks: u16,
    pub persistent: bool,
    pub door_sound: bool,
}
#[derive(Clone, Copy)]
pub struct Patch {
    pub state: usize,
    pub grass: Grass,
}
pub struct Region {
    pub global_id: usize,
    pub scene: usize,
    pub flat_floor_heights: &'static [i32],
    pub bounds: [i32; 4],
    pub collision_bounds: [i32; 4],
    pub camera: [i32; 4],
    pub grass: &'static [Patch],
    pub grass_impact: Option<crate::impact::Spec>,
    pub particle_bank: Option<particles::Bank>,
    pub grass_emitters: &'static [particles::EmitterSpec],
    pub door_debris: &'static [debris::Spec],
    pub breakables: &'static [Breakable],
    pub remote_masks: &'static [MaskBinding],
    pub reveal_masks: &'static [crate::reveal_masks::RevealMaskSpec],
    pub reveal_bindings: &'static [crate::reveal_masks::RevealMaskBinding],
    pub actors: &'static [&'static hk_sim::ActorSpec],
    pub neighbours: &'static [usize],
}
pub struct Gate {
    pub scene: usize,
    pub target_scene: usize,
    pub bounds: [i32; 4],
    pub spawn: [i32; 2],
    /// Initial vertical velocity at the destination (Q16 units per second).
    pub entry_vy: i32,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/regions.rs"));

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strike {
    pub broken: u16,
    pub door_sounds: u16,
    pub grass_cut: u16,
    pub impact_dropped: u16,
}
/// Filtered terrain retained until its source region or exclusions change.
/// Coins, Scuttlers, enemies and effects otherwise re-decode and re-filter every
/// edge per body per tick, which exceeded a VBlank with 16 coins and 7 broken objects.
const EDGE_CACHE:usize=128;
pub struct State {
    edge_cache:[[i32;4];EDGE_CACHE],
    edge_cache_count:usize,
    edge_cache_generation:u32,
    edge_cache_region_id:usize,
    edge_cache_room_key:usize,
    edge_cache_valid:bool,
    world_generation:u32,
    geo_edges:[u16;32],
    geo_edge_count:usize,
    lifeblood_edges:[u16;8],
    lifeblood_edge_count:usize,
    debris: debris::Pool,
    particles: particles::Pool,
    pub effect_draw_dropped: u32,
    impacts: crate::impact::Pool,
    broken: [u32; SCENES * BREAKABLES_PER_SCENE / 32],
    grass: [u32; SCENES * GRASS_PER_SCENE / 32],
    pub fade_left: [u16; SCENES * BREAKABLES_PER_SCENE],
    fade_active:[u32;SCENES*BREAKABLES_PER_SCENE/32],
}
fn fill_edges(geo:&[u16],life:&[u16],broken:&[u32],region:&Region,room:&Room,cache:&mut [[i32;4]]) {
    let count=cache.len();
    for(i,dst)in cache.iter_mut().enumerate(){*dst=room.edge(i);}
    let mut blank=|i:u16|{if(i as usize)<count{cache[i as usize]=[0;4];}};
    for &i in geo.iter().chain(life){blank(i);}
    for b in region.breakables.iter().filter(|b|broken[b.id/32]&(1<<(b.id%32))!=0){for &i in b.edges{blank(i);}}
}
impl State {
    pub const fn new() -> Self {
        Self {
            edge_cache:[[0;4];EDGE_CACHE],edge_cache_count:0,
            edge_cache_generation:0,edge_cache_region_id:0,edge_cache_room_key:0,edge_cache_valid:false,
            world_generation:0,
            geo_edges:[0;32],geo_edge_count:0,
            lifeblood_edges:[0;8],lifeblood_edge_count:0,
            impacts: crate::impact::Pool::new(),
            debris: debris::Pool::new(),
            particles: particles::Pool::new(),
            effect_draw_dropped: 0,
            broken: [0; SCENES * BREAKABLES_PER_SCENE / 32],
            grass: [0; SCENES * GRASS_PER_SCENE / 32],
            fade_left: [0; SCENES * BREAKABLES_PER_SCENE],
            fade_active:[0;SCENES*BREAKABLES_PER_SCENE/32],
        }
    }
    pub fn broken(&self, id: usize) -> bool {
        self.broken[id / 32] & (1 << (id % 32)) != 0
    }
    /// Returns false for an already broken source object, including another
    /// copy of its metadata after loading an overlapping region.
    pub fn break_object(&mut self, b: Breakable) -> bool {
        if self.broken(b.id) {
            return false;
        }
        self.broken[b.id / 32] |= 1 << (b.id % 32);
        self.fade_left[b.id] = b.fade_ticks;
        if b.fade_ticks!=0 {self.fade_active[b.id/32]|=1<<(b.id%32);}
        self.edge_cache_count=0;
        self.edge_cache_valid=false;
        true
    }
    pub fn cut(&self, id: usize) -> bool {
        self.grass[id / 32] & (1 << (id % 32)) != 0
    }
    pub fn cut_grass(&mut self, id: usize) {
        self.grass[id / 32] |= 1 << (id % 32);
    }
    /// One call per 60 Hz simulation tick, including GPU-wait VBlanks.
    pub fn draw_impacts(&mut self, region: &Region, room: &Room, camera: (i32, i32)) -> u32 {
        let debris = self
            .debris
            .draw(region.scene, region.door_debris, room, camera);
        let (impacts, skipped) =
            self.impacts
                .draw_limited(region.scene, region.grass_impact, room, camera, 32 - debris);
        self.effect_draw_dropped = self.effect_draw_dropped.saturating_add(skipped);
        debris
            + impacts
            + self
                .particles
                .draw(region.scene, region.particle_bank, room, camera)
    }
    #[inline(never)]
    pub fn tick_debris(&mut self, region: &Region, room: &Room) {
        self.particles.tick(region.scene, region.particle_bank);
        self.refresh_edges(region,room);
        let count=room.counts[5];
        let broken = &self.broken;
        let geo=&self.geo_edges[..self.geo_edge_count];
        let life=&self.lifeblood_edges[..self.lifeblood_edge_count];
        // Future larger rooms retain the direct path rather than dropping terrain.
        let cached=&self.edge_cache[..self.edge_cache_count];
        let edge=|i:usize| if i<cached.len() {cached[i]} else if geo.contains(&(i as u16))||life.contains(&(i as u16))||region.breakables.iter().any(|b| {
                broken[b.id / 32] & (1 << (b.id % 32)) != 0 && b.edges.contains(&(i as u16))
            }) {[0;4]}else{room.edge(i)};
        self.particles.tick_break(region.scene,region.collision_bounds,count,&edge);
        self.debris.tick(region.scene,region.door_debris,region.collision_bounds,count,edge);
    }
    /// The resident scene payload is immutable. Explicit world generation plus
    /// global region ID and the borrowed room view select the local edge
    /// reference list. The view address only disambiguates two immutable room
    /// slices during one generation; world_generation invalidates it whenever
    /// the reusable arena is replaced.
    #[inline(always)]
    fn edge_cache_matches(&self,region:&Region,room:&Room)->bool {
        self.edge_cache_valid
            && self.edge_cache_count==room.counts[5]
            && self.edge_cache_generation==self.world_generation
            && self.edge_cache_region_id==region.global_id
            && self.edge_cache_room_key==room as *const Room as usize
    }
    /// Retire every view before the reusable world/geometry arena can be
    /// overwritten. Call once for every metadata admission, including a retry
    /// of the same scene at the same address.
    pub fn begin_world_admission(&mut self) {
        self.world_generation=self.world_generation.wrapping_add(1);
        self.edge_cache_count=0;
        self.edge_cache_valid=false;
    }
    /// Rebuild only after an exclusion mutation or source change. Returns true
    /// when the table was rebuilt; larger future rooms retain the direct path.
    pub fn refresh_edges(&mut self,region:&Region,room:&Room)->bool {
        if self.edge_cache_matches(region,room){return false;}
        let count=room.counts[5];
        let Self{edge_cache,edge_cache_count,geo_edges,geo_edge_count,lifeblood_edges,lifeblood_edge_count,broken,..}=self;
        if count<=EDGE_CACHE {
            fill_edges(&geo_edges[..*geo_edge_count],&lifeblood_edges[..*lifeblood_edge_count],broken,region,room,&mut edge_cache[..count]);
            *edge_cache_count=count;
            self.edge_cache_generation=self.world_generation;
            self.edge_cache_region_id=region.global_id;
            self.edge_cache_room_key=room as *const Room as usize;
            self.edge_cache_valid=true;
            true
        } else {*edge_cache_count=0;self.edge_cache_valid=false;false}
    }
    pub fn tick(&mut self) {
        self.impacts.tick();
        // Only broken source objects with an unfinished fade need a timer
        // update; retain scene-global countdowns and exact simultaneous expiry.
        for (word,active) in self.fade_active.iter_mut().enumerate() {
            let mut bits=*active;
            while bits!=0 {
                let bit=bits.trailing_zeros()as usize;bits&=bits-1;
                let time=&mut self.fade_left[word*32+bit];
                *time=time.saturating_sub(1);
                if *time==0 {*active&=!(1<<bit);}
            }
        }
    }
    pub fn enemy_death_particles(&mut self, region: &Region, source: u32, position: [i32; 3]) {
        if let Some(bank) = region.particle_bank {
            self.particles
                .spawn_death(region.scene, source, position, bank);
        }
    }
    pub fn reset_scene(&mut self, scene: usize) {
        self.begin_world_admission();
        self.impacts.clear_scene(scene);
        self.debris.clear_scene(scene);
        self.particles.clear_scene(scene);
        assert!(scene < SCENES);
        let first = scene * GRASS_PER_SCENE / 32;
        self.grass[first..first + GRASS_PER_SCENE / 32].fill(0);
        for region in REGIONS.iter().filter(|r| r.scene == scene) {
            for object in region.breakables {
                if !object.persistent {
                    self.broken[object.id / 32] &= !(1 << (object.id % 32));
                    self.fade_left[object.id] = 0;
                    self.fade_active[object.id/32]&=!(1<<(object.id%32));
                }
            }
        }
    }
    /// Source nail active windows and exact transformed source target polygons.
    /// Breakable.Hit breaks on the first hit; health counters are not invented.
    pub fn strike(
        &mut self,
        region: &Region,
        nail: &Nail,
        params: AttackParams,
        polygons: [&[[i32; 2]]; 4],
        player: &Player,
    ) -> Strike {
        let mut result = Strike::default();
        if !nail.hitting(params) {
            return result;
        }
        let Some(points) = polygons.get(nail.kind as usize) else {
            return result;
        };
        if !(3..=16).contains(&points.len()) {
            return result;
        }
        let mut transformed = [[0; 2]; 16];
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (dst, src) in transformed.iter_mut().zip(*points) {
            *dst = [player.x - src[0] * player.facing, player.y + src[1]];
            bounds[0] = bounds[0].min(dst[0]);
            bounds[1] = bounds[1].min(dst[1]);
            bounds[2] = bounds[2].max(dst[0]);
            bounds[3] = bounds[3].max(dst[1]);
        }
        let polygon = &transformed[..points.len()];
        for object in region.breakables {
            if !self.broken(object.id)
                && overlap(bounds, object.bounds)
                && object
                    .polygons
                    .iter()
                    .any(|target| polygons_overlap(polygon, target))
                && self.break_object(*object)
            {
                self.particles.spawn_break(region.scene,object.id,nail.kind as u8,player.facing);
                self.debris.spawn(
                    region.scene,
                    region.door_debris,
                    object.id,
                    nail.kind as u8,
                    player.facing,
                );
                result.broken += 1;
                result.door_sounds += u16::from(object.door_sound);
            }
        }
        for patch in region.grass {
            if !self.cut(patch.state)
                && overlap(bounds, patch.grass.bounds)
                && hk_sim::polygon_hits_box(polygon, patch.grass.bounds)
            {
                self.cut_grass(patch.state);
                result.grass_cut += 1;
                if let Some(bank) = region.particle_bank {
                    if let Some(&emitter) = region
                        .grass_emitters
                        .iter()
                        .find(|e| e.state as usize == patch.state % GRASS_PER_SCENE)
                    {
                        self.particles.spawn_grass(region.scene, emitter, bank);
                    }
                }
                if region.grass_impact.is_some() {
                    let sign = if (patch.grass.bounds[0] as i64 + patch.grass.bounds[2] as i64) / 2
                        >= player.x as i64
                    {
                        1
                    } else {
                        -1
                    };
                    if !self.impacts.spawn(
                        region.scene,
                        patch.state,
                        patch.grass.bounds,
                        bounds,
                        sign,
                    ) {
                        result.impact_dropped += 1;
                    }
                }
            }
        }
        result
    }
    pub fn apply(&self, region: &Region) {
        crate::render::reset_visibility();
        for patch in region.grass {
            let cut = self.cut(patch.state);
            crate::render::set_visible(patch.grass.off_draw, !cut);
            crate::render::set_visible(patch.grass.on_draw, cut);
        }
        for object in region.breakables {
            let broken = self.broken(object.id);
            for &draw in object.off {
                crate::render::set_visible(draw as usize, !broken);
            }
            for &draw in object.on {
                crate::render::set_visible(draw as usize, broken);
            }
            for fade in object.fades {
                self.apply_mask(object.id, object.fade_ticks, fade);
            }
        }
        for binding in region.remote_masks {
            self.apply_mask(binding.owner, binding.fade_ticks, &binding.fade);
        }
    }
    fn apply_mask(&self, owner: usize, total_ticks: u16, fade: &MaskFade) {
        let elapsed = total_ticks.saturating_sub(self.fade_left[owner]);
        let gain = if !self.broken(owner) {
            128
        } else if fade.ticks == 0 || elapsed >= fade.ticks {
            0
        } else {
            ((fade.ticks - elapsed) as u32 * 128 / fade.ticks as u32) as u8
        };
        for &draw in fade.draws {
            crate::render::set_gain(draw as usize, gain);
        }
    }

    /// Only the exact source colliders owned by a broken object disappear.
    pub fn set_geo_edges(&mut self,edges:&[u16]) {
        assert!(edges.len()<=self.geo_edges.len());
        if self.geo_edge_count==edges.len()&&self.geo_edges[..edges.len()].iter().zip(edges).all(|(a,b)|a==b){return;}
        self.geo_edges[..edges.len()].copy_from_slice(edges);self.geo_edge_count=edges.len();self.edge_cache_count=0;self.edge_cache_valid=false;
    }
    /// Refresh local cocoon collider bindings on every region activation.
    pub fn set_lifeblood_edges(&mut self,edges:&[u16]) {
        assert!(edges.len()<=self.lifeblood_edges.len());
        if self.lifeblood_edge_count==edges.len()&&self.lifeblood_edges[..edges.len()].iter().zip(edges).all(|(a,b)|a==b){return;}
        self.lifeblood_edges[..edges.len()].copy_from_slice(edges);self.lifeblood_edge_count=edges.len();self.edge_cache_count=0;self.edge_cache_valid=false;
    }
    /// Append bounded scripted exclusions after the Lifeblood refresh. The two
    /// controllers share this eight-entry scratch; no second terrain cache.
    pub fn append_script_edges(&mut self,edges:&[u16]) {
        for &edge in edges {
            if self.lifeblood_edges[..self.lifeblood_edge_count].contains(&edge) {continue;}
            assert!(self.lifeblood_edge_count<self.lifeblood_edges.len());
            self.lifeblood_edges[self.lifeblood_edge_count]=edge;
            self.lifeblood_edge_count+=1;
            self.edge_cache_count=0;self.edge_cache_valid=false;
        }
    }
    /// Every index below `cache.len()` with the same exclusions as edge(),
    /// built in one pass over the short exclusion lists instead of scanning
    /// them once per edge.
    pub fn fill_edges(&self,region:&Region,room:&Room,cache:&mut [[i32;4]]) {
        fill_edges(&self.geo_edges[..self.geo_edge_count],&self.lifeblood_edges[..self.lifeblood_edge_count],&self.broken,region,room,cache);
    }
    /// Validate the source once for a whole physics query. The returned reader
    /// immutably borrows state/room, so exclusions cannot change while it lives.
    /// Larger rooms and invalidated caches retain exact uncached filtering.
    #[inline(always)]
    pub fn edge_reader<'a>(&'a self,region:&'a Region,room:&'a Room<'a>)
        ->impl Fn(usize)->[i32;4]+'a {
        let cached=if self.edge_cache_matches(region,room) {
            &self.edge_cache[..self.edge_cache_count]
        } else {&[]};
        move |index| {
            if index<cached.len(){cached[index]}
            else{self.edge_uncached(region,room,index)}
        }
    }
    // The cached read must stay in the caller: returning this four-word value
    // through an outlined MIPS call costs more than decoding the original edge.
    #[inline(always)]
    pub fn edge(&self, region: &Region, room: &Room, index: usize) -> [i32; 4] {
        if index<self.edge_cache_count&&self.edge_cache_matches(region,room) {return self.edge_cache[index];}
        self.edge_uncached(region,room,index)
    }
    /// Materialize a resident actor's terrain once per physics tick. The edge
    /// order and zero sentinels match edge_in_view(), but short exclusion lists
    /// are traversed once rather than once per solver edge query.
    pub fn fill_edges_in_view(&self, active:&Region, active_room:&Room,
        target:&Region, target_room:&Room, cache:&mut [[i32;4]]) {
        assert!(cache.len()<=target_room.counts[5]);
        if active.global_id==target.global_id {
            if self.edge_cache_matches(active,active_room) {
                cache.copy_from_slice(&self.edge_cache[..cache.len()]);
            } else {self.fill_edges(active,active_room,cache);}
            return;
        }
        for (i,edge) in cache.iter_mut().enumerate() {*edge=target_room.edge(i);}
        for b in target.breakables.iter().filter(|b|self.broken(b.id)) {
            for &i in b.edges {if let Some(edge)=cache.get_mut(i as usize) {*edge=[0;4];}}
        }
        if active.scene==target.scene {
            for &i in self.geo_edges[..self.geo_edge_count].iter()
                .chain(&self.lifeblood_edges[..self.lifeblood_edge_count]) {
                if (i as usize)>=active_room.counts[5] {continue;}
                let excluded=active_room.edge(i as usize);
                let reverse=[excluded[2],excluded[3],excluded[0],excluded[1]];
                for edge in cache.iter_mut() {
                    if *edge==excluded || *edge==reverse {*edge=[0;4];}
                }
            }
        }
    }
    /// Read another resident collision view without applying this view's local
    /// exclusion indices to unrelated edges. Breakables have stable scene IDs;
    /// transient Geo/cocoon/script exclusions are remapped by source segment.
    pub fn edge_in_view(&self, active:&Region, active_room:&Room,
        target:&Region, target_room:&Room, index:usize)->[i32;4] {
        if active.global_id==target.global_id {
            return self.edge(active,active_room,index);
        }
        let edge=target_room.edge(index);
        if target.breakables.iter().any(|b|self.broken(b.id)&&b.edges.contains(&(index as u16))) {
            return [0;4];
        }
        if active.scene==target.scene {
            for &i in self.geo_edges[..self.geo_edge_count].iter()
                .chain(&self.lifeblood_edges[..self.lifeblood_edge_count]) {
                if (i as usize)>=active_room.counts[5] {continue;}
                let excluded=active_room.edge(i as usize);
                if edge==excluded || edge==[excluded[2],excluded[3],excluded[0],excluded[1]] {
                    return [0;4];
                }
            }
        }
        edge
    }
    #[inline(never)]
    fn edge_uncached(&self, region:&Region,room:&Room,index:usize)->[i32;4] {
        if self.geo_edges[..self.geo_edge_count].contains(&(index as u16)) {return [0;4];}
        if self.lifeblood_edges[..self.lifeblood_edge_count].contains(&(index as u16)) {return [0;4];}
        for object in region.breakables {
            if self.broken(object.id) && object.edges.contains(&(index as u16)) {
                return [0; 4];
            }
        }
        room.edge(index)
    }
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
// World coordinates fit +/-512 and nail-local coordinates +/-16 Q16 units;
// i64 cross products remain below 2^54. Supports concave source polygons.
fn cross(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> i64 {
    (b[0] as i64 - a[0] as i64) * (p[1] as i64 - a[1] as i64)
        - (b[1] as i64 - a[1] as i64) * (p[0] as i64 - a[0] as i64)
}
fn on_segment(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> bool {
    cross(a, b, p) == 0
        && (a[0].min(b[0])..=a[0].max(b[0])).contains(&p[0])
        && (a[1].min(b[1])..=a[1].max(b[1])).contains(&p[1])
}
fn inside(poly: &[[i32; 2]], point: [i32; 2]) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if on_segment(a, b, point) {
            return true;
        }
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let side = cross(a, b, point);
            if (b[1] > a[1] && side > 0) || (b[1] < a[1] && side < 0) {
                inside = !inside;
            }
        }
    }
    inside
}
pub(crate) fn polygons_overlap(a: &[[i32; 2]], b: &[[i32; 2]]) -> bool {
    if !(3..=16).contains(&a.len()) || !(3..=16).contains(&b.len()) {
        return false;
    }
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            let (r, s) = (b[j], b[(j + 1) % b.len()]);
            let (pr, ps, rp, rq) = (
                cross(p, q, r),
                cross(p, q, s),
                cross(r, s, p),
                cross(r, s, q),
            );
            if ((pr > 0 && ps < 0 || pr < 0 && ps > 0) && (rp > 0 && rq < 0 || rp < 0 && rq > 0))
                || on_segment(p, q, r)
                || on_segment(p, q, s)
                || on_segment(r, s, p)
                || on_segment(r, s, q)
            {
                return true;
            }
        }
    }
    inside(a, b[0]) || inside(b, a[0])
}
/// Kind tags of the HKWMTA01 object records this module consumes.
pub const META_HAZARD: u16 = 3;
pub const META_CHECKPOINT: u16 = 4;
/// Copy one bank polygon into a bounded stack buffer for the shared polygon
/// test. Admission already bounded every polygon to 3..16 points.
fn bank_polygon_hits(polygon: hk_format::world_meta::Polygon, body: [i32; 4]) -> bool {
    let mut points = [[0i32; 2]; 16];
    let mut count = 0;
    for point in polygon.points().take(16) {
        let Ok(point) = point else { return false };
        points[count] = point;
        count += 1;
    }
    hk_sim::polygon_hits_box(&points[..count], body)
}
fn bank_trigger(
    region: hk_format::world_meta::Region,
    kind: u16,
    body: [i32; 4],
) -> Option<hk_format::world_meta::Object> {
    region.objects().flatten().find(|object| {
        object.kind() == kind
            && overlap(body, object.bounds())
            && object.polygons().flatten().any(|p| bank_polygon_hits(p, body))
    })
}
fn body_box(player: &Player, params: Params) -> [i32; 4] {
    [
        player.x - params.half_width,
        player.y + params.bottom,
        player.x + params.half_width,
        player.y + params.top,
    ]
}
/// Exact authored static DamageHero polygons against the Knight body box,
/// read from the admitted scene bank. Direction points away from the source
/// object's transform, for normal recoil.
pub fn hazard_contact(
    region: hk_format::world_meta::Region,
    player: &Player,
    params: Params,
) -> Option<(u16, bool, i32)> {
    let hazard = bank_trigger(region, META_HAZARD, body_box(player, params))?;
    let word = hazard.extra(1);
    let damage = (word & 0xFFFF) as u16;
    if damage == 0 {
        return None;
    }
    let direction = if player.x < hazard.extra(0) { -1 } else { 1 };
    Some((damage, word >> 16 != 0, direction))
}

/// Authored hazard checkpoint trigger, distinct from a bench or death save:
/// spawn point and facing from the admitted scene bank.
pub fn checkpoint(
    region: hk_format::world_meta::Region,
    player: &Player,
    params: Params,
) -> Option<([i32; 2], i32)> {
    let checkpoint = bank_trigger(region, META_CHECKPOINT, body_box(player, params))?;
    Some(([checkpoint.extra(0), checkpoint.extra(1)], checkpoint.extra(2)))
}

pub fn contains(bounds: [i32; 4], x: i32, y: i32) -> bool {
    x >= bounds[0] && x <= bounds[2] && y >= bounds[1] && y <= bounds[3]
}
pub fn locate(scene: usize, x: i32, y: i32) -> Option<usize> {
    REGIONS
        .iter()
        .position(|r| r.scene == scene && contains(r.bounds, x, y))
}
/// Predict only the current vertical leg. The same swept terrain solver used
/// by gameplay stops the look-ahead at floors, ceilings and the jump apex.
/// No horizontal movement is assumed; the caller retains its cheap fallback.
pub(crate) fn vertical_exit(
    bounds: [i32; 4],
    player: &Player,
    params: Params,
    edge_count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> Option<i32> {
    let direction = player.vy.signum();
    if direction == 0 {
        return None;
    }
    let mut predicted = *player;
    let jump = predicted.was_jump;
    for _ in 0..30 {
        predicted.step(params, 0, jump, edge_count, &edge);
        if direction > 0 && predicted.y > bounds[3] {
            return Some(bounds[3] + ONE / 16);
        }
        if direction < 0 && predicted.y < bounds[1] {
            return Some(bounds[1] - ONE / 16);
        }
        if predicted.grounded || predicted.vy.signum() != direction {
            break;
        }
    }
    None
}
pub fn upcoming(
    index: usize,
    player: &Player,
    room: &Room,
    state: &State,
    params: Params,
) -> Option<usize> {
    let region = &REGIONS[index];
    let bounds = region.bounds;
    let neighbour = |x, y| {
        region
            .neighbours
            .iter()
            .copied()
            .find(|&n| contains(REGIONS[n].bounds, x, y))
    };
    let vertical_candidate = (player.vy > ONE && bounds[3] - player.y < 6 * ONE)
        || (player.vy < -ONE && player.y - bounds[1] < 4 * ONE);
    if vertical_candidate {
        if let Some(y) = vertical_exit(bounds, player, params, room.counts[5], |i| {
            state.edge(region, room, i)
        }) {
            if let Some(next) = neighbour(player.x, y) {
                return Some(next);
            }
        }
    }
    let x = if player.facing > 0 {
        bounds[2] + ONE / 16
    } else {
        bounds[0] - ONE / 16
    };
    neighbour(x, player.y)
}

pub fn gate(scene: usize, player: &Player) -> Option<&'static Gate> {
    GATES
        .iter()
        .find(|g| g.scene == scene && contains(g.bounds, player.x, player.y))
}

//! Deterministic no-charm Geo. Payouts/physics parameters come from the original
//! source. Swept boxes and 50Hz integration approximate Unity Box2D contacts;
//! ObjectBounce's four-step cache and randomized reflection are retained.
//! Room-view changes never clear state. Actual scene exits discard loose Geo;
//! rock hits persist. Death removes the wallet; Shade recovery is not invented.
use hk_sim::{ONE,polygon_hits_box};
/// The coin edge copy: the edge cache's length.
const EDGE_COPY:usize=128;
pub const MAX_COINS:usize=64;
/// The source catalog contains 207 GeoRock instances. Keep persistent rock
/// state in one sparse pool instead of multiplying storage by every scene.
pub const MAX_ROCK_STATES:usize=256;
pub const MAX_ROCKS_PER_SCENE:usize=16;
pub const MAX_ENEMIES:usize=32;
pub const MAX_PENDING:usize=16;
/// A stalactite's flung rocks ride the loose coins: a coin whose denomination
/// has this bit is a rock of no value, never collected, gone `ROCK_TICKS`
/// after its first contact; the low bits are its look, which props draws.
pub const ROCK:u8=0x80;
#[cfg(not(test))]
use crate::props::{ROCK_COIN,ROCK_TICKS};
#[cfg(test)]
const ROCK_COIN:CoinSpec=CoinSpec{value:0,gravity:-60*ONE,body_offset:[0,0],half:[ONE/16;2],pickup_offset:[0,0],pickup_half:[0,0],bounce:ONE/2,threshold:ONE,friction:ONE};
#[cfg(test)]
const ROCK_TICKS:u32=570;
fn spec(params:Params,denomination:u8)->CoinSpec {if denomination&ROCK!=0 {ROCK_COIN}else{params.coins[denomination as usize]}}
/// Cooperative input service only: never consumes queued input or advances
/// simulation/RNG. Coin contact scans may span a VBlank at the full pool size.
#[inline]
fn checkpoint() {
    #[cfg(not(test))]
    crate::input::checkpoint();
}
#[derive(Clone,Copy,Debug)]
pub struct Fling {pub speed:[i32;2],pub angle:[i32;2],pub spread:[i32;2]}
#[derive(Clone,Copy,Debug)]
pub struct CoinSpec {
    pub value:u32,pub gravity:i32,pub body_offset:[i32;2],pub half:[i32;2],
    pub pickup_offset:[i32;2],pub pickup_half:[i32;2],pub bounce:i32,pub threshold:i32,pub friction:i32,
}
#[derive(Clone,Copy,Debug)]
pub struct Params {pub coins:[CoinSpec;3],pub pickup_ticks:u16,pub wallet_max:u32}
#[derive(Clone,Copy,Debug)]
pub struct RockSpec {
    pub source_id:u32,pub scene:u8,pub state:u8,pub x:i32,pub y:i32,pub bounds:[i32;4],
    pub polygons:&'static[&'static[[i32;2]]],pub hits:u8,pub per_hit:u8,pub final_payout:u8,
    pub hit_cooldown:u16,pub broken_frame:u16,pub fling:Fling,
}
#[derive(Clone,Copy,Debug)]
pub struct EnemySpec {pub source_id:u32,pub scene:u8,pub drops:[u16;3],pub fling:Fling,pub offset:[i32;2]}
/// The two authored enemy fling profiles, indexed by the source enemy's
/// `megaFlingGeo`. Every enemy in the catalogue carries one of exactly these
/// two, so the metadata bank spends a flag bit on the choice rather than six
/// constant Q16 words on each of the 236 payouts.
pub const FLING:[Fling;2]=[
    Fling{speed:[15*ONE,30*ONE],angle:[80*ONE,100*ONE],spread:[0,0]},
    Fling{speed:[30*ONE,45*ONE],angle:[65*ONE,115*ONE],spread:[0,0]},
];
#[derive(Clone,Copy,Debug)]
pub struct Coin {
    pub scene:u8,pub x:i32,pub y:i32,pub vx:i32,pub vy:i32,pub age:u32,
    pub denomination:u8,pub grounded:bool,pub landed:bool,
    was_inside:bool,cache_ticks:u8,last:[i32;2],direction:[i32;2],speed:i32,rng:u32,
    touching:bool,support:Option<usize>,support_edge:[i32;4],
}
#[derive(Clone,Copy)]
struct Rock {source:u32,key:usize,left:u8,cooldown:u16,swing:u32}
#[derive(Clone,Copy)]
struct Enemy {source:u32,scene:u8}
#[derive(Clone,Copy)]
struct Pending {source:u32,scene:u8,origin:[i32;2],fling:Fling,counts:[u16;3],rng:u32}
#[derive(Default,Clone,Copy,Debug,PartialEq,Eq)]
pub struct Strike {pub hits:u32,pub depleted:u32,pub payout:u32}
#[derive(Default,Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tick {pub collected:u32,pub coins:u32,pub bounces:u32,pub last_value:u32}
pub struct World {
    rocks:[Option<Rock>;MAX_ROCK_STATES],enemies:[Option<Enemy>;MAX_ENEMIES],
    loose:[Option<Coin>;MAX_COINS],pending:[Option<Pending>;MAX_PENDING],swing:u32,phase:u8,
    wallet:u32,pub hit_events:u32,pub depleted_rocks:u32,pub spawned_value:u32,
    pub collected_value:u32,pub discarded_value:u32,pub deferred_events:u32,
}
#[no_mangle]pub static mut HK_GEO_WALLET:u32=0;
#[no_mangle]pub static mut HK_GEO_HITS:u32=0;
#[no_mangle]pub static mut HK_GEO_ROCKS_DEPLETED:u32=0;
#[no_mangle]pub static mut HK_GEO_SPAWNED_VALUE:u32=0;
#[no_mangle]pub static mut HK_GEO_COLLECTED_VALUE:u32=0;
#[no_mangle]pub static mut HK_GEO_ACTIVE:u32=0;
#[no_mangle]pub static mut HK_GEO_PENDING_VALUE:u32=0;
impl World {
    pub const fn new()->Self {Self {rocks:[None;MAX_ROCK_STATES],enemies:[None;MAX_ENEMIES],
        loose:[None;MAX_COINS],pending:[None;MAX_PENDING],swing:0,phase:0,wallet:0,
        hit_events:0,depleted_rocks:0,spawned_value:0,collected_value:0,discarded_value:0,deferred_events:0}}
    pub fn wallet(&self)->u32 {self.wallet}
    pub fn reset(&mut self,params:Params) {*self=Self::new();self.publish(params);}
    /// Save Game restore of the wallet alone (rocks and loose coins start fresh).
    pub fn restore_wallet(&mut self,wallet:u32,params:Params) {self.wallet=wallet;self.publish(params);}
    /// HeroController::AddGeoQuietly, which the Shade's Death Start calls with
    /// the pool it carried. No coin is spawned and no pickup is collected.
    pub fn add_quietly(&mut self,amount:u32,params:Params) {
        self.wallet=self.wallet.saturating_add(amount).min(params.wallet_max);self.publish(params);
    }
    /// HeroController::TakeGeo, which a shop's Confirm Control calls with the
    /// price it just charged. Nothing is spawned or dropped; the wallet is
    /// simply lighter, and a caller that never checked CanBuy cannot push it
    /// below zero.
    pub fn take(&mut self,amount:u32,params:Params) {
        self.wallet=self.wallet.saturating_sub(amount);self.publish(params);
    }
    pub fn coins(&self)->impl Iterator<Item=&Coin> {self.loose.iter().flatten()}
    pub fn indexed_coins(&self)->impl Iterator<Item=(usize,&Coin)> {self.loose.iter().enumerate().filter_map(|(i,c)|c.as_ref().map(|c|(i,c)))}
    pub fn active(&self)->usize {self.coins().count()}
    pub fn pending_value(&self,params:Params)->u32 {self.pending.iter().flatten().map(|p|value(p.counts,params)).sum()}
    pub fn begin_attack(&mut self) {
        self.swing=self.swing.wrapping_add(1);
        if self.swing==0 {self.swing=1;for rock in self.rocks.iter_mut().flatten(){rock.swing=0;}}
    }
    pub fn hits_remaining(&self,scene:usize,state:usize)->Option<u8> {
        let key=scene.checked_mul(MAX_ROCKS_PER_SCENE)?.checked_add(state)?;
        self.rocks.iter().flatten().find(|r|r.key==key).map(|r|r.left)
    }
    pub fn rock_depleted(&self,scene:usize,state:usize)->bool {self.hits_remaining(scene,state)==Some(0)}
    /// Every rock hit at least once this session, as (scene, state, hits left):
    /// the source's `GeoRockData.hitsLeft`, for the save record.
    pub fn rock_states(&self)->impl Iterator<Item=(usize,usize,u8)>+'_ {
        self.rocks.iter().flatten().map(|r|(r.key/MAX_ROCKS_PER_SCENE,r.key%MAX_ROCKS_PER_SCENE,r.left))
    }
    /// Save Game restore of one rock. False for a rock this build does not
    /// cook, or a full pool, either of which leaves the rock as authored.
    pub fn restore_rock(&mut self,rocks:&[RockSpec],scene:usize,state:usize,left:u8)->bool {
        let Some(spec)=rocks.iter().find(|r|r.scene as usize==scene&&r.state as usize==state) else {return false;};
        let key=scene*MAX_ROCKS_PER_SCENE+state;
        let slot=self.rocks.iter().position(|r|r.is_some_and(|r|r.key==key)).or_else(||self.rocks.iter().position(Option::is_none));
        let Some(slot)=slot else {return false;};
        self.rocks[slot]=Some(Rock {source:spec.source_id,key,left:left.min(spec.hits),cooldown:0,swing:0});
        true
    }
    /// Caller supplies the current active nail polygon and starts one swing
    /// token per attack, never per rendered frame. Repeated room copies share
    /// source/state identities, so overlaps cannot mint duplicate payouts.
    #[inline(never)]
    pub fn strike(&mut self,scene:usize,rocks:&[RockSpec],polygon:&[[i32;2]])->Strike {
        checkpoint();
        let mut event=Strike::default();
        if self.swing==0 || !(3..=16).contains(&polygon.len()) {return event;}
        for spec in rocks.iter().filter(|r|r.scene as usize==scene) {
            checkpoint();
            assert!((spec.state as usize)<MAX_ROCKS_PER_SCENE && spec.hits>0);
            let Some(key)=scene.checked_mul(MAX_ROCKS_PER_SCENE).and_then(|v|v.checked_add(spec.state as usize)) else {self.deferred_events+=1;continue;};
            if !polygon_hits_box(polygon,spec.bounds) || !spec.polygons.iter().any(|p|polygons_overlap(polygon,p)) {continue;}
            let existing=self.rocks.iter().flatten().find(|r|r.key==key).copied();
            let state=existing.unwrap_or(Rock {source:spec.source_id,key,left:spec.hits,cooldown:0,swing:0});
            assert_eq!(state.source,spec.source_id,"Geo rock state alias");
            if state.left==0 || state.cooldown!=0 || state.swing==self.swing {continue;}
            // Source Hit always emits per-hit coins, including its final hit.
            // Destroy then emits the additional authored final payout.
            let count=spec.per_hit as u16+if state.left==1 {spec.final_payout as u16}else{0};
            if !self.queue(spec.source_id,spec.scene,[spec.x,spec.y],spec.fling,[count,0,0]) {
                self.deferred_events=self.deferred_events.saturating_add(1);continue;
            }
            let left=state.left-1;
            let slot=self.rocks.iter().position(|r|r.is_some_and(|r|r.key==key)).or_else(||self.rocks.iter().position(Option::is_none));
            let Some(slot)=slot else {self.deferred_events+=1;continue;};
            self.rocks[slot]=Some(Rock {left,cooldown:spec.hit_cooldown,swing:self.swing,..state});
            event.hits+=1;event.payout+=count as u32;
            if left==0 {event.depleted+=1;self.depleted_rocks+=1;}
            self.hit_events+=1;
        }
        event
    }
    /// Called by EnemyWorld's explicit one-time death event. The ledger also
    /// rejects duplicate calls; reset_enemies is tied to real actor respawn.
    #[inline(never)]
    pub fn spawn_enemy(&mut self,scene:usize,source:u32,position:[i32;2],spec:EnemySpec)->bool {
        if self.enemies.iter().flatten().any(|e|e.scene as usize==scene&&e.source==source) {return false;}
        if spec.scene as usize!=scene||spec.source_id!=source {return false;}
        let Some(slot)=self.enemies.iter().position(Option::is_none)else{return false;};
        let origin=[position[0].saturating_add(spec.offset[0]),position[1].saturating_add(spec.offset[1])];
        if !self.queue(source,spec.scene,origin,spec.fling,spec.drops) {self.deferred_events+=1;return false;}
        self.enemies[slot]=Some(Enemy {source,scene:spec.scene});true
    }
    /// One stalactite rock (`ROCK` with its `look`) flung from `at` at `degrees`
    /// and `speed` (Q16); false, and nothing flung, with every slot taken.
    pub fn launch_rock(&mut self,scene:usize,at:[i32;2],degrees:i32,speed:i32,look:u8)->bool {
        let Some(slot)=self.loose.iter_mut().find(|s|s.is_none())else{return false;};
        *slot=Some(Coin::flung(scene as u8,at,degrees*ONE,speed,ROCK|look,at[0]as u32^(speed as u32)^(degrees as u32)<<16));true
    }
    /// A chest's `Spawn Items`: its three flings at once from its own origin,
    /// through the same pending queue an enemy's payout takes.
    pub fn spawn_chest(&mut self,scene:usize,source:u32,origin:[i32;2],fling:Fling,counts:[u16;3])->bool {
        let queued=self.queue(source,scene as u8,origin,fling,counts);
        if !queued {self.deferred_events=self.deferred_events.saturating_add(1);}
        queued
    }
    pub fn reset_enemies(&mut self,scene:usize) {
        for e in &mut self.enemies {if e.is_some_and(|e|e.scene as usize==scene){*e=None;}}
    }
    fn queue(&mut self,source:u32,scene:u8,origin:[i32;2],fling:Fling,counts:[u16;3])->bool {
        if counts==[0;3] {return true;}
        let existing=self.pending.iter().position(|p|p.is_some_and(|p|p.source==source&&p.scene==scene&&p.origin==origin));
        if let Some(i)=existing {
            let p=self.pending[i].as_mut().unwrap();
            let Some(counts)=p.counts[0].checked_add(counts[0]).zip(p.counts[1].checked_add(counts[1])).zip(p.counts[2].checked_add(counts[2]))else{return false;};
            p.counts=[counts.0.0,counts.0.1,counts.1];return true;
        }
        let Some(i)=self.pending.iter().position(Option::is_none)else{return false;};
        self.pending[i]=Some(Pending {source,scene,origin,fling,counts,rng:source^(scene as u32)<<24^self.swing^0x9e3779b9});true
    }
    /// Pending Geo into free slots; once every slot is taken, into the slots
    /// of a stalactite's rocks, which are only for show, so a burst waits at
    /// most a tick behind them.
    fn emit(&mut self,scene:usize,params:Params) {
        let full=self.loose.iter().all(Option::is_some);
        for slot in &mut self.loose {
            if slot.is_some_and(|c|!full||c.denomination&ROCK==0) {continue;}
            let Some(i)=self.pending.iter().position(|p|p.is_some_and(|p|p.scene as usize==scene))else{break;};
            checkpoint();
            let p=self.pending[i].as_mut().unwrap();let kind=p.counts.iter().position(|&n|n>0).unwrap();
            let angle=range(&mut p.rng,p.fling.angle);let speed=range(&mut p.rng,p.fling.speed);
            let x=p.origin[0].saturating_add(range(&mut p.rng,[-p.fling.spread[0],p.fling.spread[0]]));
            let y=p.origin[1].saturating_add(range(&mut p.rng,[-p.fling.spread[1],p.fling.spread[1]]));
            *slot=Some(Coin::flung(p.scene,[x,y],angle,speed,kind as u8,p.rng));
            p.counts[kind]-=1;self.spawned_value=self.spawned_value.saturating_add(params.coins[kind].value);
            if p.counts==[0;3] {self.pending[i]=None;}
        }
    }
    /// 60Hz gameplay time; five source20ms physics callbacks per six ticks.
    /// Coins outside the provided resident collision apron retain their state
    /// and pause physics, matching the existing bounded actor/debris policy.
    #[inline(never)]
    pub fn tick(&mut self,scene:usize,params:Params,hero:[i32;4],coverage:[i32;4],count:usize,edge:impl Fn(usize)->[i32;4])->Tick {
        checkpoint();
        let mut event=Tick::default();
        for r in self.rocks.iter_mut().flatten(){r.cooldown=r.cooldown.saturating_sub(1);}
        self.emit(scene,params);self.phase+=50;let physics=self.phase>=60;if physics {self.phase-=60;}
        // A burst of coins (an Elder Baldur's) stepped every coin against every
        // edge, and the ticks fell behind the vblanks for most of a second. With
        // more than one coin in flight, index the edges once (the index is built
        // from a copy; the coins still read `edge`); each
        // coin then visits only the edges in its own x range (Coin::step).
        let moving=if physics {self.loose.iter().flatten().filter(|c|c.scene as usize==scene
            && !(c.grounded&&c.vx==0&&c.vy==0) && inside([c.x,c.y],coverage)).count()} else {0};
        // The same pass splits the edges by what each loop of Coin::step can
        // use: walls are vertical edges with height, floors and ceilings are
        // the edges that are not vertical. Each loop already skipped the
        // other kind, so masking them out changes no answer.
        let columns=if moving>1 && count<=EDGE_COPY {
            let mut copy=[[0i32;4];EDGE_COPY];let mut walls=[0u32;4];let mut floors=[0u32;4];
            for (i,e) in copy[..count].iter_mut().enumerate() {
                *e=edge(i);
                if e[0]==e[2] {if e[1]!=e[3] {walls[i/32]|=1<<(i%32);}} else {floors[i/32]|=1<<(i%32);}
            }
            let columns=hk_sim::runner_senses::EdgeColumns::build(&copy[..count]);
            // And the same index over y: each edge's x and y swapped.
            for e in &mut copy[..count] {*e=[e[1],e[0],e[3],e[2]];}
            Some(CoinEdges{columns,rows:hk_sim::runner_senses::EdgeColumns::build(&copy[..count]),walls,floors})
        } else {None};
        for slot in &mut self.loose {
            let Some(coin)=slot.as_mut()else{continue;};if coin.scene as usize!=scene {continue;}
            checkpoint();
            let spec=spec(params,coin.denomination);coin.age=coin.age.saturating_add(1);
            if physics && inside([coin.x,coin.y],coverage) {event.bounces+=coin.step(spec,count,&edge,columns.as_ref())as u32;}
            if coin.denomination&ROCK!=0 {
                // A rock's age counts from its first contact, and it is never taken.
                if !coin.landed {coin.age=0;}
                if coin.age>=ROCK_TICKS {*slot=None;}
                continue;
            }
            let bounds=[coin.x+spec.pickup_offset[0]-spec.pickup_half[0],coin.y+spec.pickup_offset[1]-spec.pickup_half[1],coin.x+spec.pickup_offset[0]+spec.pickup_half[0],coin.y+spec.pickup_offset[1]+spec.pickup_half[1]];
            let intersects=overlap(bounds,hero);
            // GeoControl uses OnTriggerEnter, not Stay: entering during the
            // startup lock does not collect until exit and a subsequent entry.
            let collect=intersects&&!coin.was_inside&&coin.age>=params.pickup_ticks as u32;
            coin.was_inside=intersects;
            if collect {
                self.wallet=self.wallet.saturating_add(spec.value).min(params.wallet_max);
                self.collected_value=self.collected_value.saturating_add(spec.value);
                event.collected=event.collected.saturating_add(spec.value);event.coins+=1;event.last_value=spec.value;*slot=None;
            }
        }
        checkpoint();
        self.publish(params);event
    }
    /// Actual Unity scene unload destroys loose coin objects. Never call on a
    /// spatial room-view change. Pending emissions also belong to that scene.
    pub fn leave_scene(&mut self,scene:usize,params:Params) {
        for coin in &mut self.loose {if coin.is_some_and(|c|c.scene as usize==scene) {
            self.discarded_value=self.discarded_value.saturating_add(spec(params,coin.unwrap().denomination).value);*coin=None;
        }}
        for p in &mut self.pending {if p.is_some_and(|p|p.scene as usize==scene) {
            self.discarded_value=self.discarded_value.saturating_add(value(p.unwrap().counts,params));*p=None;
        }}
        self.publish(params);
    }
    /// Returns lost currency for telemetry/future source Shade integration.
    /// This does not reset rocks or silently permit another enemy payout.
    pub fn death(&mut self)->u32 {let lost=self.wallet;self.wallet=0;unsafe {core::ptr::write_volatile(&raw mut HK_GEO_WALLET,0);}lost}
    /// Source Acid contact destroys the object without credit; root may route
    /// authored acid trigger events here using indexed_coins() stable pool slots.
    pub fn acid(&mut self,index:usize,params:Params)->bool {
        let Some(slot)=self.loose.get_mut(index)else{return false;};let Some(c)=slot.take()else{return false;};
        self.discarded_value=self.discarded_value.saturating_add(spec(params,c.denomination).value);self.publish(params);true
    }
    fn publish(&self,params:Params) {unsafe {
        core::ptr::write_volatile(&raw mut HK_GEO_WALLET,self.wallet);
        core::ptr::write_volatile(&raw mut HK_GEO_HITS,self.hit_events);
        core::ptr::write_volatile(&raw mut HK_GEO_ROCKS_DEPLETED,self.depleted_rocks);
        core::ptr::write_volatile(&raw mut HK_GEO_SPAWNED_VALUE,self.spawned_value);
        core::ptr::write_volatile(&raw mut HK_GEO_COLLECTED_VALUE,self.collected_value);
        core::ptr::write_volatile(&raw mut HK_GEO_ACTIVE,self.active()as u32);
        core::ptr::write_volatile(&raw mut HK_GEO_PENDING_VALUE,self.pending_value(params));
    }}
}
fn value(counts:[u16;3],p:Params)->u32 {(0..3).map(|i|counts[i]as u32*p.coins[i].value).sum()}
fn mul(a:i32,b:i32)->i32 {psx_math::int32::mul_shr_trunc_i32(a,b,16)}
fn random(seed:&mut u32)->u32 {*seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);*seed}
fn range(seed:&mut u32,b:[i32;2])->i32 {assert!(b[0]<=b[1]);b[0]+(random(seed)as u64%((b[1]as i64-b[0]as i64+1)as u64))as i32}
fn inside(p:[i32;2],b:[i32;4])->bool {p[0]>=b[0]&&p[0]<=b[2]&&p[1]>=b[1]&&p[1]<=b[3]}
fn overlap(a:[i32;4],b:[i32;4])->bool {a[0]<=b[2]&&a[2]>=b[0]&&a[1]<=b[3]&&a[3]>=b[1]}
fn length(v:[i32;2])->i32 {
    let n=(v[0]as i64*v[0]as i64+v[1]as i64*v[1]as i64)as u64;
    psx_math::int32::isqrt_u64(n).min(i32::MAX as u32)as i32
}
impl Coin {
    /// A coin leaving `at` at `angle` (Q16 degrees) and `speed`.
    #[inline(never)]
    fn flung(scene:u8,at:[i32;2],angle:i32,speed:i32,denomination:u8,rng:u32)->Self {
        Coin {scene,x:at[0],y:at[1],vx:mul(speed,sin(angle+90*ONE)),vy:mul(speed,sin(angle)),
            age:0,denomination,grounded:false,landed:false,was_inside:false,
            cache_ticks:0,last:[0,0],direction:[0,0],speed:0,rng,touching:false,support:None,support_edge:[0;4]}
    }
    #[inline(never)]
    fn step(&mut self,s:CoinSpec,count:usize,edge:&impl Fn(usize)->[i32;4],index:Option<&CoinEdges>)->bool {
        // Exact zero-velocity rest avoids scanning every edge. A removed or
        // changed source support invalidates the cached manifold immediately.
        if self.grounded&&self.vx==0&&self.vy==0&&self.support.is_some_and(|i|i<count&&hk_sim::same_edge(&edge(i),&self.support_edge)) {return false;}
        self.support=None;

        if self.cache_ticks==3 {self.direction=[self.x-self.last[0],self.y-self.last[1]];self.last=[self.x,self.y];self.speed=length([self.vx,self.vy]);self.cache_ticks=0;}else{self.cache_ticks+=1;}
        self.vy=self.vy.saturating_add(s.gravity/50);
        let ox=self.x+s.body_offset[0];let oy=self.y+s.body_offset[1];
        let mut nx=ox.saturating_add(self.vx/50);let mut ny=oy.saturating_add(self.vy/50);let mut normal=[0,0];
        self.grounded=false;
        // A wall can only stop the sweep between the old and the new x, and a
        // floor or ceiling only matters under the new x: edges outside those x
        // ranges change nothing, so the index's choice gives the same result.
        // A wall only stops a sweep that moves in x.
        let walls=if nx==ox {[0;4]} else {index.map_or(hk_sim::runner_senses::ALL_EDGES,|c|
            // ... and only on an edge whose y range meets the body's.
            mask_and(mask_and(c.columns.near([ox.min(nx)-s.half[0],oy,ox.max(nx)+s.half[0],oy]),c.walls),
                c.rows.near([oy-s.half[1],0,oy+s.half[1],0])))};
        for i in hk_sim::runner_senses::selected(count,walls) {
            if i&31==0 {checkpoint();}
            let[x0,y0,x1,y1]=edge(i);if x0!=x1||y0==y1||oy+s.half[1]<=y0.min(y1)||oy-s.half[1]>=y0.max(y1){continue;}
            if nx>ox&&ox+s.half[0]<=x0&&nx+s.half[0]>x0 {nx=x0-s.half[0];normal=[-ONE,0];}
            if nx<ox&&ox-s.half[0]>=x0&&nx-s.half[0]<x0 {nx=x0+s.half[0];normal=[ONE,0];}
        }
        // A landing needs the surface at or above the new bottom and the old
        // height at most 32 under the old bottom; a ceiling the old height at
        // or above the old top and the surface at or under the new top. Both
        // heights lie in the edge's y range, so an edge whose y range misses
        // lo - half ..= hi + half + 32 can do neither.
        let floors=index.map_or(hk_sim::runner_senses::ALL_EDGES,|c|mask_and(mask_and(c.columns.near([nx-s.half[0],oy,nx+s.half[0],oy]),c.floors),
            c.rows.near([oy.min(ny)-s.half[1],0,oy.max(ny)+s.half[1]+32,0])));
        for i in hk_sim::runner_senses::selected(count,floors) {
            if i&31==0 {checkpoint();}
            let[x0,y0,x1,y1]=edge(i);if x0==x1||nx+s.half[0]<=x0.min(x1)||nx-s.half[0]>=x0.max(x1){continue;}
            // A flat edge's height is y0 everywhere: mul_div_i32(_, 0, _) is 0.
            let height=|x:i32|if y0==y1 {y0} else {y0+psx_math::int32::mul_div_i32(x.clamp(x0.min(x1),x0.max(x1))-x0,y1-y0,x1-x0)};
            let old=height(ox);let surface=height(nx);
            if ny<oy&&oy-s.half[1]>=old-32&&ny-s.half[1]<=surface {ny=surface+s.half[1];normal=[-(y1-y0),x1-x0];if normal[1]<0 {normal=[-normal[0],-normal[1]];}self.grounded=true;self.support=Some(i);self.support_edge=[x0,y0,x1,y1];}
            else if ny>oy&&oy+s.half[1]<=old&&ny+s.half[1]>=surface {ny=surface-s.half[1];normal=[y1-y0,-(x1-x0)];if normal[1]>0 {normal=[-normal[0],-normal[1]];}}
        }
        self.x=nx-s.body_offset[0];self.y=ny-s.body_offset[1];
        if normal==[0,0] {self.touching=false;return false;}
        self.landed=true;let entered=!self.touching;self.touching=true;
        if entered && self.speed>s.threshold && self.direction!=[0,0] {
            let n=length(normal);let normal=[psx_math::int32::mul_div_i32(normal[0],ONE,n),psx_math::int32::mul_div_i32(normal[1],ONE,n)];
            let norm=length(self.direction);let d=[psx_math::int32::mul_div_i32(self.direction[0],ONE,norm),psx_math::int32::mul_div_i32(self.direction[1],ONE,norm)];
            let dot=mul(d[0],normal[0])+mul(d[1],normal[1]);
            let reflected=[d[0]-2*mul(dot,normal[0]),d[1]-2*mul(dot,normal[1])];
            let speed=mul(mul(self.speed,s.bounce),range(&mut self.rng,[52429,78643]));
            self.vx=mul(reflected[0],speed);self.vy=mul(reflected[1],speed);self.grounded=false;true
        }else{
            if normal[0].abs()>normal[1].abs(){self.vx=0;}else{self.vy=0;self.vx=mul(self.vx,ONE-s.friction.clamp(0,ONE));}false
        }
    }
}
/// The room's edge index for one physics tick of a coin burst: x columns,
/// y rows, and which edges can be walls and which floors or ceilings.
struct CoinEdges {columns:hk_sim::runner_senses::EdgeColumns,rows:hk_sim::runner_senses::EdgeColumns,walls:hk_sim::runner_senses::EdgeMask,floors:hk_sim::runner_senses::EdgeMask}
#[inline]
fn mask_and(a:hk_sim::runner_senses::EdgeMask,b:hk_sim::runner_senses::EdgeMask)->hk_sim::runner_senses::EdgeMask {[a[0]&b[0],a[1]&b[1],a[2]&b[2],a[3]&b[3]]}
fn cross(a:[i32;2],b:[i32;2],p:[i32;2])->i64 {(b[0]as i64-a[0]as i64)*(p[1]as i64-a[1]as i64)-(b[1]as i64-a[1]as i64)*(p[0]as i64-a[0]as i64)}
fn polygons_overlap(a:&[[i32;2]],b:&[[i32;2]])->bool {
    if a.len()<3||b.len()<3{return false;}
    for i in 0..a.len(){for j in 0..b.len(){
        let(x,y,p,q)=(a[i],a[(i+1)%a.len()],b[j],b[(j+1)%b.len()]);
        if x[0].max(y[0])<p[0].min(q[0])||p[0].max(q[0])<x[0].min(y[0])||x[1].max(y[1])<p[1].min(q[1])||p[1].max(q[1])<x[1].min(y[1]){continue;}
        let(c,d,e,f)=(cross(x,y,p),cross(x,y,q),cross(p,q,x),cross(p,q,y));
        if ((c<=0&&d>=0)||(c>=0&&d<=0))&&((e<=0&&f>=0)||(e>=0&&f<=0)){return true;}
    }}
    let contains=|poly:&[[i32;2]],p:[i32;2]| {let mut yes=false;for i in 0..poly.len(){let(a,b)=(poly[i],poly[(i+1)%poly.len()]);if(a[1]>p[1])!=(b[1]>p[1]){let c=cross(a,b,p);if(b[1]>a[1]&&c>0)||(b[1]<a[1]&&c<0){yes=!yes;}}}yes};
    contains(a,b[0])||contains(b,a[0])
}

// Q16 sine samples; linear interpolation keeps deterministic sub-degree fling.
const SINE:[i32;91]=[0,1144,2287,3430,4572,5712,6850,7987,9121,10252,11380,12505,13626,14742,15855,16962,18064,19161,20252,21336,22415,23486,24550,25607,26656,27697,28729,29753,30767,31772,32768,33754,34729,35693,36647,37590,38521,39441,40348,41243,42126,42995,43852,44695,45525,46341,47143,47930,48703,49461,50203,50931,51643,52339,53020,53684,54332,54963,55578,56175,56756,57319,57865,58393,58903,59396,59870,60326,60764,61183,61584,61966,62328,62672,62997,63303,63589,63856,64104,64332,64540,64729,64898,65048,65177,65287,65376,65446,65496,65526,65536];
fn sin(angle:i32)->i32 {
 let a=angle.rem_euclid(360*ONE);let quadrant=a/(90*ONE);let mut t=a%(90*ONE);
 if quadrant==1||quadrant==3 {t=90*ONE-t;}
 let i=(t/ONE)as usize;let v=if i==90 {ONE}else{SINE[i]+mul(SINE[i+1]-SINE[i],t%ONE)};
 if quadrant>=2 {-v}else{v}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_rock_keys_keep_scene_state_independent() {
        let mut world=World::new();
        world.rocks[0]=Some(Rock{source:11,key:0,left:4,cooldown:0,swing:0});
        world.rocks[1]=Some(Rock{source:22,key:MAX_ROCKS_PER_SCENE,left:1,cooldown:0,swing:0});
        assert_eq!(world.hits_remaining(0,0),Some(4));
        assert_eq!(world.hits_remaining(1,0),Some(1));
        assert_eq!(world.hits_remaining(0,1),None);
    }

    #[test]
    fn rock_pool_is_catalog_bounded_without_scene_multiplication() {
        assert_eq!(core::mem::size_of_val(&World::new().rocks),
                   core::mem::size_of::<Option<Rock>>() * MAX_ROCK_STATES);
        assert!(MAX_ROCK_STATES>=207);
    }
}

#[cfg(not(test))]
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/geo.rs"));

/// Rocks bound into a catalogue region, or nothing. `GEO_BINDINGS` is sparse
/// and sorted so the linked table grows with the bound rocks rather than with
/// every view the catalogue admits.
#[cfg(not(test))]
pub fn region_bindings(region:usize)->&'static [GeoDrawBinding] {
    let Ok(key)=u16::try_from(region) else {return &[];};
    match GEO_BINDINGS.binary_search_by_key(&key,|&(r,_)|r) {
        Ok(at)=>GEO_BINDINGS[at].1,
        Err(_)=>&[],
    }
}

#[path="../game/src/geo.rs"]mod geo;
use geo::*;
use hk_sim::ONE;
const SHAPE:&[[i32;2]]=&[[-ONE,-ONE],[ONE,-ONE],[ONE,ONE],[-ONE,ONE]];
const FLING:Fling=Fling{speed:[0,0],angle:[90*ONE,90*ONE],spread:[0,0]};
const COIN:CoinSpec=CoinSpec{value:1,gravity:0,body_offset:[0,0],half:[ONE/4;2],pickup_offset:[0,0],pickup_half:[ONE/4;2],bounce:45875,threshold:ONE,friction:13107};
const P:Params=Params{coins:[COIN,CoinSpec{value:5,..COIN},CoinSpec{value:25,..COIN}],pickup_ticks:15,wallet_max:9_999_999};
const ROCK:RockSpec=RockSpec{source_id:12844,scene:0,state:0,x:0,y:0,bounds:[-ONE,-ONE,ONE,ONE],polygons:&[SHAPE],hits:5,per_hit:2,final_payout:5,hit_cooldown:2,broken_frame:0,fling:FLING};
const FAR:[i32;4]=[100*ONE,100*ONE,101*ONE,101*ONE];
const NEAR:[i32;4]=[-ONE,-ONE,ONE,ONE];
const COVER:[i32;4]=[-100*ONE,-100*ONE,100*ONE,100*ONE];
fn enemy(id:u32,drops:[u16;3])->EnemySpec {EnemySpec{source_id:id,scene:0,drops,fling:FLING,offset:[0,0]}}
fn ticks(w:&mut World,n:usize,hero:[i32;4])->Tick {let mut last=Tick::default();for _ in 0..n {last=w.tick(0,P,hero,COVER,0,|_|[0;4]);}last}
#[test]fn source_rock_counts_last_hit_adds_both_payouts_and_each_attack_hits_once(){
 for spec in [ROCK,RockSpec{hits:4,per_hit:3,final_payout:6,..ROCK}]{
  let mut w=World::new();let mut total=0;
  for hit in 0..spec.hits {
   w.begin_attack();let e=w.strike(0,&[spec],SHAPE);assert_eq!(e.hits,1);total+=e.payout;
   for _ in 0..6 {assert_eq!(w.strike(0,&[spec],SHAPE),Strike::default());}
   assert_eq!(w.hits_remaining(0,0),Some(spec.hits-hit-1));ticks(&mut w,2,FAR);
  }
  assert_eq!(total,spec.hits as u32*spec.per_hit as u32+spec.final_payout as u32);
  assert_eq!(w.spawned_value,total);assert_eq!(w.wallet(),0);assert!(w.rock_depleted(0,0));
  w.begin_attack();assert_eq!(w.strike(0,&[spec],SHAPE),Strike::default());
  w.death();w.leave_scene(0,P);assert!(w.rock_depleted(0,0));
 }
}
#[test]fn cooldown_reentry_and_overlapping_region_copies_do_not_reset_rock(){
 let mut w=World::new();w.begin_attack();assert_eq!(w.strike(0,&[ROCK,ROCK],SHAPE).hits,1);
 w.begin_attack();assert_eq!(w.strike(0,&[ROCK],SHAPE).hits,0);
 ticks(&mut w,2,FAR);assert_eq!(w.strike(0,&[ROCK],SHAPE).hits,1);
 assert_eq!(w.hits_remaining(0,0),Some(3));w.leave_scene(0,P);
 ticks(&mut w,2,FAR);w.begin_attack();assert_eq!(w.strike(0,&[ROCK],SHAPE).hits,1);
 assert_eq!(w.hits_remaining(0,0),Some(2));
}
#[test]fn exact_rock_polygon_excludes_points_inside_only_bounding_box(){
 const NARROW:&[[i32;2]]=&[[0,0],[ONE,0],[0,ONE]];
 let r=RockSpec{polygons:&[NARROW],..ROCK};let poly=[[ONE*3/4,ONE*3/4],[ONE,ONE*3/4],[ONE,ONE]];
 let mut w=World::new();w.begin_attack();assert_eq!(w.strike(0,&[r],&poly).hits,0);
 assert_eq!(w.hits_remaining(0,0),None);
}
#[test]fn pickup_uses_post_lock_trigger_entry_not_stay_and_never_credits_twice(){
 let mut w=World::new();assert!(w.spawn_enemy(0,1,[0,0],enemy(1,[1,0,0])));
 ticks(&mut w,30,NEAR);assert_eq!(w.wallet(),0);assert_eq!(w.active(),1);
 ticks(&mut w,1,FAR);assert_eq!(ticks(&mut w,1,NEAR).collected,1);
 assert_eq!(w.wallet(),1);assert_eq!(w.active(),0);ticks(&mut w,30,NEAR);assert_eq!(w.wallet(),1);
 let mut w=World::new();w.spawn_enemy(0,2,[0,0],enemy(2,[1,0,0]));ticks(&mut w,14,FAR);
 assert_eq!(ticks(&mut w,1,NEAR).collected,1);assert_eq!(w.wallet(),1);
}
#[test]fn pool_overflow_defers_coins_without_early_wallet_credit_or_value_loss(){
 let mut w=World::new();let e=enemy(1,[70,0,0]);assert!(w.spawn_enemy(0,1,[0,0],e));
 assert!(!w.spawn_enemy(0,1,[0,0],e));ticks(&mut w,15,FAR);
 assert_eq!(w.active(),MAX_COINS);assert_eq!(w.pending_value(P),6);assert_eq!(w.spawned_value,64);assert_eq!(w.wallet(),0);
 assert_eq!(ticks(&mut w,1,NEAR).collected,64);ticks(&mut w,15,FAR);
 assert_eq!(w.pending_value(P),0);assert_eq!(ticks(&mut w,1,NEAR).collected,6);
 assert_eq!(w.wallet(),70);assert_eq!(w.spawned_value,70);assert_eq!(w.collected_value,70);
 assert_eq!(w.death(),70);assert_eq!(w.wallet(),0);assert!(!w.spawn_enemy(0,1,[0,0],e));
 w.reset_enemies(0);assert!(w.spawn_enemy(0,1,[0,0],e));
}
#[test]fn queue_capacity_failure_remains_retryable_and_scene_exit_discards_only_loose_value(){
 let mut w=World::new();
 for id in 1..=16 {assert!(w.spawn_enemy(0,id,[0,0],enemy(id,[1,0,0])));}
 assert!(!w.spawn_enemy(0,17,[0,0],enemy(17,[1,0,0])));assert_eq!(w.pending_value(P),16);
 ticks(&mut w,1,FAR);assert!(w.spawn_enemy(0,17,[0,0],enemy(17,[1,0,0])));
 assert_eq!(w.active(),16);assert_eq!(w.pending_value(P),1);w.leave_scene(1,P);assert_eq!(w.active(),16);
 w.leave_scene(0,P);assert_eq!(w.active(),0);assert_eq!(w.pending_value(P),0);assert_eq!(w.discarded_value,17);assert_eq!(w.wallet(),0);
}
#[test]fn denominations_wallet_cap_and_acid_never_invent_credit(){
 let mut w=World::new();w.spawn_enemy(0,1,[0,0],enemy(1,[1,1,1]));ticks(&mut w,15,FAR);
 let index=w.indexed_coins().find(|(_,c)|c.denomination==0).unwrap().0;
 assert!(w.acid(index,P));assert!(!w.acid(index,P));assert_eq!(w.wallet(),0);
 let p=Params{wallet_max:20,..P};let result=w.tick(0,p,NEAR,COVER,0,|_|[0;4]);
 assert_eq!(result.collected,30);assert_eq!(w.wallet(),20);assert_eq!(w.discarded_value,1);
}
#[test]fn authored_physics_clock_and_fling_are_deterministic(){
 let p=Params{coins:[CoinSpec{gravity:-45*ONE,..COIN};3],..P};
 let e=EnemySpec{fling:Fling{speed:[15*ONE,30*ONE],angle:[80*ONE,100*ONE],spread:[ONE/4;2]},..enemy(1,[2,0,0])};
 let mut a=World::new();let mut b=World::new();for w in [&mut a,&mut b]{w.spawn_enemy(0,1,[0,20*ONE],e);}
 for _ in 0..60 {a.tick(0,p,FAR,COVER,0,|_|[0;4]);b.tick(0,p,FAR,COVER,0,|_|[0;4]);}
 let pose=|w:&World|w.coins().map(|c|(c.x,c.y,c.vx,c.vy,c.age)).collect::<Vec<_>>();assert_eq!(pose(&a),pose(&b));
 let mut w=World::new();w.spawn_enemy(0,2,[0,20*ONE],enemy(2,[1,0,0]));
 for _ in 0..60 {w.tick(0,p,FAR,COVER,0,|_|[0;4]);}
 assert_eq!(w.coins().next().unwrap().vy,-(45*ONE/50)*50);
}
#[test]fn floor_bounce_settles_and_cached_support_removal_wakes_without_losing_coin(){
 let p=Params{coins:[CoinSpec{gravity:-45*ONE,..COIN};3],..P};
 let mut w=World::new();w.spawn_enemy(0,1,[0,3*ONE],enemy(1,[1,0,0]));let mut bounces=0;
 for _ in 0..1200 {bounces+=w.tick(0,p,FAR,COVER,1,|_|[-20*ONE,0,20*ONE,0]).bounces;}
 assert!(bounces>0);let c=w.coins().next().unwrap();assert!(c.grounded);assert_eq!((c.vx,c.vy),(0,0));assert_eq!(c.y,ONE/4);
 let y=c.y;for _ in 0..6 {w.tick(0,p,FAR,COVER,1,|_|[0;4]);}
 assert!(w.coins().next().unwrap().y<y);assert_eq!(w.active(),1);assert_eq!(w.wallet(),0);
}
#[test]fn full_pool_long_edge_scans_preserve_order_contacts_and_currency(){
 // Exercise both contact scans past many checkpoint boundaries. Irrelevant
 // geometry must leave the same deterministic result as the one-floor world.
 let p=Params{coins:[CoinSpec{gravity:-90*ONE,..COIN};3],..P};
 let spec=EnemySpec{fling:Fling{speed:[15*ONE,30*ONE],angle:[80*ONE,100*ONE],spread:[ONE/4;2]},..enemy(1,[MAX_COINS as u16,0,0])};
 let(mut small,mut large)=(World::new(),World::new());
 for w in [&mut small,&mut large]{assert!(w.spawn_enemy(0,1,[0,3*ONE],spec));}
 let floor=[-80*ONE,0,80*ONE,0];
 for _ in 0..120 {
  let a=small.tick(0,p,FAR,COVER,1,|_|floor);
  let b=large.tick(0,p,FAR,COVER,1024,|i|if i==511 {floor}else{[0;4]});
  assert_eq!(a,b);
  let pose=|w:&World|w.coins().map(|c|(c.x,c.y,c.vx,c.vy,c.age,c.grounded,c.landed)).collect::<Vec<_>>();
  assert_eq!(pose(&small),pose(&large));
 }
 assert_eq!(large.active(),MAX_COINS);assert_eq!(large.spawned_value,MAX_COINS as u32);
 assert_eq!(large.wallet(),0);assert_eq!(large.pending_value(p),0);
}
#[test]fn a_stalactite_rock_lands_is_never_taken_and_goes_after_its_lifetime(){
 let mut w=World::new();let floor=[-50*ONE,-2*ONE,50*ONE,-2*ONE];
 assert!(w.launch_rock(0,[0,0],60,17*ONE,5));
 let rock=*w.coins().next().unwrap();assert_eq!(rock.denomination,geo::ROCK|5);assert!(rock.vy>0&&rock.vx>0);
 let mut ticks=0;let mut landed_at=None;
 while w.active()>0 {
  // The Knight stands on the rock the whole time and takes nothing.
  let hero=w.coins().next().map_or(NEAR,|c|[c.x-ONE,c.y-ONE,c.x+ONE,c.y+ONE]);
  let t=w.tick(0,P,hero,COVER,1,|_|floor);assert_eq!(t.collected,0);
  if let Some(c)=w.coins().next() {assert!(c.y>floor[1]);if c.landed&&landed_at.is_none(){landed_at=Some(ticks);}}
  ticks+=1;assert!(ticks<60*30,"the rock never went");
 }
 assert!(ticks-landed_at.unwrap()>=570);assert_eq!((w.wallet(),w.spawned_value,w.collected_value),(0,0,0));
 // A full pool flings nothing.
 let mut w=World::new();for _ in 0..MAX_COINS {assert!(w.launch_rock(0,[0,0],90,ONE,0));}
 assert!(!w.launch_rock(0,[0,0],90,ONE,0));
}
#[test]fn a_geo_burst_takes_the_slots_of_stalactite_rocks_only_when_it_needs_them(){
 let mut w=World::new();
 for _ in 0..12 {assert!(w.launch_rock(0,[0,0],90,ONE,0));}
 let rocks=|w:&World|w.coins().filter(|c|c.denomination&geo::ROCK!=0).count();
 // A burst that fits the free slots leaves every rock alone.
 assert!(w.spawn_chest(0,7,[0,0],FLING,[(MAX_COINS-12)as u16,0,0]));
 ticks(&mut w,1,FAR);assert_eq!((rocks(&w),w.active(),w.pending_value(P)),(12,MAX_COINS,0));
 // One more coin takes a rock's slot rather than waiting for it to go.
 assert!(w.spawn_chest(0,8,[0,0],FLING,[5,0,0]));
 ticks(&mut w,1,FAR);assert_eq!((rocks(&w),w.active(),w.pending_value(P)),(7,MAX_COINS,0));
 // With no rock left, Geo waits in the pending queue as before.
 assert!(w.spawn_chest(0,9,[0,0],FLING,[10,0,0]));
 ticks(&mut w,1,FAR);assert_eq!((rocks(&w),w.pending_value(P)),(0,3));
}

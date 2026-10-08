#[path="../game/src/solid_occluder.rs"]mod solid_occluder;
use solid_occluder::Cache;
#[test]fn cached_convex_shape_translates_and_reflects() {
 let p=[(20,20),(300,35),(35,225),(315,240)];let mut c=Cache::new();let a=c.map(p).unwrap();
 let b=c.map(p.map(|(x,y)|(x-5,y-3))).unwrap();assert_eq!(b,[a[0]-5,a[1]-3,a[2]-5,a[3]-3]);assert_eq!(c.stats(),(1,1));
 assert!(c.map(p.map(|(x,y)|(320-x,y))).is_some());
 let mut changed=p;changed[3].0-=1;let before=c.stats().1;let _=c.map(changed);assert_eq!(c.stats().1,before+1);
}
#[test]fn offscreen_preparation_preserves_full_domain() {
 let p=[(20,20),(300,35),(35,225),(315,240)];let mut c=Cache::new();
 assert!(c.map(p.map(|(x,y)|(x+500,y))).is_none());assert_eq!(c.stats(),(0,1));
 let actual=c.map(p).unwrap();assert_eq!(c.stats(),(1,1));assert_eq!(actual,Cache::new().map(p).unwrap());
}
#[test]fn nonconvex_degenerate_and_illegal_input_fails_closed() {
 let mut c=Cache::new();
 assert!(c.map([(20,20),(300,20),(20,225),(40,40)]).is_none());
 assert!(c.map([(i16::MIN,0),(i16::MAX,0),(0,100),(100,100)]).is_none());
 assert!(c.map([(0,0),(320,0),(0,512),(320,512)]).is_none());
 assert!(c.map([(0,0);4]).is_none());
 let wide=[(0,0),(1023,0),(0,100),(1023,100)];assert!(c.map(wide).is_some());
 assert!(c.map(wide.map(|(x,y)|(x+1,y))).is_none());
}

#[test]fn shallow_rotation_uses_interior_vertex_strip() {
 let mut c=Cache::new();let r=c.map([(0,0),(320,12),(0,174),(320,186)]).unwrap();
 // The original regular nine rows missed this long interval between edges.
 assert!(r[3]-r[1]>=159);assert!(r[0]>=2 && r[2]<=319);
}

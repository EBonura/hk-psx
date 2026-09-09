//! Four independently prepared scenery banks. The active bank is immutable.
pub const BANKS:usize=4;
pub const PAGES:usize=5;
pub const CLUTS:usize=416;
// Current resident scenes consume1206 CLUTs. Keep their admitted namespace
// below the last legacy strip, whose space now holds independent Geo art.
pub const SCENE_CLUTS:usize=3*CLUTS;
/// Geo's independent 4bpp art/CLUT allocation in previously unused gaps.
/// Each tuple is x,y,width,height in VRAM halfwords, not texture pixels.
pub const GEO_RECTS:[(usize,usize,usize,usize);5]=[
    (320,464,64,16),(352,240,32,16),(352,485,32,27),(320,496,32,16),
    (352,32,32,64),
];
#[derive(Clone,Copy)]
struct Entry {region:Option<usize>,ready:bool,stamp:u32}
pub struct Banks {entries:[Entry;BANKS],active:Option<usize>,pending:Option<usize>,clock:u32}
impl Banks {
    pub const fn new()->Self {Self {entries:[Entry{region:None,ready:false,stamp:0};BANKS],active:None,pending:None,clock:0}}
    pub fn find(&self,region:usize)->Option<usize> {
        self.entries.iter().position(|e|e.region==Some(region)&&e.ready)
    }
    pub fn active(&self)->Option<usize> {self.active}
    pub fn pending(&self)->Option<(usize,usize)> {
        self.pending.map(|bank|(bank,self.entries[bank].region.unwrap()))
    }
    /// Retire an unrelated partial bank at a GPU-frame boundary. It was never
    /// ready or active; discarded contents cannot become selectable afterward.
    pub fn cancel_for_demand(&mut self,region:usize)->bool {
        let Some((bank,pending))=self.pending() else{return false;};
        if pending==region {return false;}
        assert_ne!(self.active,Some(bank));assert!(!self.entries[bank].ready);
        self.entries[bank]=Entry{region:None,ready:false,stamp:0};self.pending=None;true
    }
    /// Allocate only an unprotected bank; never cancel a partially uploaded one.
    pub fn begin(&mut self,region:usize,keep:&[usize])->Option<usize> {
        if self.pending.is_some() || self.find(region).is_some() {return None;}
        let bank=(0..BANKS).filter(|&i|self.active!=Some(i))
            .filter(|&i|self.entries[i].region.is_none_or(|r|!keep.contains(&r)))
            .max_by_key(|&i|if self.entries[i].region.is_none(){u32::MAX}else{self.clock.wrapping_sub(self.entries[i].stamp)})?;
        self.clock=self.clock.wrapping_add(1);
        self.entries[bank]=Entry{region:Some(region),ready:false,stamp:self.clock};
        self.pending=Some(bank);Some(bank)
    }
    pub fn finish(&mut self,bank:usize) {
        assert_eq!(self.pending,Some(bank));
        self.entries[bank].ready=true;self.pending=None;
    }
    pub fn activate(&mut self,region:usize)->Option<usize> {
        let bank=self.find(region)?;self.clock=self.clock.wrapping_add(1);
        self.entries[bank].stamp=self.clock;self.active=Some(bank);Some(bank)
    }
}
impl Default for Banks {fn default()->Self{Self::new()}}
/// Static texture page origin in VRAM halfwords; all twenty pages are available.
pub fn page_xy(bank:usize,page:usize)->(u16,u16) {
    assert!(bank<BANKS&&page<PAGES);let p=bank*PAGES+page;
    (384+(p%10) as u16*64,(p/10) as u16*256)
}
pub fn clut_xy(bank:usize,palette:usize)->(u16,u16) {
    assert!(bank<BANKS&&palette<CLUTS);
    match bank {
        // The font starts at row500. Overflow uses the free16-word strip
        // between its CLUT and the HUD palettes, never a framebuffer row.
        0 if palette<400=>((palette%20) as u16*16,480+(palette/20) as u16),
        0=>(336,480+(palette-400) as u16),
        1=>(320+(palette%4) as u16*16,256+(palette/4) as u16),
        2=>(320+(palette%4) as u16*16,360+(palette/4) as u16),
        _=>(352+(palette%2) as u16*16,32+(palette/2) as u16),
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn active_and_requested_banks_survive_replacement() {
        let mut b=Banks::new();let a=b.begin(10,&[]).unwrap();
        assert_eq!(b.activate(10),None);b.finish(a);assert_eq!(b.activate(10),Some(a));
        let n=b.begin(11,&[]).unwrap();assert_eq!(b.begin(12,&[]),None);b.finish(n);
        let c=b.begin(12,&[11]).unwrap();b.finish(c);
        let d=b.begin(14,&[11,12]).unwrap();b.finish(d);
        assert_eq!(b.begin(13,&[11,12,14]),None);
        let replacement=b.begin(13,&[11,14]).unwrap();assert_eq!(replacement,c);
        assert_eq!(b.find(10),Some(a));assert_eq!(b.find(11),Some(n));assert_eq!(b.find(13),None);
        b.finish(replacement);assert_eq!(b.activate(13),Some(replacement));
    }
    fn loaded(b:&mut Banks,region:usize)->usize {
        let bank=b.begin(region,&[]).unwrap();b.finish(bank);bank
    }
    #[test] fn unfinished_upload_is_never_visible_or_active_and_cannot_be_replaced() {
        let mut b=Banks::new();let current=loaded(&mut b,10);b.activate(10);
        let next=b.begin(11,&[]).unwrap();
        assert_eq!(b.find(11),None);assert_eq!(b.activate(11),None);
        assert_eq!(b.active(),Some(current));assert_eq!(b.pending(),Some((next,11)));
        assert_eq!(b.begin(12,&[]),None);assert_eq!(b.begin(11,&[]),None);
        assert_eq!(b.pending(),Some((next,11)));assert_eq!(b.find(10),Some(current));
        b.finish(next);assert_eq!(b.find(11),Some(next));assert_eq!(b.pending(),None);
        assert_eq!(b.activate(11),Some(next));assert_eq!(b.active(),Some(next));
    }
    #[test] fn ready_reentry_preserves_pixels_while_another_bank_uploads() {
        let mut b=Banks::new();let first=loaded(&mut b,10);b.activate(10);
        let second=loaded(&mut b,11);b.activate(11);
        let uploading=b.begin(12,&[10]).unwrap();
        assert_eq!(b.activate(10),Some(first));assert_eq!(b.find(11),Some(second));
        assert_eq!(b.pending(),Some((uploading,12)));
        assert_eq!(b.begin(10,&[]),None);assert_eq!(b.begin(13,&[]),None);
        b.finish(uploading);assert_eq!(b.active(),Some(first));
        assert_eq!(b.activate(11),Some(second));assert_eq!(b.pending(),None);
    }
    #[test] fn reentry_refreshes_recency_and_eviction_invalidates_old_region_immediately() {
        let mut b=Banks::new();let first=loaded(&mut b,10);b.activate(10);
        let recent=loaded(&mut b,11);let old=loaded(&mut b,12);loaded(&mut b,13);
        b.activate(11);b.activate(10);
        let replacement=b.begin(14,&[]).unwrap();assert_eq!(replacement,old);
        assert_eq!(b.find(12),None);assert_eq!(b.activate(12),None);
        assert_eq!(b.active(),Some(first));assert_eq!(b.find(11),Some(recent));
        assert_eq!(b.find(14),None);b.finish(replacement);
        assert_eq!(b.activate(14),Some(replacement));
    }
    #[test] fn exhausted_protected_banks_leave_all_readiness_and_active_state_unchanged() {
        let mut b=Banks::new();let first=loaded(&mut b,10);b.activate(10);
        let a=loaded(&mut b,11);let c=loaded(&mut b,12);let d=loaded(&mut b,13);
        for _ in 0..10 {assert_eq!(b.begin(14,&[11,12,13]),None);}
        assert_eq!(b.active(),Some(first));assert_eq!(b.pending(),None);
        assert_eq!([b.find(10),b.find(11),b.find(12),b.find(13)],
            [Some(first),Some(a),Some(c),Some(d)]);
        let replacement=b.begin(14,&[11,13]).unwrap();assert_eq!(replacement,c);
    }
    #[test] fn urgent_cancellation_invalidates_only_unrelated_inactive_partial_bank() {
        let mut b=Banks::new();let current=loaded(&mut b,10);b.activate(10);
        let ready=loaded(&mut b,11);let partial=b.begin(12,&[11]).unwrap();
        assert!(!b.cancel_for_demand(12));assert_eq!(b.pending(),Some((partial,12)));
        assert!(b.cancel_for_demand(13));assert_eq!(b.pending(),None);
        assert_eq!(b.find(12),None);assert_eq!(b.activate(12),None);
        assert_eq!(b.active(),Some(current));assert_eq!(b.find(10),Some(current));
        assert_eq!(b.find(11),Some(ready));assert!(!b.cancel_for_demand(13));
        let target=b.begin(13,&[11]).unwrap();assert_eq!(b.find(13),None);
        b.finish(target);assert_eq!(b.activate(13),Some(target));
        // A ready old room can later return, but canceled12 must upload anew.
        b.activate(10);let retry=b.begin(12,&[11,13]).unwrap();
        assert_eq!(b.activate(12),None);b.finish(retry);assert_eq!(b.activate(12),Some(retry));
    }
    #[test] fn urgent_ready_reentry_can_release_unrelated_upload_without_invalidating_either_ready_bank() {
        let mut b=Banks::new();let a=loaded(&mut b,1);b.activate(1);let dest=loaded(&mut b,2);
        b.begin(3,&[2]).unwrap();assert!(b.cancel_for_demand(2));
        assert_eq!(b.find(1),Some(a));assert_eq!(b.find(2),Some(dest));
        assert_eq!(b.activate(2),Some(dest));assert_eq!(b.find(3),None);
    }
    #[test] fn all_texture_and_palette_origins_are_ps1_encodable() {
        for bank in 0..BANKS {
            for page in 0..PAGES {
                let(x,y)=page_xy(bank,page);assert_eq!(x%64,0);assert!(y==0||y==256);
                let tpage=(x/64)|((y/256)<<4);
                assert_eq!(((tpage&15)*64,((tpage>>4)&1)*256),(x,y));
            }
            for palette in 0..CLUTS {
                let(x,y)=clut_xy(bank,palette);assert_eq!(x%16,0);
                let clut=(x/16)|(y<<6);
                assert_eq!(((clut&63)*16,clut>>6),(x,y));
            }
        }
    }
    #[test] fn all_vram_allocations_are_disjoint() {
        let mut used=std::vec![false;1024*512];
        let mut claim=|x:usize,y:usize,w:usize,h:usize| {
            assert!(x+w<=1024&&y+h<=512);
            for yy in y..y+h {for xx in x..x+w {assert!(!used[yy*1024+xx],"overlap at {xx},{yy}");used[yy*1024+xx]=true;}}
        };
        claim(0,0,320,480);claim(320,0,32,256);
        claim(352,0,4,32);claim(352,480,16,2);
        // Source SOUL frame, masked fill and eyes; their three CLUTs remain
        // below bank 0 and outside the vertical scenery-palette strips.
        claim(356,0,28,32);claim(352,482,16,3);
        // Shared subtractive black-mask fade CLUT.
        claim(320,480,16,1);
        // Original dialogue font strips and separate grayscale CLUT.
        claim(0,500,320,12);claim(320,481,16,1);
        for (x,y,w,h) in GEO_RECTS {claim(x,y,w,h);}
        for bank in 0..BANKS {
            for page in 0..PAGES {let(x,y)=page_xy(bank,page);claim(x as usize,y as usize,64,256);}
            for palette in 0..CLUTS {if bank*CLUTS+palette<SCENE_CLUTS {
                let(x,y)=clut_xy(bank,palette);claim(x as usize,y as usize,16,1);
            }}
        }
    }
    #[test] fn soul_hud_slots_fit_reserved_strip_and_encodable_uvs() {
        let mut used=[false;28*32];
        // Max pixel widths admitted by cook_hud: frame40, fill24, eyes16.
        for (x,w,h) in [(356,40,24),(366,24,32),(372,16,32)] {
            let words=(w+3)/4;assert!(x>=356&&x+words<=384);
            let u=(x-320)*4;assert!(u+w<=256&&h<=32);
            for y in 0..h {for xx in x..x+words {
                let p=y*28+xx-356;assert!(!used[p]);used[p]=true;
            }}
        }
        for y in 482..=484 {let clut=(352/16)|(y<<6);assert_eq!(((clut&63)*16,clut>>6),(352,y));}
    }
}

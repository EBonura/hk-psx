//! Checked HKSCNE01/HKSCNE02 resident scene. Version02 omits bootstrap-only
//! atlas bytes while retaining logical page/CLUT counts and all runtime data.
//! Validation happens at scene admission;
//! spatial region lookup only constructs immutable views of admitted bytes.
use crate::{Error,Room,u16_at,u32_at,HAS_ALPHA_COVERS,MAX_STREAM_BYTES};
const HEADER:usize=128;
const DESCRIPTOR:usize=40;
/// Per-texture attributes appended after the reference arrays (offset word at
/// 104, zero when absent): flags byte, three zero bytes, black core [x,y,w,h],
/// opaque core [x,y,w,h]. Derived by the packer from the final atlas words.
pub const ATTRIBUTE_STRIDE:usize=12;
#[derive(Clone,Copy)]
pub struct Scene<'a>{bytes:&'a[u8],offsets:[usize;10],counts:[usize;9],stream_bytes:usize,attributes:usize}
impl<'a> Scene<'a>{
    pub fn parse(bytes:&'a[u8])->Result<Self,Error>{
        let mut validation=SceneValidation::new(bytes)?;
        validation.step(bytes,usize::MAX)?;
        Ok(validation.view(bytes))
    }
    fn header(bytes:&'a[u8])->Result<Self,Error>{
        if bytes.len()<HEADER || (&bytes[..8]!=b"HKSCNE01" && &bytes[..8]!=b"HKSCNE02") || u32_at(bytes,52)!=HAS_ALPHA_COVERS
            || bytes[56..64].iter().chain(bytes[108..128].iter()).any(|&b|b!=0){return Err(Error::Header);}
        if u32_at(bytes,48)as usize!=bytes.len(){return Err(Error::Truncated);}
        // rooms, textures, draw/frame/clip/edge pools, palettes, pages, stream
        let counts=core::array::from_fn(|i|u32_at(bytes,12+i*4)as usize);
        for (n,limit)in counts.iter().zip([128,2048,65535,65535,65535,65535,1536,20,MAX_STREAM_BYTES]){
            if *n>limit{return Err(Error::Limit);}
        }
        if counts[0]==0 || counts[1]==0 || counts[6]==0{return Err(Error::Header);}
        // Palette IDs are global and may be fewer than texture entries.
        if counts[6]>counts[1]{return Err(Error::Reference);}
        let stream_bytes=counts[8];
        let offsets=core::array::from_fn(|i|u32_at(bytes,64+i*4)as usize);
        let compact=&bytes[..8]==b"HKSCNE02";
        let lengths=[counts[1]*16,counts[2]*44,counts[3]*20,counts[4]*16,counts[5]*16,
            if compact {0}else{counts[6]*32},if compact {0}else{counts[7]*32768},stream_bytes,counts[0]*DESCRIPTOR];
        let mut end=HEADER;
        for i in 0..9{
            let start=end.checked_add(3).ok_or(Error::Truncated)?&!3;
            if offsets[i]!=start || start>bytes.len() || bytes[end..start].iter().any(|&b|b!=0){return Err(Error::Header);}
            end=start.checked_add(lengths[i]).ok_or(Error::Truncated)?;
            if end>bytes.len(){return Err(Error::Truncated);}
        }
        if offsets[9]!=end{return Err(Error::Header);}
        let attributes=u32_at(bytes,104)as usize;
        if attributes!=0 && (attributes<offsets[9] || attributes%4!=0
            || attributes.checked_add(counts[1]*ATTRIBUTE_STRIDE).is_none_or(|end|end>bytes.len())){return Err(Error::Header);}
        Ok(Self{bytes,offsets,counts,stream_bytes,attributes})
    }
    /// # Safety
    /// Identical bytes must have passed `Scene::parse` or a completed
    /// `SceneValidation`, and remained immutable
    /// since admission, including every reference array and global pool.
    pub unsafe fn validated_view(bytes:&'a[u8])->Self{
        Self{bytes,offsets:core::array::from_fn(|i|u32_at(bytes,64+i*4)as usize),
            counts:core::array::from_fn(|i|u32_at(bytes,12+i*4)as usize),stream_bytes:u32_at(bytes,44)as usize,
            attributes:u32_at(bytes,104)as usize}
    }
    /// True when the packer appended per-texture attributes.
    pub fn has_texture_attributes(&self)->bool{self.attributes!=0}
    fn attribute(&self,texture:usize)->Option<&'a[u8]>{
        (self.attributes!=0 && texture<self.counts[1]).then(||{
            let at=self.attributes+texture*ATTRIBUTE_STRIDE;&self.bytes[at..at+ATTRIBUTE_STRIDE]})
    }
    /// Bit 0: binary black mask palette; bit 1: solid word-1 texels. Zero for
    /// streamed animation textures and for banks without attributes.
    pub fn texture_flags(&self,texture:usize)->u8{self.attribute(texture).map_or(0,|a|a[0])}
    /// Largest all-opaque-black texel rectangle [x,y,w,h], or zeros.
    pub fn black_core(&self,texture:usize)->[u8;4]{self.attribute(texture).map_or([0;4],|a|[a[4],a[5],a[6],a[7]])}
    /// Largest nontransparent STP-clear texel rectangle [x,y,w,h], or zeros.
    pub fn opaque_core(&self,texture:usize)->[u8;4]{self.attribute(texture).map_or([0;4],|a|[a[8],a[9],a[10],a[11]])}
    pub fn id(&self)->usize{u32_at(self.bytes,8)as usize}
    pub fn room_count(&self)->usize{self.counts[0]}
    pub fn texture_count(&self)->usize{self.counts[1]}
    pub fn draw_pool_count(&self)->usize{self.counts[2]}
    pub fn palette_count(&self)->usize{self.counts[6]}
    pub fn page_count(&self)->usize{self.counts[7]}
    pub fn byte_len(&self)->usize{self.bytes.len()}
    pub fn chunk_id(&self,index:usize)->Option<usize>{
        (index<self.room_count()).then(||u32_at(self.bytes,self.offsets[8]+index*DESCRIPTOR)as usize)
    }
    pub fn room_by_chunk(&self,chunk_id:usize)->Option<Room<'a>>{
        (0..self.room_count()).find(|&i|self.chunk_id(i)==Some(chunk_id)).and_then(|i|self.room(i))
    }
    pub fn room(&self,index:usize)->Option<Room<'a>>{
        if index>=self.room_count(){return None;}
        let p=self.offsets[8]+index*DESCRIPTOR;
        Some(Room{bytes:self.bytes,counts:[self.counts[7],self.counts[1],u32_at(self.bytes,p+4)as usize,
            u32_at(self.bytes,p+8)as usize,u32_at(self.bytes,p+12)as usize,u32_at(self.bytes,p+16)as usize],
            offsets:core::array::from_fn(|i|self.offsets[i]),stream_bytes:self.stream_bytes,
            references:Some(core::array::from_fn(|i|u32_at(self.bytes,p+20+i*4)as usize))})
    }
}

#[derive(Clone,Copy,PartialEq,Eq)]
enum Phase { Textures, Descriptor, Duplicate, Section, Record, Tail, Done }

/// Non-borrowing, bounded scene-admission cursor. `new` checks the fixed header;
/// each work unit checks one texture, descriptor, duplicate-ID comparison,
/// section layout, local record, or phase transition. No call scans all rooms.
///
/// This cursor never publishes a view. Completion covers the bytes observed
/// across calls: the loader must keep the entire arena private and unchanged
/// from `new` through completion and for the lifetime of any admitted Scene.
/// Length is checked on every call, even after completion. Budget zero makes
/// no progress. The parser does not replace the loader's payload checksums.
pub struct SceneValidation {
    offsets:[usize;10], counts:[usize;9], stream_bytes:usize, byte_len:usize, attributes:usize,
    phase:Phase, room_index:usize, section:usize, index:usize,
    next:usize, chunk:u32, local_counts:[usize;4], references:[usize;4],
}
impl SceneValidation {
    pub fn new(bytes:&[u8])->Result<Self,Error>{
        let scene=Scene::header(bytes)?;
        Ok(Self{offsets:scene.offsets,counts:scene.counts,
            stream_bytes:scene.stream_bytes,byte_len:bytes.len(),attributes:scene.attributes,
            phase:Phase::Textures,room_index:0,section:0,index:0,
            next:scene.offsets[9],chunk:0,local_counts:[0;4],references:[0;4]})
    }
    fn view<'a>(&self,bytes:&'a[u8])->Scene<'a>{
        Scene{bytes,offsets:self.offsets,counts:self.counts,stream_bytes:self.stream_bytes,attributes:self.attributes}
    }
    fn room<'a>(&self,bytes:&'a[u8])->Room<'a>{
        Room{bytes,counts:[self.counts[7],self.counts[1],self.local_counts[0],
            self.local_counts[1],self.local_counts[2],self.local_counts[3]],
            offsets:core::array::from_fn(|i|self.offsets[i]),
            stream_bytes:self.stream_bytes,references:Some(self.references)}
    }
    pub fn step(&mut self,bytes:&[u8],mut work_budget:usize)->Result<bool,Error>{
        if bytes.len()!=self.byte_len{return Err(Error::Truncated);}
        while work_budget!=0 && self.phase!=Phase::Done {
            work_budget-=1;
            match self.phase {
                Phase::Textures=>{
                    if self.index==self.counts[1]{
                        self.phase=Phase::Descriptor;self.index=0;continue;
                    }
                    let room=self.room(bytes);
                    if room.texture(self.index).palette as usize>=self.counts[6]{return Err(Error::Reference);}
                    room.validate_record(0,self.index)?;self.index+=1;
                }
                Phase::Descriptor=>{
                    if self.room_index==self.counts[0]{self.phase=Phase::Tail;continue;}
                    let p=self.offsets[8]+self.room_index*DESCRIPTOR;
                    self.chunk=u32_at(bytes,p);
                    if self.chunk==0 || u32_at(bytes,p+36)!=0{return Err(Error::Header);}
                    self.local_counts=core::array::from_fn(|i|u32_at(bytes,p+4+i*4)as usize);
                    self.references=core::array::from_fn(|i|u32_at(bytes,p+20+i*4)as usize);
                    self.index=0;self.phase=Phase::Duplicate;
                }
                Phase::Duplicate=>{
                    if self.index==self.room_index{
                        self.section=0;self.phase=Phase::Section;continue;
                    }
                    if u32_at(bytes,self.offsets[8]+self.index*DESCRIPTOR)==self.chunk{return Err(Error::Reference);}
                    self.index+=1;
                }
                Phase::Section=>{
                    if self.section==4{
                        self.room_index+=1;self.phase=Phase::Descriptor;continue;
                    }
                    let count=self.local_counts[self.section];
                    if count>[1024,2048,128,1024][self.section]{return Err(Error::Limit);}
                    let start=self.references[self.section];
                    let aligned=self.next.checked_add(3).ok_or(Error::Truncated)?&!3;
                    if start!=aligned || start>bytes.len() || bytes[self.next..start].iter().any(|&b|b!=0){return Err(Error::Header);}
                    self.next=start.checked_add(count.checked_mul(2).ok_or(Error::Truncated)?).ok_or(Error::Truncated)?;
                    if self.next>bytes.len(){return Err(Error::Truncated);}
                    self.index=0;self.phase=Phase::Record;
                }
                Phase::Record=>{
                    if self.index==self.local_counts[self.section]{
                        self.section+=1;self.phase=Phase::Section;continue;
                    }
                    let start=self.references[self.section];
                    if u16_at(bytes,start+self.index*2)as usize>=self.counts[self.section+2]{return Err(Error::Reference);}
                    self.room(bytes).validate_record(self.section+1,self.index)?;
                    self.index+=1;
                }
                Phase::Tail=>{
                    let mut end=self.next;
                    if self.attributes!=0 {
                        // Attributes follow the last reference array; their padding
                        // bytes and the flag byte's reserved neighbours are zero.
                        let start=end.checked_add(3).ok_or(Error::Truncated)?&!3;
                        if start!=self.attributes || bytes[end..start].iter().any(|&b|b!=0){return Err(Error::Header);}
                        end=start+self.counts[1]*ATTRIBUTE_STRIDE;
                        for texture in 0..self.counts[1]{
                            let a=&bytes[start+texture*ATTRIBUTE_STRIDE..][..ATTRIBUTE_STRIDE];
                            if a[0]>3 || a[1]|a[2]|a[3]!=0 {return Err(Error::Header);}
                        }
                    }
                    if (end.checked_add(3).ok_or(Error::Truncated)?&!3)!=bytes.len()
                        || bytes[end..].iter().any(|&b|b!=0){return Err(Error::Truncated);}
                    self.phase=Phase::Done;
                }
                Phase::Done=>unreachable!(),
            }
        }
        Ok(self.phase==Phase::Done)
    }
}
#[cfg(test)]
mod tests;

use hk_unity::{Value,playmaker::{enabled,action_fields_with_plain_strings,Fields,field}};
use hk_format::fsm::{ENABLED,UPDATE,NO_EVENT};
use hk_format::fsm::opcode::*;
const OWNER:i32=1;
const CAMERA:i32=2;
type Result<T> = std::result::Result<T,Box<dyn std::error::Error>>;
fn text(v:Option<&Value>)->Result<String>{v.and_then(Value::str).ok_or_else(||"missing string".into())}
fn get<'a>(f:&'a Fields,n:&str)->Result<&'a Value>{field(f,n).ok_or_else(||format!("missing field {n}").into())}
// Additional serialized forms used by the original Crawler actions. Keep the
// legacy Unity decoder unchanged, so existing cook fingerprints do not drift.
pub fn crawler_fields(data:&Value,index:usize)->Result<Fields>{
    let mut fields=action_fields_with_plain_strings(data,index,true)?;
    let list=|key|data.get(key).and_then(Value::list).ok_or("parameter list");
    let starts=list("actionStartIndex")?;let names=list("actionNames")?;
    let parameters=list("paramName")?;let kinds=list("paramDataType")?;
    let positions=list("paramDataPos")?;let sizes=list("paramByteDataSize")?;
    if [kinds.len(),positions.len(),sizes.len()].iter().any(|n|*n!=parameters.len()){return Err("parameter lengths".into());}
    let start=usize::try_from(starts.get(index).and_then(Value::int).ok_or("parameter start")?)?;
    let end=if index+1<names.len(){usize::try_from(starts.get(index+1).and_then(Value::int).ok_or("parameter end")?)?}else{parameters.len()};
    if start>end||end>parameters.len(){return Err("parameter bounds".into());}
    for i in start..end{
        let name=text(parameters.get(i))?;let kind=kinds[i].int().ok_or("parameter kind")?;
        let pos=usize::try_from(positions[i].int().ok_or("parameter offset")?)?;
        let size=usize::try_from(sizes[i].int().ok_or("parameter size")?)?;
        let value=if kind==2||kind==7{
            if size!=4{return Err("scalar size".into());}
            let raw=list("byteData")?.get(pos..pos.checked_add(4).ok_or("scalar overflow")?).ok_or("scalar bounds")?;
            let mut b=[0;4];for (i,v) in raw.iter().enumerate(){b[i]=u8::try_from(v.int().ok_or("scalar byte")?)?;}
            if kind==2{let f=f32::from_le_bytes(b);if !f.is_finite(){return Err("nonfinite scalar".into());}Value::F32(f)}else{Value::Int(i32::from_le_bytes(b) as i64)}
        }else if size==0&&[15,17,28].contains(&kind){
            let key=match kind{15=>"fsmFloatParams",17=>"fsmBoolParams",_=>"fsmVector3Params"};
            list(key)?.get(pos).cloned().ok_or("parameter table index")?
        }else{continue};
        if name.is_empty(){return Err("unnamed Crawler parameter".into());}
        fields.retain(|(n,_)|*n!=name);fields.push((name,value));
    }
    Ok(fields)
}
fn validate_switch(data:&Value,index:usize)->Result<()> {
    let list=|key|data.get(key).and_then(Value::list).ok_or("array metadata missing");
    let starts=list("actionStartIndex")?;let names=list("actionNames")?;
    let params=list("paramName")?;let kinds=list("paramDataType")?;let positions=list("paramDataPos")?;
    if params.len()!=kinds.len()||params.len()!=positions.len(){return Err("parameter length mismatch".into());}
    let start=usize::try_from(starts.get(index).and_then(Value::int).ok_or("start")?)?;
    let end=if index+1<names.len(){usize::try_from(starts.get(index+1).and_then(Value::int).ok_or("end")?)?}else{params.len()};
    if start>=end||end>params.len(){return Err("parameter range".into());}
    let sizes=list("arrayParamSizes")?;let types=list("arrayParamTypes")?;
    let mut cursor=start+1;let mut counts=vec![];
    for (name,kind,ty) in [("lessThan",15,"HutongGames.PlayMaker.FsmFloat"),("sendEvent",23,"HutongGames.PlayMaker.FsmEvent")] {
        if cursor>=end||params[cursor].str().as_deref()!=Some(name)||kinds[cursor].int()!=Some(12){return Err("array marker".into());}
        let array=usize::try_from(positions[cursor].int().ok_or("array index")?)?;
        let count=usize::try_from(sizes.get(array).and_then(Value::int).ok_or("array length")?)?;
        if count==0||types.get(array).and_then(Value::str).as_deref()!=Some(ty){return Err("array type".into());}
        cursor+=1;
        let finish=cursor.checked_add(count).ok_or("array range")?;
        if finish>end{return Err("array truncated".into());}
        for p in cursor..finish{if params[p].str().as_deref()!=Some("")||kinds[p].int()!=Some(kind){return Err("array element type".into());}}
        counts.push(count);cursor=finish;
    }
    if counts[0]!=counts[1]||cursor+1!=end||params[cursor].str().as_deref()!=Some("everyFrame")||kinds[cursor].int()!=Some(1){return Err("switch schema".into());}
    Ok(())
}
fn validate_bool_array(data:&Value,index:usize)->Result<()> {
    let list=|key|data.get(key).and_then(Value::list).ok_or("boolean array metadata");
    let starts=list("actionStartIndex")?;let names=list("actionNames")?;
    let parameters=list("paramName")?;let kinds=list("paramDataType")?;let positions=list("paramDataPos")?;
    if parameters.len()!=kinds.len()||parameters.len()!=positions.len(){return Err("boolean array parameter lengths".into());}
    let start=usize::try_from(starts.get(index).and_then(Value::int).ok_or("array start")?)?;
    let end=if index+1<names.len(){usize::try_from(starts.get(index+1).and_then(Value::int).ok_or("array end")?)?}else{parameters.len()};
    if start>=end||end>parameters.len()||parameters[start].str().as_deref()!=Some("boolVariables")||kinds[start].int()!=Some(12){return Err("boolean array marker".into());}
    let array=usize::try_from(positions[start].int().ok_or("array index")?)?;
    let count=usize::try_from(list("arrayParamSizes")?.get(array).and_then(Value::int).ok_or("array length")?)?;
    if count==0||count>13||list("arrayParamTypes")?.get(array).and_then(Value::str).as_deref()!=Some("HutongGames.PlayMaker.FsmBool")||start+count+4!=end{return Err("boolean array shape".into());}
    for at in start+1..start+count+1{
        if parameters[at].str().as_deref()!=Some("")||kinds[at].int()!=Some(17){return Err("boolean array element".into());}
    }
    for (offset,name,kind) in [(1,"sendEvent",23),(2,"storeResult",17),(3,"everyFrame",1)]{
        let at=start+count+offset;
        if parameters[at].str().as_deref()!=Some(name)||kinds[at].int()!=Some(kind){return Err("boolean array trailing fields".into());}
    }
    Ok(())
}
struct Tables { paired_chase:bool, variables:Vec<(String,i32,u8)>, declarations:Vec<(String,Value,u8)>, constants:Vec<i32>, events:Vec<String>, operands:Vec<(u8,u16)> }
impl Tables {
    fn event(&mut self,name:String)->u16{
        if name.is_empty(){return NO_EVENT;}
        if let Some(i)=self.events.iter().position(|s|*s==name){return i as u16;}
        let i=self.events.len();self.events.push(name);i as u16
    }
    fn scalar(v:&Value,kind:u8)->Result<i32>{
        if kind==1{return match v {Value::Bool(b)=>Ok(*b as i32),Value::Int(n) if *n==0||*n==1=>Ok(*n as i32),_=>Err("typed boolean required".into())};}
        if kind==3{return v.int().and_then(|n|i32::try_from(n).ok()).ok_or_else(||"integer range".into());}
        let n=v.float().ok_or("float value")?*65536.0;
        if !n.is_finite() || n<i32::MIN as f64 || n>i32::MAX as f64{return Err("float range".into());}
        Ok(n.round() as i32)
    }
    fn variable(&mut self,name:String)->Result<u16>{
        if let Some(i)=self.variables.iter().position(|(s,_,_)|*s==name){return Ok(i as u16);}
        let (_,value,kind)=self.declarations.iter().find(|(s,_,_)|*s==name).ok_or_else(||format!("undeclared {name}"))?;
        let initial=if *kind==3 && value.is_map(){0}else{Self::scalar(value,*kind)?};
        let i=self.variables.len();self.variables.push((name,initial,*kind));Ok(i as u16)
    }
    fn operand(&mut self,v:&Value,kind:u8)->Result<()> {
        let pair=if v.get("useVariable").is_some_and(Value::truthy){
            let name=text(v.get("name"))?;
            if name.is_empty(){(4,0)}else{
                let declared=self.declarations.iter().find(|(n,_,_)|*n==name).ok_or("undeclared variable")?.2;
                if declared!=kind{return Err("variable operand type mismatch".into());}
                (1,self.variable(name)?)
            }
        }else{
            let value=v.get("value").unwrap_or(v);
            let n=Self::scalar(value,kind)?;
            let index=if let Some(i)=self.constants.iter().position(|x|*x==n){i}else{let i=self.constants.len();self.constants.push(n);i};
            (0,index as u16)
        };
        self.operands.push(pair);Ok(())
    }
    fn arg(&mut self,f:&Fields,n:&str,kind:u8)->Result<()>{self.operand(get(f,n)?,kind)}
    fn ev(&mut self,f:&Fields,n:&str)->Result<()>{let e=self.event(text(Some(get(f,n)?))?);self.operands.push((2,e));Ok(())}
    fn object(&mut self,v:&Value)->Result<()> {
        if v.get("ownerOption").and_then(Value::int)==Some(0){self.operands.push((3,OWNER as u16));return Ok(());}
        let object=v.get("gameObject").unwrap_or(v);
        if !object.get("useVariable").is_some_and(Value::truthy){return Err("literal object unsupported".into());}
        let name=text(object.get("name"))?;
        if self.paired_chase&&name=="Hero"{self.operands.push((3,3));return Ok(());}
        if self.paired_chase&&name=="Self"{self.operand(object,3)?;return Ok(());}
        if self.declarations.iter().find(|(n,_,_)|*n==name).map(|(_,_,k)|*k)!=Some(3){return Err("object variable declaration".into());}
        let role=match name.as_str(){"Self"=>OWNER,"Camera"=>CAMERA,_=>return Err(format!("unsupported object {name}").into())};
        self.operands.push((3,role as u16));Ok(())
    }
    fn action(&mut self,name:&str,f:&Fields)->Result<(u16,u8)> {
        let mut flags=ENABLED|if field(f,"everyFrame").is_some_and(Value::truthy){UPDATE}else{0};
        let opcode=match name {
            "GetOwner"=>{self.arg(f,"storeGameObject",3)?;GET_OWNER}
            "FindGameObject"=>{
                if ["withTag","objectName"].iter().any(|n|field(f,n).and_then(|v|v.get("useVariable")).is_some_and(Value::truthy)){return Err("dynamic object query unsupported".into());}
                if text(get(f,"withTag")?.get("value"))?!="MainCamera" || !text(get(f,"objectName")?.get("value"))?.is_empty(){return Err("unsupported FindGameObject query".into());}
                self.operands.push((3,CAMERA as u16));self.arg(f,"store",3)?;FIND_OBJECT
            }
            "BoolTest"=>{self.arg(f,"boolVariable",1)?;self.ev(f,"isTrue")?;self.ev(f,"isFalse")?;BOOL_TEST}
            "Tk2dPlayAnimation"=>{
                self.object(get(f,"gameObject")?)?;
                let library=get(f,"animLibName")?;
                if library.get("useVariable").is_some_and(Value::truthy)||!text(Some(library.get("value").unwrap_or(library)))?.is_empty(){return Err("animation library override unsupported".into());}
                let clip=get(f,"clipName")?;
                if clip.get("useVariable").is_some_and(Value::truthy){return Err("dynamic animation clip unsupported".into());}
                let clip=match text(Some(clip.get("value").unwrap_or(clip)))?.as_str(){"Idle"=>0,"Startle"=>1,"Chase"=>2,_=>return Err("unsupported Vengefly animation clip".into())};
                self.operand(&Value::Int(clip),3)?;PLAY_ANIMATION
            }
            "Tk2dPlayFrame"=>{self.object(get(f,"gameObject")?)?;self.arg(f,"frame",3)?;PLAY_FRAME}
            "Tk2dWatchAnimationEvents"=>{
                self.object(get(f,"gameObject")?)?;
                if !text(Some(get(f,"animationTriggerEvent")?))?.is_empty(){return Err("animation trigger event unsupported".into());}
                self.ev(f,"animationCompleteEvent")?;flags|=UPDATE;WATCH_ANIMATION
            }
            "SetAudioPitch"=>{self.object(get(f,"gameObject")?)?;self.arg(f,"pitch",2)?;SET_AUDIO_PITCH}

            "IdleBuzz"=>{
                self.object(get(f,"gameObject")?)?;
                for name in ["waitMin","waitMax","speedMax","accelerationMax","roamingRange"]{self.arg(f,name,2)?;}
                flags|=UPDATE;IDLE_BUZZ
            }
            "ChaseObject"=>{
                self.object(get(f,"gameObject")?)?;self.object(get(f,"target")?)?;
                for name in ["targetSpread","spreadResetTimeMin","spreadResetTimeMax"]{
                    let value=get(f,name)?;
                    if value.get("useVariable").is_some_and(Value::truthy)||value.get("value").and_then(Value::float)!=Some(0.0){return Err("ChaseObject spread unsupported".into());}
                }
                self.arg(f,"speedMax",2)?;self.arg(f,"acceleration",2)?;flags|=UPDATE;CHASE_OBJECT
            }

            "FaceObject"=>{
                self.object(get(f,"objectA")?)?;self.object(get(f,"objectB")?)?;
                self.arg(f,"spriteFacesRight",1)?;self.arg(f,"playNewAnimation",1)?;
                let clip=get(f,"newAnimationClip")?;
                if clip.get("useVariable").is_some_and(Value::truthy)||!text(clip.get("value"))?.is_empty()||get(f,"playNewAnimation")?.truthy(){return Err("FaceObject animated subset unsupported".into());}
                self.operand(&Value::Int(0),3)?;self.arg(f,"resetFrame",1)?;FACE_OBJECT
            }
            "SetRotation"=>{
                self.object(get(f,"gameObject")?)?;
                for name in ["quaternion","vector","xAngle","yAngle"]{
                    let value=get(f,name)?;
                    if !value.get("useVariable").is_some_and(Value::truthy)||!text(value.get("name"))?.is_empty(){return Err("SetRotation requires original Z-only form".into());}
                }
                if get(f,"lateUpdate")?.truthy()||get(f,"space")?.int()!=Some(0){return Err("SetRotation callback/space unsupported".into());}
                self.arg(f,"zAngle",2)?;self.operand(&Value::Int(0),3)?;SET_ROTATION
            }

            "AddForce2dV2"=>{
                self.object(get(f,"gameObject")?)?;
                for name in ["atPosition","vector","vector3"]{
                    let value=get(f,name)?;
                    if !value.get("useVariable").is_some_and(Value::truthy)||!text(value.get("name"))?.is_empty(){return Err("AddForce2dV2 vector/position override unsupported".into());}
                }
                // FsmFloat None is a present object. DoAddForce reads its
                // serialized value for fallback forces and all three clamps.
                for name in ["x","y","maxSpeedX","maxSpeedY","maxSpeed"]{
                    let value=get(f,name)?;
                    if value.get("useVariable").is_some_and(Value::truthy)&&text(value.get("name"))?.is_empty(){
                        self.operand(value.get("value").ok_or("force None value")?,2)?;
                    }else{self.operand(value,2)?;}
                }
                ADD_FORCE_2D
            }

            "SendEventByName"=>{
                let target=get(f,"eventTarget")?;
                if target.get("target").and_then(Value::int)!=Some(1)||target.get("excludeSelf").and_then(|v|v.get("value")).is_none_or(Value::truthy)||target.get("sendToChildren").and_then(|v|v.get("value")).is_none_or(|v|!v.truthy())||text(target.get("fsmName").and_then(|v|v.get("value")))?!=""{return Err("SendEventByName recipient subset unsupported".into());}
                self.object(target.get("gameObject").ok_or("event target owner")?)?;
                let event=get(f,"sendEvent")?;
                if event.get("useVariable").is_some_and(Value::truthy)||text(event.get("value"))?!="TOOK DAMAGE"{return Err("SendEventByName message unsupported".into());}
                let delay=get(f,"delay")?;
                if delay.get("useVariable").is_some_and(Value::truthy)||delay.get("value").and_then(Value::float)!=Some(0.0)||get(f,"everyFrame")?.truthy(){return Err("SendEventByName schedule unsupported".into());}
                SEND_EVENT_BY_NAME
            }
            "CheckCanSeeHero"=>{self.arg(f,"storeResult",1)?;CHECK_CAN_SEE_HERO}
            "CheckAlertRange"=>{self.arg(f,"alertRange",3)?;self.arg(f,"storeResult",1)?;CHECK_ALERT_RANGE}
            "FindAlertRange"=>{
                if text(Some(get(f,"childName")?))?!="Alert Range New"{return Err("FindAlertRange child binding unsupported".into());}
                self.object(get(f,"target")?)?;self.arg(f,"storeResult",3)?;FIND_ALERT_RANGE
            }
            "GetVelocity2d"=>{
                let vector=get(f,"vector")?;
                if !vector.get("useVariable").is_some_and(Value::truthy)||!text(vector.get("name"))?.is_empty(){return Err("GetVelocity2d vector output unsupported".into());}
                self.object(get(f,"gameObject")?)?;
                self.arg(f,"x",2)?;self.arg(f,"y",2)?;
                let space=get(f,"space")?.int().ok_or("GetVelocity2d space enum")?;
                if space!=0&&space!=1{return Err("GetVelocity2d space unsupported".into());}
                self.operand(&Value::Int(space),3)?;GET_VELOCITY_2D
            }
            "Decelerate"=>{
                self.object(get(f,"gameObject")?)?;self.arg(f,"deceleration",2)?;flags|=UPDATE;DECELERATE
            }
            "SetVelocity2d"=>{
                let vector=get(f,"vector")?;
                if !vector.get("useVariable").is_some_and(Value::truthy)||!text(vector.get("name"))?.is_empty(){return Err("SetVelocity2d vector input unsupported".into());}
                self.object(get(f,"gameObject")?)?;
                self.arg(f,"x",2)?;self.arg(f,"y",2)?;SET_VELOCITY_2D
            }
            "FloatClamp"=>{
                let output=get(f,"floatVariable")?;
                if !output.get("useVariable").is_some_and(Value::truthy)||text(output.get("name"))?.is_empty(){return Err("FloatClamp requires a mutable variable".into());}
                for name in ["floatVariable","minValue","maxValue"]{self.arg(f,name,2)?;}
                FLOAT_CLAMP
            }
            "BoolAllTrue"=>{
                let values:Vec<_>=f.iter().filter(|(n,v)|n.parse::<usize>().is_ok()&&v.get("value").is_some()).map(|(_,v)|v).collect();
                if values.is_empty()||values.len()>13{return Err("BoolAllTrue array size".into());}
                for value in values{self.operand(value,1)?;}
                let output=get(f,"storeResult")?;
                if !output.get("useVariable").is_some_and(Value::truthy)||text(output.get("name"))? != "" {return Err("BoolAllTrue named output ordering unsupported".into());}
                self.arg(f,"storeResult",1)?;self.ev(f,"sendEvent")?;BOOL_ALL_TRUE
            }
            "NextFrameEvent"=>{
                self.ev(f,"sendEvent")?;flags|=UPDATE;NEXT_FRAME_EVENT
            }
            "GetDistance"=>{self.object(get(f,"gameObject")?)?;self.object(get(f,"target")?)?;self.arg(f,"storeResult",2)?;GET_DISTANCE}
            "FloatCompare"=>{
                for n in ["float1","float2","tolerance"]{self.arg(f,n,2)?;}
                for n in ["equal","lessThan","greaterThan"]{self.ev(f,n)?;}
                FLOAT_COMPARE
            }
            "FloatSwitch"=>{
                self.arg(f,"floatVariable",2)?;
                // The typed decoder retains each original array element under
                // its serialized parameter index, in the authored order.
                let values:Vec<_>=f.iter().filter(|(n,v)|n.parse::<usize>().is_ok()&&v.get("value").and_then(Value::float).is_some()).map(|(_,v)|v).collect();
                let events:Vec<_>=f.iter().filter(|(n,v)|n.parse::<usize>().is_ok()&&v.str().is_some()).map(|(_,v)|v).collect();
                if values.is_empty()||values.len()!=events.len(){return Err("unsupported FloatSwitch arrays".into());}
                for (v,e) in values.into_iter().zip(events){self.operand(v,2)?;let event=self.event(text(Some(e))?);self.operands.push((2,event));}
                FLOAT_SWITCH
            }
            "SetBoolValue"=>{self.arg(f,"boolVariable",1)?;self.arg(f,"boolValue",1)?;SET_BOOL}
            "SetFloatValue"=>{self.arg(f,"floatVariable",2)?;self.arg(f,"floatValue",2)?;SET_FLOAT}
            "RandomFloat"=>{for n in ["min","max","storeResult"]{self.arg(f,n,2)?;}RANDOM_FLOAT}
            "Wait"=>{
                let time=get(f,"time")?;
                if time.get("useVariable").is_some_and(Value::truthy){
                    if !self.paired_chase||text(time.get("name"))?!="Attention Span"{return Err("dynamic Wait duration unsupported".into());}
                }else if time.get("value").and_then(Value::float).is_none_or(|v|!v.is_finite()||v<0.0||v>10.0){return Err("Wait duration outside clock adapter bound".into());}
                self.arg(f,"time",2)?;
                let name=if self.paired_chase{"__component_wait".to_owned()}else{format!("__wait_{}",self.variables.len())};
                if !self.declarations.iter().any(|(n,_,_)|*n==name){self.declarations.push((name.clone(),Value::F32(0.0),2));}
                let timer=self.variable(name)?;self.operands.push((1,timer));
                self.ev(f,"finishEvent")?;self.arg(f,"realTime",1)?;
                flags|=UPDATE;WAIT
            }
            "Translate"=>{
                self.object(get(f,"gameObject")?)?;
                let vector=get(f,"vector")?;
                if !vector.get("useVariable").is_some_and(Value::truthy)||!text(vector.get("name"))?.is_empty(){return Err("Translate named/literal vector unsupported".into());}
                for n in ["x","y","z"]{self.arg(f,n,2)?;}
                let space=get(f,"space")?.int().ok_or("Translate space")?;
                if space!=0&&space!=1{return Err("Translate space enum".into());}
                self.operand(&Value::Int(space),1)?;self.arg(f,"perSecond",1)?;
                if get(f,"lateUpdate")?.truthy()||get(f,"fixedUpdate")?.truthy(){return Err("Translate callback phase unsupported".into());}
                TRANSLATE
            }
            "WalkLeftRight"=>{
                self.object(get(f,"gameObject")?)?;self.operands.pop();
                for (n,value) in [("groundLayer","Terrain"),("walkAnimName","walk"),("turnAnimName","turn")]{
                    let v=get(f,n)?;
                    if v.get("useVariable").is_some_and(Value::truthy)||text(v.get("value"))?!=value{return Err("WalkLeftRight art/layer binding unsupported".into());}
                }
                if !get(f,"spriteFacesLeft")?.truthy(){return Err("WalkLeftRight facing contract".into());}
                self.arg(f,"walkSpeed",2)?;self.arg(f,"turnDelay",2)?;
                for n in ["startLeft","startRight","keepDirection"]{self.arg(f,n,1)?;}
                flags|=UPDATE;WALK_LEFT_RIGHT
            }
            "FaceDirection"=>{
                if self.paired_chase {
                    self.object(get(f,"gameObject")?)?;self.arg(f,"spriteFacesRight",1)?;self.arg(f,"playNewAnimation",1)?;
                    let clip=get(f,"newAnimationClip")?;
                    if clip.get("useVariable").is_some_and(Value::truthy){return Err("dynamic turn clip unsupported".into());}
                    let clip=match text(clip.get("value"))?.as_str(){""=>0,"TurnToIdle"=>3,"TurnToFly"=>4,_=>return Err("Vengefly turn clip unsupported".into())};
                    self.operand(&Value::Int(clip),3)?;self.arg(f,"pauseBetweenTurns",1)?;self.arg(f,"pauseTime",2)?;
                    return Ok((FACE_DIRECTION,flags));
                }
                if get(f,"playNewAnimation")?.truthy()||get(f,"pauseBetweenTurns")?.truthy(){return Err("unsupported animated FaceDirection".into());}
                self.object(get(f,"gameObject")?)?;// Retain validated object role for the kernel.
                self.arg(f,"spriteFacesRight",1)?;FACE_DIRECTION
            }
            "SetVelocityAsAngle"=>{
                self.object(get(f,"gameObject")?)?;
                self.arg(f,"angle",2)?;self.arg(f,"speed",2)?;SET_VELOCITY_ANGLE
            }
            "CheckCollisionSide"|"CheckCollisionSideEnter"=>{
                if get(f,"ignoreTriggers")?.get("value").unwrap_or(get(f,"ignoreTriggers")?).truthy(){return Err("trigger filtering unsupported by Gruzzer adapter".into());}
                for n in ["topHit","rightHit","bottomHit","leftHit"]{
                    if !text(get(f,n)?.get("name"))?.is_empty(){return Err("named collision output requires kernel storage".into());}
                }
                self.arg(f,"otherLayer",1)?;self.arg(f,"ignoreTriggers",1)?;
                for n in ["topHitEvent","rightHitEvent","bottomHitEvent","leftHitEvent"]{self.ev(f,n)?;}
                flags|=UPDATE;
                if name=="CheckCollisionSide"{COLLISION_STAY}else{COLLISION_ENTER}
            }
            _=>return Err(format!("unsupported enabled action {name}").into()),
        };
        Ok((opcode,flags))
    }
}
pub fn cook(fsm:&Value)->Result<Vec<u8>> {
    let states=fsm.get("states").and_then(Value::list).ok_or("states")?;
    let names:Vec<_>=states.iter().map(|s|text(s.get("name"))).collect::<Result<_>>()?;
    let state_index=|name:String|->Result<u16>{names.iter().position(|s|*s==name).map(|n|n as u16).ok_or_else(||format!("missing state {name}").into())};
    let paired_chase=matches!(fsm.get("name").and_then(Value::str).as_deref(),Some("chaser"|"flyer_receive_direction_msg"));
    if paired_chase {
        for state in states {
            let data=state.get("actionData").ok_or("action data")?;
            let names=data.get("actionNames").and_then(Value::list).ok_or("action names")?;
            let enabled=data.get("actionEnabled").and_then(Value::list).ok_or("action enabled")?;
            let waits=names.iter().zip(enabled).filter(|(name,on)|on.truthy()&&name.str().as_deref()==Some("HutongGames.PlayMaker.Actions.Wait")).count();
            if names.len()!=enabled.len()||waits>1{return Err("paired component has concurrent Wait actions".into());}
        }
    }
    let mut t=Tables{paired_chase,variables:vec![],declarations:vec![],constants:vec![],events:vec!["FINISHED".into()],operands:vec![]};
    let Value::Map(groups)=fsm.get("variables").ok_or("variables")? else{return Err("variables groups".into())};
    let mut all_declarations=vec![];
    for (group,items) in groups {
        if !group.ends_with("Variables"){continue;}
        let Some(items)=items.list()else{continue};
        for item in items {
            let name=text(item.get("name"))?;
            if all_declarations.contains(&name){return Err("duplicate variable declaration".into());}
            all_declarations.push(name);
        }
        let kind=match &**group {"boolVariables"=>1,"floatVariables"=>2,"gameObjectVariables"=>3,"objectVariables" if paired_chase=>3,_=>continue};
        for item in items {
            if group=="objectVariables"&&(text(item.get("name"))?!="Alert Range"||text(item.get("typeName"))?!="AlertRange"||item.get("value").and_then(|v|v.get("m_FileID")).and_then(Value::int)!=Some(0)||item.get("value").and_then(|v|v.get("m_PathID")).and_then(Value::int)!=Some(0)){return Err("unsupported object variable role".into());}
            let n=text(item.get("name"))?;t.declarations.push((n,item.get("value").cloned().ok_or("variable initial value")?,kind));
        }
    }
    if paired_chase {
        let names:Vec<_>=t.declarations.iter().map(|(name,_,_)|name.clone()).collect();
        for name in names{t.variable(name)?;}
    }
    let mut state_rows=vec![];let mut actions=vec![];let mut edges=vec![];let mut collision_layers=vec![];
    for state in states {
        let action_start=actions.len() as u16;
        for (name,index) in enabled(state){
            let data=state.get("actionData").ok_or("actionData")?;
            let kinds=data.get("paramDataType").and_then(Value::list).ok_or("parameter types")?;
            let starts=data.get("actionStartIndex").and_then(Value::list).ok_or("parameter starts")?;
            let start=usize::try_from(starts.get(index).and_then(Value::int).ok_or("parameter start")?)?;
            let end=starts.get(index+1).and_then(Value::int).map(usize::try_from).transpose()?.unwrap_or(kinds.len());
            let selected=kinds.get(start..end).ok_or("parameter bounds")?;
            let parameter_names=data.get("paramName").and_then(Value::list).ok_or("parameter names")?;
            for (offset,v) in selected.iter().enumerate(){
                if v.int()==Some(0)&&["CheckCollisionSide","CheckCollisionSideEnter"].contains(&name.as_str())
                    && parameter_names.get(start+offset).and_then(Value::str).as_deref()==Some("otherLayerNumber"){
                    let at=start+offset;
                    let pos=usize::try_from(data.get("paramDataPos").and_then(Value::list).and_then(|v|v.get(at)).and_then(Value::int).ok_or("layer position")?)?;
                    let size=data.get("paramByteDataSize").and_then(Value::list).and_then(|v|v.get(at)).and_then(Value::int).ok_or("layer size")?;
                    if size!=4{return Err("layer integer size".into());}
                    let raw=data.get("byteData").and_then(Value::list).and_then(|v|v.get(pos..pos.checked_add(4)?)).ok_or("layer bounds")?;
                    let mut bytes=[0;4];for (i,v) in raw.iter().enumerate(){bytes[i]=u8::try_from(v.int().ok_or("layer byte")?)?;}
                    collision_layers.push(i32::from_le_bytes(bytes));
                    continue;
                }
                let crawler=fsm.get("name").and_then(Value::str).as_deref()==Some("Crawler");
                if !matches!(v.int(),Some(1|12|15|17|18|19|20|23))&&!(crawler&&matches!(v.int(),Some(2|7|28)))&&!(paired_chase&&matches!(v.int(),Some(3|5|7|24|28|31|35|37))){return Err("unsupported enabled parameter type".into());}
            }
            let raw=data.get("actionNames").and_then(Value::list).and_then(|v|v.get(index)).and_then(Value::str).ok_or("action identity")?;
            let expected=format!("HutongGames.PlayMaker.Actions.{name}");
            if raw!=expected&&!(paired_chase&&matches!(raw.as_str(),"CheckCanSeeHero"|"CheckAlertRange"|"FindAlertRange")){return Err(format!("unsupported action identity {raw}").into());}
            if name=="FloatSwitch"{validate_switch(data,index)?;}
            if name=="BoolAllTrue"{validate_bool_array(data,index)?;}
            let f=if paired_chase{crate::vengefly::action_fields(data,index)?}else if fsm.get("name").and_then(Value::str).as_deref()==Some("Crawler"){crawler_fields(data,index)?}else{action_fields_with_plain_strings(data,index,true)?};
            let start=t.operands.len() as u16;
            let (opcode,flags)=t.action(&name,&f)?;
            actions.push((opcode,start,(t.operands.len()-start as usize) as u8,flags));
        }
        let edge_start=edges.len() as u16;
        for edge in state.get("transitions").and_then(Value::list).unwrap_or(&[]) {
            let e=t.event(text(edge.get("fsmEvent").and_then(|v|v.get("name")))?);
            edges.push((e,state_index(text(edge.get("toState"))?)?));
        }
        state_rows.push([action_start,actions.len() as u16-action_start,edge_start,edges.len() as u16-edge_start]);
    }
    let before_global=edges.len();
    for edge in fsm.get("globalTransitions").and_then(Value::list).unwrap_or(&[]) {
        let e=t.event(text(edge.get("fsmEvent").and_then(|v|v.get("name")))?);
        edges.push((e,state_index(text(edge.get("toState"))?)?));
    }
    for row in &state_rows {
        for local in row[2] as usize..(row[2]+row[3]) as usize {
            if edges[before_global..].iter().any(|(event,_)|*event==edges[local].0){return Err("overlapping local/global event precedence unsupported".into());}
        }
    }
    if fsm.get("name").and_then(Value::str).as_deref()==Some("Bouncer Control") {
        let expected=["Self","Camera","Starts Inactive","Distance","Angle","Facing Right","Speed","Start Up"];
        if t.variables.iter().map(|(name,_,_)|name.as_str()).collect::<Vec<_>>()!=expected
            || names.get(0).map(String::as_str)!=Some("Initialise")
            || names.get(16).map(String::as_str)!=Some("Fly 2")
            || names.get(17).map(String::as_str)!=Some("Stopped") {
            return Err("original Gruzzer guest role layout changed".into());
        }
        let mut declared=all_declarations.clone();declared.sort();
        let mut expected_declarations=vec!["Angle","Bounds","Camera","Contact Normal","Distance","Facing Right","Hero","Moving Up","Normal X","Normal Y","Origin","Raycast Down","Raycast Left","Raycast Right","Raycast X","Self","Speed","Start Up","Starts Inactive"];expected_declarations.sort();
        if declared.iter().map(String::as_str).collect::<Vec<_>>()!=expected_declarations{return Err(format!("Gruzzer declaration accounting changed: {declared:?}").into());}
        if collision_layers!=[0,0,25,25,24,24]{return Err("original Gruzzer collision layer binding changed".into());}
        let fly=state_rows[16];
        let authored=[(COLLISION_STAY,0),(COLLISION_ENTER,0),(COLLISION_STAY,1),(COLLISION_ENTER,1),(COLLISION_STAY,1),(COLLISION_ENTER,1)];
        let selected=&actions[fly[0] as usize+2..(fly[0]+fly[1]) as usize];
        if selected.len()!=authored.len(){return Err("Gruzzer collision action count".into());}
        for (action,(opcode,other)) in selected.iter().zip(authored){
            let start=action.1 as usize;
            let operand=t.operands[start];
            if action.0!=opcode||operand.0!=0||t.constants[operand.1 as usize]!=other{return Err("Gruzzer collision action order".into());}
            for event in &t.operands[start+2..start+6]{if event.0!=2||event.1==NO_EVENT{return Err("Gruzzer terrain event cannot consume contact".into());}}
        }
        // All enabled action operand types above are supported; named live
        // references resolve through declarations, while the eleven remaining
        // declared values have no enabled action operand in this graph.
    }
    let counts=[states.len(),actions.len(),edges.len(),t.operands.len(),t.variables.len(),t.constants.len(),t.events.len(),edges.len()-before_global];
    if counts.iter().any(|n|*n>u16::MAX as usize){return Err("table limit".into());}
    let mut b=Vec::from(*hk_format::fsm::MAGIC);
    for n in counts{b.extend((n as u16).to_le_bytes());}
    b.extend(state_index(text(fsm.get("startState"))?)?.to_le_bytes());b.extend(0u16.to_le_bytes());b.extend(0u32.to_le_bytes());
    for row in state_rows{for n in row{b.extend(n.to_le_bytes());}}
    for (opcode,start,count,flags) in actions{b.extend(opcode.to_le_bytes());b.extend(start.to_le_bytes());b.extend([count,flags,0,0]);}
    for (event,target) in edges{b.extend(event.to_le_bytes());b.extend(target.to_le_bytes());}
    for (kind,index) in t.operands{b.extend([kind,0]);b.extend(index.to_le_bytes());}
    for (_,initial,kind) in t.variables{b.extend(initial.to_le_bytes());b.extend([kind,0,0,0]);}
    for n in t.constants{b.extend(n.to_le_bytes());}
    let len=b.len() as u32;b[28..32].copy_from_slice(&len.to_le_bytes());
    let program=hk_format::fsm::Program::parse(&b).map_err(|e|format!("invalid cooked graph {e:?}"))?;
    program.action_groups().map_err(|e|format!("unsupported graph action group {e:?}"))?;
    Ok(b)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bool_literals_reject_truthiness_coercion(){
        for v in [Value::Int(2),Value::Str(b"false".to_vec()),Value::Map(vec![])]{assert!(Tables::scalar(&v,1).is_err());}
        assert_eq!(Tables::scalar(&Value::Bool(false),1).unwrap(),0);
        assert_eq!(Tables::scalar(&Value::Int(1),1).unwrap(),1);
    }
    #[test]
    fn typed_variables_do_not_alias_float_and_bool(){
        let mut t=Tables{paired_chase:false,variables:vec![],declarations:vec![("x".into(),Value::F32(0.0),2)],constants:vec![],events:vec![],operands:vec![]};
        let v=Value::Map(vec![("useVariable".into(),Value::Bool(true)),("name".into(),Value::Str(b"x".to_vec()))]);
        assert!(t.operand(&v,1).is_err());assert!(t.operand(&v,2).is_ok());
    }
    #[test]
    fn missing_array_metadata_fails_closed(){assert!(validate_switch(&Value::Map(vec![]),0).is_err());}
}

use hk_dotnet::{Assembly,il::{body,decode,Operand}};
fn main()->Result<(),Box<dyn std::error::Error>>{
    let args:Vec<_>=std::env::args().collect();
    let a=Assembly::open(std::path::Path::new(args.get(1).ok_or("assembly")?))?;
    for typename in &args[2..] {
        for (_,_,name,rva) in a.methods_of(typename) {
            if !["DoCompare","DoFloatCompare","DoRandomFloat","DoFloatSwitch","CheckTouching","OnEnter","DoCollisionStay2D","DoCollisionEnter2D"].contains(&name.as_str()){continue;}
            println!("{typename}::{name}");
            for i in decode(body(&a,rva)?)? {
                let value=if let Operand::Token(t)=i.operand{format!("{}",a.token_name(t).unwrap_or("?"))}else{format!("{:?}",i.operand)};
                println!("{:04x} {} {value}",i.offset,i.name);
            }
        }
    }
    Ok(())
}

fn main() {
    let path = std::env::args().nth(1).expect("room pack path");
    let bytes = std::fs::read(path).expect("read room pack");
    let room = hk_format::Room::parse(&bytes).expect("valid room pack");
    println!("Validated {} bytes: {:?}", bytes.len(), room.counts);
}

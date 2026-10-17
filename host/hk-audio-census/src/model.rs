//! Plain data and the small rules the tables share.

/// One AudioClip as the install stores it. Header fields only.
#[derive(Clone, Debug)]
pub struct Clip {
    /// `file:path_id`, the key the cookers use (`sharedassets48.assets:28`).
    pub key: String,
    pub name: String,
    pub file: String,
    pub path_id: i64,
    pub channels: i64,
    pub rate: i64,
    pub seconds: f64,
    /// Bytes of the compressed FSB in the install's resource stream.
    pub stored: u64,
}

/// A MusicCue: up to six looping layers (stems), optionally conditional alternatives.
#[derive(Clone, Debug, Default)]
pub struct Cue {
    pub key: String,
    pub name: String,
    /// Clip index per layer slot, None for an empty slot.
    pub layers: Vec<Option<usize>>,
    pub alternatives: Vec<String>,
    pub event: String,
}

/// An AtmosCue: which of the AudioManager's atmos channels it enables.
#[derive(Clone, Debug, Default)]
pub struct Atmos {
    pub name: String,
    pub channels: Vec<usize>,
}

/// How a cue or clip reached a scene.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Reach {
    /// The scene's SceneManager music cue (the area theme).
    SceneCue,
    /// A MusicRegion collider's enter or exit cue.
    Region,
    /// Referenced from a PlayMaker FSM or another behaviour (boss fights, events).
    Script,
    /// Reached only through a prefab or ScriptableObject the scene points at.
    Indirect,
}

impl Reach {
    pub fn label(self) -> &'static str {
        match self {
            Reach::SceneCue => "scene",
            Reach::Region => "region",
            Reach::Script => "script",
            Reach::Indirect => "indirect",
        }
    }
}

/// What a scene is for, which decides whether the CD is free while it plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Loc {
    Menu,
    Cutscene,
    /// A fight room whose gates close: nothing streams while it plays.
    Arena,
    /// Everything else: rooms the Knight walks through, where the next scene may stream.
    Traversal,
}

impl Loc {
    pub fn label(self) -> &'static str {
        match self {
            Loc::Menu => "menu",
            Loc::Cutscene => "cutscene",
            Loc::Arena => "boss arena",
            Loc::Traversal => "traversal",
        }
    }
}

pub const AREAS: [&str; 26] = [
    "Menus, helpers and tests",
    "Cinematics and openings",
    "King's Pass",
    "Dirtmouth",
    "Interior rooms",
    "Tram",
    "Forgotten Crossroads",
    "Greenpath",
    "Fungal Wastes",
    "Fog Canyon and Queen's Gardens",
    "City of Tears",
    "Resting Grounds",
    "Howling Cliffs",
    "Crystal Peak",
    "Deepnest",
    "Kingdom's Edge",
    "Colosseum of Fools",
    "Ancient Basin and Abyss",
    "Royal Waterways",
    "The Hive",
    "White Palace",
    "Grimm Troupe",
    "Dreams",
    "Hollow Knight finale",
    "Godhome",
    "Other",
];

/// Area from the scene name, the same prefix rules the scene catalog uses.
pub fn area_of(scene: &str) -> &'static str {
    let s = |p: &str| scene.starts_with(p);
    let a = |i: usize| AREAS[i];
    if s("_test")
        || s("Pre_Menu")
        || s("Menu_")
        || s("Quit_To")
        || s("BetaEnd")
        || s("Knight_Pickup")
    {
        return a(0);
    }
    if s("Opening_Sequence") || s("Cutscene_") || s("Cinematic_") {
        return a(1);
    }
    if s("Tutorial_") {
        return a(2);
    }
    if scene == "Town" {
        return a(3);
    }
    if s("Room_Tram") {
        return a(5);
    }
    if s("Room_Final_Boss") {
        return a(23);
    }
    if s("Room_Colosseum") {
        return a(16);
    }
    if s("Room_GG") || s("GG_") {
        return a(24);
    }
    if s("Room_spider") {
        return a(14);
    }
    if s("Room_") {
        return a(4);
    }
    let table: [(&str, usize); 17] = [
        ("Crossroads", 6),
        ("Fungus1", 7),
        ("Fungus2", 8),
        ("Fungus3", 9),
        ("Ruins", 10),
        ("RestingGrounds", 11),
        ("Cliffs", 12),
        ("Mines", 13),
        ("Deepnest_East", 15),
        ("Deepnest", 14),
        ("Abyss", 17),
        ("Waterways", 18),
        ("Hive", 19),
        ("White_Palace", 20),
        ("Grimm", 21),
        ("Dream", 22),
        ("Fungus3", 9),
    ];
    for (p, i) in table {
        if s(p) {
            return a(i);
        }
    }
    a(25)
}

/// The expansion a scene belongs to: "grimm", "godhome" or "".
pub fn dlc_of(area: &str) -> &'static str {
    match area {
        "Grimm Troupe" => "grimm",
        "Godhome" => "godhome",
        _ => "",
    }
}

/// Scene name to location class. Names carry the evidence the data does not:
/// `_boss` rooms, the Godhome fight halls, the dream fights and the Grimm tents.
/// A boss fought inside an ordinary room is caught by its script-applied cue
/// instead (see `SceneRec::cues`), not here.
pub fn loc_of(scene: &str) -> Loc {
    let l = scene.to_ascii_lowercase();
    if l.starts_with("menu_")
        || l.starts_with("pre_menu")
        || l.starts_with("quit_to")
        || l == "betaend"
    {
        return Loc::Menu;
    }
    let cutscene = [
        "gg_entrance_cutscene",
        "gg_end_sequence",
        "gg_door_5_finale",
        "dream_nailcollection",
        "dream_abyss",
        "room_final_boss_atrium",
        "end_game_completion",
        "permadeath",
        "permadeath_unlock",
    ];
    if l.starts_with("cinematic_")
        || l.starts_with("cutscene_")
        || l.starts_with("opening_sequence")
        || l.starts_with("end_credits")
        || cutscene.contains(&l.as_str())
    {
        return Loc::Cutscene;
    }
    let gg_traversal = [
        "gg_waterways",
        "gg_atrium",
        "gg_atrium_roof",
        "gg_workshop",
        "gg_unlock",
        "gg_unlock_wastes",
        "gg_blue_room",
        "gg_land_of_storms",
        "gg_boss_door_entrance",
        "gg_spa",
        "gg_pipeway",
        "room_gg_shortcut",
    ];
    if gg_traversal.contains(&l.as_str()) || l.ends_with("_defeated") {
        return Loc::Traversal;
    }
    if l.contains("_boss")
        || l.starts_with("gg_")
        || l.starts_with("room_colosseum_bronze")
        || l.starts_with("room_colosseum_silver")
        || l.starts_with("room_colosseum_gold")
        || l == "room_final_boss_core"
        || l == "room_spider_small"
        || l == "grimm_main_tent"
        || l == "grimm_nightmare"
        || [
            "dream_02_mage_lord",
            "dream_03_infected_knight",
            "dream_04_white_defender",
            "dream_final_boss",
        ]
        .contains(&l.as_str())
    {
        return Loc::Arena;
    }
    Loc::Traversal
}

/// PS1 SPU ADPCM bytes for `seconds` of mono audio played at `rate`, with the
/// rate capped at the source's own: 16 bytes hold 28 samples.
pub fn adpcm_bytes(seconds: f64, rate: i64, source_rate: i64) -> u64 {
    let r = rate.min(source_rate).max(1);
    let samples = (seconds * r as f64).ceil() as u64;
    samples.div_ceil(28) * 16
}

/// `Boss Fight (12)` and `Boss Fight(Clone)` count as one object name.
pub fn normalise(name: &str) -> String {
    let mut n = name.trim().to_string();
    if let Some(i) = n.find("(Clone)") {
        n.truncate(i);
        n = n.trim().to_string();
    }
    if n.ends_with(')') {
        if let Some(i) = n.rfind('(') {
            if n[i + 1..n.len() - 1].chars().all(|c| c.is_ascii_digit()) && i > 0 {
                n.truncate(i);
                n = n.trim().to_string();
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adpcm_is_sixteen_bytes_per_twenty_eight_samples() {
        // one second at 22,050 Hz is 788 blocks of 28 samples (787.5 rounded up)
        assert_eq!(adpcm_bytes(1.0, 22050, 44100), 788 * 16);
        // the rate is capped at the clip's own
        assert_eq!(
            adpcm_bytes(1.0, 22050, 11025),
            adpcm_bytes(1.0, 11025, 11025)
        );
        assert_eq!(adpcm_bytes(0.0, 22050, 44100), 0);
    }

    #[test]
    fn names_lose_clone_and_index_suffixes() {
        assert_eq!(normalise("Zombie Runner (3)"), "Zombie Runner");
        assert_eq!(normalise("Boss(Clone)"), "Boss");
        assert_eq!(normalise("Hornet Boss (Hit)"), "Hornet Boss (Hit)");
    }

    #[test]
    fn scene_names_map_to_areas_and_locations() {
        assert_eq!(area_of("GG_Radiance"), "Godhome");
        assert_eq!(area_of("Fungus1_04_boss"), "Greenpath");
        assert_eq!(area_of("Grimm_Main_Tent"), "Grimm Troupe");
        assert_eq!(loc_of("Crossroads_10_boss"), Loc::Arena);
        assert_eq!(loc_of("Crossroads_10_boss_defeated"), Loc::Traversal);
        assert_eq!(loc_of("GG_Workshop"), Loc::Traversal);
        assert_eq!(loc_of("Cinematic_Ending_A"), Loc::Cutscene);
        assert_eq!(loc_of("Menu_Title"), Loc::Menu);
        assert_eq!(loc_of("Town"), Loc::Traversal);
        assert_eq!(dlc_of(area_of("GG_Atrium")), "godhome");
    }
}

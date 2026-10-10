//! Measured scenery quality and the development-region residency layout (host/quality.py).
//!
//! The tables are data; they were generated from the Python module and are checked against
//! it by `hk-cook-parity quality`, which compares every value including whether it was an
//! integer or a float (the cook writes them to JSON as such).

/// A number as host/quality.py wrote it: integer or float.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    I(i64),
    F(f64),
}

impl Num {
    pub fn f64(self) -> f64 {
        match self {
            Num::I(v) => v as f64,
            Num::F(v) => v,
        }
    }
}

/// One row of `SCENE_TABLE`.
pub struct SceneRow {
    pub scene_id: usize,
    pub scene_name: &'static str,
    pub file: &'static str,
    pub runtime_bounds: [Num; 4],
    pub camera_global_bounds: [Num; 4],
}

pub type Bounds = [Num; 4];

pub const SCENERY_MAX_AXIS: i64 = 48;
pub const SCENERY_TEXEL_CAP: i64 = 252;
pub const STATIC_PAGE_BUDGET: usize = 19;
pub const TEXTURE_BUDGET: usize = 416;
pub const ROOM_BYTE_BUDGET: usize = 393216;

/// `SCENERY_SCENE_CAPS`.
pub const SCENERY_SCENE_CAPS: [(&str, i64); 4] = [
    ("Tutorial_01", 96),
    ("Town", 160),
    ("Crossroads_50", 48),
    ("Fungus1_10", 48),
];

/// `SCENE_TABLE`.
pub const SCENE_TABLE: [SceneRow; 60] = [
    SceneRow {
        scene_id: 0,
        scene_name: "Tutorial_01",
        file: "level6",
        runtime_bounds: [Num::I(0), Num::I(-5), Num::I(210), Num::I(100)],
        camera_global_bounds: [Num::I(10), Num::I(0), Num::I(196), Num::I(92)],
    },
    SceneRow {
        scene_id: 1,
        scene_name: "Town",
        file: "level7",
        runtime_bounds: [Num::I(0), Num::I(-5), Num::I(270), Num::I(76)],
        camera_global_bounds: [Num::I(10), Num::I(8), Num::I(258), Num::I(68)],
    },
    SceneRow {
        scene_id: 2,
        scene_name: "Crossroads_01",
        file: "level37",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(102), Num::I(49)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(100), Num::I(42)],
    },
    SceneRow {
        scene_id: 3,
        scene_name: "Crossroads_02",
        file: "level38",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(92), Num::I(36)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(90), Num::I(30)],
    },
    SceneRow {
        scene_id: 4,
        scene_name: "Crossroads_07",
        file: "level43",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(45), Num::I(116)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(43), Num::I(110)],
    },
    SceneRow {
        scene_id: 5,
        scene_name: "Crossroads_03",
        file: "level39",
        runtime_bounds: [Num::I(-2), Num::I(-6), Num::I(32), Num::I(77)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(30), Num::I(72)],
    },
    SceneRow {
        scene_id: 6,
        scene_name: "Crossroads_04",
        file: "level40",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(162), Num::I(32)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(160), Num::I(30)],
    },
    SceneRow {
        scene_id: 7,
        scene_name: "Crossroads_05",
        file: "level41",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(77), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(75), Num::I(25)],
    },
    SceneRow {
        scene_id: 8,
        scene_name: "Crossroads_06",
        file: "level42",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(62), Num::I(61)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(60), Num::I(60)],
    },
    SceneRow {
        scene_id: 9,
        scene_name: "Crossroads_08",
        file: "level44",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(54), Num::I(44)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(52), Num::I(43)],
    },
    SceneRow {
        scene_id: 10,
        scene_name: "Crossroads_09",
        file: "level45",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(88), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(86), Num::I(25)],
    },
    SceneRow {
        scene_id: 11,
        scene_name: "Crossroads_10",
        file: "level46",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(77), Num::I(69)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(75), Num::I(68)],
    },
    SceneRow {
        scene_id: 12,
        scene_name: "Crossroads_12",
        file: "level51",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(72), Num::I(25)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(24)],
    },
    SceneRow {
        scene_id: 13,
        scene_name: "Crossroads_13",
        file: "level52",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(82), Num::I(48)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(80), Num::I(47)],
    },
    SceneRow {
        scene_id: 14,
        scene_name: "Crossroads_14",
        file: "level53",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(35), Num::I(49)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(33), Num::I(48)],
    },
    SceneRow {
        scene_id: 15,
        scene_name: "Crossroads_16",
        file: "level55",
        runtime_bounds: [Num::I(-2), Num::I(-6), Num::I(78), Num::I(28)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(76), Num::I(27)],
    },
    SceneRow {
        scene_id: 16,
        scene_name: "Crossroads_18",
        file: "level56",
        runtime_bounds: [Num::I(-1), Num::I(-6), Num::I(43), Num::I(51)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(41), Num::I(50)],
    },
    SceneRow {
        scene_id: 17,
        scene_name: "Crossroads_19",
        file: "level57",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(52), Num::I(50)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(50), Num::I(45)],
    },
    SceneRow {
        scene_id: 18,
        scene_name: "Crossroads_21",
        file: "level58",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(102), Num::I(34)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(100), Num::I(29)],
    },
    SceneRow {
        scene_id: 19,
        scene_name: "Crossroads_11_alt",
        file: "level50",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(122), Num::I(37)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(120), Num::I(36)],
    },
    SceneRow {
        scene_id: 20,
        scene_name: "Crossroads_15",
        file: "level54",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(62), Num::I(19)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(60), Num::I(18)],
    },
    SceneRow {
        scene_id: 21,
        scene_name: "Crossroads_22",
        file: "level59",
        runtime_bounds: [Num::I(-1), Num::I(-6), Num::I(106), Num::I(36)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(105), Num::I(35)],
    },
    SceneRow {
        scene_id: 22,
        scene_name: "Crossroads_25",
        file: "level60",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(72), Num::I(28)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(27)],
    },
    SceneRow {
        scene_id: 23,
        scene_name: "Crossroads_27",
        file: "level61",
        runtime_bounds: [Num::I(-2), Num::I(-6), Num::I(32), Num::I(73)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(30), Num::I(72)],
    },
    SceneRow {
        scene_id: 24,
        scene_name: "Crossroads_30",
        file: "level62",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(51), Num::I(24)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(50), Num::I(23)],
    },
    SceneRow {
        scene_id: 25,
        scene_name: "Crossroads_31",
        file: "level63",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(71), Num::I(24)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(69), Num::I(23)],
    },
    SceneRow {
        scene_id: 26,
        scene_name: "Crossroads_33",
        file: "level64",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(47), Num::I(54)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(45), Num::I(50)],
    },
    SceneRow {
        scene_id: 27,
        scene_name: "Crossroads_35",
        file: "level65",
        runtime_bounds: [Num::I(-1), Num::I(-6), Num::I(72), Num::I(76)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(75)],
    },
    SceneRow {
        scene_id: 28,
        scene_name: "Crossroads_36",
        file: "level66",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(62), Num::I(59)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(60), Num::I(58)],
    },
    SceneRow {
        scene_id: 29,
        scene_name: "Crossroads_37",
        file: "level67",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(112), Num::I(31)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(110), Num::I(30)],
    },
    SceneRow {
        scene_id: 30,
        scene_name: "Crossroads_38",
        file: "level68",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(71), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(25)],
    },
    SceneRow {
        scene_id: 31,
        scene_name: "Crossroads_39",
        file: "level69",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(90), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(88), Num::I(25)],
    },
    SceneRow {
        scene_id: 32,
        scene_name: "Crossroads_40",
        file: "level70",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(90), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(88), Num::I(25)],
    },
    SceneRow {
        scene_id: 33,
        scene_name: "Crossroads_42",
        file: "level71",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(112), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(110), Num::I(25)],
    },
    SceneRow {
        scene_id: 34,
        scene_name: "Crossroads_43",
        file: "level72",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(90), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(88), Num::I(25)],
    },
    SceneRow {
        scene_id: 35,
        scene_name: "Crossroads_45",
        file: "level73",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(72), Num::I(51)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(50)],
    },
    SceneRow {
        scene_id: 36,
        scene_name: "Crossroads_46",
        file: "level74",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(56), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(55), Num::I(25)],
    },
    SceneRow {
        scene_id: 37,
        scene_name: "Crossroads_46b",
        file: "level75",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(57), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(55), Num::I(25)],
    },
    SceneRow {
        scene_id: 38,
        scene_name: "Crossroads_47",
        file: "level77",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(49), Num::I(20)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(47), Num::I(19)],
    },
    SceneRow {
        scene_id: 39,
        scene_name: "Crossroads_48",
        file: "level78",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(58), Num::I(20)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(57), Num::I(19)],
    },
    SceneRow {
        scene_id: 40,
        scene_name: "Crossroads_49",
        file: "level79",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(32), Num::I(177)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(30), Num::I(176)],
    },
    SceneRow {
        scene_id: 41,
        scene_name: "Crossroads_49b",
        file: "level80",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(32), Num::I(87)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(30), Num::I(86)],
    },
    SceneRow {
        scene_id: 42,
        scene_name: "Crossroads_50",
        file: "level81",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(262), Num::I(61)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(260), Num::I(60)],
    },
    SceneRow {
        scene_id: 43,
        scene_name: "Crossroads_52",
        file: "level82",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(56), Num::I(71)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(55), Num::I(70)],
    },
    SceneRow {
        scene_id: 44,
        scene_name: "Crossroads_ShamanTemple",
        file: "level76",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(143), Num::I(76)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(142), Num::I(75)],
    },
    SceneRow {
        scene_id: 45,
        scene_name: "Room_shop",
        file: "level17",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(34), Num::I(31)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(33), Num::I(30)],
    },
    SceneRow {
        scene_id: 46,
        scene_name: "Fungus1_01",
        file: "level128",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(172), Num::I(31)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(170), Num::I(30)],
    },
    SceneRow {
        scene_id: 47,
        scene_name: "Fungus1_01b",
        file: "level129",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(47), Num::I(46)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(45), Num::I(45)],
    },
    SceneRow {
        scene_id: 48,
        scene_name: "Fungus1_02",
        file: "level130",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(48), Num::I(72)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(46), Num::I(71)],
    },
    SceneRow {
        scene_id: 49,
        scene_name: "Fungus1_05",
        file: "level134",
        runtime_bounds: [Num::I(-1), Num::I(-6), Num::I(33), Num::I(90)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(31), Num::I(85)],
    },
    SceneRow {
        scene_id: 50,
        scene_name: "Fungus1_06",
        file: "level135",
        runtime_bounds: [Num::I(-2), Num::I(-6), Num::I(171), Num::I(31)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(170), Num::I(30)],
    },
    SceneRow {
        scene_id: 51,
        scene_name: "Fungus1_07",
        file: "level136",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(72), Num::I(63)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(70), Num::I(58)],
    },
    SceneRow {
        scene_id: 52,
        scene_name: "Fungus1_08",
        file: "level137",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(81), Num::I(56)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(80), Num::I(55)],
    },
    SceneRow {
        scene_id: 53,
        scene_name: "Fungus1_09",
        file: "level138",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(252), Num::I(35)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(250), Num::I(34)],
    },
    SceneRow {
        scene_id: 54,
        scene_name: "Fungus1_10",
        file: "level139",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(182), Num::I(30)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(180), Num::I(25)],
    },
    SceneRow {
        scene_id: 55,
        scene_name: "Fungus1_14",
        file: "level143",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(101), Num::I(26)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(100), Num::I(25)],
    },
    SceneRow {
        scene_id: 56,
        scene_name: "Fungus1_15",
        file: "level144",
        runtime_bounds: [Num::I(-1), Num::I(-5), Num::I(59), Num::I(51)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(57), Num::I(50)],
    },
    SceneRow {
        scene_id: 57,
        scene_name: "Fungus1_17",
        file: "level146",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(82), Num::I(36)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(80), Num::I(35)],
    },
    SceneRow {
        scene_id: 58,
        scene_name: "Fungus1_19",
        file: "level147",
        runtime_bounds: [Num::I(-2), Num::I(-6), Num::I(92), Num::I(25)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(90), Num::I(24)],
    },
    SceneRow {
        scene_id: 59,
        scene_name: "Fungus1_30",
        file: "level157",
        runtime_bounds: [Num::I(-2), Num::I(-5), Num::I(102), Num::I(37)],
        camera_global_bounds: [Num::I(0), Num::I(0), Num::I(100), Num::I(32)],
    },
];

/// `REGION_LAYOUT`: (scene id, bounds).
pub const REGION_LAYOUT: [(usize, Bounds); 98] = [
    (0, [Num::I(15), Num::I(-5), Num::I(62), Num::I(25)]),
    (0, [Num::I(48), Num::I(11), Num::I(72), Num::I(27)]),
    (0, [Num::I(0), Num::I(-5), Num::I(24), Num::I(11)]),
    (0, [Num::I(48), Num::I(-5), Num::I(72), Num::I(11)]),
    (0, [Num::I(72), Num::I(-5), Num::I(96), Num::I(11)]),
    (0, [Num::I(96), Num::I(-5), Num::I(120), Num::I(11)]),
    (0, [Num::I(120), Num::I(-5), Num::I(144), Num::I(11)]),
    (0, [Num::I(144), Num::I(-5), Num::I(168), Num::I(11)]),
    (0, [Num::I(168), Num::I(-5), Num::I(192), Num::I(11)]),
    (0, [Num::I(192), Num::I(-5), Num::I(210), Num::I(11)]),
    (0, [Num::I(0), Num::I(11), Num::I(24), Num::I(27)]),
    (0, [Num::I(24), Num::I(11), Num::I(48), Num::I(27)]),
    (0, [Num::I(72), Num::I(11), Num::F(84.0), Num::I(27)]),
    (0, [Num::F(84.0), Num::I(11), Num::I(96), Num::I(27)]),
    (0, [Num::I(96), Num::I(11), Num::F(108.0), Num::I(27)]),
    (0, [Num::F(108.0), Num::I(11), Num::I(120), Num::F(19.0)]),
    (0, [Num::F(108.0), Num::F(19.0), Num::I(120), Num::I(27)]),
    (0, [Num::I(120), Num::I(11), Num::F(132.0), Num::F(19.0)]),
    (0, [Num::I(120), Num::F(19.0), Num::F(132.0), Num::I(27)]),
    (0, [Num::F(132.0), Num::I(11), Num::I(144), Num::F(19.0)]),
    (0, [Num::F(132.0), Num::F(19.0), Num::I(144), Num::I(27)]),
    (0, [Num::I(144), Num::I(11), Num::F(156.0), Num::I(27)]),
    (0, [Num::F(156.0), Num::I(11), Num::I(168), Num::I(27)]),
    (0, [Num::I(168), Num::I(11), Num::I(192), Num::I(27)]),
    (0, [Num::I(192), Num::I(11), Num::I(210), Num::I(27)]),
    (0, [Num::I(0), Num::I(27), Num::I(24), Num::I(43)]),
    (0, [Num::I(24), Num::I(27), Num::I(48), Num::I(43)]),
    (0, [Num::I(48), Num::I(27), Num::I(72), Num::I(43)]),
    (0, [Num::I(72), Num::I(27), Num::F(84.0), Num::I(43)]),
    (0, [Num::F(84.0), Num::I(27), Num::I(96), Num::I(43)]),
    (0, [Num::I(96), Num::I(27), Num::F(108.0), Num::I(43)]),
    (0, [Num::F(108.0), Num::I(27), Num::I(120), Num::I(43)]),
    (0, [Num::I(120), Num::I(27), Num::F(132.0), Num::F(35.0)]),
    (0, [Num::I(120), Num::F(35.0), Num::F(132.0), Num::I(43)]),
    (0, [Num::F(132.0), Num::I(27), Num::I(144), Num::F(35.0)]),
    (0, [Num::F(132.0), Num::F(35.0), Num::I(144), Num::I(43)]),
    (0, [Num::I(144), Num::I(27), Num::F(156.0), Num::F(35.0)]),
    (0, [Num::I(144), Num::F(35.0), Num::F(156.0), Num::I(43)]),
    (0, [Num::F(156.0), Num::I(27), Num::I(168), Num::I(43)]),
    (0, [Num::I(168), Num::I(27), Num::I(192), Num::I(43)]),
    (0, [Num::I(192), Num::I(27), Num::I(210), Num::I(43)]),
    (0, [Num::I(0), Num::I(43), Num::I(24), Num::I(59)]),
    (0, [Num::I(24), Num::I(43), Num::I(48), Num::I(59)]),
    (0, [Num::I(48), Num::I(43), Num::F(60.0), Num::I(59)]),
    (0, [Num::F(60.0), Num::I(43), Num::I(72), Num::F(51.0)]),
    (0, [Num::F(60.0), Num::F(51.0), Num::I(72), Num::I(59)]),
    (0, [Num::I(72), Num::I(43), Num::F(84.0), Num::I(59)]),
    (0, [Num::F(84.0), Num::I(43), Num::I(96), Num::I(59)]),
    (0, [Num::I(96), Num::I(43), Num::F(108.0), Num::F(51.0)]),
    (0, [Num::I(96), Num::F(51.0), Num::F(108.0), Num::I(59)]),
    (0, [Num::F(108.0), Num::I(43), Num::I(120), Num::F(51.0)]),
    (0, [Num::F(108.0), Num::F(51.0), Num::I(120), Num::I(59)]),
    (0, [Num::I(120), Num::I(43), Num::F(132.0), Num::F(51.0)]),
    (0, [Num::I(120), Num::F(51.0), Num::F(132.0), Num::I(59)]),
    (0, [Num::F(132.0), Num::I(43), Num::I(144), Num::I(59)]),
    (0, [Num::I(144), Num::I(43), Num::F(156.0), Num::I(59)]),
    (0, [Num::F(156.0), Num::I(43), Num::I(168), Num::I(59)]),
    (0, [Num::I(168), Num::I(43), Num::I(192), Num::I(59)]),
    (0, [Num::I(192), Num::I(43), Num::I(210), Num::I(59)]),
    (0, [Num::I(0), Num::I(59), Num::I(24), Num::I(75)]),
    (0, [Num::I(24), Num::I(59), Num::I(48), Num::I(75)]),
    (0, [Num::I(48), Num::I(59), Num::I(72), Num::I(75)]),
    (0, [Num::I(72), Num::I(59), Num::I(96), Num::I(75)]),
    (0, [Num::I(96), Num::I(59), Num::I(120), Num::I(75)]),
    (0, [Num::I(120), Num::I(59), Num::I(144), Num::I(75)]),
    (0, [Num::I(144), Num::I(59), Num::I(168), Num::I(75)]),
    (0, [Num::I(168), Num::I(59), Num::I(192), Num::I(75)]),
    (0, [Num::I(192), Num::I(59), Num::I(210), Num::I(75)]),
    (0, [Num::I(0), Num::I(75), Num::I(24), Num::I(91)]),
    (0, [Num::I(24), Num::I(75), Num::I(48), Num::I(91)]),
    (0, [Num::I(48), Num::I(75), Num::I(72), Num::I(91)]),
    (0, [Num::I(72), Num::I(75), Num::I(96), Num::I(91)]),
    (0, [Num::I(96), Num::I(75), Num::I(120), Num::I(91)]),
    (0, [Num::I(120), Num::I(75), Num::I(144), Num::I(91)]),
    (0, [Num::I(144), Num::I(75), Num::I(168), Num::I(91)]),
    (0, [Num::I(168), Num::I(75), Num::I(192), Num::I(91)]),
    (0, [Num::I(192), Num::I(75), Num::I(210), Num::I(91)]),
    (0, [Num::I(0), Num::I(91), Num::I(24), Num::I(100)]),
    (0, [Num::I(24), Num::I(91), Num::I(48), Num::I(100)]),
    (0, [Num::I(48), Num::I(91), Num::I(72), Num::I(100)]),
    (0, [Num::I(72), Num::I(91), Num::I(96), Num::I(100)]),
    (0, [Num::I(96), Num::I(91), Num::I(120), Num::I(100)]),
    (0, [Num::I(120), Num::I(91), Num::I(144), Num::I(100)]),
    (0, [Num::I(144), Num::I(91), Num::I(168), Num::I(100)]),
    (0, [Num::I(168), Num::I(91), Num::I(192), Num::I(100)]),
    (0, [Num::I(192), Num::I(91), Num::I(210), Num::I(100)]),
    (1, [Num::I(0), Num::I(-5), Num::I(24), Num::I(11)]),
    (1, [Num::I(24), Num::I(-5), Num::I(48), Num::I(11)]),
    (1, [Num::I(0), Num::I(11), Num::I(24), Num::I(27)]),
    (1, [Num::I(24), Num::I(11), Num::I(48), Num::I(27)]),
    (1, [Num::I(0), Num::I(27), Num::I(24), Num::I(43)]),
    (1, [Num::I(24), Num::I(27), Num::I(48), Num::I(43)]),
    (1, [Num::I(0), Num::I(43), Num::I(24), Num::I(59)]),
    (1, [Num::I(24), Num::I(43), Num::I(48), Num::I(59)]),
    (1, [Num::I(0), Num::I(59), Num::I(24), Num::I(75)]),
    (1, [Num::I(24), Num::I(59), Num::I(48), Num::I(75)]),
    (1, [Num::I(0), Num::I(75), Num::I(24), Num::I(76)]),
    (1, [Num::I(24), Num::I(75), Num::I(48), Num::I(76)]),
];

/// `MEASURED_VIEW_LAYOUTS`: scene name to its measured view boxes.
pub const MEASURED_VIEW_LAYOUTS: [(&str, &[Bounds]); 2] = [
    (
        "Fungus1_02",
        &[
            [Num::I(-2), Num::I(-5), Num::I(22), Num::I(11)],
            [Num::I(22), Num::I(-5), Num::I(46), Num::I(11)],
            [Num::I(46), Num::I(-5), Num::I(48), Num::I(11)],
            [Num::I(-2), Num::I(11), Num::I(22), Num::I(27)],
            [Num::I(22), Num::I(11), Num::I(46), Num::I(27)],
            [Num::I(46), Num::I(11), Num::I(48), Num::I(27)],
            [Num::I(-2), Num::I(27), Num::I(10), Num::I(43)],
            [Num::I(10), Num::I(27), Num::I(22), Num::I(35)],
            [Num::I(10), Num::I(35), Num::I(22), Num::I(43)],
            [Num::I(22), Num::I(27), Num::I(34), Num::I(35)],
            [Num::I(22), Num::I(35), Num::I(34), Num::I(43)],
            [Num::I(34), Num::I(27), Num::I(46), Num::I(43)],
            [Num::I(46), Num::I(27), Num::I(48), Num::I(43)],
            [Num::I(-2), Num::I(43), Num::I(10), Num::I(59)],
            [Num::I(10), Num::I(43), Num::I(22), Num::I(51)],
            [Num::I(10), Num::I(51), Num::I(22), Num::I(59)],
            [Num::I(22), Num::I(43), Num::I(46), Num::I(59)],
            [Num::I(46), Num::I(43), Num::I(48), Num::I(59)],
            [Num::I(-2), Num::I(59), Num::I(22), Num::I(72)],
            [Num::I(22), Num::I(59), Num::I(46), Num::I(72)],
            [Num::I(46), Num::I(59), Num::I(48), Num::I(72)],
        ],
    ),
    (
        "Fungus1_19",
        &[
            [Num::I(-2), Num::I(-6), Num::I(22), Num::I(10)],
            [Num::I(22), Num::I(-6), Num::I(46), Num::I(10)],
            [Num::I(46), Num::I(-6), Num::I(58), Num::I(10)],
            [Num::I(58), Num::I(-6), Num::I(70), Num::I(10)],
            [Num::I(70), Num::I(-6), Num::I(92), Num::I(10)],
            [Num::I(-2), Num::I(10), Num::I(22), Num::I(25)],
            [Num::I(22), Num::I(10), Num::I(46), Num::I(25)],
            [Num::I(46), Num::I(10), Num::I(70), Num::I(25)],
            [Num::I(70), Num::I(10), Num::I(92), Num::I(25)],
        ],
    ),
];

/// `TOWN_EXTENSION_LAYOUT`: (bounds, camera).
pub const TOWN_EXTENSION_LAYOUT: [(Bounds, Bounds); 8] = [
    (
        [Num::I(48), Num::I(-5), Num::I(96), Num::I(40)],
        [Num::I(40), Num::I(8), Num::I(96), Num::I(42)],
    ),
    (
        [Num::I(96), Num::I(-5), Num::I(168), Num::I(40)],
        [Num::I(96), Num::I(8), Num::I(168), Num::I(42)],
    ),
    (
        [Num::I(168), Num::I(-5), Num::I(216), Num::I(40)],
        [Num::I(168), Num::I(8), Num::I(216), Num::I(42)],
    ),
    (
        [Num::I(216), Num::I(-5), Num::I(270), Num::I(40)],
        [Num::I(216), Num::I(8), Num::I(258), Num::I(42)],
    ),
    (
        [Num::I(216), Num::I(40), Num::I(270), Num::I(76)],
        [Num::I(216), Num::I(32), Num::I(258), Num::I(68)],
    ),
    (
        [Num::I(48), Num::I(40), Num::I(96), Num::I(76)],
        [Num::I(48), Num::I(32), Num::I(96), Num::I(68)],
    ),
    (
        [Num::I(96), Num::I(40), Num::I(168), Num::I(76)],
        [Num::I(96), Num::I(32), Num::I(168), Num::I(68)],
    ),
    (
        [Num::I(168), Num::I(40), Num::I(216), Num::I(76)],
        [Num::I(168), Num::I(32), Num::I(216), Num::I(68)],
    ),
];

/// `SCENERY_SCENE_CAPS.get(scene_name, SCENERY_TEXEL_CAP)`.
pub fn scenery_cap(scene_name: &str) -> i64 {
    SCENERY_SCENE_CAPS
        .iter()
        .find(|(n, _)| *n == scene_name)
        .map_or(SCENERY_TEXEL_CAP, |(_, c)| *c)
}

/// `grid_layout(runtime_bounds, step=(24, 16))`: the measured 24x16 view stepping over a scene's
/// runtime envelope. Python's `range` over the integer corners; every `runtime_bounds` is integral.
pub fn grid_layout(runtime_bounds: [i64; 4]) -> Vec<[i64; 4]> {
    let [left, bottom, right, top] = runtime_bounds;
    let mut out = Vec::new();
    let mut y = bottom;
    while y < top {
        let mut x = left;
        while x < right {
            out.push([x, y, (x + 24).min(right), (y + 16).min(top)]);
            x += 24;
        }
        y += 16;
    }
    out
}

/// `GRID_SCENE_LAYOUTS`: for every scene but the first two, its measured layout or the 24x16 grid;
/// keyed by scene id, ascending.
pub fn grid_scene_layouts() -> Vec<(usize, Vec<Bounds>)> {
    SCENE_TABLE[2..]
        .iter()
        .map(|scene| {
            let boxes = match MEASURED_VIEW_LAYOUTS
                .iter()
                .find(|(n, _)| *n == scene.scene_name)
            {
                Some((_, b)) => b.to_vec(),
                None => {
                    let rb = scene.runtime_bounds.map(|v| match v {
                        Num::I(i) => i,
                        Num::F(f) => f as i64,
                    });
                    grid_layout(rb).into_iter().map(|b| b.map(Num::I)).collect()
                }
            };
            (scene.scene_id, boxes)
        })
        .collect()
}

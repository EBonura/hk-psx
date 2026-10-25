//! The chain's transfer, measured: for each channel the (value the encoder is given, value the PlayStation
//! shows) points of the intro build before this correction was last refined (bayer16 dither, paced mux,
//! the correction of the first measurement in front), from 18 frames of both movies matched to the clips
//! (`new-game-movie-frames`), in bins of eight source values on the emulator's display (its 5 bit expansion
//! is `v << 3 | v >> 2`). The correction is the inverse of this. Measured on the Intro's frames (the larger
//! sample); the Prologue's agrees within a step. The first measurement (the ordered dither of swscale and no
//! correction) found the chain 5% bright and 1 to 3 steps green-high; the second (here) found the first
//! correction 2% too strong, the channels level.

pub const RED: [(f64, f64); 32] = [
    (2.0, 1.4), (11.0, 10.1), (21.0, 19.5), (26.0, 25.5), (35.0, 34.8), (43.0, 43.1), (51.0, 51.0),
    (57.0, 57.4), (64.0, 65.0), (71.0, 72.8), (79.0, 81.4), (87.0, 89.0), (94.0, 97.6),
    (102.0, 105.1), (111.0, 113.4), (119.0, 120.7), (125.0, 127.4), (133.0, 135.7), (141.0, 143.4),
    (150.0, 152.1), (156.0, 159.1), (163.0, 168.1), (171.0, 175.1), (178.0, 182.7), (185.0, 189.3),
    (193.0, 197.2), (201.0, 206.1), (209.0, 213.8), (215.0, 220.9), (224.0, 229.8), (232.0, 238.3),
    (239.0, 245.2),
];
pub const GREEN: [(f64, f64); 30] = [
    (1.0, 1.5), (10.0, 9.9), (18.0, 18.2), (26.0, 26.3), (33.0, 34.0), (42.0, 42.3), (49.0, 50.0),
    (59.0, 59.4), (63.0, 65.4), (69.0, 71.9), (79.0, 80.9), (86.0, 89.1), (93.0, 97.6),
    (102.0, 105.8), (109.0, 112.7), (116.0, 120.0), (124.0, 127.8), (129.0, 134.4), (139.0, 144.0),
    (146.0, 149.9), (154.0, 158.9), (162.0, 167.8), (171.0, 176.6), (178.0, 182.6), (183.0, 188.9),
    (190.0, 196.7), (197.0, 204.9), (206.0, 213.6), (215.0, 221.3), (223.0, 230.4),
];
pub const BLUE: [(f64, f64); 31] = [
    (1.0, 1.2), (12.0, 11.0), (20.0, 18.0), (26.0, 25.5), (34.0, 34.3), (43.0, 42.9), (51.0, 50.8),
    (58.0, 58.2), (66.0, 65.8), (72.0, 73.0), (81.0, 82.4), (87.0, 88.7), (94.0, 96.3),
    (103.0, 105.5), (110.0, 113.4), (118.0, 120.6), (125.0, 127.3), (132.0, 135.4), (141.0, 143.7),
    (148.0, 150.1), (156.0, 159.2), (163.0, 168.0), (170.0, 174.3), (179.0, 182.5), (184.0, 187.9),
    (192.0, 197.1), (200.0, 206.0), (208.0, 213.6), (215.0, 221.0), (224.0, 231.0), (228.0, 235.3),
];

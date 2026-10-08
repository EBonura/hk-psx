"""Measured scenery quality and stable development-region residency layout.

Only static SpriteRenderer scenery uses this cap. Knight/nail/enemy animation
sampling and HUD quality remain unchanged. The first 98 region IDs remain
stable. Eight Town views append full supported scenery/collision coverage;
spatial partitions do not implement source scripts.
"""
# Scene catalog: every cooker, generated table and guest constant derives its
# scene list from here. Runtime/camera envelopes follow tools/cook_scene_pack.py.
#
# Before adding a row, measure the candidate with tools/cook_scene_pack.py and
# read its per-view numbers: the 416-slot TEXTURE_BUDGET below is per view, so a
# scene's answer is its worst view, not its total. Appending keeps every existing
# chunk id, because fixed_regions() lays the grid scenes out in scene_id order
# and cook_fingerprints() deliberately leaves this file out of the cache key.
#
# Room_shop (level17), Sly's shop and the only scene that makes P17's purchases
# reachable, fits with room to spare: 6 views, chunks 694 to 699, tightest view
# 87 of 416 CLUT slots, 1 of 5 pages, 70,372 of 393,216 room bytes, a
# 71,656-byte scene bank, decoder and packet bound PASS, 7.8 s to cook. Its way
# in is Dirtmouth's door_sly, whose serialized targetScene is empty because its
# Door Control FSM holds the destination; `regions.door_destination` reads it,
# so the gate cooks. See out_of_reach in .hkpsx/shop-catalog.json.
#
# That 87 was written here as 90 until it was re-measured against the cooked
# packs. 90 is the view's texture count, which is bounded separately at 640 by
# what HKROOM02's table can address; the 416 below counts distinct palettes.
# The two were the same number before frames started tiling and are not now, so
# read tools/texture_headroom.py's `cluts` column and not its `textures` one.
SCENE_TABLE = (
    {'scene_id': 0, 'scene_name': 'Tutorial_01', 'file': 'level6',
     'runtime_bounds': [0,-5,210,100], 'camera_global_bounds': [10,0,196,92]},
    {'scene_id': 1, 'scene_name': 'Town', 'file': 'level7',
     'runtime_bounds': [0,-5,270,76], 'camera_global_bounds': [10,8,258,68]},
    {'scene_id': 2, 'scene_name': 'Crossroads_01', 'file': 'level37',
     'runtime_bounds': [-2,-5,102,49], 'camera_global_bounds': [0,0,100,42]},
    {'scene_id': 3, 'scene_name': 'Crossroads_02', 'file': 'level38',
     'runtime_bounds': [-2,-5,92,36], 'camera_global_bounds': [0,0,90,30]},
    {'scene_id': 4, 'scene_name': 'Crossroads_07', 'file': 'level43',
     'runtime_bounds': [-2,-5,45,116], 'camera_global_bounds': [0,0,43,110]},
    {'scene_id': 5, 'scene_name': 'Crossroads_03', 'file': 'level39',
     'runtime_bounds': [-2, -6, 32, 77], 'camera_global_bounds': [0, 0, 30, 72]},
    {'scene_id': 6, 'scene_name': 'Crossroads_04', 'file': 'level40',
     'runtime_bounds': [-2, -5, 162, 32], 'camera_global_bounds': [0, 0, 160, 30]},
    {'scene_id': 7, 'scene_name': 'Crossroads_05', 'file': 'level41',
     'runtime_bounds': [-2, -5, 77, 26], 'camera_global_bounds': [0, 0, 75, 25]},
    {'scene_id': 8, 'scene_name': 'Crossroads_06', 'file': 'level42',
     'runtime_bounds': [-2, -5, 62, 61], 'camera_global_bounds': [0, 0, 60, 60]},
    {'scene_id': 9, 'scene_name': 'Crossroads_08', 'file': 'level44',
     'runtime_bounds': [-2, -5, 54, 44], 'camera_global_bounds': [0, 0, 52, 43]},
    {'scene_id': 10, 'scene_name': 'Crossroads_09', 'file': 'level45',
     'runtime_bounds': [-2, -5, 88, 26], 'camera_global_bounds': [0, 0, 86, 25]},
    {'scene_id': 11, 'scene_name': 'Crossroads_10', 'file': 'level46',
     'runtime_bounds': [-2, -5, 77, 69], 'camera_global_bounds': [0, 0, 75, 68]},
    {'scene_id': 12, 'scene_name': 'Crossroads_12', 'file': 'level51',
     'runtime_bounds': [-2, -5, 72, 25], 'camera_global_bounds': [0, 0, 70, 24]},
    {'scene_id': 13, 'scene_name': 'Crossroads_13', 'file': 'level52',
     'runtime_bounds': [-2, -5, 82, 48], 'camera_global_bounds': [0, 0, 80, 47]},
    {'scene_id': 14, 'scene_name': 'Crossroads_14', 'file': 'level53',
     'runtime_bounds': [-2, -5, 35, 49], 'camera_global_bounds': [0, 0, 33, 48]},
    {'scene_id': 15, 'scene_name': 'Crossroads_16', 'file': 'level55',
     'runtime_bounds': [-2, -6, 78, 28], 'camera_global_bounds': [0, 0, 76, 27]},
    {'scene_id': 16, 'scene_name': 'Crossroads_18', 'file': 'level56',
     'runtime_bounds': [-1, -6, 43, 51], 'camera_global_bounds': [0, 0, 41, 50]},
    {'scene_id': 17, 'scene_name': 'Crossroads_19', 'file': 'level57',
     'runtime_bounds': [-2, -5, 52, 50], 'camera_global_bounds': [0, 0, 50, 45]},
    {'scene_id': 18, 'scene_name': 'Crossroads_21', 'file': 'level58',
     'runtime_bounds': [-2, -5, 102, 34], 'camera_global_bounds': [0, 0, 100, 29]},    {'scene_id': 19, 'scene_name': 'Crossroads_11_alt', 'file': 'level50',
     'runtime_bounds': [-2, -5, 122, 37], 'camera_global_bounds': [0, 0, 120, 36]},
    {'scene_id': 20, 'scene_name': 'Crossroads_15', 'file': 'level54',
     'runtime_bounds': [-2, -5, 62, 19], 'camera_global_bounds': [0, 0, 60, 18]},
    {'scene_id': 21, 'scene_name': 'Crossroads_22', 'file': 'level59',
     'runtime_bounds': [-1, -6, 106, 36], 'camera_global_bounds': [0, 0, 105, 35]},
    {'scene_id': 22, 'scene_name': 'Crossroads_25', 'file': 'level60',
     'runtime_bounds': [-2, -5, 72, 28], 'camera_global_bounds': [0, 0, 70, 27]},
    {'scene_id': 23, 'scene_name': 'Crossroads_27', 'file': 'level61',
     'runtime_bounds': [-2, -6, 32, 73], 'camera_global_bounds': [0, 0, 30, 72]},
    {'scene_id': 24, 'scene_name': 'Crossroads_30', 'file': 'level62',
     'runtime_bounds': [-1, -5, 51, 24], 'camera_global_bounds': [0, 0, 50, 23]},
    {'scene_id': 25, 'scene_name': 'Crossroads_31', 'file': 'level63',
     'runtime_bounds': [-1, -5, 71, 24], 'camera_global_bounds': [0, 0, 69, 23]},
    {'scene_id': 26, 'scene_name': 'Crossroads_33', 'file': 'level64',
     'runtime_bounds': [-2, -5, 47, 54], 'camera_global_bounds': [0, 0, 45, 50]},
    {'scene_id': 27, 'scene_name': 'Crossroads_35', 'file': 'level65',
     'runtime_bounds': [-1, -6, 72, 76], 'camera_global_bounds': [0, 0, 70, 75]},
    {'scene_id': 28, 'scene_name': 'Crossroads_36', 'file': 'level66',
     'runtime_bounds': [-1, -5, 62, 59], 'camera_global_bounds': [0, 0, 60, 58]},
    {'scene_id': 29, 'scene_name': 'Crossroads_37', 'file': 'level67',
     'runtime_bounds': [-1, -5, 112, 31], 'camera_global_bounds': [0, 0, 110, 30]},
    {'scene_id': 30, 'scene_name': 'Crossroads_38', 'file': 'level68',
     'runtime_bounds': [-1, -5, 71, 26], 'camera_global_bounds': [0, 0, 70, 25]},
    {'scene_id': 31, 'scene_name': 'Crossroads_39', 'file': 'level69',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 32, 'scene_name': 'Crossroads_40', 'file': 'level70',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 33, 'scene_name': 'Crossroads_42', 'file': 'level71',
     'runtime_bounds': [-2, -5, 112, 26], 'camera_global_bounds': [0, 0, 110, 25]},
    {'scene_id': 34, 'scene_name': 'Crossroads_43', 'file': 'level72',
     'runtime_bounds': [-2, -5, 90, 26], 'camera_global_bounds': [0, 0, 88, 25]},
    {'scene_id': 35, 'scene_name': 'Crossroads_45', 'file': 'level73',
     'runtime_bounds': [-2, -5, 72, 51], 'camera_global_bounds': [0, 0, 70, 50]},
    {'scene_id': 36, 'scene_name': 'Crossroads_46', 'file': 'level74',
     'runtime_bounds': [-2, -5, 56, 26], 'camera_global_bounds': [0, 0, 55, 25]},
    {'scene_id': 37, 'scene_name': 'Crossroads_46b', 'file': 'level75',
     'runtime_bounds': [-1, -5, 57, 26], 'camera_global_bounds': [0, 0, 55, 25]},
    {'scene_id': 38, 'scene_name': 'Crossroads_47', 'file': 'level77',
     'runtime_bounds': [-1, -5, 49, 20], 'camera_global_bounds': [0, 0, 47, 19]},
    {'scene_id': 39, 'scene_name': 'Crossroads_48', 'file': 'level78',
     'runtime_bounds': [-2, -5, 58, 20], 'camera_global_bounds': [0, 0, 57, 19]},
    {'scene_id': 40, 'scene_name': 'Crossroads_49', 'file': 'level79',
     'runtime_bounds': [-2, -5, 32, 177], 'camera_global_bounds': [0, 0, 30, 176]},
    {'scene_id': 41, 'scene_name': 'Crossroads_49b', 'file': 'level80',
     'runtime_bounds': [-2, -5, 32, 87], 'camera_global_bounds': [0, 0, 30, 86]},
    {'scene_id': 42, 'scene_name': 'Crossroads_50', 'file': 'level81',
     'runtime_bounds': [-2, -5, 262, 61], 'camera_global_bounds': [0, 0, 260, 60]},
    {'scene_id': 43, 'scene_name': 'Crossroads_52', 'file': 'level82',
     'runtime_bounds': [-2, -5, 56, 71], 'camera_global_bounds': [0, 0, 55, 70]},
    {'scene_id': 44, 'scene_name': 'Crossroads_ShamanTemple', 'file': 'level76',
     'runtime_bounds': [-2, -5, 143, 76], 'camera_global_bounds': [0, 0, 142, 75]},
    # Sly's shop, reached through Dirtmouth's door_sly. Envelope and camera from
    # tools/cook_scene_pack.py; the 24x16 grid over it is the same six views that
    # measurement cooked, so its numbers carry over unchanged.
    #
    # This row stays last. A row inserted anywhere else renumbers the chunk ids
    # the packs, the guest tables and the save records all address by;
    # tests/test_regions.py is what guards that. Re-run host/cook_music.py, then
    # host/ambience.py, then host/regions.py after any change here: the first two
    # are keyed on this table (ambience checks the music provenance covers the
    # same scene list, and AMBIENCE_SCENES is indexed by scene id), so running
    # them out of order raises instead of indexing off the end. Room_shop needs
    # no new SPU audio: one SceneManager, atmos cue SurfaceInterior, channels
    # [1,3,5,6] all resident, environmentType 0.
    #
    # Growing this table can move chunks it did not add, because
    # postpack_similarity picks representatives world-wide and new textures can
    # change what an unrelated view collapses to. Admitting Room_shop did not:
    # all 693 existing chunk packs, data/room.hk and data/params.rs came back
    # byte-identical against the previous build's recorded hashes, and only
    # data/regions.json moved. Compare hashes rather than assuming either way.
    {'scene_id': 45, 'scene_name': 'Room_shop', 'file': 'level17',
     'runtime_bounds': [-1, -5, 34, 31], 'camera_global_bounds': [0, 0, 33, 30]},
    # Greenpath begins here. Thirteen unadmitted scenes sit behind an enabled
    # serialized gate from the other 46, and Fungus1_01 (level128) was the only
    # one of them in Greenpath. Its pair needs nothing the shop's door needed:
    # Crossroads_11_alt `left1` names `Fungus1_01/right1` and Fungus1_01
    # `right1` names it back, both enabled, both colliders on from frame one, no
    # `Door Control` and no runtime destination. Replaying `scene_metadata` plus
    # `world.generate`'s gate rule over the 723-region layout resolves both
    # directions: in at spawn (169.5, 9.5) as a side-1 gate landing in the new
    # chunk 707, out at spawn (0.5, 19.0) as a side-2 gate landing in the
    # existing chunk 336. No other admitted scene has a gate into Greenpath, so
    # this row adds exactly one new dangling exit, `left1` into Fungus1_01b.
    #
    # Measured by tools/cook_scene_pack.py before admission, and the 24x16 grid
    # over this envelope reproduces that run's 24 views exactly: 24 views,
    # tightest view 300 of 416 CLUT slots and 309 of 640 texture records, 4 of 5
    # pages, 205,860 of 393,216 room bytes, 167,572 resident bytes against the
    # 417,748-byte arena so the arena does not move, 482,100-byte scene bank,
    # 647 textures and 613 palettes against SCENE_TEXTURE_CAPACITY 1152, 1,842
    # draws against the world's current 3,040, decoder and packet bound PASS,
    # 51.9 s to cook. Its eight cook refusals are all classes the Crossroads
    # already refuse: seven rotated GrassCut boxes and one reveal controller.
    #
    # Two things it does not bring, recorded rather than discovered later.
    # Sixteen actor placements and none of them admitted: four Moss Walkers and
    # eight Pigeons that `walker_control` refuses as a Crawler nail response
    # variant, a Plant Trap with no Crawler FSM, and three that carry a `Walker`
    # and so do reach the Runner recognizer, which refuses two Mossman_Shakers
    # for having no single Zombie Swipe or Zombie Leap FSM and the
    # Mossman_Runner as an unverified Runner FSM variant. Unsupported actors
    # stay records and cook nothing, so the scene is walkable and empty.
    #
    # And it was silent, which was a region-wide gap rather than a scene one.
    # The SceneManager's atmos cue is `Greenpath`; the only channel it enables
    # is 7, `green_path_atmos_loop`, and the resident set at the time did not
    # carry it, so the scene cooked a zero mask and the region had no sound.
    # Channel 6 gave way to 7 for it, at no cost: both clips are 10.00 s and
    # both cooked to exactly 45,728 bytes. The set has since been raised to
    # eight channels and 7 is still in it, at 4 kHz rather than 8 now, which is
    # the one place Greenpath pays for the raise. The set itself is only ever
    # `cook_music.RESIDENT_ATMOS_CHANNELS`; the decision written above it is the
    # current one and this comment is not.
    #
    # Two figures that used to sit here were both wrong and are worth recording
    # as wrong, because they were quoted while scoping this. There was never a
    # seventh SPU voice to take: all 24 are allocated and the cook names each
    # one's owner, and what made a longer set possible was pooling ambience's
    # own six rather than finding another. And free SPU was reported as 136,704
    # bytes by a cook that counted the Focus and Runner banks above ambience as
    # headroom, when the contiguous figure was 8,032.
    {'scene_id': 46, 'scene_name': 'Fungus1_01', 'file': 'level128',
     'runtime_bounds': [-2, -5, 172, 31], 'camera_global_bounds': [0, 0, 170, 30]},
    # Greenpath's entrance corridor and the loop it opens onto: thirteen more
    # scenes, 220 views, every one of them reachable on foot from Fungus1_01.
    # Each row's envelope and camera are the ones tools/cook_scene_pack.py
    # derived, and every scene was recooked against the current cooker rather
    # than read off the packs that were on disc, which were from 16 September
    # and predate several cook changes. All thirteen cook: decoder and packet
    # bound PASS, worst view 5 of 5 pages, 373 of 416 CLUT slots (Fungus1_02),
    # 399 of 640 texture records, 249,560 of 393,216 room bytes.
    #
    # Those per-view figures are read after the scene actor bank, not before
    # it, because the bank is appended to every view of a scene once the views
    # have already been priced, and it is the one thing that can spend a view's
    # remaining CLUT slots all at once. Only Fungus1_01b has one here: its
    # Pigeons cost 16 slots, taking its tightest view from 308 to 324, and that
    # figure held while the placement count went from 11 to 13, which is the
    # per-type catalogue behaving as advertised. An actor bank is also what
    # keeps Fungus1_03 out. Free slots at each scene's tightest view, which is
    # what a new enemy family has to fit inside: Fungus1_02 43, Fungus1_19 62,
    # Fungus1_05 76, Fungus1_01b 92, Fungus1_08 107, Fungus1_30 116,
    # Fungus1_17 118, Fungus1_10 127, Fungus1_07 141, Fungus1_06 155,
    # Fungus1_15 171, Fungus1_09 198, Fungus1_14 211. For scale, the three
    # Crossroads types Fungus1_03 carries cost 115.
    #
    # The arena does not move. It is max(scene raw) + max(meta bank) + 4 and
    # both maxima are still Tutorial_01: the largest resident scene here is
    # Fungus1_10 at 174,088 against scene 0's 349,032, and the largest metadata
    # bank is Fungus1_09 at 27,396 in isolation against scene 0's 69,640.
    # Isolated banks understate the cooked one, because the pack has no
    # resolved gates to carry: Fungus1_01 measured 9,344 alone and 15,824 in
    # the world, about 1.7x, which puts Fungus1_09 near 47,000. Nothing here
    # reaches scene 0 on either term, so SCENE_ARENA_BYTES stays 418,676 and no
    # admitted scene pays for this batch. Fungus1_09 is the one to re-measure
    # first if that ever stops being true.
    #
    # Nor do the generated working-set constants grow: the widest scene bank
    # here is Fungus1_05 at 612 textures against SCENE_TEXTURE_CAPACITY 1152,
    # and the busiest view is Fungus1_08 at 671 draws against
    # SCENE_DRAW_CAPACITY 704. Per-scene VRAM peaks at 9 of 19 pages and 569 of
    # 1248 palettes (Fungus1_05).
    #
    # What stops a batch being larger is the region catalogue, not RAM.
    # `world.generate` refuses above 1024 chunks, these thirteen take the world
    # from 723 to 943, and the twenty-four Greenpath scenes that cook on the
    # plain grid would have needed 404. The bound is called a sanity bound in
    # host/regions.py and the only linked cost per view left is one byte of
    # REGION_SCENES plus four of REGION_SCENE_LOCAL, so raising it is a
    # decision someone can make on evidence; this batch does not need it.
    #
    # What stops this one is reach. Every scene here is walkable from
    # Crossroads_11_alt, and the rest of the region sits behind four doors,
    # each refused by a different clause:
    #   Fungus1_03 by the CLUT budget, but only once its actor bank is on.
    #     The bank is three Crossroads types and costs 115 slots in every view,
    #     which takes chunk 8 of its pack from 346 to 461 and chunk 3 from 305
    #     to 420, both over 416. Its views fit until then.
    #   Fungus1_11 by STATIC_PAGE_BUDGET: four of its grid views cook 6 pages.
    #   Fungus1_22 and Fungus1_31 by the draw-packet reservation, which is a
    #     fifth per-view limit and not one of the four in `over_budget`:
    #     `scenery_geometry` reserves ACTOR_RESERVE 264 of CAP 1032 and six of
    #     Fungus1_22's grid views and seven of Fungus1_31's leave less than
    #     that, the worst being Fungus1_22 [-2, 26, 22, 42] at 1018 + 264.
    # Fungus1_12, Fungus1_13, Fungus1_16_alt, Fungus1_20_v02, Fungus1_25,
    # Fungus1_26, Fungus1_29, Fungus1_34, Fungus1_35, Fungus1_36, Fungus1_37
    # and Fungus1_Slug all cook on the plain grid and wait only on those four.
    # Fungus1_23 waits on something else: its two exits both name Fungus3
    # rooms, so no Greenpath scene reaches it at all.
    {'scene_id': 47, 'scene_name': 'Fungus1_01b', 'file': 'level129',
     'runtime_bounds': [-2, -5, 47, 46], 'camera_global_bounds': [0, 0, 45, 45]},
    {'scene_id': 48, 'scene_name': 'Fungus1_02', 'file': 'level130',
     'runtime_bounds': [-2, -5, 48, 72], 'camera_global_bounds': [0, 0, 46, 71]},
    {'scene_id': 49, 'scene_name': 'Fungus1_05', 'file': 'level134',
     'runtime_bounds': [-1, -6, 33, 90], 'camera_global_bounds': [0, 0, 31, 85]},
    {'scene_id': 50, 'scene_name': 'Fungus1_06', 'file': 'level135',
     'runtime_bounds': [-2, -6, 171, 31], 'camera_global_bounds': [0, 0, 170, 30]},
    {'scene_id': 51, 'scene_name': 'Fungus1_07', 'file': 'level136',
     'runtime_bounds': [-2, -5, 72, 63], 'camera_global_bounds': [0, 0, 70, 58]},
    {'scene_id': 52, 'scene_name': 'Fungus1_08', 'file': 'level137',
     'runtime_bounds': [-2, -5, 81, 56], 'camera_global_bounds': [0, 0, 80, 55]},
    {'scene_id': 53, 'scene_name': 'Fungus1_09', 'file': 'level138',
     'runtime_bounds': [-2, -5, 252, 35], 'camera_global_bounds': [0, 0, 250, 34]},
    {'scene_id': 54, 'scene_name': 'Fungus1_10', 'file': 'level139',
     'runtime_bounds': [-2, -5, 182, 30], 'camera_global_bounds': [0, 0, 180, 25]},
    {'scene_id': 55, 'scene_name': 'Fungus1_14', 'file': 'level143',
     'runtime_bounds': [-2, -5, 101, 26], 'camera_global_bounds': [0, 0, 100, 25]},
    {'scene_id': 56, 'scene_name': 'Fungus1_15', 'file': 'level144',
     'runtime_bounds': [-1, -5, 59, 51], 'camera_global_bounds': [0, 0, 57, 50]},
    {'scene_id': 57, 'scene_name': 'Fungus1_17', 'file': 'level146',
     'runtime_bounds': [-2, -5, 82, 36], 'camera_global_bounds': [0, 0, 80, 35]},
    {'scene_id': 58, 'scene_name': 'Fungus1_19', 'file': 'level147',
     'runtime_bounds': [-2, -6, 92, 25], 'camera_global_bounds': [0, 0, 90, 24]},
    {'scene_id': 59, 'scene_name': 'Fungus1_30', 'file': 'level157',
     'runtime_bounds': [-2, -5, 102, 37], 'camera_global_bounds': [0, 0, 100, 32]},
)
SCENE_COUNT = len(SCENE_TABLE)
SCENE_FILES = tuple(scene['file'] for scene in SCENE_TABLE)
SCENERY_MAX_AXIS = 48  # hashed into the region cook key; neither scenery nor the Great Door (DOOR_MAX_AXIS) uses it
# Scenery is cooked ONE texture per (sprite, renderer alpha) per scene, sampled at the
# largest size any instance in the scene projects to, never above the sprite's own
# pixels and never above this long-axis cap. The 48-pixel cap was chosen for four
# resident 5-page banks; scene-gate residency gives a scene the 19 static pages
# pack_scenes.MAX_PAGES admits, and one texture per sprite (instead of one per
# instance size) is what makes full resolution fit. Measured (hk-texmem 2026-09-23):
# at 252 every scene fits 19 pages except Tutorial_01 and Town, whose caps below are
# the largest that fit; tools in hk-texmem/out/scenery_inventory.json list per scene.
SCENERY_TEXEL_CAP = 252
# Crossroads_50 and Fungus1_10 stay at today's 48: at full resolution their
# views need more scenery packets than a view holds (region 604: 817 + 264 >
# 1,032), and a 128 cap does not fit either (hk-seamless, 2026-09-23).
SCENERY_SCENE_CAPS = {'Tutorial_01': 96, 'Town': 160, 'Crossroads_50': 48, 'Fungus1_10': 48}
# A view's static pages live in the scene's VRAM atlas, not in any RAM arena, so a
# view may use as many as the scene can (pack_scenes.MAX_PAGES); the scene bank
# enforces the real ceiling. ROOM_BYTE_BUDGET below counts a view's bytes without
# its pages for the same reason (see regions.over_budget).
STATIC_PAGE_BUDGET = 19
TEXTURE_BUDGET = 416  # Four disjoint CLUT banks; see hk-cache::residency.
ROOM_BYTE_BUDGET = 384 * 1024
REGION_LAYOUT = (
    (0, (15, -5, 62, 25)),
    (0, (48, 11, 72, 27)),
    (0, (0, -5, 24, 11)),
    (0, (48, -5, 72, 11)),
    (0, (72, -5, 96, 11)),
    (0, (96, -5, 120, 11)),
    (0, (120, -5, 144, 11)),
    (0, (144, -5, 168, 11)),
    (0, (168, -5, 192, 11)),
    (0, (192, -5, 210, 11)),
    (0, (0, 11, 24, 27)),
    (0, (24, 11, 48, 27)),
    (0, (72, 11, 84.0, 27)),
    (0, (84.0, 11, 96, 27)),
    (0, (96, 11, 108.0, 27)),
    (0, (108.0, 11, 120, 19.0)),
    (0, (108.0, 19.0, 120, 27)),
    (0, (120, 11, 132.0, 19.0)),
    (0, (120, 19.0, 132.0, 27)),
    (0, (132.0, 11, 144, 19.0)),
    (0, (132.0, 19.0, 144, 27)),
    (0, (144, 11, 156.0, 27)),
    (0, (156.0, 11, 168, 27)),
    (0, (168, 11, 192, 27)),
    (0, (192, 11, 210, 27)),
    (0, (0, 27, 24, 43)),
    (0, (24, 27, 48, 43)),
    (0, (48, 27, 72, 43)),
    (0, (72, 27, 84.0, 43)),
    (0, (84.0, 27, 96, 43)),
    (0, (96, 27, 108.0, 43)),
    (0, (108.0, 27, 120, 43)),
    (0, (120, 27, 132.0, 35.0)),
    (0, (120, 35.0, 132.0, 43)),
    (0, (132.0, 27, 144, 35.0)),
    (0, (132.0, 35.0, 144, 43)),
    (0, (144, 27, 156.0, 35.0)),
    (0, (144, 35.0, 156.0, 43)),
    (0, (156.0, 27, 168, 43)),
    (0, (168, 27, 192, 43)),
    (0, (192, 27, 210, 43)),
    (0, (0, 43, 24, 59)),
    (0, (24, 43, 48, 59)),
    (0, (48, 43, 60.0, 59)),
    (0, (60.0, 43, 72, 51.0)),
    (0, (60.0, 51.0, 72, 59)),
    (0, (72, 43, 84.0, 59)),
    (0, (84.0, 43, 96, 59)),
    (0, (96, 43, 108.0, 51.0)),
    (0, (96, 51.0, 108.0, 59)),
    (0, (108.0, 43, 120, 51.0)),
    (0, (108.0, 51.0, 120, 59)),
    (0, (120, 43, 132.0, 51.0)),
    (0, (120, 51.0, 132.0, 59)),
    (0, (132.0, 43, 144, 59)),
    (0, (144, 43, 156.0, 59)),
    (0, (156.0, 43, 168, 59)),
    (0, (168, 43, 192, 59)),
    (0, (192, 43, 210, 59)),
    (0, (0, 59, 24, 75)),
    (0, (24, 59, 48, 75)),
    (0, (48, 59, 72, 75)),
    (0, (72, 59, 96, 75)),
    (0, (96, 59, 120, 75)),
    (0, (120, 59, 144, 75)),
    (0, (144, 59, 168, 75)),
    (0, (168, 59, 192, 75)),
    (0, (192, 59, 210, 75)),
    (0, (0, 75, 24, 91)),
    (0, (24, 75, 48, 91)),
    (0, (48, 75, 72, 91)),
    (0, (72, 75, 96, 91)),
    (0, (96, 75, 120, 91)),
    (0, (120, 75, 144, 91)),
    (0, (144, 75, 168, 91)),
    (0, (168, 75, 192, 91)),
    (0, (192, 75, 210, 91)),
    (0, (0, 91, 24, 100)),
    (0, (24, 91, 48, 100)),
    (0, (48, 91, 72, 100)),
    (0, (72, 91, 96, 100)),
    (0, (96, 91, 120, 100)),
    (0, (120, 91, 144, 100)),
    (0, (144, 91, 168, 100)),
    (0, (168, 91, 192, 100)),
    (0, (192, 91, 210, 100)),
    (1, (0, -5, 24, 11)),
    (1, (24, -5, 48, 11)),
    (1, (0, 11, 24, 27)),
    (1, (24, 11, 48, 27)),
    (1, (0, 27, 24, 43)),
    (1, (24, 27, 48, 43)),
    (1, (0, 43, 24, 59)),
    (1, (24, 43, 48, 59)),
    (1, (0, 59, 24, 75)),
    (1, (24, 59, 48, 75)),
    (1, (0, 75, 24, 76)),
    (1, (24, 75, 48, 76)),
)

# These measured views cover the rest of Town without changing the original
# first 98 chunk IDs. Cameras describe extraction coverage, not source lock FSMs.
# Lower camera tops reach activation top+2 so descending the entrance hill
# crosses into these views continuously with the fixed player camera offset.
def grid_layout(runtime_bounds, step=(24, 16)):
    """The measured 24x16 view stepping over a scene's runtime envelope."""
    left, bottom, right, top = runtime_bounds
    return tuple([x, y, min(x + step[0], right), min(y + step[1], top)]
                 for y in range(bottom, top, step[1]) for x in range(left, right, step[0]))
# Two scenes the 24x16 grid cannot hold, at the layout the cook chose instead.
#
# STATIC_PAGE_BUDGET is 5, and pages are what these two reach first. Six of
# Fungus1_02's fifteen grid views and one of Fungus1_19's eight cook 6 pages;
# CLUT slots, texture records and room bytes all still have room, and so does
# the draw-packet reservation that refuses Fungus1_22 and Fungus1_31.
#
# The 6 is not the conservative pre-similarity reading that
# tools/cook_scene_pack.py splits on: cooking the plain grid with that gate
# removed still ends at `Similarity result exceeds runtime budget in region8:
# 6pages/396textures/363cluts/281308bytes`, which is the postpass refusing its
# own output. So the grid is not a measurement that carries over for these two,
# and nothing about them would have been learned by admitting them and watching
# a whole-world recook fail twelve minutes in.
#
# What is recorded here is the layout `cook_scene` reached by halving an
# over-budget view along its longer axis and recooking, verbatim, for the two
# scenes in this batch that needed it. Both cover their envelope exactly, with
# no hole and no overlap, which is what `tests/test_regions.py` checks of every
# scene; both cook at 5 pages or fewer per view; and both are pinned by
# `test_greenpath_records_the_two_layouts_the_grid_could_not_hold` so a later
# cooker that would choose different boxes is caught here rather than trusted.
#
# Fungus1_02 costs 21 views where the grid wanted 15 and Fungus1_19 9 where it
# wanted 8. Both earn it by being doors rather than rooms: Fungus1_02 is the
# only way out of Fungus1_01b and so the only way into the region at all, and
# Fungus1_19 is the only admitted way through to Fungus1_10, Fungus1_30,
# Fungus1_05 and Fungus1_14, since the other route runs through Fungus1_03.
MEASURED_VIEW_LAYOUTS = {
    'Fungus1_02': ((-2, -5, 22, 11), (22, -5, 46, 11), (46, -5, 48, 11),
                   (-2, 11, 22, 27), (22, 11, 46, 27), (46, 11, 48, 27),
                   (-2, 27, 10, 43), (10, 27, 22, 35), (10, 35, 22, 43),
                   (22, 27, 34, 35), (22, 35, 34, 43), (34, 27, 46, 43), (46, 27, 48, 43),
                   (-2, 43, 10, 59), (10, 43, 22, 51), (10, 51, 22, 59),
                   (22, 43, 46, 59), (46, 43, 48, 59),
                   (-2, 59, 22, 72), (22, 59, 46, 72), (46, 59, 48, 72)),
    'Fungus1_19': ((-2, -6, 22, 10), (22, -6, 46, 10), (46, -6, 58, 10),
                   (58, -6, 70, 10), (70, -6, 92, 10),
                   (-2, 10, 22, 25), (22, 10, 46, 25), (46, 10, 70, 25), (70, 10, 92, 25)),
}
# Scenes after the two hand-tuned ones use the grid over their catalog envelope.
GRID_SCENE_LAYOUTS = {scene['scene_id']: MEASURED_VIEW_LAYOUTS.get(scene['scene_name'])
                      or grid_layout(scene['runtime_bounds']) for scene in SCENE_TABLE[2:]}
# The PS1 camera centre clamps 10.8 in from a scene's sides (its narrower view
# stops on the wall the original's does), so King's Pass and Town cook their
# left views from x 10. Town's reaches down to 8 because the original's does: its Well
# lock holds the camera at y 10.4, the Graveyard's at 12 and Town (1) leaves it
# the scene's own 8.3 (CameraController, hk-camera), so the lower views cook
# scenery for that viewpoint rather than vanish at the bottom of the screen.
TOWN_EXTENSION_LAYOUT = (
    ((48, -5, 96, 40), (40, 8, 96, 42)),
    ((96, -5, 168, 40), (96, 8, 168, 42)),
    ((168, -5, 216, 40), (168, 8, 216, 42)),
    ((216, -5, 270, 40), (216, 8, 258, 42)),
    ((216, 40, 270, 76), (216, 32, 258, 68)),
    ((48, 40, 96, 76), (48, 32, 96, 68)),
    ((96, 40, 168, 76), (96, 32, 168, 68)),
    ((168, 40, 216, 76), (168, 32, 216, 68)),
)

#!/usr/bin/env python3
"""Replay the single current CUE, with hash-bound input and gameplay diagnostics.

No build, disc copy, GUI or RAM patch. Captures both software and hardware views.
"""
import argparse,csv,hashlib,json,re,struct,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WATCHES='''HK_PLAYER_X HK_PLAYER_Y HK_REGION_ID HK_GAME_MODE HK_HEALTH HK_SOUL
HK_ROOM_LOAD_ERROR HK_CD_STREAM_ERROR __psx_rt_fault_count HK_SCENERY_REPAIR_FAILURES
HK_BOUNDARY_WAIT_TICKS HK_BOUNDARY_WAIT_MAX HK_INPUT_FAULT HK_INPUT_MISSED_VBLANKS HK_INPUT_QUEUE_PEAK HK_INPUT_DROPPED_SAMPLES HK_INPUT_SKIPPED_TICKS
HK_SCENE_LOADS HK_SCENE_GATE_LOADS HK_SCENE_GATE_TICKS HK_INPUT_LOADING_SAMPLES HK_INPUT_LOADING_TICKS HK_CD_SECTORS_READ HK_SCENE_SECTORS_READ HK_VRAM_UPLOAD_BYTES HK_REGION_ACTIVATIONS
HK_BREAK_COUNT HK_ENEMY_KILLS HK_CRAWLER_X HK_CRAWLER_Y HK_CRAWLER_HP HK_DEBRIS_ACTIVE HK_DEBRIS_DRAWN HK_PARTICLES_SPAWNED
HK_PARTICLES_DROPPED HK_READ_SOURCE HK_READ_PAGE HK_READ_OPENED HK_READ_CLOSED
HK_INPUT_PENDING HK_BENCH_RESTS HK_ROOM_LOAD_STATE HK_ROOM_DECODE_PHASE HK_ROOM_READ_REGION HK_ROOM_DECODE_REGION HK_REGION_LOADS HK_CD_STREAM_PHASE HK_ANIM_CACHE_MISSES HK_ANIM_UPLOAD_MAX_FRAME
HK_PAUSED HK_PAUSE_CONTROLS HK_SFX_LEVEL HK_AMBIENCE_LEVEL HK_DEATHS
HK_MENU_PAGE HK_MENU_ROW HK_MENU_SFX HK_MENU_AMBIENCE
HK_GEO_WALLET HK_GEO_HITS HK_GEO_ROCKS_DEPLETED HK_GEO_SPAWNED_VALUE HK_GEO_COLLECTED_VALUE
HK_GEO_ACTIVE HK_GEO_PENDING_VALUE HK_GEO_LOST HK_GEO_DRAWN'''.split()
WATCHES+='HK_LIFEBLOOD_OPENED HK_LIFEBLOOD_ACTIVE HK_LIFEBLOOD_STRUCK HK_LIFEBLOOD_GRANTED HK_BLUE_HEALTH HK_GEO_AUDIO_READY HK_GEO_PICKUP_SFX HK_GEO_ROCK_HIT_SFX HK_GEO_ROCK_BREAK_SFX'.split()
WATCHES+='HK_AMBIENCE_READY HK_AMBIENCE_LOADED_MASK HK_AMBIENCE_PLAYING_MASK HK_AMBIENCE_START_COUNT HK_AMBIENCE_TRANSITIONS HK_AMBIENCE_SCENE HK_AMBIENCE_FADE_TICKS'.split()
WATCHES+='HK_GREAT_DOOR_HITS HK_GREAT_DOOR_STAGE HK_GREAT_DOOR_OPENED HK_GREAT_DOOR_WAIT HK_GREAT_DOOR_ENTRY_WAIT HK_GREAT_DOOR_TRANSITIONS'.split()
WATCHES+='HK_AUDIO_STREAM_STARTS HK_AUDIO_STREAM_REFILLS HK_AUDIO_STREAM_SOURCE_WRAPS HK_AUDIO_STREAM_MAX_SERVICE_GAP HK_AUDIO_STREAM_UNDERRUNS HK_GREAT_DOOR_HIT_SFX HK_FOOTSTEP_STARTS HK_HARD_LAND_SFX'.split()
WATCHES+='HK_MUSIC_PLAYING HK_MUSIC_POSITION_SECTORS HK_MUSIC_WRAPS HK_MUSIC_ERROR HK_MUSIC_LEVEL HK_MENU_MUSIC'.split()
WATCHES+='HK_FOCUS_AUDIO_READY HK_FOCUS_CHARGE_STARTS HK_FOCUS_HEAL_SOUNDS HK_FOCUS_CHARGE_GAIN HK_FOCUS_STARTED HK_FOCUS_COMPLETED HK_FOCUS_HEALED'.split()
WATCHES+='HK_COVERAGE_LOADS HK_COVERAGE_BYTES HK_COVERAGE_SCENE'.split()
WATCHES+='HK_CHEATS HK_CHEAT_NAIL_DAMAGE HK_CHEAT_MASK_CAP HK_SAVE_WRITES HK_SAVE_ERRORS HK_SAVE_LOADED HK_SAVE_SLOT HK_SAVE_PROFILE HK_SAVE_FAULT HK_INPUT_BLOCKED_VBLANKS'.split()
WATCHES+='HK_SHADE_PRESENT HK_SHADE_GEO_POOL HK_SHADE_HP HK_SHADE_KILLS HK_SHADE_X HK_SHADE_Y HK_SHADE_DRAWN'.split()
WATCHES+='HK_RUNNER_AUDIO_READY HK_RUNNER_LOOP_STARTS HK_RUNNER_CALLS HK_RUNNER_LOOP_DROPPED HK_RUNNER_DUST_EVENTS HK_ENEMY_HITS HK_CLIMBER_X HK_CLIMBER_Y HK_CLIMBER_HP'.split()
WATCHES+='HK_NPC_SOURCE HK_NPC_PAGE HK_NPC_OPENED HK_NPC_MET HK_MET_ELDERBUG'.split()
WATCHES+='HK_SCRIPT_ACTIVE HK_SCRIPT_TRANSITIONS HK_SCRIPT_WRITES HK_SCRIPT_FLAGS HK_SCRIPT_HALTS HK_SCRIPT_NATIVE'.split()
# The only counter that says an ability clip owned the Knight's body this
# frame rather than that the Knight moved. Exported since the ability art
# landed and never watched, which left every ability route asserting a
# position instead.
WATCHES+=['HK_ABILITY_DRAWN']
# A queued display flip the GPU never released (no GP0(1Fh) reached it) and
# the present loop wrote by hand after its timeout: a frame missing its close.
WATCHES+=['HK_FLIP_TIMEOUTS']
# Hidden walls and cracked floors (secret_breaks.rs): hits the counters took,
# breaks, and swings a floor refused for the Knight standing outside its range.
WATCHES+='HK_SECRET_HITS HK_SECRET_BREAKS HK_SECRET_REFUSED HK_REVEAL_MASKS_HIDDEN HK_REVEAL_MASKS_PARTIAL HK_REVEAL_MASKS_VISIBLE'.split()
# The follow camera and the hit-flash overlays (camera.rs, render.rs).
WATCHES+='HK_CAMERA_X HK_CAMERA_Y HK_FLASH_QUADS HK_FLASH_SKIPPED HK_UI_SFX'.split()
# The drawn view (chosen from the camera) beside the Knight's HK_REGION_ID.
WATCHES+='HK_VIEW_ID HK_VIEW_BINDS HK_VIEW_ELSEWHERE_FRAMES HK_VIEW_NPC_DRAWS'.split()
# Sly's counter. The Knight stands still while the shelf is up and a purchase
# moves only the wallet, which a Geo rock also moves, so without these the
# shop has nothing a replay can assert.
WATCHES+='HK_SHOP_OPENED HK_SHOP_CLOSED HK_SHOP_PURCHASES HK_SHOP_GEO_SPENT HK_SHOP_OPEN HK_SHOP_PROMPT'.split()
# What a purchase left behind, which is the half that has to survive a quit.
WATCHES+='HK_SHOP_MASKS HK_SHOP_SHARDS HK_SHOP_SOUL_RESERVE HK_SHOP_FRAGMENTS'.split()
# The False Knight. Deaths and conversions are separate on purpose: the last
# exposure empties the Head too, and only the tail after it is a death.
WATCHES+='HK_FK_TRIGGERED HK_FK_DROPPED HK_FK_STAGGERS HK_FK_CONVERSIONS HK_FK_DEATHS HK_FK_HP HK_FK_HEAD_HP HK_FK_ACTIVE HK_FK_EXPOSED HK_FK_STUNNED HK_FK_ARENA HK_FK_ACTIVATED'.split()
WATCHES+='HK_ARENA_GATES HK_ARENA_GATE_CLOSES HK_ARENA_GATE_OPENS'.split()
# Two counters rather than one: a barrel the pool recycled under pressure
# never breaks, so spawns alone cannot tell a fight the player dodged from
# one where the rage's eight evicted each other.
WATCHES+='HK_FK_BARRELS HK_FK_BARRELS_BROKEN'.split()
# The floor `Floor Control` swaps (0 whole, 1 cracked, 2 broken) and the nail
# hits the exposed Head answered with `Head Hit`, the death exposure's included.
WATCHES+='HK_FK_FLOOR HK_FK_HEAD_HITS'.split()
# The slam's Shockwave Wave: spawns, the spurts that reached the hero, and the
# fight's CD-DA (HK_MUSIC_BOSS is 1 while it plays).
WATCHES+='HK_FK_WAVES HK_FK_WAVE_HITS HK_MUSIC_BOSS HK_MUSIC_BOSS_STARTS'.split()
# Scene one-shot banks: plays asked for that the resident bank did not hold.
WATCHES+=['HK_SCENE_SFX_MISSED']
# Grub jars broken and grubs freed (the grub-jar route pins both).
WATCHES+='HK_GRUBS HK_GRUBS_FREED'.split()
# Cornifer's sale and the quick map (mapper.rs, game_map.rs), and the Snail
# Shaman's Vengeful Spirit (shaman.rs).
WATCHES+='HK_CORNIFER_TALKS HK_CORNIFER_SOLD HK_MAP_OPENS HK_MAP_ROOMS_DRAWN HK_MAP_RELOADS'.split()
WATCHES+='HK_SHAMAN_STATE HK_SHAMAN_TALKS HK_SPELL_EARNED'.split()
# The Elder Baldur's spat rollers (blocker_roller.rs).
WATCHES+='HK_ROLLERS_SPAWNED HK_ROLLERS_KILLED HK_ROLLER_HITS HK_BLOCKERS_DEAD'.split()
# The world the save record carries (game/src/persist.rs), what a load put
# back, and the cheat bits the False Knight died under.
WATCHES+='HK_WORLD_ITEMS HK_WORLD_PLAYER HK_WORLD_RESTORED HK_WORLD_OVERFLOW HK_SAVE_BYTES HK_FK_KILL_CHEATS'.split()
# Brooding Mawlek (enemies.rs) and the CD-DA track its fight started.
WATCHES+='HK_MW_WOKEN HK_MW_DEATHS HK_MW_KILL_CHEATS HK_MW_BLOWN HK_MW_HP HK_MW_PHASE HK_MW_ACTIVE HK_MW_ARENA HK_MW_ACTIVATED'.split()
WATCHES+='HK_GZ_WOKEN HK_GZ_CHARGES HK_GZ_SLAMS HK_GZ_HITS HK_GZ_DEATHS HK_GZ_KILL_CHEATS HK_GZ_BLOWN HK_GZ_RELEASED HK_GZ_FLY_DEATHS HK_GZ_PHASE HK_GZ_ARENA HK_GZ_ACTIVATED'.split()
WATCHES+='HK_MW_X HK_MW_Y HK_MW_SPRAYS HK_MW_LEAPS HK_MW_SWIPES HK_MW_PARRIES HK_MW_HEAD_SHOTS HK_MW_HITS HK_MW_SHOT_HITS HK_MUSIC_BOSS_TRACK'.split()

def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--tape',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--screenshot-interval',type=int,default=60)
    p.add_argument('--frontend',type=Path,default=ROOT.parent/'PSoXide-emulator/target/release/frontend')
    p.add_argument('--memcard',type=Path,help='port 1 card image; a missing file boots freshly formatted and is written back')
    a=p.parse_args();r=json.loads((ROOT/'.hkpsx/build.json').read_text())
    cue=Path(r['artifacts']['cue']);exe=Path(r['artifacts']['exe']);mp=Path(r['link_map']['path'])
    if cue.resolve()!=(Path.home()/'Downloads/ps1 games/hk-psx.cue').resolve():raise ValueError('Not the canonical disc')
    for path,info in r['outputs'].items():
        if digest(Path(path))!=info['sha256']:raise ValueError('Build artifact changed: '+path)
    if digest(mp)!=r['link_map']['sha256']:raise ValueError('Build map changed')
    tape=a.tape.resolve();data=tape.read_bytes()
    if data[:8]!=b'PXITAPE2':raise ValueError('Not a poll-bound tape')
    count,start=struct.unpack_from('<II',data,8)
    if len(data)!=16+count*6 or a.screenshot_interval<=0:raise ValueError('Invalid tape/capture interval')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    help_text=subprocess.check_output([str(a.frontend),'launch','--help'],text=True)
    for flag in ('--embedded-playtest','--stop-at-poll','--dump-hw','--dump-spu-ram','--route-watch-u32'):
        if flag not in help_text:raise ValueError('Unsupported frontend flag '+flag)
    (out/'frontend-help.txt').write_text(help_text)
    command=[str(a.frontend),'launch','--path',str(cue),'--embedded-playtest',
        '--config-dir',str(out/'emulator'),'--steps','6000000000','--input-tape',str(tape),
        '--stop-at-poll',str(start+count),'--route-screenshot-interval',str(a.screenshot_interval)]
    for flag,name in {'route-log':'route.csv','route-screenshot-dir':'screenshots','dump-display':'display.ppm',
        'dump-hw':'hw.ppm','dump-vram':'vram.ppm','dump-spu-ram':'spu.bin','dump-ram':'ram.bin','dump-audio':'audio.wav','cd-command-log':'cd.csv'}.items():
        command+=['--'+flag,str(out/name)]
    symbols={s[-1]:'0x'+s[0] for line in mp.read_text().splitlines()if len(s:=line.split())==5}
    watches={n:symbols[n] for n in WATCHES if n in symbols}
    for addr in watches.values():command+=['--route-watch-u32',addr]
    if a.memcard:command+=['--memcard',str(a.memcard.resolve())]
    paths=[cue,Path(r['artifacts']['bin']),exe,mp,tape,a.frontend]
    def inputs():return {str(path):{'bytes':path.stat().st_size,'sha256':digest(path)}for path in paths}
    before=inputs();report={'command':command,'inputs':before,'watches':watches,'samples':count,'start_poll':start}
    (out/'command.json').write_text(json.dumps(report,indent=2)+'\n');(out/'build-report.json').write_text(json.dumps(r,indent=2)+'\n')
    (out/'game.map').write_bytes(mp.read_bytes())
    with (out/'replay.log').open('w')as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    report['inputs_unchanged']=inputs()==before;report['exit_code']=result.returncode
    ram=(out/'ram.bin').read_bytes()
    report['final_ram']={n:struct.unpack_from('<I',ram,int(addr,16)&0x1fffff)[0]for n,addr in watches.items()}
    rows=list(csv.DictReader((out/'route.csv').open()))
    report['maximum_observed']={n:max([int(row['ram_'+addr[2:]])for row in rows]+[report['final_ram'][n]])for n,addr in watches.items()}
    report['stop']=re.findall(r'route-ticks=(\d+)\s+port1-polls=(\d+)',(out/'replay.log').read_text())
    report['completed']=bool(report['stop'])and int(report['stop'][-1][1])==start+count
    faults=[n for n in ('HK_ROOM_LOAD_ERROR','HK_CD_STREAM_ERROR','__psx_rt_fault_count','HK_SCENERY_REPAIR_FAILURES','HK_INPUT_FAULT','HK_INPUT_MISSED_VBLANKS','HK_INPUT_DROPPED_SAMPLES','HK_INPUT_SKIPPED_TICKS','HK_AUDIO_STREAM_UNDERRUNS','HK_MUSIC_ERROR','HK_FLIP_TIMEOUTS')if report['maximum_observed'].get(n,0)!=0]
    report['faults']=faults
    (out/'command.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k]for k in ('completed','exit_code','faults','final_ram')},indent=2))
    if result.returncode or not report['completed']or not report['inputs_unchanged']or faults:raise SystemExit(1)
if __name__=='__main__':main()

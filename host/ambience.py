"""ADPCM block helpers shared with the Runner cook (runner_audio.py).

The ambience cook itself is Rust now (host/hk-cook/src/ambience.rs). This file
keeps the checksum and loop-payload helpers runner_audio.py, build_guest.py and
game_map.py still import.
"""

def fnv(data):
    value=0x811c9dc5
    for byte in data:value=((value^byte)*0x01000193)&0xffffffff
    return value

def validate_blocks(data):
    if not data or len(data)%16:raise ValueError('partial or empty ADPCM blocks')
    if data[0]>>4:raise ValueError('initial ADPCM predictor must be zero')
    for i in range(0,len(data),16):
        if data[i]>>4>4 or data[i]&15>12:raise ValueError('invalid ADPCM header')

def loop_payload(data):
    validate_blocks(data)
    if any(data[i+1] for i in range(0,len(data),16)):raise ValueError('input contains transport flags')
    result=bytearray(data);result[1]=4;result[-15]|=3
    validate_loop(result)
    return bytes(result)

def validate_loop(data):
    validate_blocks(data)
    for i in range(0,len(data),16):
        expected=(4 if i==0 else 0)|(3 if i==len(data)-16 else 0)
        if data[i+1]!=expected:raise ValueError('invalid loop start/end flags')

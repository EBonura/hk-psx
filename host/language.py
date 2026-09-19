"""Decrypted localization sheets, in one place.

Three extractors needed these independently and each grew its own copy: one
carried the key as a literal, one derived it from the assembly, and the third
imported the second's private helper. A literal key goes stale silently the day
the install changes, so the key is always derived here and never written down.

`Language.GetLanguageFileContents` loads `Languages/<code>_<sheet>` and hands it
to `Encryption.Decrypt`: base64, then AES-256 in ECB with PKCS7 padding. openssl
does the block cipher, the way the cooking tools already lean on ffmpeg and
clang.
"""
import html, json, re, subprocess
from functools import lru_cache
from pathlib import Path
from inspect_il import inspect
from source import ROOT

def managed_directory():
    """The installed Managed directory, from the doctor report."""
    data = json.load(open(ROOT / '.hkpsx/doctor.json'))['installs'][0]['data_directory']
    return Path(data) / 'Managed'

@lru_cache(maxsize=1)
def encryption_key(managed=None):
    """The language key, read from Encryption's static constructor."""
    managed = Path(managed) if managed else managed_directory()
    body = inspect(managed / 'TeamCherry.SharedUtils.dll', {'Encryption'}).split('Encryption::.cctor')[1]
    keys = re.findall(r"ldstr\s+'([^']+)'", body)
    assert len(keys) == 1 and len(keys[0]) == 32, 'the language key is no longer one 32 byte literal'
    return keys[0]

def sheet(source, name, language='EN'):
    """One language sheet, decrypted and parsed into `{key: text}`."""
    key = encryption_key()
    wanted = f'{language}_{name}'
    file = source.file('resources.assets')
    for obj in file.objects.values():
        if obj.type.name != 'TextAsset':
            continue
        asset = obj.read()
        if asset.m_Name != wanted:
            continue
        cipher = asset.m_Script
        if isinstance(cipher, str):
            cipher = cipher.encode('utf8', 'surrogateescape')
        plain = subprocess.run(
            ['openssl', 'enc', '-d', '-aes-256-ecb', '-K', key.encode('utf8').hex(), '-base64', '-A'],
            input=bytes(cipher), stdout=subprocess.PIPE, check=True).stdout.decode('utf8')
        entries = re.findall(r'<entry name="([^"]+)">(.*?)</entry>', plain, re.S)
        assert entries, f'{wanted} decrypted without entries'
        return {k: html.unescape(v).strip() for k, v in entries}
    raise LookupError(f'no {wanted} language sheet')

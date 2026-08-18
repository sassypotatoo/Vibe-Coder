#!/usr/bin/env python3
from __future__ import annotations
import hashlib, sys, zipfile
from pathlib import Path
ENTRY='lib/arm64-v8a/libvibecoder_jcode_exec.so'

def fail(msg): raise SystemExit('verify_jcode_runtime_split: '+msg)
def sha_bytes(data): return hashlib.sha256(data).hexdigest()
def sha_file(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for c in iter(lambda:f.read(1024*1024),b''): h.update(c)
    return h.hexdigest()
if len(sys.argv) not in (2,3): fail('usage:SPLIT_APK [EXPECTED_JCODE_BINARY]')
apk=Path(sys.argv[1]); expected=Path(sys.argv[2]) if len(sys.argv)==3 else None
if not apk.is_file() or apk.stat().st_size<=0: fail('split_apk_missing')
try:
    with zipfile.ZipFile(apk) as z:
        names=set(z.namelist())
        if ENTRY not in names: fail('jcode_native_entry_missing')
        unexpected=[n for n in names if n.startswith('lib/') and n.endswith('.so') and n!=ENTRY]
        if unexpected: fail('unexpected_native_entries:'+','.join(sorted(unexpected)))
        data=z.read(ENTRY)
except zipfile.BadZipFile:
    fail('split_apk_not_zip')
if not data: fail('jcode_native_entry_empty')
if expected is not None:
    if not expected.is_file(): fail('expected_jcode_missing')
    if sha_bytes(data)!=sha_file(expected): fail('jcode_payload_hash_mismatch')
print(f'Jcode runtime split verification PASSED\napk_sha256={sha_file(apk)}\njcode_sha256={sha_bytes(data)}\nfile={apk}')

#!/usr/bin/env python3
from __future__ import annotations
import hashlib, os, shutil, subprocess, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
DEST=ROOT/'android/jcode_runtime/build/generated/jniLibs/arm64-v8a/libvibecoder_jcode_exec.so'

def fail(msg): raise SystemExit('stage_jcode_runtime_split: '+msg)
def sha(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''): h.update(chunk)
    return h.hexdigest()
if len(sys.argv)!=2: fail('usage:JCODE_BINARY')
src=Path(sys.argv[1]).resolve()
if not src.is_file() or src.stat().st_size<=0: fail('jcode_binary_missing')
subprocess.run([sys.executable,str(ROOT/'scripts/verify_android_elf.py'),str(src)],cwd=ROOT,check=True,stdout=subprocess.DEVNULL)
DEST.parent.mkdir(parents=True,exist_ok=True)
shutil.copyfile(src,DEST); os.chmod(DEST,0o644)
print(f'jcode_runtime_split_staged sha256={sha(DEST)} size={DEST.stat().st_size} file={DEST}')

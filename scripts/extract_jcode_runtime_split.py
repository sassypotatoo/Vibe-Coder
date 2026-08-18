#!/usr/bin/env python3
from __future__ import annotations
import io, sys, zipfile
from pathlib import Path
ENTRY='lib/arm64-v8a/libvibecoder_jcode_exec.so'

def fail(msg): raise SystemExit('extract_jcode_runtime_split: '+msg)
if len(sys.argv)!=3: fail('usage:APKS OUTPUT_APK')
apks=Path(sys.argv[1]); out=Path(sys.argv[2])
if not apks.is_file(): fail('apks_missing')
candidates=[]
with zipfile.ZipFile(apks) as archive:
    for name in archive.namelist():
        if not name.lower().endswith('.apk') or 'jcode_runtime' not in name:
            continue
        data=archive.read(name)
        try:
            with zipfile.ZipFile(io.BytesIO(data)) as apk:
                if ENTRY in apk.namelist(): candidates.append((name,data))
        except zipfile.BadZipFile:
            continue
if len(candidates)!=1:
    fail('expected_exactly_one_jcode_feature_apk:found='+str([n for n,_ in candidates]))
out.parent.mkdir(parents=True,exist_ok=True)
out.write_bytes(candidates[0][1])
print('jcode_runtime_split_extracted source='+candidates[0][0]+' output='+str(out))

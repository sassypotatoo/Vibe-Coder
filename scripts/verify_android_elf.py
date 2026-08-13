#!/usr/bin/env python3
"""Fail-closed ELF64/AArch64 Android executable verifier for packaged child runtimes."""
from __future__ import annotations
import argparse, struct, sys
from pathlib import Path

ELFCLASS64=2; ELFDATA2LSB=1; ET_DYN=3; EM_AARCH64=183
PT_LOAD=1; PT_INTERP=3; REQUIRED_PAGE=16*1024

class VerifyError(Exception): pass

def u16(b,o): return struct.unpack_from('<H', b, o)[0]
def u32(b,o): return struct.unpack_from('<I', b, o)[0]
def u64(b,o): return struct.unpack_from('<Q', b, o)[0]

def inspect(path: Path) -> dict:
    data=path.read_bytes()
    if len(data)<64 or data[:4]!=b'\x7fELF' or data[4]!=ELFCLASS64 or data[5]!=ELFDATA2LSB:
        raise VerifyError('not_little_endian_elf64')
    if u16(data,16)!=ET_DYN: raise VerifyError('not_pie_or_shared_et_dyn')
    if u16(data,18)!=EM_AARCH64: raise VerifyError('not_aarch64')
    phoff=u64(data,32); phentsize=u16(data,54); phnum=u16(data,56)
    if phentsize<56 or phnum<1 or phnum>256: raise VerifyError('invalid_program_header_table')
    if phoff<64 or phoff+phentsize*phnum>len(data): raise VerifyError('program_header_table_out_of_bounds')
    loads=0; page16=True; interp=None
    for i in range(phnum):
        o=phoff+i*phentsize; typ=u32(data,o); off=u64(data,o+8); vaddr=u64(data,o+16)
        filesz=u64(data,o+32); memsz=u64(data,o+40); align=u64(data,o+48)
        if off+filesz>len(data): raise VerifyError('segment_out_of_bounds')
        if typ==PT_LOAD:
            loads+=1
            if filesz>memsz: raise VerifyError('load_filesz_gt_memsz')
            if align<REQUIRED_PAGE or align & (align-1) or off%REQUIRED_PAGE != vaddr%REQUIRED_PAGE:
                page16=False
        elif typ==PT_INTERP:
            if filesz<2 or filesz>256: raise VerifyError('invalid_interp_size')
            raw=data[off:off+filesz]
            if not raw.endswith(b'\0'): raise VerifyError('interp_not_nul_terminated')
            try: interp=raw[:-1].decode('utf-8')
            except UnicodeDecodeError as e: raise VerifyError('interp_not_utf8') from e
    if not loads: raise VerifyError('no_pt_load')
    if not page16: raise VerifyError('not_16k_page_compatible')
    if interp not in (None, '/system/bin/linker64'):
        raise VerifyError(f'non_android_interpreter:{interp}')
    return {'elf64': True, 'aarch64': True, 'et_dyn': True, 'page16': True,
            'interpreter': interp, 'android_linker_compatible': True}

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('path'); args=ap.parse_args()
    path=Path(args.path)
    if not path.is_file(): print('verify_android_elf: file_missing', file=sys.stderr); return 2
    try: result=inspect(path)
    except (OSError, VerifyError) as e:
        print(f'verify_android_elf: FAILED: {e}', file=sys.stderr); return 1
    for k,v in result.items(): print(f'{k}={v}')
    return 0
if __name__=='__main__': raise SystemExit(main())

"""Embed an ICO in a Windows EXE without changing its executable code sections."""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import tempfile


def read_ico(path):
    raw = path.read_bytes()
    if len(raw) < 6:
        raise ValueError('Truncated ICO header')
    reserved, kind, count = struct.unpack_from('<HHH', raw)
    if reserved or kind != 1 or not 1 <= count <= 100:
        raise ValueError('Invalid ICO header')
    if len(raw) < 6 + count * 16:
        raise ValueError('Truncated ICO index')
    images = []
    for index in range(count):
        entry = struct.unpack_from('<BBBBHHII', raw, 6 + index * 16)
        width, height, colors, reserved, planes, bits, length, offset = entry
        if reserved or not length or offset < 6 + count * 16 or offset + length > len(raw):
            raise ValueError('Invalid ICO image range')
        images.append((entry[:6], raw[offset:offset + length]))
    return images


def windows_api():
    if os.name != 'nt':
        raise RuntimeError('EXE icon embedding requires Windows')
    api = ctypes.WinDLL('kernel32', use_last_error=True)
    api.BeginUpdateResourceW.argtypes = [wintypes.LPCWSTR, wintypes.BOOL]
    api.BeginUpdateResourceW.restype = wintypes.HANDLE
    api.UpdateResourceW.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                   wintypes.WORD, ctypes.c_void_p, wintypes.DWORD]
    api.UpdateResourceW.restype = wintypes.BOOL
    api.EndUpdateResourceW.argtypes = [wintypes.HANDLE, wintypes.BOOL]
    api.EndUpdateResourceW.restype = wintypes.BOOL
    api.LoadLibraryExW.argtypes = [wintypes.LPCWSTR, wintypes.HANDLE, wintypes.DWORD]
    api.LoadLibraryExW.restype = wintypes.HMODULE
    api.FindResourceExW.argtypes = [wintypes.HMODULE, ctypes.c_void_p, ctypes.c_void_p, wintypes.WORD]
    api.FindResourceExW.restype = wintypes.HANDLE
    api.LoadResource.argtypes = [wintypes.HMODULE, wintypes.HANDLE]
    api.LoadResource.restype = wintypes.HANDLE
    api.SizeofResource.argtypes = [wintypes.HMODULE, wintypes.HANDLE]
    api.SizeofResource.restype = wintypes.DWORD
    api.LockResource.argtypes = [wintypes.HANDLE]
    api.LockResource.restype = ctypes.c_void_p
    api.FreeLibrary.argtypes = [wintypes.HMODULE]
    api.FreeLibrary.restype = wintypes.BOOL
    return api


def checked(value):
    if not value:
        raise ctypes.WinError(ctypes.get_last_error())
    return value


def code_sections(path):
    raw = path.read_bytes()
    if raw[:2] != b'MZ' or len(raw) < 64:
        raise ValueError('Not a Windows executable')
    pe = struct.unpack_from('<I', raw, 60)[0]
    if raw[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Missing PE header')
    sections, optional_size = struct.unpack_from('<H12xH', raw, pe + 6)
    table = pe + 24 + optional_size
    result = {}
    for i in range(sections):
        entry = table + i * 40
        name = raw[entry:entry + 8].rstrip(b'\0').decode('ascii', errors='replace')
        size, offset = struct.unpack_from('<II', raw, entry + 16)
        flags = struct.unpack_from('<I', raw, entry + 36)[0]
        if flags & 0x20000000:  # IMAGE_SCN_MEM_EXECUTE
            result[name] = hashlib.sha256(raw[offset:offset + size]).hexdigest()
    if not result:
        raise ValueError('No executable code section')
    return result


def verify_resources(api, executable, group, images):
    module = checked(api.LoadLibraryExW(str(executable), None, 0x2 | 0x20))
    try:
        def resource(kind, number):
            found = checked(api.FindResourceExW(module, kind, number, 0))
            size = api.SizeofResource(module, found)
            memory = checked(api.LockResource(checked(api.LoadResource(module, found))))
            return ctypes.string_at(memory, size)
        if resource(14, 1) != group:
            raise ValueError('Embedded icon group differs from ICO')
        for number, (_, image) in enumerate(images, 1):
            if resource(3, number) != image:
                raise ValueError('Embedded icon image differs from ICO')
    finally:
        api.FreeLibrary(module)


def embed(executable, icon):
    executable = executable.resolve(strict=True)
    icon = icon.resolve(strict=True)
    if executable.suffix.lower() != '.exe':
        raise ValueError('Destination must be an EXE')
    before = code_sections(executable)
    images = read_ico(icon)
    group = struct.pack('<HHH', 0, 1, len(images))
    for number, (entry, image) in enumerate(images, 1):
        group += struct.pack('<BBBBHHIH', *entry, len(image), number)
    api = windows_api()
    handle, temporary = tempfile.mkstemp(prefix=executable.stem + '-icon-', suffix='.exe', dir=executable.parent)
    os.close(handle)
    temporary = Path(temporary).resolve()
    if temporary.parent != executable.parent:
        raise ValueError('Temporary file escaped the explicit destination directory')
    try:
        shutil.copyfile(executable, temporary)
        update = checked(api.BeginUpdateResourceW(str(temporary), False))
        try:
            for number, (_, image) in enumerate(images, 1):
                buffer = ctypes.create_string_buffer(image)
                checked(api.UpdateResourceW(update, 3, number, 0, buffer, len(image)))
            buffer = ctypes.create_string_buffer(group)
            checked(api.UpdateResourceW(update, 14, 1, 0, buffer, len(group)))
        except BaseException:
            api.EndUpdateResourceW(update, True)
            raise
        checked(api.EndUpdateResourceW(update, False))
        verify_resources(api, temporary, group, images)
        if code_sections(temporary) != before:
            raise ValueError('Executable code changed during icon embedding')
        os.replace(temporary, executable)
    finally:
        if temporary.exists():
            temporary.unlink()
    return {'iconSizes': [entry[0] or 256 for entry, _ in images],
            'embeddedImagesVerified': len(images), 'executableCodeUnchanged': True,
            'bytes': executable.stat().st_size,
            'sha256': hashlib.sha256(executable.read_bytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('executable', type=Path)
    parser.add_argument('icon', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    target = args.executable
    if args.output:
        target = args.output.resolve()
        if target.exists():
            raise FileExistsError('Output already exists; refusing to overwrite it')
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(args.executable, target)
    print(json.dumps(embed(target, args.icon)))


if __name__ == '__main__':
    main()

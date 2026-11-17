"""Independent LZ4 blocks for the native boot image and paged font."""
import ctypes
from ctypes.util import find_library
from functools import lru_cache
from pathlib import Path


@lru_cache(maxsize=1)
def _library():
    name = find_library("lz4")
    if not name:
        name = next((str(path) for path in (
            Path("/opt/homebrew/opt/lz4/lib/liblz4.dylib"),
            Path("/usr/local/opt/lz4/lib/liblz4.dylib"),
        ) if path.is_file()), None)
    if not name:
        raise RuntimeError("install liblz4 to build native DOS images")
    lib = ctypes.CDLL(name)
    lib.LZ4_compress_HC.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int,
                                    ctypes.c_int, ctypes.c_int]
    lib.LZ4_compressBound.argtypes = [ctypes.c_int]
    return lib


def compress(data):
    lib = _library()
    buf = ctypes.create_string_buffer(lib.LZ4_compressBound(len(data)))
    size = lib.LZ4_compress_HC(bytes(data), buf, len(data), len(buf), 12)
    if size <= 0:
        raise ValueError("could not compress LZ4 block")
    return buf.raw[:size]

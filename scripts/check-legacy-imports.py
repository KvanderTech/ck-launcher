"""Reject known Windows 8+ DLL/API imports in a Windows 7 distribution.

This is an import-table regression guard, not a replacement for tests on the actual OS.
It reads PE files only and never loads or executes a checked binary.
"""
import struct, sys
from pathlib import Path

DLLS = {"combase.dll", "bcryptprimitives.dll", "shcore.dll", "api-ms-win-core-synch-l1-2-0.dll"}
APIS = {"ProcessPrng", "WaitOnAddress", "WakeByAddressSingle", "WakeByAddressAll",
        "GetSystemTimePreciseAsFileTime", "GetCurrentThreadStackLimits", "GetOverlappedResultEx",
        "GetTempPath2A", "GetTempPath2W", "GetDpiForWindow", "GetDpiForSystem",
        "SetProcessDpiAwareness", "SetThreadDpiAwarenessContext", "CreateFile2",
        "SetThreadDescription", "VirtualAlloc2", "MapViewOfFile3"}

class PE:
    def __init__(self, path):
        self.data = path.read_bytes()
        if self.data[:2] != b"MZ": raise ValueError("Missing DOS header")
        pe = self.u32(0x3c)
        if self.data[pe:pe+4] != b"PE\0\0": raise ValueError("Missing PE header")
        optional = pe + 24
        magic = self.u16(optional)
        if magic not in (0x10b, 0x20b): raise ValueError("Unsupported PE format")
        self.width = 8 if magic == 0x20b else 4
        self.base = self.u64(optional+24) if self.width == 8 else self.u32(optional+28)
        self.directories = optional + (112 if self.width == 8 else 96)
        self.header_size = self.u32(optional+60)
        sections = optional + self.u16(pe+20)
        self.sections = [struct.unpack_from("<IIII", self.data, sections+i*40+8)
                         for i in range(self.u16(pe+6))]
    def u16(self, at): return struct.unpack_from("<H", self.data, at)[0]
    def u32(self, at): return struct.unpack_from("<I", self.data, at)[0]
    def u64(self, at): return struct.unpack_from("<Q", self.data, at)[0]
    def offset(self, rva):
        if rva < self.header_size: return rva
        for virtual_size, virtual, raw_size, raw in self.sections:
            if virtual <= rva < virtual + max(virtual_size, raw_size):
                position = raw + rva - virtual
                if position >= len(self.data): raise ValueError("Invalid RVA")
                return position
        raise ValueError("RVA outside image")
    def name(self, rva):
        at = self.offset(rva)
        end = self.data.find(b"\0", at, at+4096)
        if end < 0: raise ValueError("Unterminated import name")
        return self.data[at:end].decode("ascii")
    def thunks(self, rva):
        at = self.offset(rva)
        while True:
            value = self.u64(at) if self.width == 8 else self.u32(at)
            if not value: return
            if not value & (1 << (self.width*8-1)): yield self.name(value+2)
            at += self.width
    def imports(self):
        rva = self.u32(self.directories+8)
        if rva:
            at = self.offset(rva)
            while any(self.data[at:at+20]):
                original, _, _, name, first = struct.unpack_from("<IIIII", self.data, at)
                yield self.name(name), list(self.thunks(original or first))
                at += 20
        delay = self.u32(self.directories+13*8)
        if delay:
            at = self.offset(delay)
            while any(self.data[at:at+32]):
                attrs, name, _, _, names, _, _, _ = struct.unpack_from("<IIIIIIII", self.data, at)
                if not attrs & 1:
                    name -= self.base
                    names -= self.base
                yield self.name(name), list(self.thunks(names))
                at += 32

def main():
    root = Path(sys.argv[1])
    files = [root] if root.is_file() else sorted(p for p in root.rglob("*") if p.suffix.lower() in (".exe", ".dll"))
    if not files: raise SystemExit("No PE files to check")
    failures = []
    for path in files:
        for dll, names in PE(path).imports():
            if dll.lower() in DLLS or dll.lower().startswith("api-ms-win-core-winrt-"):
                failures.append(f"{path.name}: unsupported DLL {dll}")
            failures.extend(f"{path.name}: unsupported API {name}" for name in names if name in APIS)
    if failures: raise SystemExit("\n".join(failures))
    print(f"PASS: {len(files)} PE files contain no known Windows 8+ imports from the regression guard")

if __name__ == "__main__": main()

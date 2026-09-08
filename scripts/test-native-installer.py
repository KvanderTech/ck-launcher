"""Exercise NSIS safety only against a disposable, unregistered fixture package."""
import hashlib
import ctypes
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import winreg


def run(arguments, cwd, timeout=120):
    result = subprocess.run(arguments, cwd=cwd, capture_output=True, timeout=timeout,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        raise AssertionError((result.stdout + result.stderr).decode("utf-8", "replace")[-12000:])
    return result


def registry_values(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
            values = {}
            index = 0
            while True:
                try:
                    name, value, kind = winreg.EnumValue(key, index)
                except OSError:
                    return values
                values[name] = (value, kind)
                index += 1
    except FileNotFoundError:
        return None


def main():
    root = Path(__file__).resolve().parent.parent
    makensis, plugin = map(lambda value: str(Path(value).resolve()), sys.argv[1:3])
    powershell = shutil.which("pwsh") or "powershell.exe"
    scratch = root / "build" / "installer-safety"
    scratch.mkdir(parents=True, exist_ok=True)
    keys = [r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CKLauncher",
            r"Software\Classes\.mrpack", r"Software\Classes\ck-launcher\shell\open\command",
            r"Software\Classes\CKLauncher.ModrinthPack\shell\open\command"]
    before = {key: registry_values(key) for key in keys}
    with tempfile.TemporaryDirectory(prefix="case-", dir=scratch) as directory:
        case = Path(directory).resolve()
        package = case / "package"
        package.mkdir()
        assert package.is_relative_to(scratch.resolve())
        registered = (before[keys[0]] or {}).get("InstallLocation", ("", 0))[0]
        assert os.path.normcase(registered) != os.path.normcase(str(package)), "Fixture must never be registered"
        owned = ["ck-launcher-qt.exe", "ck-launcher-service.exe", "ck-launcher-updater.exe",
                 "platforms/qwindows.dll", "licenses/dependency.txt", "licenses/SHA256SUMS.txt", "LICENSE"]
        for name in owned:
            target = package / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b"inert installer fixture, not an executable\n")
        run([powershell, "-NoProfile", "-File", str(root / "scripts/build-native-installer.ps1"),
             "-PackageRoot", str(package), "-Makensis", makensis, "-ThemePluginDir", plugin,
             "-OutFile", str(case / "setup.exe")], root)
        assert (package / "uninstall.exe").is_file()
        manifest = {}
        for line in (package / "SHA256SUMS.txt").read_text(encoding="utf-8-sig").splitlines():
            digest, name = line.split("  ", 1)
            assert hashlib.sha256((package / name).read_bytes()).hexdigest() == digest
            manifest[name] = digest
        assert "uninstall.exe" in manifest, "ZIP updater must replace the old uninstaller"
        assert set(manifest) == set(owned) | {"uninstall.exe"}
        assert not any(name.endswith(".nsh") or "emit" in name for name in manifest)

        probe = case / "path-probe.exe"
        run([makensis, "/INPUTCHARSET", "UTF8", f"/DTHEME_PLUGIN_DIR={plugin}",
             f"/DPACKAGE={package}", "/DCK_PATH_CHECK", f"/DOUTFILE={probe}",
             str(root / "scripts/native-installer.nsi")], root)
        empty = case / "empty with spaces"
        empty.mkdir()
        nonempty = case / "nonempty"
        nonempty.mkdir()
        (nonempty / "keep.txt").write_text("user file", encoding="utf-8")
        checks = [(Path(os.environ["SystemDrive"] + "\\"), "unsafe"),
                  (Path(os.environ["USERPROFILE"]), "unsafe"),
                  (Path(os.environ["APPDATA"]) / "CKLauncher", "unsafe"),
                  (Path(os.environ["APPDATA"]) / "CKLauncher" / "new-child", "unsafe"),
                  (nonempty, "unsafe"), (empty, "safe"), (case / "new folder", "safe")]
        for path, expected in checks:
            # A separate probe option bypasses NSIS's own /D fallback, so even
            # forbidden roots reach the custom validator exactly as entered.
            run(f'"{probe}" /S /CKPATH="{path}"', root)
            result = (package / "path-result.txt").read_text(errors="replace").splitlines()
            assert result[0] == expected, f"{path}: {result}"
        print("PASS: installer rejects roots/data/nonempty folders and accepts separate empty paths")
        run(f'"{probe}" /S /CKPATH="{package}" /CKPACKAGECHECK=1', root)
        assert (package / "path-result.txt").read_text(errors="replace").splitlines()[0] == "safe"

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32,
                                       ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
        kernel.CreateFileW.restype = ctypes.c_void_p
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        locked = kernel.CreateFileW(str(package / "ck-launcher-service.exe"), 0x80000000, 1, None, 3, 0, None)
        assert locked != ctypes.c_void_p(-1).value
        try:
            blocked = subprocess.run(f'"{package / "uninstall.exe"}" /S _?={package}', cwd=root,
                                     timeout=30, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
            assert blocked.returncode == 2, "An in-use executable must block before any deletion"
            assert all((package / name).exists() for name in owned)
        finally:
            kernel.CloseHandle(locked)
        print("PASS: locked service blocks removal before any package file is deleted")

        outside = case / "outside"
        outside.mkdir()
        (outside / "dependency.txt").write_text("external user data", encoding="utf-8")
        alias = case / "alias"
        def make_fixture_junction(link):
            assert link.is_relative_to(case) and outside.is_relative_to(case)
            # Junctions need no symbolic-link privilege. Both ends are explicit
            # disposable fixture paths; no shell-built filesystem command runs.
            environment = dict(os.environ, CK_TEST_LINK=str(link), CK_TEST_TARGET=str(outside))
            result = subprocess.run([powershell, "-NoProfile", "-Command",
                                     "New-Item -ItemType Junction -Path $env:CK_TEST_LINK -Target $env:CK_TEST_TARGET -ErrorAction Stop | Out-Null"],
                                    cwd=root, env=environment, capture_output=True, timeout=30,
                                    creationflags=subprocess.CREATE_NO_WINDOW)
            assert result.returncode == 0, result.stderr.decode("utf-8", "replace")
            assert link.lstat().st_file_attributes & 0x400
        make_fixture_junction(alias)
        run(f'"{probe}" /S /CKPATH="{alias / "new-child"}"', root)
        assert (package / "path-result.txt").read_text(errors="replace").splitlines()[0] == "unsafe"
        (package / "licenses/dependency.txt").unlink()
        (package / "licenses/SHA256SUMS.txt").unlink()
        (package / "licenses").rmdir()
        make_fixture_junction(package / "licenses")
        run(f'"{probe}" /S /CKPATH="{package}" /CKPACKAGECHECK=1', root)
        assert (package / "path-result.txt").read_text(errors="replace").splitlines()[0] == "unsafe"
        assert (outside / "dependency.txt").read_text() == "external user data"
        print("PASS: package extraction preflight rejects an existing junction before copying files")

        (package / "user-save.txt").write_text("keep root data", encoding="utf-8")
        (package / "platforms/user-settings.txt").write_text("keep nested data", encoding="utf-8")
        # _?= fixes the test target and prevents NSIS copying/relaunching against any
        # installed program. Only this fresh, unregistered fixture can be touched.
        run(f'"{package / "uninstall.exe"}" /S _?={package}', root)
        for name in owned:
            if name.startswith("licenses/"):
                continue
            assert not (package / name).exists(), f"Owned package file remains: {name}"
        assert (package / "user-save.txt").read_text() == "keep root data"
        assert (package / "platforms/user-settings.txt").read_text() == "keep nested data"
        assert (nonempty / "keep.txt").read_text() == "user file"
        assert (outside / "dependency.txt").read_text() == "external user data"
        print("PASS: junction install paths are rejected; uninstall cannot traverse a replaced package directory")
        assert before == {key: registry_values(key) for key in keys}, "Unregistered fixture changed registry"
        print("PASS: emitted uninstaller is checksummed; removes only owned fixture files; preserves unrelated files and registry")

        invalid = case / "invalid-package"
        invalid.mkdir()
        (invalid / "bad$INSTDIR.txt").write_text("fixture", encoding="utf-8")
        rejected = subprocess.run([powershell, "-NoProfile", "-File", str(root / "scripts/write-native-package-metadata.ps1"),
                                   "-PackageRoot", str(invalid)], cwd=root, timeout=30, capture_output=True,
                                  creationflags=subprocess.CREATE_NO_WINDOW)
        assert rejected.returncode != 0 and not Path(str(invalid) + ".uninstall.nsh").exists()
        print("PASS: package manifest rejects NSIS interpolation in filenames")


if __name__ == "__main__":
    main()

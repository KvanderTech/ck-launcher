"""Check a deployed native launcher using only its own DLLs and Windows system paths."""
import os, subprocess, sys
from pathlib import Path

def main():
    package = Path(sys.argv[1]).resolve()
    screenshot = Path(sys.argv[2]).resolve()
    screenshot.parent.mkdir(parents=True, exist_ok=True)
    system = os.environ["SystemRoot"]
    env = dict(os.environ, PATH=system + r"\System32;" + system,
               QT_ASSUME_STDERR_HAS_CONSOLE="1", QT_FORCE_STDERR_LOGGING="1")
    for name in ("QT_PLUGIN_PATH", "QT_QPA_PLATFORM_PLUGIN_PATH", "QML2_IMPORT_PATH", "QML_IMPORT_PATH"):
        env.pop(name, None)
    result = subprocess.run([str(package / "ck-launcher-qt.exe"), "--smoke", "--screenshot",
                             str(screenshot), "-platform", "offscreen"], cwd=package, env=env,
                            capture_output=True, timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        print((result.stdout + result.stderr).decode("utf-8", "replace")[-16000:])
        raise SystemExit("Native startup failed: " + str(result.returncode))
    if not screenshot.is_file() or screenshot.stat().st_size < 1000:
        raise SystemExit("Native startup did not produce the expected screenshot")
    print("PASS: deployed Qt startup with isolated APPDATA and no developer tools in PATH")

if __name__ == "__main__":
    main()

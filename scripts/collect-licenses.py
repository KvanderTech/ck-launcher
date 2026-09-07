"""Collect declared licenses and upstream notice files into a native distribution."""
import json, os, re, shutil, subprocess, sys
from pathlib import Path
output = Path(sys.argv[1]) / "licenses" / "rust"
output.mkdir(parents=True, exist_ok=True)
command = ["cargo"]
if os.environ.get("CK_RUST_TOOLCHAIN"): command.append("+" + os.environ["CK_RUST_TOOLCHAIN"])
command += ["metadata", "--format-version", "1", "--locked"]
metadata = json.loads(subprocess.check_output(command, text=True, encoding="utf-8"))
entries = []
for package in metadata["packages"]:
    if not package.get("source"): continue
    name = package["name"] + "-" + package["version"]
    if not re.fullmatch(r"[a-zA-Z0-9_.+-]+", name): raise ValueError("Invalid package name")
    source = Path(package["manifest_path"]).parent
    destination = output / name
    destination.mkdir(exist_ok=True)
    notices = set()
    for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "UNLICENSE*"):
        notices.update(path for path in source.glob(pattern) if path.is_file())
    if package.get("license_file"):
        path = Path(package["license_file"])
        if not path.is_absolute(): path = source / path
        if path.is_file() and path.resolve().is_relative_to(source.resolve()): notices.add(path)
    for notice in notices: shutil.copyfile(notice, destination / notice.name)
    entries.append({"name": package["name"], "version": package["version"], "license": package.get("license"), "repository": package.get("repository"), "noticeFiles": sorted(p.name for p in notices)})
(output / "inventory.json").write_text(json.dumps(entries, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")
print("Collected notices for", len(entries), "resolved registry packages")

# The metadata inventory includes optional packages; retain the exact target feature graph too.
tree_command = ["cargo"]
if os.environ.get("CK_RUST_TOOLCHAIN"): tree_command.append("+" + os.environ["CK_RUST_TOOLCHAIN"])
tree_command += ["tree", "--locked", "-p", "ck-launcher-service", "-e", "normal,build,features"]
if os.environ.get("CK_RUST_TARGET"): tree_command += ["--target", os.environ["CK_RUST_TARGET"]]
(output / "dependency-tree.txt").write_text(subprocess.check_output(tree_command, text=True, encoding="utf-8"), encoding="utf-8")

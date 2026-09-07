# Third-party components

Qt Widgets/Core/Gui/Network are dynamically linked. Qt 6.8.3 and Qt 5.15.2 source archives and corresponding build instructions are available from https://download.qt.io/official_releases/qt/ and https://download.qt.io/archive/qt/ respectively. Users may replace compatible Qt DLLs and rebuild this frontend. This project does not impose restrictions on debugging modifications to those libraries. Qt license texts accompany the package in `licenses/qt/`.

Rust dependencies are pinned in the root Cargo.lock. `scripts/collect-licenses.py` collects their declared licenses and available license/notice files from Cargo metadata into each native distribution. That inventory includes optional dependencies in Cargo's resolved metadata; actual target features are also recorded in a dependency tree. Review notices when changing dependencies or publishing binaries.

Existing visual assets and the CK brand retain their original repository ownership. Existing SPEmotes attribution remains in `app/src/assets/emotes/README.md`. This migration does not choose or change the overall project's license on the owner's behalf.

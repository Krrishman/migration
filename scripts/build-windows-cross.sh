#!/usr/bin/env bash
# Cross-compile the portable Windows x64 build from Linux (MinGW toolchain).
#   sudo apt-get install mingw-w64 && rustup target add x86_64-pc-windows-gnu
#   ./scripts/build-windows-cross.sh
# Output: dist-portable/MigrationAssistant-<version>-win-x64.zip
# Note: the GNU build needs WebView2Loader.dll beside the exe (copied here).
# The MSVC build from scripts/build-portable.ps1 links it statically instead.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(node -p "require('./package.json').version")
npm run build
(cd src-tauri && cargo build --release --target x86_64-pc-windows-gnu --features tauri/custom-protocol)
R=src-tauri/target/x86_64-pc-windows-gnu/release
P=dist-portable/MigrationAssistant
rm -rf dist-portable && mkdir -p "$P"
cp "$R/migration-assistant.exe" "$P/MigrationAssistant.exe"
cp "$R/WebView2Loader.dll" "$P/"
(cd "$P" && sha256sum MigrationAssistant.exe WebView2Loader.dll > SHA256SUMS.txt)
(cd dist-portable && python3 -c "import shutil; shutil.make_archive('MigrationAssistant-$VERSION-win-x64','zip','.','MigrationAssistant')")
echo "Built dist-portable/MigrationAssistant-$VERSION-win-x64.zip"

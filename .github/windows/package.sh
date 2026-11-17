#!/usr/bin/env bash
# The Windows downloads from a built rustyshelf.exe: <name>-windows.zip (a RustyShelf folder holding RustyShelf.exe and
# its data folder) and <name>-windows-setup.exe (the installer, setup.iss).
#   package.sh <rustyshelf.exe> <file name start, e.g. RustyShelf-0.5.1> <version> <out folder>
set -euo pipefail
exe=$1 name=$2 version=$3 out=$4
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)

stage=$(mktemp -d)
mkdir -p "$stage/RustyShelf/data"
cp "$exe" "$stage/RustyShelf/RustyShelf.exe"
cp "$here/../../LICENSE" "$stage/RustyShelf/LICENSE.txt"
cp "$here/data-readme.txt" "$stage/RustyShelf/data/README.txt"
(cd "$stage" && 7z a -tzip -mx=9 "$out/$name-windows.zip" RustyShelf >/dev/null)
rm -rf "$stage"

iscc=$(command -v iscc || true)
for c in "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "/c/Program Files/Inno Setup 6/ISCC.exe"; do
  [ -z "$iscc" ] && [ -f "$c" ] && iscc=$c
done
if [ -z "$iscc" ]; then
  choco install innosetup --no-progress -y >/dev/null
  iscc="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
fi
exe_w=$(cygpath -w "$(cd "$(dirname "$exe")" && pwd)/$(basename "$exe")")
MSYS_NO_PATHCONV=1 "$iscc" /Q "/DAppVersion=$version" "/DExe=$exe_w" "/DOutName=$name-windows-setup" "/DOutDir=$(cygpath -w "$out")" "$(cygpath -w "$here/setup.iss")"
ls -l "$out"

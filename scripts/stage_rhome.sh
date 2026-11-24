#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PROFILE="${1:-debug}"
RHOME="${2:-$ROOT/target/rhome}"

mkdir -p "$RHOME"
RHOME="$(cd "$RHOME" && pwd -P)"
cd "$ROOT"
if [ -f env/env.sh ]; then
  source env/env.sh
fi
export CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=3

cargo_profile=()
case "$PROFILE" in
  debug) ;;
  release) cargo_profile=(--release) ;;
  *) cargo_profile=(--profile "$PROFILE") ;;
esac

LIB_SRC="${CARGO_TARGET_DIR:-$ROOT/target}/$PROFILE/libroxide_libr.a"
flags_file="$LIB_SRC.native-static-libs"
if [ "${ROXIDE_SKIP_BUILD:-0}" != 1 ] || [ ! -f "$LIB_SRC" ] || [ ! -s "$flags_file" ]; then
  if ! cargo_output="$(bash scripts/cargo_slot.sh cargo rustc --color never -p roxide-libr --lib ${cargo_profile[@]+"${cargo_profile[@]}"} -- --print native-static-libs 2>&1)"; then
    printf '%s\n' "$cargo_output" >&2
    exit 1
  fi
  printf '%s\n' "$cargo_output" >&2
  printf '%s\n' "$cargo_output" | python3 -c '
import shlex
import sys

prefix = "note: native-static-libs: "
for line in sys.stdin:
    if line.startswith(prefix):
        for flag in shlex.split(line.removeprefix(prefix)):
            print(flag)
        break
else:
    sys.exit("Cargo did not report native static libraries")
' > "$flags_file"
fi

native_libs=()
while IFS= read -r flag; do
  native_libs+=("$flag")
done < "$flags_file"

mkdir -p "$RHOME"/{bin/exec,include,lib,etc,share/make}
UNAME_S="$(uname -s)"
arch=()
case "$UNAME_S" in
  Darwin)
    if [ "$(uname -m)" = arm64 ]; then
      arch=(-arch arm64)
    fi
    LIBR_EXT=dylib
    EXPORTS="$RHOME/lib/libR.exp"
    SHLIB_LDFLAGS="-dynamiclib -Wl,-headerpad_max_install_names -undefined dynamic_lookup"
    BLAS_LIBS="-framework Accelerate"
    LAPACK_LIBS="-framework Accelerate"
    launcher_link=()
    ;;
  Linux)
    LIBR_EXT=so
    EXPORTS="$RHOME/lib/libR.ver"
    SHLIB_LDFLAGS="-shared"
    BLAS_LIBS="-lopenblas"
    LAPACK_LIBS="-lopenblas"
    launcher_link=('-Wl,-rpath,$ORIGIN/../../lib')
    ;;
  *)
    printf 'Cannot stage libR on %s\n' "$UNAME_S" >&2
    exit 1
    ;;
esac
LIBR="$RHOME/lib/libR.$LIBR_EXT"

python3 - "$LIB_SRC" "$EXPORTS" <<'PY'
import json
from pathlib import Path
import subprocess
import sys

archive, output = map(Path, sys.argv[1:])
darwin = sys.platform == "darwin"
prefix = "_" if darwin else ""
flags = ["-gU", "--no-llvm-bc", "--quiet"] if darwin else ["-g", "--defined-only"]
lines = subprocess.check_output(["nm", *flags, str(archive)], text=True)
defined = {
    parts[-1].removeprefix(prefix) for line in lines.splitlines()
    if len(parts := line.split()) == 3 and len(parts[1]) == 1
}
manifest = json.loads(Path("tools/capi/manifest.json").read_text())
allowed = {row["name"] for row in manifest["symbols"]
           if row["apitype"] in {"api", "eapi", "emb", "for", "data"}}
exports = defined & allowed
if not exports:
    sys.exit(f"No manifest C API symbols found in {archive}")
if "roxide_main" not in defined:
    sys.exit(f"roxide_main is missing from {archive}")
exports.add("roxide_main")
if darwin:
    contents = "".join(f"_{name}\n" for name in sorted(exports))
else:
    symbols = "".join(f"    {name};\n" for name in sorted(exports))
    contents = "{\n  global:\n" + symbols + "  local: *;\n};\n"
output.write_text(contents)
PY

if [ "$UNAME_S" = Darwin ]; then
  "${CC:-cc}" -dynamiclib ${arch[@]+"${arch[@]}"} -o "$LIBR" \
    "-Wl,-force_load,$LIB_SRC" "-Wl,-exported_symbols_list,$EXPORTS" \
    -install_name "$LIBR" -Wl,-headerpad_max_install_names \
    -framework Accelerate -lc++ ${native_libs[@]+"${native_libs[@]}"}
else
  "${CC:-cc}" -shared -o "$LIBR" \
    -Wl,--whole-archive "$LIB_SRC" -Wl,--no-whole-archive \
    "-Wl,--version-script=$EXPORTS" -Wl,-soname,libR.so \
    -lopenblas -lstdc++ ${native_libs[@]+"${native_libs[@]}"}
fi

"${CC:-cc}" ${arch[@]+"${arch[@]}"} -o "$RHOME/bin/exec/R" \
  crates/roxide-libr/csrc/launcher.c -L"$RHOME/lib" -lR \
  ${launcher_link[@]+"${launcher_link[@]}"}

python3 - "$RHOME/bin" <<'PY'
from pathlib import Path
import sys
from tempfile import TemporaryDirectory

bin_dir = Path(sys.argv[1])
with TemporaryDirectory(dir=bin_dir) as scratch:
    for name in ("R", "Rscript"):
        wrapper = Path(scratch) / name
        wrapper.write_text("""#!/usr/bin/env bash
set -euo pipefail
R_HOME="$(cd "$(dirname "$0")/.." && pwd -P)"
export R_HOME
exec -a "${0##*/}" "$R_HOME/bin/exec/R" "$@"
""")
        wrapper.chmod(0o755)
        wrapper.replace(bin_dir / name)
PY

cp -R crates/roxide-capi/include/. "$RHOME/include/"
if [ -f share/make/shlib.mk ]; then
  cp -f share/make/shlib.mk "$RHOME/share/make/shlib.mk"
fi

opt_flags="-g -O2 -Wall${arch[*]:+ ${arch[*]}}"
fc="${FC-$(command -v gfortran || true)}"
flibs="${FLIBS-}"
if [ "${FLIBS+x}" != x ] && [ -n "$fc" ]; then
  runtime="$("$fc" "-print-file-name=libgfortran.$LIBR_EXT" 2>/dev/null || true)"
  if [[ "$runtime" = */* && -f "$runtime" ]]; then
    flibs="-L\"$(dirname "$runtime")\" -lgfortran"
  fi
fi
{
  cat <<MAKE
R_HOME = $RHOME
R_INCLUDE_DIR = $RHOME/include
R_SHARE_DIR = $RHOME/share
CC = ${CC:-clang}
CXX = ${CXX:-clang++}
FC = $fc
AR = ${AR:-ar}
RANLIB = ${RANLIB:-ranlib}
AWK = ${AWK:-awk}
SED = ${SED:-sed}
TAR = ${TAR:-tar}
R_XTRA_CPPFLAGS = -I"\$(R_INCLUDE_DIR)"
CPPFLAGS = -DNDEBUG
CFLAGS = $opt_flags
CXXFLAGS = $opt_flags
CPICFLAGS = -fPIC
CXXPICFLAGS = -fPIC
FPICFLAGS = -fPIC
SHLIB_EXT = .so
SHLIB_LD = \$(CC)
SHLIB_CXXLD = \$(CXX)
SHLIB_LDFLAGS = $SHLIB_LDFLAGS
SHLIB_CXXLDFLAGS = $SHLIB_LDFLAGS
LIBR = -L"\$(R_HOME)/lib" -lR
BLAS_LIBS = $BLAS_LIBS
LAPACK_LIBS = $LAPACK_LIBS
FLIBS = $flibs
MAKE
  for version in 11 14 17 20 23 26; do
    cat <<MAKE
CXX$version = \$(CXX)
CXX${version}STD = -std=gnu++$version
CXX${version}PICFLAGS = \$(CXXPICFLAGS)
CXX${version}FLAGS = \$(CXXFLAGS)
MAKE
  done
} > "$RHOME/etc/Makeconf"

printf 'Staged Roxide R_HOME at %s (profile=%s, libR=libR.%s)\n' "$RHOME" "$PROFILE" "$LIBR_EXT"

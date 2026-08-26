#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/install-linux.sh [--release|--debug] [--prefix PATH] [--no-build]

Installs:
  - binary to <prefix>/bin/repositorytree
  - desktop entry to ~/.local/share/applications/repositorytree.desktop
  - icons to ~/.local/share/icons/hicolor/<size>x<size>/apps/repositorytree.png
    sizes: 32, 48, 128, 256, 512

Defaults:
  --release, --prefix ~/.local, build if needed
EOF
}

mode="release"
prefix="${HOME}/.local"
build=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) mode="release"; shift ;;
    --debug) mode="debug"; shift ;;
    --prefix) prefix="$2"; shift 2 ;;
    --no-build) build=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 2 ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_src="${repo_root}/target/${mode}/repositorytree"

if [[ $build -eq 1 && ! -x "$bin_src" ]]; then
  cargo_mode_flag=()
  if [[ "$mode" == "release" ]]; then
    cargo_mode_flag=(--release)
  fi
  (cd "$repo_root" && cargo build -p repositorytree "${cargo_mode_flag[@]}")
fi

if [[ ! -x "$bin_src" ]]; then
  echo "Binary not found or not executable: $bin_src" >&2
  echo "Build first or omit --no-build." >&2
  exit 1
fi

bindir="${prefix}/bin"
appdir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"
iconsroot="${XDG_DATA_HOME:-${HOME}/.local/share}/icons/hicolor"
icon_sizes=(32 48 128 256 512)

install -Dm755 "$bin_src" "${bindir}/repositorytree"

# Install desktop file with absolute Exec path so it works even if ~/.local/bin isn't on PATH.
tmp_desktop="$(mktemp)"
trap 'rm -f "$tmp_desktop"' EXIT
sed "s|^Exec=.*$|Exec=${bindir}/repositorytree|g" \
  "${repo_root}/assets/linux/repositorytree.desktop" >"$tmp_desktop"
install -Dm644 "$tmp_desktop" "${appdir}/repositorytree.desktop"

for size in "${icon_sizes[@]}"; do
  install -Dm644 "${repo_root}/assets/linux/hicolor/${size}x${size}/apps/repositorytree.png" \
    "${iconsroot}/${size}x${size}/apps/repositorytree.png"
done

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$appdir" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache "${iconsroot}" >/dev/null 2>&1 || true

echo "Installed RepositoryTree:"
echo "  ${bindir}/repositorytree"
echo "  ${appdir}/repositorytree.desktop"
for size in "${icon_sizes[@]}"; do
  echo "  ${iconsroot}/${size}x${size}/apps/repositorytree.png"
done
echo "If GNOME still shows a generic icon, log out/in (or restart GNOME Shell)."

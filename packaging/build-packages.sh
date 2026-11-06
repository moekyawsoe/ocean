#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RAW_VERSION="${1:-${VERSION:-}}"
if [[ -z "$RAW_VERSION" ]]; then
  RAW_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
fi

CLEAN_VERSION="${RAW_VERSION#v}"
if [[ -z "$CLEAN_VERSION" ]]; then
  echo "Error: could not determine package version." >&2
  exit 1
fi

DEB_VERSION="${CLEAN_VERSION//-/\~}-1"
RPM_VERSION="${CLEAN_VERSION//-/\~}"

HOST_ARCH="$(uname -m)"
case "$HOST_ARCH" in
  x86_64|amd64)
    DEB_ARCH="amd64"
    RPM_ARCH="x86_64"
    ;;
  aarch64|arm64)
    DEB_ARCH="arm64"
    RPM_ARCH="aarch64"
    ;;
  *)
    DEB_ARCH="$HOST_ARCH"
    RPM_ARCH="$HOST_ARCH"
    ;;
esac

# Ensure Cargo.toml version is in sync with the package version
sed -i "0,/^version = \".*\"/s//version = \"$CLEAN_VERSION\"/" "$ROOT_DIR/Cargo.toml"

BIN_PATH="$ROOT_DIR/target/release/ocean"
if [[ ! -x "$BIN_PATH" ]] || [[ "$("$BIN_PATH" --version 2>/dev/null | awk '{print $2}')" != "$CLEAN_VERSION" ]]; then
  echo "==> Building release binary for version $CLEAN_VERSION..."
  cargo build --release
fi

DIST_DIR="$ROOT_DIR/dist"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$DIST_DIR"
rm -f "$DIST_DIR"/*

populate_usr_tree() {
  local dest_root="$1"
  install -Dm755 "$BIN_PATH" "$dest_root/usr/bin/ocean"
  ln -sf ocean "$dest_root/usr/bin/edit"
  ln -sf ocean "$dest_root/usr/bin/file"
  ln -sf ocean "$dest_root/usr/bin/file-ocean"
  install -Dm644 "$ROOT_DIR/packaging/linux/ocean.desktop" "$dest_root/usr/share/applications/ocean.desktop"
  install -Dm644 "$ROOT_DIR/packaging/linux/ocean.metainfo.xml" "$dest_root/usr/share/metainfo/ocean.metainfo.xml"
  install -Dm644 "$ROOT_DIR/assets/icons/ocean.svg" "$dest_root/usr/share/icons/hicolor/scalable/apps/ocean.svg"
  for sz in 16 32 48 64 128 256 512; do
    install -Dm644 \
      "$ROOT_DIR/assets/icons/${sz}x${sz}/ocean.png" \
      "$dest_root/usr/share/icons/hicolor/${sz}x${sz}/apps/ocean.png"
  done
  install -Dm644 "$ROOT_DIR/assets/icons/256x256/ocean.png" "$dest_root/usr/share/pixmaps/ocean.png"
  install -Dm644 "$ROOT_DIR/config.example" "$dest_root/usr/share/doc/ocean/config.example"
  install -Dm644 "$ROOT_DIR/README.md" "$dest_root/usr/share/doc/ocean/README.md"
}

# ---------------------------------------------------------------------------
# 1. Build Debian / Ubuntu (.deb) package
# ---------------------------------------------------------------------------
echo "==> Building .deb package (ocean_${CLEAN_VERSION}_${DEB_ARCH}.deb)..."
DEB_STAGE="$WORK_DIR/deb-stage"
mkdir -p "$DEB_STAGE/DEBIAN"
populate_usr_tree "$DEB_STAGE"

INSTALLED_KB="$(du -sk "$DEB_STAGE/usr" | awk '{print $1}')"

cat > "$DEB_STAGE/DEBIAN/control" <<EOF
Package: ocean
Version: ${DEB_VERSION}
Section: x11
Priority: optional
Architecture: ${DEB_ARCH}
Installed-Size: ${INSTALLED_KB}
Depends: libc6, libgcc-s1, libwayland-client0 | libx11-6, libegl1 | libgl1, libxkbcommon0
Recommends: fonts-padauk | fonts-noto-core
Provides: x-terminal-emulator
Maintainer: Moe Kyaw Soe <https://github.com/moekyawsoe/ocean>
Homepage: https://github.com/moekyawsoe/ocean
Description: Clean, lightweight GPU-accelerated terminal with Myanmar/Burmese support
 Ocean is a clean, minimal, GPU-accelerated terminal emulator written in Rust
 featuring full HarfBuzz (mym2/mymr) Myanmar/Burmese OpenType shaping, Wayland
 and X11 IME preedit, keyboard-first split panes, a Neovim-style status bar,
 and Ghostty-compatible configuration.
EOF

chmod 755 "$DEB_STAGE/DEBIAN"
chmod 644 "$DEB_STAGE/DEBIAN/control"

DEB_OUT="$DIST_DIR/ocean_${CLEAN_VERSION}_${DEB_ARCH}.deb"
dpkg-deb --root-owner-group --build "$DEB_STAGE" "$DEB_OUT" >/dev/null
cp "$DEB_OUT" "$DIST_DIR/ocean_${DEB_ARCH}.deb"

# ---------------------------------------------------------------------------
# 2. Build Fedora / RHEL / openSUSE (.rpm) package
# ---------------------------------------------------------------------------
echo "==> Building .rpm package (ocean-${RPM_VERSION}-1.${RPM_ARCH}.rpm)..."
RPM_TOP="$WORK_DIR/rpmbuild"
RPM_PAYLOAD="$WORK_DIR/rpm-payload"
mkdir -p "$RPM_TOP"/{BUILD,RPMS,SOURCES,SPECS,SRPMS} "$RPM_PAYLOAD"
populate_usr_tree "$RPM_PAYLOAD"

cat > "$RPM_TOP/SPECS/ocean.spec" <<EOF
%global debug_package %{nil}
%define _build_id_links none

Name:           ocean
Version:        ${RPM_VERSION}
Release:        1
Summary:        Clean, lightweight GPU-accelerated terminal with Myanmar/Burmese support
License:        MIT
URL:            https://github.com/moekyawsoe/ocean
BuildArch:      ${RPM_ARCH}
Recommends:     sil-padauk-fonts

%description
Ocean is a clean, minimal, GPU-accelerated terminal emulator written in Rust
featuring full HarfBuzz (mym2/mymr) Myanmar/Burmese OpenType shaping, Wayland
and X11 IME preedit, keyboard-first split panes, a Neovim-style status bar,
and Ghostty-compatible configuration.

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a "${RPM_PAYLOAD}/usr" %{buildroot}/usr

%files
%attr(0755, root, root) /usr/bin/ocean
/usr/bin/edit
/usr/bin/file
/usr/bin/file-ocean
/usr/share/applications/ocean.desktop
/usr/share/metainfo/ocean.metainfo.xml
/usr/share/icons/hicolor/scalable/apps/ocean.svg
/usr/share/icons/hicolor/16x16/apps/ocean.png
/usr/share/icons/hicolor/32x32/apps/ocean.png
/usr/share/icons/hicolor/48x48/apps/ocean.png
/usr/share/icons/hicolor/64x64/apps/ocean.png
/usr/share/icons/hicolor/128x128/apps/ocean.png
/usr/share/icons/hicolor/256x256/apps/ocean.png
/usr/share/icons/hicolor/512x512/apps/ocean.png
/usr/share/pixmaps/ocean.png
%doc /usr/share/doc/ocean/config.example
%doc /usr/share/doc/ocean/README.md
EOF

rpmbuild --define "_topdir $RPM_TOP" -bb "$RPM_TOP/SPECS/ocean.spec" >/dev/null
RPM_BUILT="$(find "$RPM_TOP/RPMS" -name "*.rpm" | head -n 1)"
RPM_OUT="$DIST_DIR/ocean-${RPM_VERSION}-1.${RPM_ARCH}.rpm"
cp "$RPM_BUILT" "$RPM_OUT"
cp "$RPM_OUT" "$DIST_DIR/ocean.${RPM_ARCH}.rpm"

# ---------------------------------------------------------------------------
# 3. Build portable Linux tarball (.tar.gz) & SHA256SUMS
# ---------------------------------------------------------------------------
echo "==> Building portable tarball..."
TAR_DIR_NAME="ocean-v${CLEAN_VERSION}-linux-${RPM_ARCH}"
TAR_STAGE="$WORK_DIR/$TAR_DIR_NAME"
mkdir -p "$TAR_STAGE/icons"
cp "$BIN_PATH" "$TAR_STAGE/ocean"
cp "$ROOT_DIR/config.example" "$TAR_STAGE/config.example"
cp "$ROOT_DIR/README.md" "$TAR_STAGE/README.md"
cp "$ROOT_DIR/packaging/linux/ocean.desktop" "$TAR_STAGE/ocean.desktop"
cp "$ROOT_DIR/assets/icons/ocean.svg" "$TAR_STAGE/icons/ocean.svg"
cp "$ROOT_DIR/assets/icons/256x256/ocean.png" "$TAR_STAGE/icons/ocean.png"

TAR_OUT="$DIST_DIR/${TAR_DIR_NAME}.tar.gz"
tar -C "$WORK_DIR" -czf "$TAR_OUT" "$TAR_DIR_NAME"
cp "$TAR_OUT" "$DIST_DIR/ocean-linux-${RPM_ARCH}.tar.gz"

(
  cd "$DIST_DIR"
  sha256sum ./*.deb ./*.rpm ./*.tar.gz > SHA256SUMS
)

echo "==> Built packages in $DIST_DIR:"
ls -lh "$DIST_DIR"

# Maintainer: Heri Efendi <heriefendi@gmail.com>
pkgname=db-sync-desktop-client
pkgver=0.7.2
pkgrel=1
pkgdesc="DB-Sync Client (PMA to Local MySQL)"
arch=('x86_64')
url="https://github.com/HeriEfendi/DB-Sync-Desktop-Client"
license=('MIT')
depends=('webkit2gtk-4.1' 'gtk3' 'cairo' 'hicolor-icon-theme' 'openssl')
makedepends=('nodejs' 'npm' 'cargo' 'rust')
provides=('db-sync-desktop-client')
conflicts=('db-sync-desktop-client')

build() {
  cd "$startdir"
  npm run build
  export CARGO_TARGET_DIR="$srcdir/target"
  cargo build --manifest-path src-tauri/Cargo.toml --release
}

package() {
  cd "$startdir"
  
  # Install binary
  install -Dm755 "$srcdir/target/release/db-sync-desktop-client" "$pkgdir/usr/bin/db-sync-desktop-client"
  
  # Install desktop application launcher
  install -d "$pkgdir/usr/share/applications"
  cat <<EOF > "$pkgdir/usr/share/applications/db-sync-desktop-client.desktop"
[Desktop Entry]
Name=DB-Sync Client
Comment=DB-Sync Client (PMA to Local MySQL)
Exec=db-sync-desktop-client
Icon=db-sync-desktop-client
Terminal=false
Type=Application
Categories=Development;Database;Utility;
EOF

  # Install icons
  if [ -f "src-tauri/icons/128x128.png" ]; then
    install -Dm644 "src-tauri/icons/128x128.png" "$pkgdir/usr/share/icons/hicolor/128x128/apps/db-sync-desktop-client.png"
  fi
  if [ -f "src-tauri/icons/32x32.png" ]; then
    install -Dm644 "src-tauri/icons/32x32.png" "$pkgdir/usr/share/icons/hicolor/32x32/apps/db-sync-desktop-client.png"
  fi
}

#!/usr/bin/env bash
set -euo pipefail

app_id="com.puretype.app"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop_dir="$data_home/applications"
icon_dir="$data_home/icons/hicolor/512x512/apps"
desktop_file="$desktop_dir/$app_id.desktop"
icon_file="$icon_dir/$app_id.png"
binary_path="$repo_root/src-tauri/target/debug/$app_id"

mkdir -p "$desktop_dir" "$icon_dir"
install -m 0644 "$repo_root/src-tauri/icons/icon.png" "$icon_file"

cat > "$desktop_file" <<EOF
[Desktop Entry]
Type=Application
Name=PureType
Comment=A high-performance Markdown note-taking app inspired by nvALT.
Exec=$binary_path
Icon=$app_id
StartupWMClass=$app_id
Terminal=false
Categories=Utility;TextEditor;
EOF

chmod 0644 "$desktop_file"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q "$data_home/icons/hicolor" >/dev/null 2>&1 || true
fi

echo "Installed $desktop_file"
echo "Installed $icon_file"

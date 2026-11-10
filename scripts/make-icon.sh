#!/bin/sh
# Rebuilds bundle/AppIcon.icns from bundle/AppIcon.svg. Quick Look renders
# the SVG; sips and iconutil turn it into the iconset macOS expects.
set -eu

cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

qlmanage -t -s 1024 -o "$work" bundle/AppIcon.svg >/dev/null
master="$work/AppIcon.svg.png"
set_dir="$work/AppIcon.iconset"
mkdir "$set_dir"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$master" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$master" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o bundle/AppIcon.icns
echo "Wrote bundle/AppIcon.icns"

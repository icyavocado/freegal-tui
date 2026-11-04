#!/usr/bin/env bash
# install the freegal omarchy bar widget (user-owned plugin, update-safe)
set -euo pipefail

plugin_dir="$HOME/.config/omarchy/plugins/freegal.now-playing"
src="$(cd "$(dirname "$0")/.." && pwd)/assets/omarchy-plugin"

mkdir -p "$plugin_dir"
cp "$src/manifest.json" "$src/NowPlaying.qml" "$plugin_dir/"

python3 - "$HOME/.config/omarchy/shell.json" <<'EOF'
import json, sys
path = sys.argv[1]
cfg = json.load(open(path))
layout = cfg.setdefault("bar", {}).setdefault("layout", {})
right = layout.setdefault("right", [])
widget = {"id": "freegal.now-playing"}
if widget not in right:
    right.insert(0, widget)
    json.dump(cfg, open(path, "w"), indent=2)
    print("added freegal.now-playing to the bar")
else:
    print("bar already has freegal.now-playing")
EOF

omarchy-shell shell rescanPlugins 2>/dev/null || omarchy restart shell
echo "installed; the widget appears on the bar when freegal starts playing"
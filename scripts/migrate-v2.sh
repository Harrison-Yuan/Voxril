#!/bin/bash
set -euo pipefail

WORKSPACE="${1:-$HOME/Desktop/my_voice_training}"
DRY_RUN="${2:-false}"

echo "=== RVC 数据迁移: data/ + models/ → experiments/{name}/{ver_sr}/ ==="
echo "Workspace: $WORKSPACE"
echo ""

shopt -s nullglob

move_item() {
    local src="$1" dst="$2"
    if [ -e "$src" ]; then
        if [ "$DRY_RUN" = "true" ]; then
            echo "  [DRY] cp $src → $dst"
        else
            mkdir -p "$(dirname "$dst")"
            if [ -d "$src" ]; then
                cp -a "$src"/. "$dst"/ 2>/dev/null || true
            else
                cp -a "$src" "$dst" 2>/dev/null || true
            fi
            echo "  ✓ $src → $dst"
        fi
    fi
}

detect_ver_sr() {
    local config="$1"
    local ver="v2" sr="40k"
    if [ -f "$config" ]; then
        ver=$(python3 -c "
import json,sys
try:
    c=json.load(open('$config'))
    g=c.get('model',{}).get('gin_channels',256)
    sr_raw=c.get('data',{}).get('sampling_rate',40000)
    v='v2' if g!=0 else 'v1'
    s='32k' if sr_raw==32000 else ('48k' if sr_raw==48000 else '40k')
    print(f'{v}_{s}')
except: print('v2_40k')
" 2>/dev/null || echo "v2_40k")
    else
        ver=$(python3 -c "
import json,sys
try:
    c=json.load(open('$config'))
    g=c.get('model',{}).get('gin_channels',256)
    v='v2' if g!=0 else 'v1'
    print(v)
except: print('v2')
" 2>/dev/null || echo "v2")
    fi
    echo "$ver"
}

experiments=()
for d in "$WORKSPACE"/data/*/; do
    name=$(basename "$d")
    experiments+=("$name")
done
for d in "$WORKSPACE"/models/*/; do
    name=$(basename "$d")
    if [[ ! " ${experiments[*]} " =~ " $name " ]]; then
        experiments+=("$name")
    fi
done

if [ ${#experiments[@]} -eq 0 ]; then
    echo "No experiments found."
    exit 0
fi

echo "Found experiments: ${experiments[*]}"
echo ""

for name in "${experiments[@]}"; do
    echo "--- $name ---"

    CONFIG="$WORKSPACE/models/$name/config.json"
    [ ! -f "$CONFIG" ] && CONFIG="$WORKSPACE/data/$name/config.json"
    
    if [ -f "$CONFIG" ]; then
        VER_SR=$(detect_ver_sr "$CONFIG")
    else
        echo "  ⚠ no config.json found, defaulting to v2_40k"
        VER_SR="v2_40k"
    fi

    EXP_DIR="$WORKSPACE/experiments/$name/$VER_SR"
    echo "  → $name/$VER_SR"

    move_item "$WORKSPACE/data/$name/" "$EXP_DIR/"
    move_item "$WORKSPACE/models/$name/" "$EXP_DIR/"

    CKPT_DIR="$EXP_DIR/checkpoints"
    for pth in "$EXP_DIR"/G_*.pth "$EXP_DIR"/D_*.pth "$EXP_DIR"/*_e*_s*.pth; do
        if [ -f "$pth" ]; then
            if [ "$DRY_RUN" = "true" ]; then
                echo "  [DRY] mv $(basename "$pth") → checkpoints/"
            else
                mkdir -p "$CKPT_DIR"
                mv "$pth" "$CKPT_DIR/"
                echo "  ✓ $(basename "$pth") → checkpoints/"
            fi
        fi
    done

    PROJECT_WEIGHTS="$(dirname "$0")/../../assets/weights"
    [ -f "$PROJECT_WEIGHTS/$name.pth" ] && move_item "$PROJECT_WEIGHTS/$name.pth" "$EXP_DIR/model.pth"

    echo ""
done

if [ "$DRY_RUN" != "true" ]; then
    for subdir in data models; do
        if [ -d "$WORKSPACE/$subdir" ] && [ -z "$(ls -A "$WORKSPACE/$subdir" 2>/dev/null)" ]; then
            rmdir "$WORKSPACE/$subdir" 2>/dev/null && echo "Removed empty $subdir/"
        fi
    done
fi

echo ""
echo "=== Migration done ==="
echo "New structure:"
find "$WORKSPACE/experiments" -maxdepth 4 -not -path '*/0_gt_wavs/*' -not -path '*/1_16k_wavs/*' -not -path '*/2a_f0/*' -not -path '*/2b-f0nsf/*' -not -path '*/3_feature*/*' | sort | head -50

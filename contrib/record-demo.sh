#!/usr/bin/env bash
# Record a short GIF of niri-stash in action.
#
# Switches to an empty workspace, opens a few tiled and floating applications, records a
# stash / restore / stash / unstash sequence, converts it to a GIF, and closes everything it
# opened. The niri-stash daemon must be running.
#
# Usage: contrib/record-demo.sh [output.gif]
# Environment: DEMO_TILED and DEMO_FLOATING: `|`-separated commands to open as tiled and as
#              floating windows (defaults below; each must open one new window),
#              DEMO_TERMINAL (default $TERMINAL or alacritty; `wezterm` is run as `wezterm start`),
#              FPS (default 20), WIDTH (GIF width, default 960), PAUSE (seconds between steps, 1.6).
#
# Needs: niri, niri-stash, wf-recorder, ffmpeg, python3, and the applications in the lists.

set -euo pipefail

out=${1:-demo.gif}
terminal=${DEMO_TERMINAL:-${TERMINAL:-alacritty}}
[[ $terminal == wezterm ]] && terminal="wezterm start"
IFS='|' read -ra tiled_cmds <<<"${DEMO_TILED:-nautilus --new-window /usr/share/icons|$terminal}"
IFS='|' read -ra floating_cmds <<<"${DEMO_FLOATING:-gnome-calculator|shelly-ui|gnome-text-editor --new-window}"
fps=${FPS:-20}
width=${WIDTH:-960}
pause=${PAUSE:-1.6}

for cmd in niri niri-stash wf-recorder ffmpeg python3 "${tiled_cmds[@]}" "${floating_cmds[@]}"; do
    command -v "${cmd%% *}" >/dev/null || { echo "record-demo: ${cmd%% *} is not installed" >&2; exit 1; }
done

work=$(mktemp -d)
recorder=
spawned=()
window=

# Field of the focused workspace: idx or output.
focused_workspace() {
    niri msg -j workspaces | python3 -c '
import json, sys
ws = next(w for w in json.load(sys.stdin) if w["is_focused"])
print(ws[sys.argv[1]])' "$1"
}

# Index of the empty workspace niri keeps at the end of the focused output.
empty_workspace() {
    niri msg -j workspaces | python3 -c '
import json, sys
ws = [w for w in json.load(sys.stdin) if w["output"] == sys.argv[1]]
last = max(ws, key=lambda w: w["idx"])
if last["active_window_id"] is not None:
    sys.exit("the last workspace is not empty")
print(last["idx"])' "$1"
}

window_ids() {
    niri msg -j windows | python3 -c 'import json, sys; print(*sorted(w["id"] for w in json.load(sys.stdin)))'
}

# Run $1 and put the id of the window it opens in $window once niri lists it.
spawn_window() {
    local cmd=$1 before after id
    before=" $(window_ids) "
    niri msg action spawn -- sh -c "$cmd"
    for _ in $(seq 1 80); do
        sleep 0.1
        after=$(window_ids)
        for id in $after; do
            [[ $before == *" $id "* ]] || { spawned+=("$id"); window=$id; return; }
        done
    done
    echo "record-demo: '$cmd' did not open a new window (already running?)" >&2
    return 1
}

cleanup() {
    set +e
    if [[ -n $recorder ]] && kill -0 "$recorder" 2>/dev/null; then
        kill -INT "$recorder"; wait "$recorder"
    fi
    for id in "${spawned[@]}"; do
        niri msg action close-window --id "$id" >/dev/null 2>&1
    done
    niri msg action focus-workspace "$origin_idx" >/dev/null 2>&1
    rm -rf "$work"
}
trap cleanup EXIT

origin_idx=$(focused_workspace idx)
output=$(focused_workspace output)
demo_idx=$(empty_workspace "$output")

echo "record-demo: using workspace $demo_idx on $output"
niri msg action focus-workspace "$demo_idx"
sleep 0.5

for cmd in "${tiled_cmds[@]}"; do
    spawn_window "$cmd"
    sleep 0.3
done
# Floating windows, staggered across the screen. Positions are in logical pixels; applications
# with a fixed size keep it.
places=("80 60" "640 180" "360 480" "900 420" "200 300")
i=0
for cmd in "${floating_cmds[@]}"; do
    spawn_window "$cmd"
    niri msg action move-window-to-floating --id "$window"
    niri msg action set-window-width --id "$window" 560
    niri msg action set-window-height --id "$window" 340
    # shellcheck disable=SC2086
    set -- ${places[i % ${#places[@]}]}
    niri msg action move-floating-window --id "$window" -x "$1" -y "$2"
    i=$((i + 1))
    sleep 0.3
done
niri msg action focus-window --id "$window"
sleep 1

echo "record-demo: recording"
wf-recorder -o "$output" -r "$fps" -f "$work/demo.mp4" >"$work/wf-recorder.log" 2>&1 &
recorder=$!
sleep 1

sleep "$pause"
niri-stash toggle;  sleep "$pause"
niri-stash toggle;  sleep "$pause"
niri-stash stash;   sleep "$pause"
niri-stash unstash; sleep "$pause"

kill -INT "$recorder"; wait "$recorder" || true
recorder=

echo "record-demo: converting"
ffmpeg -loglevel error -y -i "$work/demo.mp4" \
    -vf "fps=$fps,scale=$width:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle" \
    "$out"
echo "record-demo: wrote $out ($(du -h "$out" | cut -f1))"

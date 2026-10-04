#!/usr/bin/env bash
set -euo pipefail
lane=$(cd -- "$(dirname -- "$0")/.." && pwd)
out="$lane/target/repair-evidence"
mkdir -p "$out"
cp "$lane/docs/x11-text-delay-20261005/packets.txt" "$out/packets.txt"
tr -d '\n' < "$out/packets.txt" > "$out/expected.txt"
printf '\n' >> "$out/expected.txt"
Xvfb -displayfd 3 -screen 0 800x600x24 -nolisten tcp 3>"$out/display.txt" >"$out/xserver.log" 2>&1 &
server_pid=$!
receiver_pid=
cleanup() {
    if [[ -n "$receiver_pid" ]]; then kill "$receiver_pid" 2>/dev/null || true; wait "$receiver_pid" 2>/dev/null || true; fi
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
}
trap cleanup EXIT
for _ in {1..100}; do [[ -s "$out/display.txt" ]] && break; sleep 0.05; done
scratch_display=":$(cat "$out/display.txt")"
export DISPLAY="$scratch_display"
ENIGO_TEST_DISPLAY="$scratch_display" cargo test --locked --jobs 2 --lib unicode_click_keeps_another_clients_shift_held -- --ignored
bytes=$(wc -c < "$out/expected.txt")
xterm -class EnigoTextReceiver -geometry 100x12 -e bash -c 'stty -echo; head -c "$1" > "$2"' receiver "$bytes" "$out/received.txt" >"$out/xterm.log" 2>&1 &
receiver_pid=$!
window=$(xdotool search --sync --class EnigoTextReceiver | head -n 1)
xdotool windowfocus --sync "$window"
"$lane/target/debug/examples/enigo_text_probe" "$scratch_display" "$out/packets.txt" "$out/received.txt" > "$out/timings.txt"
wait "$receiver_pid"
receiver_pid=
cmp "$out/expected.txt" "$out/received.txt"
printf 'EXACT_TEXT bytes=%s scratch_display=%s\n' "$bytes" "$scratch_display"

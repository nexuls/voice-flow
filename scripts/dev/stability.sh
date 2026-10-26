#!/usr/bin/env bash
# Development aid (Linux, PipeWire): idle CPU and memory over 12 dictations
# through a temporary null-sink microphone. Uses an isolated config dir with
# auto-paste off; saves and restores the clipboard. See TESTING.md.
set -u
S=${S:-$(mktemp -d)}
mkdir -p "$S/xdg-config/dev.voiceflow.app"
[ -f "$S/xdg-config/dev.voiceflow.app/settings.json" ] || echo '{"version":1,"autoPaste":false}' > "$S/xdg-config/dev.voiceflow.app/settings.json"
cd "$(dirname "$0")/../.."
wl-paste --no-newline > $S/clip.bak 2>/dev/null
MOD=$(pactl load-module module-null-sink sink_name=vf_sink channel_map=mono)
cleanup() {
  kill $APP 2>/dev/null; wait $APP 2>/dev/null
  if [ -s $S/clip.bak ]; then wl-copy < $S/clip.bak; else wl-copy --clear; fi; rm -f $S/clip.bak
  pactl unload-module $MOD >/dev/null
}
trap cleanup EXIT
XDG_CONFIG_HOME=$S/xdg-config PULSE_SOURCE=vf_sink.monitor VOICEFLOW_SHORTCUT=F19 VOICEFLOW_LOG=voiceflow_lib=info \
  ./src-tauri/target/release/voiceflow --background > $S/stab.log 2>&1 &
APP=$!
sleep 4
ticks() { awk '{print $14+$15}' /proc/$APP/stat; }
rss() { awk '/VmRSS/{print $2}' /proc/$APP/status; }
T0=$(ticks); sleep 30; T1=$(ticks)
echo "idle: $((T1-T0)) CPU ticks in 30 s (100 ticks = 1 core-second)"
echo "rss_start_kb $(rss)"
for i in $(seq 1 12); do
  ./src-tauri/target/debug/examples/hold_key F19 7600 & HK=$!
  sleep 2.6
  pw-play --target vf_sink src-tauri/tests/fixtures/quick_brown_fox_en.wav
  wait $HK
  sleep 2
  echo "rss_after_${i}_kb $(rss)"
done
sleep 5; echo "rss_end_kb $(rss)"; echo "threads $(ls /proc/$APP/task | wc -l)"

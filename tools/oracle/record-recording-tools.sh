#!/usr/bin/env bash
# Writes the recorded (real-*) answers of the programs the recording side asks, for the recorder,
# regionpicker, captureexclusion and launcher families: what hyprctl, gpu-screen-recorder and
# pactl print on the machine it runs on (it needs a running Hyprland session), and ffmpeg's
# volumedetect logs for generated audio. The oracle never runs these programs: the replay shims in
# tests/golden/shims answer from these files, so the goldens do not depend on the machine.
#
#   corpus/hyprctl/<request>/real-desktop.json   hyprctl -j monitors, activeworkspace, clients,
#                                                layers, plugin list, version, getoption
#                                                cursor:no_hardware_cursors, and the plain
#                                                `omareel-exclude status` (when the plugin is loaded)
#   corpus/gpu-screen-recorder/<option>/real-desktop.txt   --list-capture-options,
#                                                          --list-audio-devices
#   corpus/pactl/list-short-sources/real-desktop.txt
#   corpus/recorder/volumedetect/<case>.log       ffmpeg's log for Recorder's audio arguments with
#                                                 lavfi sources in place of PulseAudio
#
# Window titles in the hyprctl answers are replaced by "Window <n>" (they name whatever was open).
# In the ffmpeg logs the output path is written @PATH@ and log-context addresses 0x0, as
# record-ffprobe.sh does, and the speed and elapsed time of the final statistics line N/A;
# FFMPEG_VERSION is the first line of `ffmpeg -version`. Run it when the
# corpus is created or the recordings should be refreshed on purpose.
set -euo pipefail

repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
corpus="$repo/tests/golden/corpus"
for tool in hyprctl gpu-screen-recorder pactl ffmpeg python3; do
    command -v "$tool" >/dev/null || { echo "record-recording-tools.sh: $tool is required" >&2; exit 1; }
done
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

scrub_titles() {
    python3 -I -c '
import json, sys
data = json.load(sys.stdin)
items = data if isinstance(data, list) else [data]
for index, item in enumerate(items, 1):
    for key in ("title", "initialTitle", "lastwindowtitle"):
        if key in item and item[key]:
            item[key] = "Window %d" % index
print(json.dumps(data, indent=4))'
}

mkdir -p "$corpus/hyprctl"/{monitors,activeworkspace,clients,layers,plugin-list,version,getoption-cursor,exclude-status}
hyprctl -j monitors > "$corpus/hyprctl/monitors/real-desktop.json"
hyprctl -j activeworkspace | scrub_titles > "$corpus/hyprctl/activeworkspace/real-desktop.json"
hyprctl -j clients | scrub_titles > "$corpus/hyprctl/clients/real-desktop.json"
hyprctl -j layers > "$corpus/hyprctl/layers/real-desktop.json"
hyprctl -j plugin list > "$corpus/hyprctl/plugin-list/real-desktop.json"
hyprctl -j version > "$corpus/hyprctl/version/real-desktop.json"
hyprctl -j getoption cursor:no_hardware_cursors > "$corpus/hyprctl/getoption-cursor/real-desktop.json"
if grep -q '"omareel-capture-exclude"' "$corpus/hyprctl/plugin-list/real-desktop.json"; then
    hyprctl omareel-exclude status > "$corpus/hyprctl/exclude-status/real-desktop.json"
fi

mkdir -p "$corpus/gpu-screen-recorder"/{list-capture-options,list-audio-devices}
gpu-screen-recorder --list-capture-options > "$corpus/gpu-screen-recorder/list-capture-options/real-desktop.txt"
gpu-screen-recorder --list-audio-devices > "$corpus/gpu-screen-recorder/list-audio-devices/real-desktop.txt"
mkdir -p "$corpus/pactl/list-short-sources"
pactl list short sources > "$corpus/pactl/list-short-sources/real-desktop.txt"

# Recorder's startAudioCapture arguments, with lavfi sources standing in for -f pulse -i <source>.
volumedetect="$corpus/recorder/volumedetect"
mkdir -p "$volumedetect"
find "$volumedetect" -maxdepth 1 -name '*.log' ! -name 'crafted-*' -delete
ffmpeg -version | head -n 1 > "$volumedetect/FFMPEG_VERSION"
head=(-y -hide_banner -nostats -loglevel info)
tail=(-c:a aac -b:a 192k -flush_packets 1 -cluster_time_limit 500)
record() {
    local name=$1
    shift
    local output="$tmp/$name.mka"
    ffmpeg "$@" > "$tmp/$name.log" 2>&1 || true
    sed -e "s|$output|@PATH@|g" -e 's/ @ 0x[0-9a-f]*\]/ @ 0x0]/g' \
        -e 's|speed= *[0-9.e+]*x|speed=N/A|; s|elapsed=[0-9:.]*|elapsed=N/A|' "$tmp/$name.log" > "$volumedetect/$name.log"
}
microphone() { # one source: the microphone alone, as Recorder records it
    local name=$1 source=$2
    record "$name" "${head[@]}" -thread_queue_size 1024 -f lavfi -i "$source" \
        -map 0:a -af volumedetect "${tail[@]}" "$tmp/$name.mka"
}
microphone silence-mono "anullsrc=r=48000:cl=mono:d=1"
microphone silence-stereo "anullsrc=r=48000:cl=stereo:d=1"
microphone tone-mono "sine=frequency=440:sample_rate=48000:duration=1"
microphone tone-stereo "sine=frequency=440:sample_rate=48000:duration=1,aformat=channel_layouts=stereo"
microphone quiet-mono "sine=frequency=440:sample_rate=48000:duration=1,volume=-70dB"
microphone loud-clipped "sine=frequency=440:sample_rate=48000:duration=1,volume=30dB"
microphone full-scale-square "aevalsrc=if(lt(mod(t*100\,1)\,0.5)\,1\,-1):s=48000:d=1"
microphone tone-44100 "sine=frequency=1000:sample_rate=44100:duration=0.5"
microphone empty "anullsrc=r=48000:cl=mono:d=0"
# Desktop audio and a microphone: amix after volumedetect on the second input.
record desktop-and-microphone "${head[@]}" \
    -thread_queue_size 1024 -f lavfi -i "sine=frequency=220:sample_rate=48000:duration=1" \
    -thread_queue_size 1024 -f lavfi -i "sine=frequency=880:sample_rate=48000:duration=1,volume=-12dB" \
    -filter_complex "[1:a]volumedetect[m];[0:a][m]amix=inputs=2:duration=longest[a]" -map "[a]" \
    "${tail[@]}" "$tmp/desktop-and-microphone.mka"
# Desktop audio only: no volumedetect at all.
record desktop-only "${head[@]}" \
    -thread_queue_size 1024 -f lavfi -i "sine=frequency=220:sample_rate=48000:duration=1" \
    -map 0:a "${tail[@]}" "$tmp/desktop-only.mka"
echo "recorded into $corpus"

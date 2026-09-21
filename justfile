set dotenv-load := true

build_dir := "build/macos"
out_dir := "build/captures"
tas := "target/release/melee-tas"
movies := "crates/melee-tas/movies"
disc := env_var_or_default("MELEE_DISC", "")
events_port := "7788"
quiet := "MELEE_MUTE=1"
size := "1920x1080"
hrc_movie := "hrc_puff_rest_bat"

export DAWN_INCLUDE_DIR := justfile_directory() / build_dir / "_deps/dawn_prebuilt-src/include"

# list recipes
default:
    @just --list --unsorted

# configure the CMake build (needed once, and after CMake changes)
configure:
    cmake --preset macos-default

# build the game and the disc-free smoke host
build:
    ninja -C {{build_dir}} melee debug_ui_smoke

# build only the smoke host (fast loop for UI work)
build-smoke:
    ninja -C {{build_dir}} debug_ui_smoke

# build the melee-tas movie compiler
build-tas:
    cargo build --release -p melee-tas

# format the Rust workspace
fmt:
    cargo fmt --all

# clippy the Rust workspace with the house flags
lint:
    cargo clippy --all --benches --tests --examples --all-features -- -D warnings

# run unit tests; pass a filter to run a subset, e.g. `just test overlay`
test filter="":
    cargo test --workspace {{filter}}

# fmt check, clippy and tests for the Rust workspace
check:
    cargo fmt --all --check
    just lint
    just test

# run the smoke host with the HUD overlays; frames=0 runs until the window closes
smoke frames="0": build-smoke
    {{build_dir}}/debug_ui_smoke {{frames}} overlays

# run the smoke host with the debug window and the scripted controller input
smoke-pad frames="120": build-smoke
    {{build_dir}}/debug_ui_smoke {{frames}} pad

# record the smoke host to build/captures/smoke.mp4 and print its stream info
smoke-capture frames="300": build-smoke _out
    MELEE_CAPTURE={{out_dir}}/smoke.mp4 {{build_dir}}/debug_ui_smoke {{frames}} overlays
    just probe {{out_dir}}/smoke.mp4

# play the game normally; extra args go to the game, e.g. `just run --no-card`
run *args: build _disc
    {{build_dir}}/melee {{args}} "{{disc}}"

# play with the HUD overlays on (F3 toggles them, F2 opens the debug window)
run-hud *args: build _disc
    MELEE_DEBUG_OVERLAYS=1 {{build_dir}}/melee {{args}} "{{disc}}"

# play while recording inputs to build/captures/<name>.mrc
record name="session": build _disc _out
    MELEE_NET_RECORD={{out_dir}}/{{name}}.mrc {{build_dir}}/melee --no-card "{{disc}}"

# compile a .tas script to build/captures/<stem>.mrc
tas script: build-tas _out
    {{tas}} compile {{script}} {{out_dir}}/{{file_stem(script)}}.mrc

# turn a recorded .mrc back into an editable .tas script
untas movie: build-tas _out
    {{tas}} decompile {{movie}} {{out_dir}}/{{file_stem(movie)}}.tas

# replay a .mrc movie with the HUD on; frames=0 runs until the window closes
replay movie frames="0": build _disc
    {{quiet}} MELEE_NET_REPLAY={{movie}} MELEE_DEBUG_VS=cpu MELEE_DEBUG_OVERLAYS=1 MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"

# render a .mrc movie to build/captures/<stem>.mp4 at an exact 60 fps
render movie frames="0": build _disc _out
    {{quiet}} MELEE_CAPTURE={{out_dir}}/{{file_stem(movie)}}.mp4 MELEE_NET_REPLAY={{movie}} MELEE_DEBUG_VS=cpu MELEE_DEBUG_OVERLAYS=1 MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"
    just probe {{out_dir}}/{{file_stem(movie)}}.mp4

# compile the bundled full-match script and render it to video
demo: (tas movies / "debug_vs_full_match.tas")
    just render {{out_dir}}/debug_vs_full_match.mrc 8697

# play Home Run Contest with no menus; character is a CKind number (15 is Jigglypuff)
hrc character="15" *args: build _disc
    MELEE_BOOT_SCENE=homerun MELEE_BOOT_CHARACTER={{character}} MELEE_DEBUG_OVERLAYS=1 {{build_dir}}/melee {{args}} --no-card "{{disc}}"

# render a Home Run Contest movie to build/captures/<stem>.mp4, muted
hrc-render movie character="15" frames="0": build _disc _out
    {{quiet}} MELEE_WINDOW_SIZE={{size}} MELEE_CAPTURE={{out_dir}}/{{file_stem(movie)}}.mp4 MELEE_NET_REPLAY={{movie}} MELEE_BOOT_SCENE=homerun MELEE_BOOT_CHARACTER={{character}} MELEE_DEBUG_OVERLAYS=1 MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"
    just probe {{out_dir}}/{{file_stem(movie)}}.mp4

# compile a bundled Home Run Contest movie and render it to video, e.g. `just hrc-demo hrc_puff_rest 800`
hrc-demo movie=hrc_movie frames="1150": (tas movies / movie + ".tas")
    just hrc-render {{out_dir}}/{{movie}}.mrc 15 {{frames}}

# replay a Home Run Contest movie and write player positions to build/captures/<stem>.csv
hrc-trace movie character="15" frames="0": build _disc _out
    {{quiet}} MELEE_TRACE={{out_dir}}/{{file_stem(movie)}}.csv MELEE_NET_REPLAY={{movie}} MELEE_BOOT_SCENE=homerun MELEE_BOOT_CHARACTER={{character}} MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"

# replay a Home Run Contest movie while shipping match events (run `just events` first)
hrc-events movie character="15" frames="0": build _disc
    {{quiet}} MELEE_EVENTS_ADDR=127.0.0.1:{{events_port}} MELEE_NET_REPLAY={{movie}} MELEE_BOOT_SCENE=homerun MELEE_BOOT_CHARACTER={{character}} MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"

# replay a movie while shipping match events to a local collector (run `just events` first)
replay-events movie frames="0": build _disc
    {{quiet}} MELEE_EVENTS_ADDR=127.0.0.1:{{events_port}} MELEE_NET_REPLAY={{movie}} MELEE_DEBUG_VS=cpu MELEE_EXIT_AFTER_FRAMES={{frames}} {{build_dir}}/melee --no-card "{{disc}}"

# act on one melee-demo ticket in <project> when a Home Run Contest ends; dry-run unless --live. Start it before the game
sidecar project *args:
    cargo run --release -p melee-sidecar -- --addr 127.0.0.1:{{events_port}} --project {{project}} {{args}}

# the whole bit: sidecar, the bundled movie, and a video of it; extra args go to the sidecar, e.g. `just hrc-jira INFERENG --done-status Closed --live`
hrc-jira project *args: build _disc _out (tas movies / hrc_movie + ".tas")
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release -p melee-sidecar
    target/release/melee-sidecar --addr 127.0.0.1:{{events_port}} --project {{project}} {{args}} &
    sidecar=$!
    until lsof -nP -iTCP:{{events_port}} -sTCP:LISTEN >/dev/null 2>&1; do kill -0 $sidecar; sleep 0.2; done
    {{quiet}} MELEE_WINDOW_SIZE={{size}} MELEE_CAPTURE={{out_dir}}/hrc_jira.mp4 MELEE_EVENTS_ADDR=127.0.0.1:{{events_port}} MELEE_NET_REPLAY={{out_dir}}/{{hrc_movie}}.mrc MELEE_BOOT_SCENE=homerun MELEE_BOOT_CHARACTER=15 MELEE_DEBUG_OVERLAYS=1 MELEE_EXIT_AFTER_FRAMES=1500 {{build_dir}}/melee --no-card "{{disc}}"
    wait $sidecar

# listen for NDJSON match events and pretty-print them
events:
    nc -l 127.0.0.1 {{events_port}}

# print codec, size, frame rate and an exact frame count for a video
probe video:
    ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=codec_name,width,height,r_frame_rate,duration,nb_read_frames -of default=nw=1 {{video}}

# extract one frame of a video as a PNG next to it
frame video n="0":
    ffmpeg -hide_banner -loglevel error -y -i {{video}} -vf "select=eq(n\,{{n}})" -frames:v 1 {{without_extension(video)}}_f{{n}}.png
    @echo {{without_extension(video)}}_f{{n}}.png

# delete recordings and compiled movies
clean-captures:
    rm -rf {{out_dir}}

_out:
    @mkdir -p {{out_dir}}

_disc:
    @test -n "{{disc}}" || { echo "set MELEE_DISC to your disc image path, e.g. in fish: set -Ux MELEE_DISC /path/to/disc.iso" >&2; exit 1; }
    @test -f "{{disc}}" || { echo "MELEE_DISC does not exist: {{disc}}" >&2; exit 1; }

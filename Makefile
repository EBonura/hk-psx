PYTHON ?= python3
EMULATOR ?= ../PSoXide-emulator/target/release/frontend
.PHONY: build run test validate doctor
build:
	cargo hk-build build --no-validate
run: build
	$(PYTHON) tools/run.py --emulator "$(EMULATOR)"
test:
	.venv/bin/python -m unittest discover -s tests
	cargo test --manifest-path shared/hk-format/Cargo.toml --locked
	cargo test --manifest-path shared/hk-sim/Cargo.toml --locked
	cargo test --manifest-path shared/hk-cache/Cargo.toml --locked
	$(PYTHON) tools/test_world.py
	mkdir -p .hkpsx/hud-tests
	rustc --edition=2021 --test game/src/hud_state.rs -o .hkpsx/hud-tests/hud-state-tests
	.hkpsx/hud-tests/hud-state-tests
	rustc --edition=2021 --test game/src/preload.rs -o .hkpsx/hud-tests/preload-tests
	.hkpsx/hud-tests/preload-tests
	rustc --edition=2021 --test game/src/scenery_geometry.rs -o .hkpsx/hud-tests/scenery-geometry-tests
	.hkpsx/hud-tests/scenery-geometry-tests
	rustc --edition=2021 --test tests/scenery_bounds_runtime.rs -o .hkpsx/hud-tests/scenery-bounds-tests
	.hkpsx/hud-tests/scenery-bounds-tests
	rustc --edition=2021 --test game/src/draw_packet.rs -o .hkpsx/hud-tests/draw-packet-tests
	.hkpsx/hud-tests/draw-packet-tests
	rustc --edition=2021 --test game/src/volume.rs -o .hkpsx/hud-tests/volume-tests
	.hkpsx/hud-tests/volume-tests
	rustc --edition=2021 --test tests/audio_stream_runtime.rs -o .hkpsx/hud-tests/audio-stream-tests
	.hkpsx/hud-tests/audio-stream-tests
validate:
	cargo hk-build validate --frontend "$(EMULATOR)"
validate-legacy:
	$(PYTHON) tools/validate.py --emulator "$(EMULATOR)"
doctor:
	$(PYTHON) tools/doctor.py

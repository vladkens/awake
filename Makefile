.PHONY: prepare check test build update clean

CARGO_FLAGS := --release --locked

prepare:
	cargo +nightly fmt
	uvx ruff format --line-length 100 scripts
	uv run scripts/blank-lines.py --fix
	cargo clippy $(CARGO_FLAGS) --fix --all-targets --allow-dirty --allow-no-vcs -- -D warnings

check:
	cargo +nightly fmt --check
	uvx ruff format --check --line-length 100 scripts
	uv run scripts/blank-lines.py
	cargo clippy $(CARGO_FLAGS) --all-targets -- -D warnings

test:
	cargo test $(CARGO_FLAGS)

build:
	cargo build $(CARGO_FLAGS)
	ls -lh target/release/$(shell basename $(CURDIR))

update:
	cargo upgrade -i
	cargo update

clean:
	cargo clean

.PHONY: run dist pack

# dist's target: this machine's, or T.
T ?= $(shell rustc -vV | sed -n 's/^host: //p')

run: # Awake Dev, the debug build, beside an installed Awake
	-pkill -f "Awake Dev.app/Contents/MacOS/awake"
	cargo build
	shipo app --bin target/debug/awake --id me.vladkens.awake-dev --name "Awake Dev"
	"target/Awake Dev.app/Contents/MacOS/awake"

dist: # T's binary in target/distrib/T, for CI (vladkens/actions/rust-build); pack makes the files
	cargo build --locked --profile dist --target $(T)
	mkdir -p target/distrib/$(T)
	cp target/$(T)/dist/awake target/distrib/$(T)/

pack: # the DMGs of target/distrib's binaries, and on a release update.txt, signed by env's secrets
	shipo release target/distrib $(if $(TAG),--tag $(TAG))

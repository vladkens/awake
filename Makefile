.PHONY: prepare check test build update clean

CARGO_FLAGS := --release --locked

prepare:
	cargo +nightly fmt
	uv run scripts/blank-lines.py --fix
	cargo clippy $(CARGO_FLAGS) --fix --all-targets --allow-dirty --allow-no-vcs -- -D warnings

check:
	cargo +nightly fmt --check
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

.PHONY: run

run: # Awake Dev, the debug build, beside an installed Awake
	-pkill -f "Awake Dev.app/Contents/MacOS/awake"
	cargo build
	shipo app --bin target/debug/awake --id me.vladkens.awake-dev --name "Awake Dev"
	"target/Awake Dev.app/Contents/MacOS/awake"

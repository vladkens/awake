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

# awake
.PHONY: run bundle install

# FLAVOR=dev: Awake Dev, `make run`'s, an app of its own beside the installed Awake: its bundle id
# me.vladkens.awake-dev, so its own settings and login item, and a grey icon with a DEV badge. The
# binary takes it from AWAKE_FLAVOR (build.rs), the bundle from scripts/bundle.sh.
run: # Awake Dev from its bundle, logging: make run RUST_LOG=debug
	-pkill -x "Awake Dev"
	$(MAKE) bundle FLAVOR=dev
	RUST_LOG=$${RUST_LOG:-info} "target/release/Awake Dev.app/Contents/MacOS/Awake Dev"

bundle: # target/release/Awake.app, signed ad hoc; FLAVOR=dev: Awake Dev.app
	AWAKE_FLAVOR=$(FLAVOR) scripts/bundle.sh

install: # ~/Applications/Awake.app, started
	-pkill -x Awake
	scripts/bundle.sh ~/Applications
	open ~/Applications/Awake.app

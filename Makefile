.PHONY: prepare check test build update clean

LINT := uv run --script https://raw.githubusercontent.com/vladkens/lint/main/rust.py
CARGO_FLAGS := --release --locked

prepare:
	cargo +nightly fmt
	$(LINT) fix
	cargo clippy $(CARGO_FLAGS) --fix --all-targets --allow-dirty --allow-no-vcs -- -D warnings

check:
	cargo +nightly fmt --check
	cargo clippy $(CARGO_FLAGS) --all-targets -- -D warnings
	$(LINT)

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

.PHONY: run dist pack i18n shots

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

# xgettext reads the texts marked with t! and tn! (and the `// Translators:` comments above them)
# into target/messages.pot, msgmerge brings each locales/*.po up to it, msgfmt checks them and
# counts the texts translated, fuzzy and missing. Needs GNU gettext 0.24 or newer (Rust): `brew
# install gettext`.
i18n: # the marked texts into target/messages.pot, merged into locales/*.po and checked
	mkdir -p target
	xgettext --language=Rust --keyword='t!:1' --keyword='tn!:1,2' --add-comments=Translators \
	  --from-code=UTF-8 --sort-by-file --package-name=Awake -o target/messages.pot src/*.rs
	for po in locales/*.po; do \
	  msgmerge --quiet --update --backup=none $$po target/messages.pot && \
	  printf '%s: ' $$po && msgfmt -c --statistics -o /dev/null $$po || exit 1; \
	done

# LOCALE=ru (or pseudo, nimbo's accented and longer texts) draws them in that language, into
# target/shots/<LOCALE>/.
shots: # the settings window's pages and About's update states, headless, into target/shots/
	AWAKE_SHOTS=$(CURDIR)/target/shots$(if $(LOCALE),/$(LOCALE)) $(if $(LOCALE),AWAKE_SHOTS_LOCALE=$(LOCALE)) \
	  cargo test $(CARGO_FLAGS) -- every_page_renders

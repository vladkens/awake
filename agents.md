# awake

A menu bar app that keeps the Mac awake, as KeepingYouAwake does: a power assertion, as
`caffeinate` takes one, on until turned off or for a while, and the settings that let the Mac
sleep again (Low Power Mode, a low battery). Agents carry out each change end to end by following
this file.

Each fact has one source, which this file points to: what the app does is `readme.md` and the
rustdoc (`src/main.rs` first), dependencies `Cargo.toml`, checks the `Makefile`.

## Scope

- macOS only, Apple Silicon and Intel, macOS 13+ (`SMAppService`). A menu bar app: no Dock icon,
  no windows; the menu is the whole UI.
- Pure Rust: the status item, its menu, the settings window and launch at login through nimbo;
  Foundation, IOKit and what nimbo lacks of AppKit (`src/appkit.rs`) through the `objc2-*` crates;
  no Swift, no C compiled, no bindings generated. Enable only the `objc2-*` features the code uses.
- Prefer a small own routine to a dependency. Change dependencies only with `cargo add` /
  `cargo remove` / `cargo upgrade`, never by hand.
- Keep it small and simple: what KeepingYouAwake does that people use, not every option it has.

## Code

- Start every `.rs` file with the line
  `// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake`.
- Comment only what the code can't say: platform quirks, measured numbers, ordering that matters.
  Write code that reads like the code around it.
- AppKit runs on the main thread, which `MainThreadMarker` proves; other threads and timers reach
  it through the main dispatch queue. The menu's events come from the run loop, never while the
  app's state is borrowed.
- Nothing runs while nothing changes: the battery, Low Power Mode and wake come as notifications,
  timers only while a timed session runs. No polling.
- The text the app shows is English.

## Tests

- `make check` (format, clippy with warnings denied) and `make test` run before every commit.
- Unit-test what breaks silently: the time left, the settings' rules. Look at the menu itself in
  the running app (`make run`: Awake Dev, `me.vladkens.awake-dev`, beside an installed Awake);
  check the assertion with `pmset -g assertions`.

## Workflow

- Before committing, run `make check` and `make test`. `make prepare` rewrites files: not a check.
- Make each commit one meaningful change a person can check: one short lowercase line, no body,
  docs updated in the same commit. Push only when asked.
- When fixing your own unpushed commit, amend or fix up that commit instead of adding a separate
  fix.
- Ask the person only when what the app does must change, a heavy dependency is needed, or the
  request is ambiguous. Otherwise decide and say so in your report.
- Keep docs to `readme.md` (users), the rustdoc and `agents.md`. No progress journals, run logs or
  disclaimer paragraphs.

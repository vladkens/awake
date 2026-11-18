# awake

Keeps your Mac awake from the menu bar, like [KeepingYouAwake](https://github.com/newmarcel/KeepingYouAwake), written in Rust.

## 🌟 Features

- A click on the cup opens the menu and never toggles anything, so a missed click turns nothing off.
- Keep awake until turned off, or for 15 minutes to 8 hours; the menu counts down the time left, and the menu bar can show it beside the cup.
- Let the display sleep: only the system stays awake (`caffeinate -i` instead of `-d`).
- Turn off when the screen locks.
- Battery rules: turn off below a battery level (10–90 %), in Low Power Mode, or when unplugged. A session turned on by hand while a rule applies stays on until its cause clears.
- Launch at login, and keep awake at launch.
- Settings… in the menu, or open the app again, to get the settings window.
- Updates itself from the releases here: checked once a day, Update to … in the menu installs it and relaunches; About has Check for Updates.
- A command line for scripts and agents: `awake on 1h`, `awake off`, `awake status`, named holds, and `awake -- <command>`.
- In English and 12 more languages: the Mac's, or the one set for Awake in System Settings.
- Idle at 0 % CPU: the battery, Low Power Mode, wake and the screen lock come as notifications, nothing polls.

## 📥 Install

macOS 13 or later, Apple silicon or Intel. With Homebrew, which also links the command line as `awake`:

```sh
brew install --cask vladkens/tap/awake
```

Or download the DMG from the [latest release](https://github.com/vladkens/awake/releases/latest). Awake updates itself from there.

To build it from source, with a recent stable Rust and [shipo](https://github.com/vladkens/shipo):

```sh
git clone https://github.com/vladkens/awake && cd awake
make run   # builds and starts Awake Dev
```

## 🚀 Usage

Click the cup in the menu bar and pick Keep Awake Indefinitely, or a length under Keep Awake For; Turn Off in the same place ends it.

```sh
pmset -g assertions   # shows what keeps the Mac awake
```

### Command line

The app's binary takes commands and passes them to the running Awake, opening it first when needed. The Homebrew cask links it as `awake`; otherwise run `/Applications/Awake.app/Contents/MacOS/awake`.

```sh
awake on 1h               # keep awake for an hour: 90 (minutes), 45m, 2h, 1h30m; no length: until off
awake off                 # turn off, holds too
awake status              # what keeps the Mac awake
awake -- make release     # keep awake while the command runs, exit with its status
awake hold build --for 2h # a named hold: until `awake release build`, or for the length
awake release build
```

Holds keep the Mac awake beside the menu's session, so a script or an agent never turns off what you turned on. The menu lists them, and Turn Off ends them too. The battery rules apply to them; locking the screen doesn't end them. A command's hold ends with it, even when it's killed.

For Claude Code, hooks in `~/.claude/settings.json` keep the Mac awake while it works, an hour at most if a turn never ends. Each session holds by its own name, so one that ends leaves the others' holds. They need `jq`, part of macOS since 15 (`brew install jq` before it):

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "awake hold claude-$(jq -r '.session_id[:8]') --for 1h" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "awake release claude-$(jq -r '.session_id[:8]')" }] }]
  }
}
```

Awake Dev, `make run`'s, has its own id and settings, so it runs beside an installed Awake. `pmset -g assertions` shows which of the two holds the Mac.

## 🦀 Why Rust

I write Rust every day and find it easy. For a small app like this I didn't want Swift or an Xcode project. The UI is drawn by [nimbo](https://github.com/vladkens/nimbo), my own small Rust UI toolkit, and this app also helps test it. My other Mac tools in Rust: [macmon](https://github.com/vladkens/macmon) and [wavo](https://github.com/vladkens/wavo).

How it compares, measured on an M5 with macOS 27.2:

- Memory at idle, only the menu bar icon: Awake 13 MB, KeepingYouAwake 1.6.8 (Objective-C) 13 MB. The same.
- Memory with the settings window open: Awake on nimbo 22 MB, the same app with an AppKit window 29 MB.
- App size: Awake.app 3.6 MB (0.8 MB of it the icon), KeepingYouAwake.app 7.6 MB.
- Stripped binary: 2.6 MiB on Apple silicon, 2.7 MiB on Intel.
- Clean release build: about 12 s.

## 🤝 Contributing

Pull requests are welcome, and so are your agents: point them at [agents.md](agents.md).

## 📝 License

Distributed under the [MIT License](LICENSE).

# awake

Keeps the Mac awake from the menu bar, as KeepingYouAwake does, in pure Rust: the UI drawn by
[nimbo](https://github.com/vladkens/nimbo), IOKit through `objc2`, no Swift, no C toolchain.

## 🌟 Features

A click on the cup opens the menu: Keep Awake until turned off, for 1 hour or for 3 hours, with
the time left on top. Settings… (or opening the app again) shows a settings window with
General, Battery and About tabs, and the Dock icon while it is open:

- 🖥️ Allow display to sleep: only the system stays awake (`caffeinate -i` instead of `-d`)
- 🔋 Turn off below a battery level (a slider, 10–90 %), in Low Power Mode, or when unplugged; turned on by hand
  while one of these applies, Awake stays on until it clears, and the cup shows steam
- 🚀 Launch at login, keep awake on launch

Idle at 0 % CPU: the battery, Low Power Mode and wake come as notifications, nothing
polls, and the window is built only while it is open.

## 🚀 Usage

```sh
make install   # ~/Applications/Awake.app, started
make run       # Awake Dev, beside Awake with its own settings, with logs
```

Launch at Login works from `Awake.app` only. `pmset -g assertions` lists what keeps the Mac
awake, Awake's assertion among them.

## 📝 License

[MIT](LICENSE)

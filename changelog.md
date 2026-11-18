## v0.2.0 – 2026-11-18

### Features

- Added 12 languages: German, Spanish, French, Italian, Japanese, Korean, Polish, Portuguese (Brazil), Russian, Turkish, Ukrainian and Simplified Chinese; Awake speaks the Mac's language, or the one set for it in System Settings (#14)

### Improvements

- Added a line under each setting that says what it does (#13)
- Shortened About's buttons to GitHub, Donate and the update button, in one row (#13)

**Full Changelog**: https://github.com/vladkens/awake/compare/v0.1.0...v0.2.0

---

## v0.1.0 – 2026-11-18

The first release: a menu bar app that keeps the Mac awake, as KeepingYouAwake does.

### Features

- Added the menu: a click on the cup opens it and never toggles anything; keep awake until turned off or for 15 minutes to 8 hours, with the time left counting down (#1)
- Added the settings: launch at login, keep awake at launch, let the display sleep, turn off when the screen locks, the time left beside the cup, and the battery rules (below a level, Low Power Mode, unplugged) (#1)
- Added updates in place: checked once a day, Update to … in the menu, Check for Updates in About (#3)
- Added the command line: `awake on 1h`, `awake off`, `awake status`, named holds for scripts and agents (`awake hold claude --for 1h`, `awake release claude`), and `awake -- <command>` (#4)
- Added DMGs for Apple silicon and Intel, and the Homebrew cask `vladkens/tap/awake` (#2)

**Full Changelog**: https://github.com/vladkens/awake/commits/v0.1.0

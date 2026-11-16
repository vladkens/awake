// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! Updates in place with [shipo]: a signed feed beside each release on GitHub. The app checks at
//! launch, on wake and when the menu opens, once a day at most, and when the person asks in About.
//! A newer release shows as a menu item, which downloads it and relaunches as the new version;
//! About says how a check went. Where the app can't update (a build, a folder the person can't
//! write, a dev build), none of it shows.
//!
//! The [Updater] is the state without the network: the app runs shipo's check, download and
//! install on threads of its own and brings each result back to it.

use std::time::{Duration, SystemTime};

use shipo::{Error, Feed, Release};

/// `update.txt` beside the latest release's files, which CI's publish job writes and signs
/// (`make pack`'s `shipo release --tag`); `latest` is never the dev pre-release. The public keys
/// `shipo keygen` printed: the working one, then the spare. Fakeable: `SHIPO_FAKE_UPDATE=1.2.0`
/// (shipo's readme's Fake update) shows the updater in any build, with no release or network.
pub const FEED: Feed = Feed::new(
  "https://github.com/vladkens/awake/releases/latest/download/update.txt",
  &[
    "b55b05ab868877ae73615488842d798f177cef7f38414281513c337bb8951ada",
    "995bba6ccf0282159e60b49d67da30646f6386df5394275705772c953d601062",
  ],
)
.fakeable();

/// An automatic check once a day; one that got no answer is tried again at the next chance.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A dev build's version, a pre-release (`0.1.0-5-gabc`, CI's between releases), which takes no
/// update: shipo's [check](shipo::check) refuses it, and its [supported](shipo::supported) doesn't
/// tell.
pub fn dev(version: &str) -> bool {
  version.contains('-')
}

/// A check's answer as the [Updater] takes it: a missing feed (404) and one whose content or
/// signature is wrong are nothing newer; another error is its text ([why]).
pub fn found(checked: shipo::Result<Option<Release>>) -> Result<Option<Release>, String> {
  checked.or_else(|e| match e {
    Error::Http { status: Some(404), .. } | Error::Feed(_) | Error::Signature(_) => Ok(None),
    _ => Err(why(&e)),
  })
}

/// An error's text as About shows it: a request's without its URL, which the log has.
pub fn why(e: &Error) -> String {
  match e {
    Error::Http { message, .. } => message.clone(),
    _ => e.to_string(),
  }
}

/// `v1.2.0`, as the app shows a version.
pub fn shown(version: &str) -> String {
  format!("v{}", version.trim_start_matches('v'))
}

/// The whole percent of `done` bytes of `total`: a download tells the app only when it changes.
pub fn percent(done: u64, total: u64) -> u8 {
  (done.saturating_mul(100) / total.max(1)).min(100) as u8
}

/// What the updater shows, in the menu and in About.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
  /// Nothing: no answer yet, or nothing newer from an automatic check.
  Idle,
  /// A check the person asked for runs; an automatic one shows nothing.
  Checking,
  /// The check the person asked for found nothing newer.
  UpToDate,
  Available(Release),
  /// With the whole percent done; then the app relaunches as the new version.
  Downloading(Release, u8),
  /// Why the check, the download or the install failed: with the release when it was its
  /// download or install, which trying again downloads; else trying again checks.
  Failed(Option<Release>, String),
}

/// The state, and when to check.
#[derive(Debug)]
pub struct Updater {
  pub state: State,
  /// A check runs: whether the person asked for it.
  checking: Option<bool>,
  /// When a check last got an answer.
  answered: Option<SystemTime>,
  /// The app can't update itself: nothing shows and no check runs.
  off: bool,
}

impl Updater {
  /// `on`: the app updates itself, as decided at launch.
  pub fn new(on: bool) -> Self {
    Self { state: State::Idle, checking: None, answered: None, off: !on }
  }

  /// What the menu and About show, `None` when the app can't update.
  pub fn showing(&self) -> Option<&State> {
    (!self.off).then_some(&self.state)
  }

  /// An automatic check: none answered yet, or the last a day before `now` (or after it, the clock
  /// set back).
  pub fn due(&self, now: SystemTime) -> bool {
    let old = |t: SystemTime| now.duration_since(t).map_or(true, |d| d >= DAY);
    !self.off && self.checking.is_none() && self.answered.is_none_or(old)
  }

  /// Whether to start a check: not while one runs, which the person's joins, or while an update is
  /// on its way. The person's shows Checking.
  pub fn check(&mut self, manual: bool) -> bool {
    if self.off || matches!(self.state, State::Downloading(..)) {
      return false;
    }

    if manual {
      self.state = State::Checking;
    }

    let start = self.checking.is_none();
    self.checking = Some(manual || self.checking == Some(true));
    start
  }

  /// A check's answer at `now` ([found]): the newer release, none, or why the check failed, which
  /// is tried again at the next chance. Nothing newer and an error show only to the person who
  /// asked; nothing newer takes back an offer the feed no longer makes, a failed one's too, and a
  /// newer release replaces it.
  pub fn checked(&mut self, found: Result<Option<Release>, String>, now: SystemTime) {
    let Some(manual) = self.checking.take() else { return };
    if found.is_ok() {
      self.answered = Some(now);
    }

    if matches!(self.state, State::Downloading(..)) {
      return;
    }

    let offered = matches!(self.state, State::Available(_) | State::Failed(Some(_), _));
    self.state = match found {
      Ok(Some(r)) => State::Available(r),
      Ok(None) if manual => State::UpToDate,
      Ok(None) if offered => State::Idle,
      Err(e) if manual => State::Failed(None, e),
      _ => return,
    };
  }

  /// Install, or try again after a failed download or install: the release to download.
  pub fn install(&mut self) -> Option<Release> {
    let (State::Available(r) | State::Failed(Some(r), _)) = &self.state else { return None };
    let r = r.clone();
    self.state = State::Downloading(r.clone(), 0);
    Some(r)
  }

  /// The download's whole percent ([percent]).
  pub fn progress(&mut self, done: u8) {
    if let State::Downloading(_, at) = &mut self.state {
      *at = done;
    }
  }

  /// The download or the install failed, `why`.
  pub fn failed(&mut self, why: String) {
    if let State::Downloading(r, _) = &self.state {
      self.state = State::Failed(Some(r.clone()), why);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const NOW: SystemTime = SystemTime::UNIX_EPOCH;

  fn r(v: &str) -> Release {
    Release::test(v)
  }

  fn newer(v: &str) -> Result<Option<Release>, String> {
    Ok(Some(r(v)))
  }

  fn error() -> Result<Option<Release>, String> {
    Err("offline".into())
  }

  /// `u` after an automatic or a manual check answered `found`.
  fn checked(u: &mut Updater, manual: bool, found: Result<Option<Release>, String>) {
    assert!(u.check(manual));
    u.checked(found, NOW);
  }

  #[test]
  fn a_dev_build_takes_no_update() {
    assert!(dev("0.1.0-5-gabc") && dev("1.2.0-rc.1"));
    assert!(!dev("0.1.0") && !dev("1.2.0"));
  }

  /// A missing feed and one whose content or signature is wrong are nothing newer; another error
  /// shows its text.
  #[test]
  fn a_missing_or_bad_feed_is_nothing_newer() {
    let url = FEED.url.to_owned();
    let http =
      |status, message: &str| Error::Http { url: url.clone(), status, message: message.into() };
    let quiet = [
      http(Some(404), "HTTP 404"),
      Error::Feed("no version, or a bad one".into()),
      Error::Signature("no key of the app verifies it".into()),
    ];
    for e in quiet {
      let what = e.to_string();
      assert_eq!(found(Err(e)), Ok(None), "{what}");
    }

    let offline = "The Internet connection appears to be offline.";
    assert_eq!(found(Err(http(None, offline))), Err(offline.into()));
    assert_eq!(found(Err(http(Some(503), "HTTP 503"))), Err("HTTP 503".into()));
    assert_eq!(found(Ok(Some(r("1.2.0")))), newer("1.2.0"));
  }

  #[test]
  fn the_feed_has_valid_keys() {
    assert!(FEED.url.starts_with("https://") && FEED.keys.len() == 2);
    for key in FEED.keys {
      assert!(key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()), "{key}");
    }

    // Read with no network: valid keys get past `Keys`, and the signature is what fails.
    let read = FEED.read(b"version 1\nsig 00\n", "0.1.0").unwrap_err();
    assert!(matches!(read, Error::Signature(_)), "{read}");
  }

  #[test]
  fn the_percent_is_whole() {
    let got = [(0, 100), (419, 1_000), (999, 1_000), (1_000, 1_000), (5, 0), (u64::MAX, 2)];
    assert_eq!(got.map(|(done, total)| percent(done, total)), [0, 41, 99, 100, 100, 100]);
  }

  #[test]
  fn an_automatic_check_is_due_daily_and_after_a_failure() {
    let mut u = Updater::new(true);
    assert!(u.due(NOW), "at launch");
    assert!(u.check(false));
    assert!(!u.due(NOW) && !u.check(false), "one at a time");
    u.checked(error(), NOW);
    assert!(u.due(NOW), "no answer: again at the next chance");

    checked(&mut u, false, Ok(None));
    assert!(!u.due(NOW + DAY - Duration::from_secs(1)) && u.due(NOW + DAY));
    assert!(u.due(NOW - Duration::from_secs(1)), "the clock set back");
  }

  #[test]
  fn an_app_that_cant_update_shows_nothing() {
    let mut u = Updater::new(false);
    assert_eq!(u.showing(), None);
    assert!(!u.due(NOW) && !u.check(true));

    let u = Updater::new(true);
    assert_eq!(u.showing(), Some(&State::Idle));
  }

  #[test]
  fn an_automatic_check_shows_only_what_is_new() {
    let mut u = Updater::new(true);
    checked(&mut u, false, Ok(None));
    checked(&mut u, false, error());
    assert_eq!(u.state, State::Idle, "nothing new, offline: quiet");

    checked(&mut u, false, newer("1.2.0"));
    assert_eq!(u.state, State::Available(r("1.2.0")));
    checked(&mut u, false, error());
    assert_eq!(u.state, State::Available(r("1.2.0")), "offline: the offer stays");
    checked(&mut u, false, Ok(None));
    assert_eq!(u.state, State::Idle, "the feed took it back");
  }

  #[test]
  fn the_persons_check_says_how_it_went() {
    let mut u = Updater::new(true);
    assert!(u.check(true));
    assert_eq!(u.state, State::Checking);
    u.checked(Ok(None), NOW);
    assert_eq!(u.state, State::UpToDate);

    checked(&mut u, true, error());
    assert_eq!(u.state, State::Failed(None, "offline".into()));
    assert_eq!(u.install(), None, "nothing to download: trying again checks");
    checked(&mut u, true, newer("1.2.0"));
    assert_eq!(u.state, State::Available(r("1.2.0")));

    // The person's joins an automatic one.
    let mut u = Updater::new(true);
    assert!(u.check(false));
    assert!(!u.check(true), "no second thread");
    assert_eq!(u.state, State::Checking);
    u.checked(Ok(None), NOW);
    assert_eq!(u.state, State::UpToDate);
  }

  #[test]
  fn an_install_downloads_or_fails_and_tries_again() {
    let mut u = Updater::new(true);
    assert_eq!(u.install(), None);
    checked(&mut u, false, newer("1.2.0"));
    assert_eq!(u.install(), Some(r("1.2.0")));
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 0));
    assert!(!u.check(true) && !u.check(false), "no check while it downloads");
    u.progress(percent(41_990, 100_000));
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 41));

    let e = "1.2.0 is not the file";
    u.failed(e.into());
    let failed = State::Failed(Some(r("1.2.0")), e.into());
    assert_eq!(u.state, failed);
    checked(&mut u, false, error());
    assert_eq!(u.state, failed, "offline: the failure stays");
    assert_eq!(u.install(), Some(r("1.2.0")), "trying again");
    u.failed(e.into());
    checked(&mut u, false, newer("1.3.0"));
    assert_eq!(u.state, State::Available(r("1.3.0")), "a newer one replaces it");
    u.install();
    u.failed(e.into());
    checked(&mut u, false, Ok(None));
    assert_eq!(u.state, State::Idle, "the feed took it back");

    // A check answered during the download leaves it be.
    let mut u = Updater::new(true);
    checked(&mut u, false, newer("1.2.0"));
    assert!(u.check(false));
    u.install();
    u.checked(newer("1.3.0"), NOW);
    assert_eq!(u.state, State::Downloading(r("1.2.0"), 0));
  }
}

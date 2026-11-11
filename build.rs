// Copyright (c) vladkens | MIT License | https://github.com/vladkens/awake
//! The build script: the flavor, `cfg(awake_dev)` with `AWAKE_FLAVOR=dev` (`make run`'s bundle):
//! Awake Dev, an app of its own beside the release (main.rs's `DEV`); unset, empty or `release`:
//! Awake.

fn main() {
  println!("cargo:rustc-check-cfg=cfg(awake_dev)");
  println!("cargo:rerun-if-env-changed=AWAKE_FLAVOR");
  match std::env::var("AWAKE_FLAVOR").unwrap_or_default().as_str() {
    "" | "release" => {}
    "dev" => println!("cargo:rustc-cfg=awake_dev"),
    other => panic!("AWAKE_FLAVOR={other}: release or dev"),
  }
}

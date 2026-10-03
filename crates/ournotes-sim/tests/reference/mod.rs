//! Reference vectors recorded from the game client. They are not distributed with this crate: the
//! `native-fixtures` feature enables the tests that read them, and `OURNOTES_FIXTURES` names their directory.
//! A missing variable or file fails the test.
#![allow(dead_code)]

use std::path::PathBuf;

pub fn path(name: &str) -> PathBuf {
    let dir = std::env::var_os("OURNOTES_FIXTURES")
        .expect("the native-fixtures feature reads its fixtures from the directory in OURNOTES_FIXTURES");
    let path = PathBuf::from(dir).join(name);
    assert!(path.is_file(), "missing fixture {}", path.display());
    path
}

pub fn read(name: &str) -> String {
    let path = path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn json(name: &str) -> serde_json::Value {
    serde_json::from_str(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

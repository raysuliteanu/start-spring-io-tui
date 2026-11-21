use assert_cmd::Command;

fn bin() -> Command {
    Command::cargo_bin("start-spring-io-tui").expect("binary is built by cargo test")
}

#[test]
fn version_flag_shows_git_and_rustc() {
    let out = bin()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).expect("utf8");
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "{out}");
    assert!(out.contains("rustc "), "{out}");
    assert!(out.contains(" built "), "{out}");
}

#[test]
fn help_lists_offline() {
    let out = bin()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(out).expect("utf8").contains("--offline"));
}

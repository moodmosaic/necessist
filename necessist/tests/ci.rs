use assert_cmd::assert::OutputAssertExt;
use cargo_metadata::MetadataCommand;
use regex::Regex;
use std::{
    env::{remove_var, set_current_dir},
    fs::{read_to_string, OpenOptions},
    io::{stderr, Write},
    path::Path,
    process::Command,
    str::from_utf8,
};
use tempfile::tempdir;

#[ctor::ctor]
fn initialize() {
    remove_var("CARGO_TERM_COLOR");
    set_current_dir("..").unwrap();
}

#[test]
fn clippy() {
    clippy_command(&[], &["--deny=warnings", "--warn=clippy::pedantic"])
        .assert()
        .success();
}

#[test]
fn dylint() {
    // smoelius: Generate `warnings.json` and run Clippy for `overscoped_allow`.
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("warnings.json")
        .unwrap();

    clippy_command(
        &["--message-format=json"],
        &[
            "--force-warn=clippy::all",
            "--force-warn=clippy::pedantic",
            "--force-warn=clippy::expect_used",
            "--force-warn=clippy::unwrap_used",
            "--force-warn=clippy::panic",
        ],
    )
    .stdout(file)
    .assert()
    .success();

    Command::new("cargo")
        .args(["dylint", "--all", "--", "--all-features", "--all-targets"])
        .env("DYLINT_RUSTFLAGS", "--deny warnings")
        .assert()
        .success();
}

#[test]
fn format() {
    preserves_cleanliness(|| {
        Command::new("cargo")
            .args(["+nightly", "fmt"])
            .assert()
            .success();
    });
}

#[test]
fn github() {
    const EXCEPTIONS: &[&str] = &["ci_is_disabled", "dogfood", "general"];

    let metadata = MetadataCommand::new().no_deps().exec().unwrap();
    let package = metadata
        .packages
        .into_iter()
        .find(|package| package.name == "necessist")
        .unwrap();
    let mut metadata_tests = package
        .targets
        .into_iter()
        .filter_map(|target| {
            if target.is_test() && !EXCEPTIONS.contains(&target.name.as_str()) {
                Some(target.name)
            } else {
                None
            }
        })
        .chain(std::iter::once(String::from("other")))
        .collect::<Vec<_>>();
    metadata_tests.sort();

    let ci_yml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/ci.yml");
    let contents = read_to_string(ci_yml).unwrap();
    let test_array = contents
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("test: "))
        .unwrap();
    let ci_tests = test_array
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap()
        .split(", ")
        .collect::<Vec<_>>();

    assert_eq!(metadata_tests, ci_tests);
}

#[test]
fn license() {
    let re = Regex::new(r"^[^:]*\b(Apache-2.0|0BSD|BSD-\d-Clause|CC0-1.0|MIT|MPL-2\.0)\b").unwrap();

    for line in std::str::from_utf8(
        &Command::new("cargo")
            .arg("license")
            .assert()
            .get_output()
            .stdout,
    )
    .unwrap()
    .lines()
    {
        if line == "AGPL-3.0 (3): necessist, necessist-core, necessist-frameworks" {
            continue;
        }
        assert!(re.is_match(line), "{line:?} does not match");
    }
}

#[test]
fn markdown_link_check() {
    let tempdir = tempdir().unwrap();

    // smoelius: Pin `markdown-link-check` to version 3.10.3 until the following issue is resolved:
    // https://github.com/tcort/markdown-link-check/issues/246
    Command::new("npm")
        .args(["install", "markdown-link-check@3.10.3"])
        .current_dir(&tempdir)
        .assert()
        .success();

    // smoelius: https://github.com/rust-lang/crates.io/issues/788
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/markdown_link_check.json");

    let readme_md = Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md");

    Command::new("npx")
        .args([
            "markdown-link-check",
            "--config",
            &config.to_string_lossy(),
            &readme_md.to_string_lossy(),
        ])
        .current_dir(&tempdir)
        .assert()
        .success();
}

#[test]
fn modules() {
    let metadata = MetadataCommand::new().no_deps().exec().unwrap();

    for package in metadata.workspace_packages() {
        Command::new("cargo")
            .args([
                "modules",
                "generate",
                "graph",
                "--acyclic",
                "--layout=none",
                "--package",
                &package.name,
            ])
            .assert()
            .success();
    }
}

#[test]
fn prettier() {
    let tempdir = tempdir().unwrap();

    Command::new("npm")
        .args(["install", "prettier"])
        .current_dir(&tempdir)
        .assert()
        .success();

    Command::new("npx")
        .args([
            "prettier",
            "--check",
            &format!("{}/../**/*.json", env!("CARGO_MANIFEST_DIR")),
            &format!("{}/../**/*.md", env!("CARGO_MANIFEST_DIR")),
            &format!("{}/../**/*.yml", env!("CARGO_MANIFEST_DIR")),
            &format!("!{}/../examples/**", env!("CARGO_MANIFEST_DIR")),
            &format!("!{}/../target/**", env!("CARGO_MANIFEST_DIR")),
            &format!("!{}/../warnings.json", env!("CARGO_MANIFEST_DIR")),
        ])
        .current_dir(&tempdir)
        .assert()
        .success();
}

#[test]
fn readme_contains_usage() {
    let readme = read_to_string("README.md").unwrap();

    let stdout = assert_cmd::Command::cargo_bin("necessist")
        .unwrap()
        .arg("--help")
        .assert()
        .get_output()
        .stdout
        .clone();

    let usage = from_utf8(&stdout).unwrap();

    assert!(readme.contains(usage));
}

#[test]
fn sort() {
    Command::new("cargo")
        .args(["sort", "--check", "--grouped"])
        .assert()
        .success();
}

#[test]
fn update() {
    preserves_cleanliness(|| {
        Command::new("cargo")
            .args(["update", "--workspace"])
            .assert()
            .success();
    });
}

fn clippy_command(cargo_args: &[&str], rustc_args: &[&str]) -> Command {
    // smoelius: The next command should match what's in scripts/clippy.sh.
    let mut command = Command::new("cargo");
    command
        .args(["+nightly", "clippy", "--all-features", "--all-targets"])
        .args(cargo_args)
        .args(["--"])
        .args(rustc_args)
        .args([
            "--allow=clippy::let-underscore-untyped",
            "--allow=clippy::missing-errors-doc",
            "--allow=clippy::missing-panics-doc",
        ]);
    command
}

fn preserves_cleanliness(f: impl FnOnce()) {
    if dirty() {
        #[allow(clippy::explicit_write)]
        writeln!(stderr(), "Skipping as repository is dirty").unwrap();
        return;
    }

    f();

    assert!(!dirty());
}

fn dirty() -> bool {
    Command::new("git")
        .args(["diff", "--exit-code"])
        .assert()
        .try_success()
        .is_err()
}


#![feature(rustc_private)]

#[macro_use] extern crate error_chain;
extern crate env_logger;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate fmt_macros;
extern crate walkdir;

use std::fs;
use std::io::{self, Read};
use std::process::Command;
use std::path::{PathBuf};

pub const COMPILETEST_COMMAND: &[&str] = &[
    "{BUILDROOT}/{HOST}/stage0-tools/x86_64-unknown-linux-gnu/release/compiletest",
    "--compile-lib-path",
    "{BUILDROOT}/{HOST}/stage1/lib",
    "--run-lib-path",
    "{BUILDROOT}/{HOST}/stage1/lib/rustlib/x86_64-unknown-linux-gnu/lib",
    "--rustc-path",
    "{BUILDROOT}/{HOST}/stage1/bin/rustc",
    "--src-base",
    "{SRCROOT}/src/test/{SUITE}",
    "--build-base",
    "{TARGETDIR}",
    "--stage-id",
    "stage1-{HOST}",
    "--mode",
    "{SUITE}",
    "--target",
    "{HOST}",
    "--host",
    "{HOST}",
    "--llvm-filecheck",
    "{BUILDROOT}/{HOST}/llvm/build/bin/FileCheck",
    "--host-rustcflags",
    "-Crpath -O",
    "--target-rustcflags",
    "-Crpath -O -Lnative={BUILDROOT}/{HOST}/native/rust-test-helpers {EXTRA_FLAGS}",
    "--llvm-version",
    "4.0.1",
    "--lldb-python",
    "/dev/null",
    "--docck-python",
    "/dev/null",
    "",
    "--cc",
    "",
    "--cxx",
    "",
    "--cflags",
    "",
    "--llvm-components",
    "",
    "--llvm-cxxflags",
    "",
    "--adb-path",
    "adb",
    "--adb-test-dir",
    "/data/tmp/work",
    "--android-cross-path",
    "",
];


mod errors {
    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain! {
        links {
        }

        foreign_links {
            Io(::std::io::Error);
            Var(::std::env::VarError);
            Toml(::toml::de::Error);
            StripPrefix(::std::path::StripPrefixError);
            WalkDir(::walkdir::Error);
        }
    }
}

use errors::*;

quick_main!(run);

#[derive(Clone, Deserialize)]
pub struct Configuration {
    python: PathBuf,
    rust_buildroot: PathBuf,
    rust_srcroot: PathBuf,
    host: String,
    cachedir: PathBuf,
    datadir: PathBuf,
}

impl Configuration {
    fn target_dir(&self, mode: &str, kind: &str) -> PathBuf {
        self.cachedir.join(mode).join(kind)
    }
}

fn cvpath(p: &PathBuf) -> Result<String> {
    match p.to_str() {
        Some(s) => Ok(s.to_owned()),
        None => bail!("bad character in path `{:?}`",
                      p.to_string_lossy()),
    }
}

pub fn run_tester(cfg: &Configuration,
                  suite: &str,
                  kind: &str,
                  extra_flags: &str) -> Result<()> {
    info!("running tests in configuration {:?} {:?} {:?}", suite, kind, extra_flags);
    let args = COMPILETEST_COMMAND.iter().map(|c| {
        let parser = fmt_macros::Parser::new(c);
        parser.map(|p| {
            match p {
                fmt_macros::Piece::String(s) => Ok(s.to_owned()),
                fmt_macros::Piece::NextArgument(a) => match a.position {
                    fmt_macros::Position::ArgumentNamed("PYTHON") =>
                        cvpath(&cfg.python),
                    fmt_macros::Position::ArgumentNamed("BUILDROOT") =>
                        cvpath(&cfg.rust_buildroot),
                    fmt_macros::Position::ArgumentNamed("SRCROOT") =>
                        cvpath(&cfg.rust_srcroot),
                    fmt_macros::Position::ArgumentNamed("TARGETDIR") =>
                        cvpath(&cfg.target_dir(suite, kind)),
                    fmt_macros::Position::ArgumentNamed("HOST") =>
                        Ok(cfg.host.clone()),
                    fmt_macros::Position::ArgumentNamed("SUITE") =>
                        Ok(suite.to_owned()),
                    fmt_macros::Position::ArgumentNamed("CACHEDIR") =>
                        cvpath(&cfg.cachedir),
                    fmt_macros::Position::ArgumentNamed("EXTRA_FLAGS") =>
                        Ok(extra_flags.to_owned()),
                    fmt_macros::Position::ArgumentNamed(a) =>
                        bail!("bad var {:?}", a),
                    _ => unreachable!()
                }
            }
        }).collect()
    }).collect::<Result<Vec<String>>>()?;

    let status = Command::new(&args[0]).args(&args[1..]).env_remove("RUST_LOG").status()?;
    info!("running tests in configuration {:?} {:?} - status={:?}",
          suite, kind, status);
    Ok(())

}

#[derive(Debug, Copy, Clone)]
enum TestResult {
    Ignored,
    NoChange,
    NoOutput,
    NoExpected,
    Modified,
    ExpectedSuccess,
    ExpectedFailure,
}

impl TestResult {
    fn code(self) -> &'static str {
        use TestResult::*;
        match self {
            Ignored => "I",
            NoChange => "J",
            NoOutput => "X",
            NoExpected => "U",
            Modified => "M",
            ExpectedSuccess => "S",
            ExpectedFailure => "F",
        }
    }
}

fn read_file(path: PathBuf) -> Result<Option<String>> {
    let mut buf = String::new();
    match fs::File::open(path) {
        Ok(mut f) => {
            f.read_to_string(&mut buf)?;
            Ok(Some(buf))
        }
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(e) => Err(From::from(e))
    }
}

fn check_test(cfg: &Configuration,
              suite: &str,
              ignore: &[&str],
              filename: &str)
              -> Result<Option<TestResult>>
{
    if !filename.ends_with(".err") {
        return Ok(None);
    }
    if filename.ends_with(".mir.err") {
        info!("skipping MIR test {:?}", filename);
        return Ok(None);
    }
    if ignore.iter().any(|ign| filename == *ign) {
        info!("ignoring test {:?}", filename);
        return Ok(Some(TestResult::Ignored));
    }
    info!("comparing test {:?}", filename);

    let ast_path = cfg.target_dir(suite, "ast").join(&filename);
    let mut ast_result = String::new();
    fs::File::open(ast_path)?.read_to_string(&mut ast_result)?;

    let mir_result = cfg.target_dir(suite, "mir").join(&filename);
    let mir_result = match read_file(mir_result)? {
        Some(result) => result,
        None => return Ok(Some(TestResult::NoOutput))
    };

    if ast_result == mir_result {
        return Ok(Some(TestResult::NoChange));
    }

    let blessed_path = cfg.datadir.join("known-good").join(&filename);
    if let Some(blessed) = read_file(blessed_path)? {
        if mir_result == blessed {
            return Ok(Some(TestResult::ExpectedSuccess));
        } else {
            return Ok(Some(TestResult::Modified));
        }
    }

    let cursed_path = cfg.datadir.join("known-bad").join(&filename);
    if let Some(cursed) = read_file(cursed_path)? {
        if mir_result == cursed {
            return Ok(Some(TestResult::ExpectedFailure));
        } else {
            return Ok(Some(TestResult::Modified));
        }
    }

    Ok(Some(TestResult::NoExpected))
}

fn on_suite(cfg: &Configuration, suite: &str) -> Result<()> {
    run_tester(cfg, suite, "ast", "")?;
    run_tester(cfg, suite, "mir", "-Z borrowck=mir")?;

    let mut ignore = String::new();
    let ignore_path = cfg.datadir.join("IGNORE");
    fs::File::open(ignore_path)?.read_to_string(&mut ignore)?;
    let ignore: Vec<_> = ignore.split('\n').collect();

    let mut walkdir = walkdir::WalkDir::new(cfg.target_dir(suite, "ast")).into_iter();
    let root = walkdir.next().expect("no root in walkdir")?;
    let files : Result<Vec<_>> = walkdir.map(|w| {
        Ok::<_, Error>(w?.path().strip_prefix(root.path())?.to_owned())
    }).collect();
    let mut files = files?;
    files.sort();

    for filename in files {
        let filename = match filename.to_str() {
            Some(s) => s,
            None => continue
        };
        let test_result = check_test(cfg, suite, &ignore, &filename)?;
        if let Some(result) = test_result {
            println!("{} {}/{}", result.code(), suite, filename);
        }
    }
    Ok(())
}

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");
    let mut cfg = String::new();
    fs::File::open("nll-probe.toml")?.read_to_string(&mut cfg)?;
    let cfg: Configuration = toml::from_str(&cfg)?;
    on_suite(&cfg, "run-pass")?;
    on_suite(&cfg, "compile-fail")?;
    Ok((0))
}

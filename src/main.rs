#![feature(rustc_private)]

#[macro_use] extern crate error_chain;
extern crate env_logger;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate fmt_macros;

use std::fs;
use std::io::{self, Read};
use std::process::Command;
use std::path::PathBuf;

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
        }
    }
}

use errors::*;

quick_main!(run);

#[derive(Clone, Deserialize)]
struct Configuration {
    python: PathBuf,
    rust_buildroot: PathBuf,
    rust_srcroot: PathBuf,
    host: String,
    cachedir: PathBuf,
    blessed_dir: PathBuf,
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

fn run_tester(cfg: &Configuration,
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

    let status = Command::new(&args[0]).args(&args[1..]).status()?;
    info!("running tests in configuration {:?} {:?} - status={:?}",
          suite, kind, status);
    Ok(())

}

fn on_suite(cfg: &Configuration, suite: &str) -> Result<()> {
    run_tester(cfg, suite, "ast", "")?;
    run_tester(cfg, suite, "mir", "-Z borrowck-mir")?;

    for file in fs::read_dir(cfg.target_dir(suite, "ast"))? {
        let file = file?;
        let filename = file.file_name();
        let filename = match filename.into_string() {
            Ok(s) => s,
            Err(_) => continue
        };
        if !filename.ends_with(".err") {
            continue
        }
        info!("comparing test {:?}", filename);

        let mut ast_result = String::new();
        fs::File::open(file.path())?.read_to_string(&mut ast_result)?;

        let mut mir_result = String::new();
        let mir_path = cfg.target_dir(suite, "mir").join(&filename);
        let mut mir_file = match fs::File::open(mir_path) {
            Ok(f) => f,
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                // file was not compiled due to other errors
                println!("X {}/{}", suite, filename);
                continue
            }
            Err(e) => return Err(From::from(e)),
        };
        mir_file.read_to_string(&mut mir_result)?;

        if ast_result != mir_result {
            let blessed_path = cfg.blessed_dir.join(&filename);
            match fs::File::open(blessed_path) {
                Ok(mut f) => {
                    let mut blessed_result = String::new();
                    f.read_to_string(&mut blessed_result)?;
                    if mir_result != blessed_result {
                        println!("R {}/{}", suite, filename);
                        continue
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                    // file is not blessed
                    println!("E {}/{}", suite, filename);
                    continue
                }
                Err(e) => return Err(From::from(e)),
            }
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
    Ok((0))
}

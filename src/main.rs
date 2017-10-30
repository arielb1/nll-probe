#![feature(rustc_private)]

#[macro_use] extern crate error_chain;
extern crate env_logger;
#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate fmt_macros;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;
use std::process::Command;

pub const COMPILETEST_COMMAND: &[&str] = &[
    "{BUILDROOT}/{HOST}/stage0-tools/x86_64-unknown-linux-gnu/release/compiletest",
    "--compile-lib-path",
    "{BUILDROOT}/{HOST}/stage1/lib",
    "--run-lib-path",
    "{BUILDROOT}/{HOST}/stage1/lib/rustlib/x86_64-unknown-linux-gnu/lib",
    "--rustc-path",
    "{BUILDROOT}/{HOST}/stage1/bin/rustc",
    "--src-base",
    "{SRCROOT}/src/test/{MODE}",
    "--build-base",
    "{CACHEDIR}/{MODE}/{KIND}",
    "--stage-id",
    "stage1-{HOST}",
    "--mode",
    "{MODE}",
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
    python: String,
    rust_buildroot: String,
    rust_srcroot: String,
    host: String,
    cachedir: String,
}

fn run_tester(cfg: &Configuration,
              mode: &str,
              kind: &str,
              extra_flags: &str) -> Result<()> {
    info!("running tests in configuration {:?} {:?} {:?}", mode, kind, extra_flags);
    let args = COMPILETEST_COMMAND.iter().map(|c| {
        let parser = fmt_macros::Parser::new(c);
        parser.map(|p| {
            match p {
                fmt_macros::Piece::String(s) => Ok(s.to_owned()),
                fmt_macros::Piece::NextArgument(a) => Ok(match a.position {
                    fmt_macros::Position::ArgumentNamed("PYTHON") =>
                        cfg.python.clone(),
                    fmt_macros::Position::ArgumentNamed("BUILDROOT") =>
                        cfg.rust_buildroot.clone(),
                    fmt_macros::Position::ArgumentNamed("SRCROOT") =>
                        cfg.rust_srcroot.clone(),
                    fmt_macros::Position::ArgumentNamed("HOST") =>
                        cfg.host.clone(),
                    fmt_macros::Position::ArgumentNamed("CACHEDIR") =>
                        cfg.cachedir.clone(),
                    fmt_macros::Position::ArgumentNamed("MODE") =>
                        mode.to_owned(),
                    fmt_macros::Position::ArgumentNamed("KIND") =>
                        kind.to_owned(),
                    fmt_macros::Position::ArgumentNamed("EXTRA_FLAGS") =>
                        extra_flags.to_owned(),
                    fmt_macros::Position::ArgumentNamed(a) =>
                        bail!("bad var {:?}", a),
                    _ => unreachable!()
                })
            }
        }).collect()
    }).collect::<Result<Vec<String>>>()?;

    let status = Command::new(&args[0]).args(&args[1..]).status()?;
    info!("running tests in configuration {:?} - status={:?}", mode, status);
    Ok(())

}

fn collect_data(cfg: &Configuration, suite: &str) -> Result<()> {
    run_tester(cfg, suite, "ast", "")?;
    run_tester(cfg, suite, "mir", "-Z borrowck-mir")?;
    Ok(())
}

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");
    let mut cfg = String::new();
    File::open("nll-probe.toml")?.read_to_string(&mut cfg)?;
    let cfg: Configuration = toml::from_str(&cfg)?;
    collect_data(&cfg, "run-pass")?;
    Ok((0))
}

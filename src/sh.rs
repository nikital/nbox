use std::{
    ffi::OsStr,
    process::{Command, Stdio},
};

use eyre::{OptionExt, eyre};

pub(crate) fn run(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> eyre::Result<()> {
    let mut args = args.into_iter();
    let program = args.next().ok_or_eyre("empty args")?;
    let args: Vec<_> = args.collect();
    let exit = Command::new(program.as_ref()).args(&args).spawn()?.wait()?;
    if !exit.success() {
        let full = std::iter::once(program.as_ref())
            .chain(args.iter().map(|s| s.as_ref()))
            .collect::<Vec<_>>();
        Err(eyre!("failed to run {full:?}"))
    } else {
        Ok(())
    }
}

pub(crate) fn output(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> eyre::Result<String> {
    let mut args = args.into_iter();
    let program = args.next().ok_or_eyre("empty args")?;
    let args: Vec<_> = args.collect();
    let output = Command::new(program.as_ref())
        .args(&args)
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        let full = std::iter::once(program.as_ref())
            .chain(args.iter().map(|s| s.as_ref()))
            .collect::<Vec<_>>();
        Err(eyre!("failed to run {full:?}"))
    } else {
        Ok(String::from_utf8(output.stdout)?)
    }
}

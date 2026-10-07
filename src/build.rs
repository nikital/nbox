use std::{
    borrow::Cow,
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::Write as _,
    path::PathBuf,
    process::{Command, Stdio},
    sync::LazyLock,
};

use eyre::{Context, OptionExt as _, eyre};

static BUILT_IN: LazyLock<BTreeMap<&str, &[u8]>> = LazyLock::new(|| {
    BTreeMap::from([
        (
            "fedora-toolbox",
            include_bytes!("../images/fedora-toolbox/Containerfile").as_slice(),
        ),
        (
            "ubuntu",
            include_bytes!("../images/ubuntu/Containerfile").as_slice(),
        ),
    ])
});

#[derive(Debug)]
struct Image {
    name: String,
    source: ImageSource,
}

#[derive(Debug)]
enum ImageSource {
    BuiltIn(&'static [u8]),
    Dir(PathBuf),
}

fn get_images() -> BTreeMap<String, Image> {
    let built_in = BUILT_IN.iter().map(|(&k, v)| {
        (
            k.to_owned(),
            Image {
                name: k.to_owned(),
                source: ImageSource::BuiltIn(v),
            },
        )
    });

    let external = || -> Option<_> {
        let dir = crate::config_dir().ok()?.join("images");
        let dirs = dir.read_dir().ok()?.filter_map(|d| d.ok());

        dirs.filter_map(|d| {
            let name = d.file_name().into_string().ok()?;
            Some((
                name.clone(),
                Image {
                    name,
                    source: ImageSource::Dir(d.path()),
                },
            ))
        })
        .into()
    };

    let mut result: BTreeMap<_, _> = built_in.collect();
    // External overwrite built in
    result.extend(external().into_iter().flatten());
    result
}

pub(crate) fn build(image: Option<String>) -> eyre::Result<()> {
    let images = get_images();
    let item = crate::pick_interactive("Choose image to build", image.as_deref(), images.iter())
        .ok_or_eyre("image not found")?;
    build_recursive(&images, item)?;
    Ok(())
}

fn build_recursive(images: &BTreeMap<String, Image>, item: &Image) -> eyre::Result<()> {
    if let Some(dep) = find_dependency(images, &item.source)? {
        build_recursive(images, dep)?;
    }

    let pwd = crate::linux::getpwuid_r(unsafe { libc::getuid() })?.ok_or_eyre("no pwd entry")?;

    fn cat(prefix: &str, suffix: &OsStr) -> OsString {
        let mut result = OsStr::new(prefix).to_owned();
        result.push(suffix);
        result
    }

    let mut cmd = Command::new("podman");
    cmd.args(["build"])
        .arg("--build-arg")
        .arg(cat("USER=", &pwd.pw_name))
        .arg("--build-arg")
        .arg(cat("HOME=", &pwd.pw_dir))
        .arg("--build-arg")
        .arg("UID=".to_string() + &pwd.pw_uid.to_string())
        .args(["-t", &(item.name.clone() + ":nbox")]);

    let status = match &item.source {
        ImageSource::BuiltIn(content) => {
            let mut child = cmd.args(["-f", "-"]).stdin(Stdio::piped()).spawn()?;
            child.stdin.as_mut().expect("no stdin").write_all(content)?;
            child.wait()
        }
        ImageSource::Dir(dir) => cmd.arg(dir).stdin(Stdio::null()).status(),
    }
    .context("failed to build")?;
    if !status.success() {
        return Err(eyre!("build command failed"));
    }
    Ok(())
}

fn find_dependency<'a>(
    images: &'a BTreeMap<String, Image>,
    item: &ImageSource,
) -> eyre::Result<Option<&'a Image>> {
    let content: Cow<[u8]> = match item {
        ImageSource::BuiltIn(content) => Cow::Borrowed(content),
        ImageSource::Dir(dir) => {
            let s = fs::read(dir.join("Containerfile"))
                .with_context(|| format!("no Containerfile in {dir:?}"))?;
            Cow::Owned(s)
        }
    };
    for line in content.split(|c| *c == b'\n') {
        let Ok(line) = str::from_utf8(line) else {
            continue;
        };
        let Some(name) = line.trim().strip_prefix("FROM localhost/") else {
            continue;
        };
        let Some(name) = name.strip_suffix(":nbox") else {
            return Ok(None);
        };
        return Ok(images.get(name));
    }
    Ok(None)
}

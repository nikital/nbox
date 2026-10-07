use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::{Path, PathBuf},
};

use eyre::{OptionExt, eyre};

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Nbox(pub(crate) BTreeMap<String, Project>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Project {
    pub(crate) container_name: String,
    pub(crate) extra_mounts: BTreeMap<String, ExtraMount>,
    pub(crate) mount_cache: Option<MountCache>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct MountCache {
    pub(crate) started_at: String,
    pub(crate) pid: String,
    pub(crate) mounts: BTreeMap<String, CachedMount>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ExtraMount {
    pub(crate) src: String,
    pub(crate) ro: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CachedMount {
    pub(crate) src: String,
    pub(crate) ro: bool,
    pub(crate) kind: CachedMountType,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum CachedMountType {
    File,
    Dir { ro_relative_paths: BTreeSet<String> },
}

fn config_file() -> eyre::Result<PathBuf> {
    Ok(crate::config_dir()?.join("nbox.json"))
}

pub(crate) fn get_projects() -> eyre::Result<Vec<(PathBuf, Project)>> {
    let Ok(file) = File::open(config_file()?) else {
        return Ok(vec![]);
    };
    let nbox: Nbox = serde_json::from_reader(file)?;
    Ok(nbox
        .0
        .into_iter()
        .map(|(r, p)| (PathBuf::from(r), p))
        .collect())
}

pub(crate) fn get_project_above(path: &Path) -> eyre::Result<Option<(PathBuf, Project)>> {
    let mut projs = get_projects()?;
    for path in path.ancestors() {
        if let Some(p) = projs
            .extract_if(.., |(root, _)| Path::new(root) == path)
            .next()
        {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

pub(crate) fn insert_project(root: &Path, proj: Project) -> eyre::Result<()> {
    upsert_project(root, proj, false)
}

pub(crate) fn update_project(root: &Path, proj: Project) -> eyre::Result<()> {
    upsert_project(root, proj, true)
}

pub(crate) fn remove_project(root: &Path) -> eyre::Result<Option<Project>> {
    let file = config_file()?;
    let mut nbox: Nbox = serde_json::from_reader(File::open(&file)?)?;
    let root = root.to_str().ok_or_eyre("non-UTF-8 root path")?;
    let removed = nbox.0.remove(root);
    serde_json::to_writer_pretty(File::create(&file)?, &nbox)?;
    Ok(removed)
}

fn upsert_project(root: &Path, proj: Project, should_exist: bool) -> eyre::Result<()> {
    let file = config_file()?;
    let mut nbox: Nbox = match File::open(&file) {
        Ok(f) => serde_json::from_reader(f)?,
        Err(_) => Nbox::default(),
    };
    let root = root.to_str().ok_or_eyre("non-UTF-8 root path")?;
    let did_exist = nbox.0.insert(root.to_owned(), proj).is_some();

    if did_exist != should_exist {
        return Err(if should_exist {
            eyre!("project not found at {root}")
        } else {
            eyre!("project already exists at {root}")
        });
    }
    serde_json::to_writer_pretty(File::create(&file)?, &nbox)?;
    Ok(())
}

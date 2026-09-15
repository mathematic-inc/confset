//! Fault boundaries are exercised with real files and a durable, partially applied journal.

use super::*;

fn setup() -> Result<(tempfile::TempDir, Project, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let project = Project::acquire(&root)?;
    let path = root.join("config.json");
    Ok((directory, project, path))
}

fn state_with(project: &Project, path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    let state = State {
        version: STATE_VERSION,
        root: project.root.clone(),
        generation: "previous".into(),
        outputs: BTreeMap::from([(
            path.to_path_buf(),
            Record {
                hash: digest(bytes),
            },
        )]),
    };
    write_json(&project.state_dir.join("state.json"), &state)
}

fn prepare(project: &Project, path: &Path, bytes: &[u8]) -> Result<(Journal, NamedTempFile)> {
    let before = read_snapshot(path)?;
    let stage = stage_file(path, bytes, None)?;
    let journal = Journal {
        version: STATE_VERSION,
        root: project.root.clone(),
        generation: "interrupted".into(),
        changes: vec![Change {
            path: path.to_path_buf(),
            before,
            after: Some(snapshot_file(stage.as_file())?),
            stage: Some(stage.path().to_path_buf()),
        }],
    };
    write_json(&project.state_dir.join("journal.json"), &journal)?;
    Ok((journal, stage))
}

#[test]
fn interrupted_replacement_restores_the_previous_generation() -> Result<()> {
    let (_directory, project, path) = setup()?;
    state_with(&project, &path, b"previous")?;
    let (_, stage) = prepare(&project, &path, b"next")?;
    stage.persist(&path).map_err(|error| error.error)?;
    project.recover()?;
    assert_eq!(std::fs::read(path)?, b"previous");
    assert!(!project.state_dir.join("journal.json").exists());
    assert_eq!(project.state()?.generation, "previous");
    Ok(())
}

#[test]
fn committed_generation_is_kept_after_interrupted_journal_cleanup() -> Result<()> {
    let (_directory, project, path) = setup()?;
    state_with(&project, &path, b"previous")?;
    let (journal, stage) = prepare(&project, &path, b"next")?;
    stage.persist(&path).map_err(|error| error.error)?;
    let mut state = project.state()?;
    state.generation = journal.generation;
    state.outputs.insert(
        path.clone(),
        Record {
            hash: digest(b"next"),
        },
    );
    write_json(&project.state_dir.join("state.json"), &state)?;
    project.recover()?;
    assert_eq!(std::fs::read(path)?, b"next");
    assert!(!project.state_dir.join("journal.json").exists());
    Ok(())
}

#[test]
fn interrupted_creation_removes_only_the_file_written_by_this_generation() -> Result<()> {
    let (_directory, project, path) = setup()?;
    let (_, stage) = prepare(&project, &path, b"new")?;
    stage
        .persist_noclobber(&path)
        .map_err(|error| error.error)?;
    project.recover()?;
    assert!(!path.exists());
    Ok(())
}

#[test]
fn losing_an_exclusive_creation_race_preserves_the_foreign_file() -> Result<()> {
    let (_directory, project, path) = setup()?;
    let (_, stage) = prepare(&project, &path, b"same content")?;
    // Equal bytes alone cannot establish ownership of a newly created file.
    std::fs::write(&path, b"same content")?;
    drop(stage);
    project.recover()?;
    assert_eq!(std::fs::read(path)?, b"same content");
    assert!(!project.state_dir.join("journal.json").exists());
    Ok(())
}

#[test]
fn recovery_preserves_an_external_edit_to_a_previously_owned_file() -> Result<()> {
    let (_directory, project, path) = setup()?;
    state_with(&project, &path, b"previous")?;
    let (_, stage) = prepare(&project, &path, b"next")?;
    stage.persist(&path).map_err(|error| error.error)?;
    std::fs::write(&path, b"edited outside Confset")?;
    assert!(project.recover().is_err());
    assert_eq!(std::fs::read(path)?, b"edited outside Confset");
    assert!(project.state_dir.join("journal.json").exists());
    Ok(())
}

#[test]
fn an_active_writer_excludes_another_writer() -> Result<()> {
    let (_directory, project, _) = setup()?;
    assert!(Project::acquire(&project.root).is_err());
    Ok(())
}

#[test]
fn interrupted_obsolete_file_removal_restores_the_file() -> Result<()> {
    let (_directory, project, path) = setup()?;
    state_with(&project, &path, b"previous")?;
    let journal = Journal {
        version: STATE_VERSION,
        root: project.root.clone(),
        generation: "interrupted".into(),
        changes: vec![Change {
            path: path.clone(),
            before: read_snapshot(&path)?,
            after: None,
            stage: None,
        }],
    };
    write_json(&project.state_dir.join("journal.json"), &journal)?;
    std::fs::remove_file(&path)?;
    project.recover()?;
    assert_eq!(std::fs::read(path)?, b"previous");
    Ok(())
}

#[test]
fn a_corrupt_journal_cannot_target_internal_state() -> Result<()> {
    let (_directory, project, path) = setup()?;
    state_with(&project, &path, b"previous")?;
    let (mut journal, _stage) = prepare(&project, &path, b"next")?;
    journal.changes[0].path = project.state_dir.join("write.lock");
    write_json(&project.state_dir.join("journal.json"), &journal)?;
    assert!(project.recover().is_err());
    assert_eq!(std::fs::read(path)?, b"previous");
    assert!(project.state_dir.join("journal.json").is_file());
    Ok(())
}

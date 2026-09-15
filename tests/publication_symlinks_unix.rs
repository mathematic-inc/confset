#![warn(rust_2018_idioms)]
#![cfg(unix)] // Windows reparse-point coverage uses Windows-native CI fixtures.

mod support {
    pub(crate) mod project;
}
use std::os::unix::fs::symlink;
use support::project::{Project, Result};

#[test]
fn symlink_destinations_and_parent_directories_are_preserved() -> Result {
    let project = Project::new()?;
    project.config(r#"files { ["a"] { path = "out.json" } }"#)?;
    project.write("handwritten.json", "keep me")?;
    symlink(project.path("handwritten.json"), project.path("out.json"))?;
    project.error(&["generate"], "symlink")?;
    assert_eq!(project.read("handwritten.json")?, "keep me");
    std::fs::remove_file(project.path("out.json"))?;
    let outside = tempfile::tempdir()?;
    symlink(outside.path(), project.path("nested"))?;
    project.config(r#"files { ["a"] { path = "nested/out.json" } }"#)?;
    project.error(&["generate"], "symlink")?;
    assert!(!outside.path().join("out.json").exists());
    Ok(())
}

#[test]
fn a_replaced_owned_output_is_not_deleted_by_clean() -> Result {
    let project = Project::new()?;
    project.config(r#"files { ["a"] { path = "out.json" } }"#)?;
    project.ok(&["generate"])?;
    std::fs::remove_file(project.path("out.json"))?;
    project.write("handwritten.json", "keep me")?;
    symlink(project.path("handwritten.json"), project.path("out.json"))?;
    project.error(&["clean"], "preserved modified")?;
    assert_eq!(project.read("handwritten.json")?, "keep me");
    assert!(
        std::fs::symlink_metadata(project.path("out.json"))?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

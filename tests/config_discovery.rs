#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result, header};

#[test]
fn explicit_initialization_creates_parents_and_preserves_existing_source() -> Result {
    let project = Project::new()?;
    project.ok(&["--config", "nested/settings.pkl", "init"])?;
    let original = project.read("nested/settings.pkl")?;
    project.ok(&["--config", "nested/settings.pkl", "init"])?;
    assert_eq!(project.read("nested/settings.pkl")?, original);
    assert!(!project.path("confset.pkl").exists());
    assert!(!project.path("nested/.confset").exists());
    Ok(())
}

#[test]
fn environment_override_uses_the_configuration_directory() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.write(
        "nested/settings.pkl",
        format!(r#"{}tools {{ ["format"] = Builtins.oxfmt }}"#, header()),
    )?;
    let result = project
        .command()
        .env("CONFSET_CONFIG", "nested/settings.pkl")
        .arg("generate")
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(project.path("nested/.oxfmtrc.json").exists());
    assert!(!project.path(".oxlintrc.json").exists());
    Ok(())
}

#[test]
fn user_fallback_renders_in_the_callers_directory() -> Result {
    let project = Project::new()?;
    // dirs follows each OS's conventional user configuration location.
    let directory = if cfg!(target_os = "macos") {
        "home/Library/Application Support/confset"
    } else {
        "user-config/confset"
    };
    project.write(
        &format!("{directory}/config.pkl"),
        format!(r#"{}tools {{ ["lint"] = Builtins.oxlint }}"#, header()),
    )?;
    project.ok(&["generate"])?;
    assert!(project.path(".oxlintrc.json").exists());
    assert!(
        !project
            .path(&format!("{directory}/.oxlintrc.json"))
            .exists()
    );
    Ok(())
}

#[test]
fn clean_uses_the_project_state_when_source_was_deleted() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.ok(&["generate"])?;
    std::fs::remove_file(project.path("confset.pkl"))?;
    project.ok(&["clean"])?;
    assert!(!project.path(".oxlintrc.json").exists());
    Ok(())
}

#[test]
fn output_files_cannot_also_be_parent_directories_or_old_inputs() -> Result {
    let project = Project::new()?;
    project.config(
        r#"files {
      ["parent"] { path = "nested" }
      ["child"] { path = "nested/out.json" }
    }"#,
    )?;
    project.error(&["validate"], "both an output file and an output directory")?;
    assert!(!project.path("nested").exists());
    project.config(
        r#"files {
      ["input"] {
        path = "settings.pkl"
        format = "text"
        config = "width = 80\n"
      }
    }"#,
    )?;
    project.ok(&["generate"])?;
    project.config(
        r#"import "settings.pkl"
    tools { ["format"] = (Builtins.oxfmt) { config { printWidth = settings.width } } }"#,
    )?;
    project.error(&["generate"], "now a configuration input")?;
    assert_eq!(project.read("settings.pkl")?, "width = 80\n");
    Ok(())
}

#[test]
fn a_nested_project_selects_its_own_nearest_entrypoint() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.write(
        "packages/web/confset.pkl",
        format!("{}{}", header(), r#"tools { ["format"] = Builtins.oxfmt }"#),
    )?;
    project.write("packages/web/src/keep", "")?;
    let result = project
        .command()
        .current_dir(project.path("packages/web/src"))
        .arg("generate")
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(project.path("packages/web/.oxfmtrc.json").is_file());
    assert!(!project.path(".oxlintrc.json").exists());
    assert!(!project.path(".confset").exists());
    Ok(())
}

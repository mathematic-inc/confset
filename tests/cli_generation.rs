#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

const OXLINT: &str = r#"
tools {
  ["lint"] = (Builtins.oxlint) {
    config { categories { correctness = "deny" } }
  }
}
"#;

#[test]
fn initialization_does_not_generate_or_replace_source() -> Result {
    let project = Project::new()?;
    project.ok(&["init"])?;
    assert!(project.path("confset.pkl").is_file());
    assert!(!project.path(".oxlintrc.json").exists());
    project.config(OXLINT)?;
    let before = project.read("confset.pkl")?;
    project.ok(&["init", "--gitignore"])?;
    assert_eq!(project.read("confset.pkl")?, before);
    assert!(!project.path(".oxlintrc.json").exists());
    assert!(project.read(".gitignore")?.contains("/.oxlintrc.json"));
    project.ok(&["generate"])?;
    let output: serde_json::Value = serde_json::from_str(&project.read(".oxlintrc.json")?)?;
    assert_eq!(output["categories"]["correctness"], "deny");
    Ok(())
}

#[test]
fn validation_and_listing_do_not_materialize_outputs() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.ok(&["validate"])?;
    let listing = project.ok(&["list"])?;
    assert!(listing.contains("missing\tjson"));
    assert!(!project.path(".confset").exists());
    assert!(!project.path(".oxlintrc.json").exists());
    Ok(())
}

#[test]
fn repeated_generation_preserves_timestamps() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.ok(&["generate"])?;
    let timestamp = filetime::FileTime::from_unix_time(1_700_000_000, 1_000_000);
    filetime::set_file_mtime(project.path(".oxlintrc.json"), timestamp)?;
    project.ok(&["generate"])?;
    let after = filetime::FileTime::from_last_modification_time(&std::fs::metadata(
        project.path(".oxlintrc.json"),
    )?);
    assert_eq!(timestamp, after);
    Ok(())
}

#[test]
fn unowned_and_externally_edited_files_are_preserved() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.write(".oxlintrc.json", "{\"handwritten\":true}")?;
    project.error(&["generate"], "unowned")?;
    assert_eq!(project.read(".oxlintrc.json")?, "{\"handwritten\":true}");
    std::fs::remove_file(project.path(".oxlintrc.json"))?;
    project.ok(&["generate"])?;
    project.write(".oxlintrc.json", "{\"edited\":true}")?;
    project.error(&["generate"], "modified outside Confset")?;
    project.error(&["clean"], "preserved modified")?;
    assert_eq!(project.read(".oxlintrc.json")?, "{\"edited\":true}");
    Ok(())
}

#[test]
fn format_change_removes_the_owned_alternative() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["format"] = Builtins.prettier }"#)?;
    project.ok(&["generate"])?;
    project.config(r#"tools { ["format"] = (Builtins.prettier) { format = "yaml" } }"#)?;
    project.ok(&["generate"])?;
    assert!(!project.path(".prettierrc.json").exists());
    assert!(project.path(".prettierrc.yaml").is_file());
    project.ok(&["validate"])?;
    Ok(())
}

#[test]
fn unowned_alternatives_and_invalid_native_filenames_fail() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.write("oxlint.config.ts", "export default {};")?;
    project.error(&["generate"], "conflict")?;
    assert!(!project.path(".oxlintrc.json").exists());
    std::fs::remove_file(project.path("oxlint.config.ts"))?;
    project.config(&OXLINT.replace("config {", "path = \"elsewhere.json\"\nconfig {"))?;
    project.error(&["generate"], "requires native filename")?;
    project.config(&OXLINT.replace("config {", "format = \"javascript\"\nconfig {"))?;
    project.error(&["validate"], "does not support generating")?;
    Ok(())
}

#[test]
fn removed_outputs_are_deleted_and_clean_works_with_broken_pkl() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.ok(&["generate"])?;
    project.config("tools {}")?;
    project.ok(&["generate"])?;
    assert!(!project.path(".oxlintrc.json").exists());
    project.config(OXLINT)?;
    project.ok(&["generate"])?;
    project.write("confset.pkl", "invalid ???")?;
    project.ok(&["clean"])?;
    assert!(!project.path(".oxlintrc.json").exists());
    assert_eq!(project.read("confset.pkl")?, "invalid ???");
    Ok(())
}

#[test]
fn compilation_failure_never_partially_updates_outputs() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.ok(&["generate"])?;
    let before = project.read(".oxlintrc.json")?;
    project.config(
        r#"
tools {
  ["lint"] = (Builtins.oxlint) {
    config { categories { correctness = "warn" } }
  }
  ["bad"] = (Builtins.rustfmt) {
    config { max_width = null }
  }
}
"#,
    )?;
    project.error(&["generate"], "TOML")?;
    assert_eq!(project.read(".oxlintrc.json")?, before);
    assert!(!project.path("rustfmt.toml").exists());
    Ok(())
}

#[test]
fn nested_output_directories_and_absolute_destinations_work() -> Result {
    let project = Project::new()?;
    let elsewhere = tempfile::tempdir()?;
    let destination = serde_json::to_string(
        &elsewhere
            .path()
            .canonicalize()?
            .join("config.yaml")
            .to_string_lossy(),
    )?;
    project.config(&format!(
        r#"
tools {{
  ["nested"] = (Builtins.oxlint) {{
    directory = "packages/web"
    config {{ rules {{ eqeqeq = "deny" }} }}
  }}
}}
files {{
  ["elsewhere"] {{
    path = {destination}
    format = "yaml"
    config {{ strict = true }}
  }}
}}
"#
    ))?;
    project.ok(&["generate"])?;
    assert!(project.path("packages/web/.oxlintrc.json").exists());
    assert!(elsewhere.path().join("config.yaml").exists());
    project.ok(&["clean"])?;
    assert!(!elsewhere.path().join("config.yaml").exists());
    Ok(())
}

#[test]
fn duplicate_and_input_destinations_fail_before_writing() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
files {
  ["a"] { path = "same.json" }
  ["b"] { path = "same.json" }
}
"#,
    )?;
    project.error(&["generate"], "both write")?;
    assert!(!project.path("same.json").exists());
    project.config(r#"files { ["source"] { path = "confset.pkl" } }"#)?;
    project.error(&["generate"], "configuration input")?;
    project.config(r#"files { ["state"] { path = ".confset/state.json" } }"#)?;
    project.error(&["generate"], "internal state")?;
    Ok(())
}

#[test]
fn ancestor_discovery_and_explicit_override_are_distinct() -> Result {
    let project = Project::new()?;
    project.config(OXLINT)?;
    project.write("sub/directory/keep", "")?;
    let output = project
        .command()
        .current_dir(project.path("sub/directory"))
        .arg("generate")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.path(".oxlintrc.json").exists());
    project.write(
        "other.pkl",
        format!("{}tools {{}}", support::project::header()),
    )?;
    project.ok(&["--config", "other.pkl", "list"])?;
    project.error(&["--config", "missing.pkl", "validate"], "does not exist")?;
    let output = project
        .command()
        .env("CONFSET_CONFIG", "missing.pkl")
        .args(["--config", "confset.pkl", "validate"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn a_conflicting_package_manifest_is_reported() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["prettier"] = Builtins.prettier }"#)?;
    project.write("package.json", r#"{"prettier":{"semi":false}}"#)?;
    project.error(&["generate"], "competes")?;
    Ok(())
}

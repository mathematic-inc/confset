#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

#[test]
fn public_examples_evaluate_offline_with_empty_package_caches() -> Result {
    for source in [
        include_str!("../examples/web/confset.pkl"),
        include_str!("../examples/polyglot/confset.pkl"),
        include_str!("../examples/custom/confset.pkl"),
    ] {
        let project = Project::new()?;
        project.write("confset.pkl", source)?;
        project.ok(&["validate"])?;
        project.ok(&["generate"])?;
        project.ok(&["clean"])?;
        assert_eq!(project.read("confset.pkl")?, source);
    }
    Ok(())
}

#[test]
fn all_embedded_producers_support_their_default_formats() -> Result {
    let project = Project::new()?;
    let catalog: serde_json::Value = serde_json::from_str(include_str!("../pkl/catalog.json"))?;
    let mut source = String::from("tools {\n");
    for entry in catalog.as_array().ok_or("catalog must be an array")? {
        let symbol = entry["symbol"].as_str().ok_or("missing symbol")?;
        source.push_str(&format!(
            "[\"{symbol}\"] = (Builtins.{symbol}) {{ directory = \"{symbol}\" }}\n"
        ));
    }
    source.push_str("}\n");
    project.config(&source)?;
    project.ok(&["generate"])?;
    for entry in catalog.as_array().ok_or("catalog must be an array")? {
        let symbol = entry["symbol"].as_str().ok_or("missing symbol")?;
        let format = entry["default"].as_str().ok_or("missing default")?;
        let filename = entry["formats"][format]
            .as_str()
            .ok_or("missing filename")?;
        assert!(project.path(&format!("{symbol}/{filename}")).is_file());
    }
    Ok(())
}

#[test]
fn nested_modules_share_values_and_keep_destinations_relative_to_the_entrypoint() -> Result {
    let project = Project::new()?;
    for (path, contents) in [
        (
            "confset.pkl",
            include_str!("../examples/nested/confset.pkl"),
        ),
        (
            "config/web.pkl",
            include_str!("../examples/nested/config/web.pkl"),
        ),
        (
            "config/python.pkl",
            include_str!("../examples/nested/config/python.pkl"),
        ),
        (
            "config/shared/settings.pkl",
            include_str!("../examples/nested/config/shared/settings.pkl"),
        ),
    ] {
        project.write(path, contents)?;
    }
    project.ok(&["generate"])?;
    assert!(project.read("packages/web/.oxfmtrc.json")?.contains("100"));
    assert!(project.read("packages/python/ruff.toml")?.contains("100"));
    assert!(!project.path("config/packages").exists());
    Ok(())
}

#[test]
fn builtin_defaults_survive_native_property_amendments() -> Result {
    let project = Project::new()?;
    project.config(
        r#"tools {
      ["yaml"] = (Builtins.ryl) {
        config { rules { ["trailing-spaces"] = "enable" } }
      }
      ["protobuf"] = (Builtins.buf) {
        config { version = "v1" }
      }
    }"#,
    )?;
    project.ok(&["generate"])?;
    let rules: toml::Value = toml::from_str(&project.read(".ryl.toml")?)?;
    assert_eq!(rules["rules"]["key-duplicates"].as_str(), Some("enable"));
    assert_eq!(rules["rules"]["trailing-spaces"].as_str(), Some("enable"));
    let protobuf: serde_json::Value = serde_yaml_ng::from_str(&project.read("buf.yaml")?)?;
    assert_eq!(protobuf["version"], "v1");
    Ok(())
}

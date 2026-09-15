use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=pkl");
    let catalog: serde_json::Value = serde_json::from_slice(&std::fs::read("pkl/catalog.json")?)?;
    let mut builtins = String::from(
        "/// Native configuration producers bundled with Confset.\nmodule confset.Builtins\nimport \"Config.pkl\"\n",
    );
    for entry in catalog.as_array().ok_or("catalog must be an array")? {
        let symbol = entry["symbol"]
            .as_str()
            .ok_or("catalog entry needs symbol")?;
        let id = entry["id"].as_str().ok_or("catalog entry needs id")?;
        let format = entry["default"]
            .as_str()
            .ok_or("catalog entry needs default")?;
        builtins.push_str(&format!(
            "\n{symbol} = new Config.Tool {{\n  builtin = {id:?}\n  format = {format:?}\n"
        ));
        if let Some(config) = entry.get("config") {
            builtins.push_str(&format!("  config = {}\n", pkl_value(config, 2)));
        }
        builtins.push_str("}\n");
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    std::fs::write(out.join("Builtins.pkl"), &builtins)?;
    let archive = std::fs::File::create(out.join("confset-package.zip"))?;
    let mut zip = zip::ZipWriter::new(archive);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .system(zip::System::Unix)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file("Builtins.pkl", options)?;
    zip.write_all(builtins.as_bytes())?;
    for name in ["Config.pkl", "Renderers.pkl"] {
        zip.start_file(name, options)?;
        // Package identity must not depend on the checkout's line endings.
        let source = std::fs::read_to_string(PathBuf::from("pkl").join(name))?;
        zip.write_all(source.replace("\r\n", "\n").as_bytes())?;
    }
    zip.finish()?;
    let version = std::env::var("CARGO_PKG_VERSION")?;
    let name = format!("confset@{version}");
    let uri =
        format!("package://github.com/mathematic-inc/confset/releases/download/v{version}/{name}");
    let bytes = std::fs::read(out.join("confset-package.zip"))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let metadata = serde_json::json!({
        "name": "confset",
        "packageUri": uri,
        "version": version,
        "packageZipUrl": format!("https://github.com/mathematic-inc/confset/releases/download/v{version}/{name}.zip"),
        "packageZipChecksums": { "sha256": hash },
        "dependencies": {},
        "license": "MIT",
        "sourceCode": "https://github.com/mathematic-inc/confset",
        "issueTracker": "https://github.com/mathematic-inc/confset/issues"
    });
    let metadata = serde_json::to_vec_pretty(&metadata)?;
    std::fs::write(out.join(&name), &metadata)?;
    let metadata_hash: String = Sha256::digest(&metadata)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    std::fs::write(out.join(format!("{name}.sha256")), metadata_hash)?;
    std::fs::write(out.join(format!("{name}.zip")), bytes)?;
    Ok(())
}

fn pkl_value(value: &serde_json::Value, indent: usize) -> String {
    match value {
        serde_json::Value::Object(fields) => {
            let fields = fields
                .iter()
                .map(|(key, value)| {
                    // Pkl properties and entries are distinct. Ordinary native
                    // names must support property amendments such as config.rules.
                    let identifier = key
                        .starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
                        && key
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                    let key = if identifier {
                        format!("`{key}`")
                    } else {
                        format!(
                            "[{}]",
                            serde_json::to_string(key).expect("string serialization")
                        )
                    };
                    format!(
                        "{}{key} = {}",
                        " ".repeat(indent + 2),
                        pkl_value(value, indent + 2)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("new Dynamic {{\n{fields}\n{}}}", " ".repeat(indent))
        }
        serde_json::Value::Array(values) => {
            format!(
                "List({})",
                values
                    .iter()
                    .map(|value| pkl_value(value, indent))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        scalar => scalar.to_string(),
    }
}

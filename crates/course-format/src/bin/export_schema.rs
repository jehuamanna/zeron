//! Export the generated course-manifest JSON Schema.

use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas/course-manifest-v2.schema.json"),
        PathBuf::from,
    );
    let json = serde_json::to_string_pretty(&course_format::manifest_schema())? + "\n";
    fs::write(&output, json)?;
    println!("wrote {}", output.display());
    Ok(())
}

// Copyright (c) Facebook, Inc. and its affiliates
// SPDX-License-Identifier: MIT OR Apache-2.0

use serde_generate::{cpp, java, python3, rust, test_utils, CodegenConfig, SourceInstaller};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_that_python_code_parses() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("test.py");
    let mut source = File::create(&source_path).unwrap();
    python3::output(&mut source, &registry).unwrap();

    let python_path = format!(
        "{}:runtime/python",
        std::env::var("PYTHONPATH").unwrap_or_default()
    );
    let status = Command::new("python3")
        .arg(source_path)
        .env("PYTHONPATH", python_path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_that_installed_python_code_passes_pyre_check() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();

    let config = CodegenConfig::new("test".to_string());
    let installer = python3::Installer::new(dir.path().join("src"), /* serde package */ None);
    installer.install_module(&config, &registry).unwrap();
    installer.install_serde_runtime().unwrap();
    installer.install_bincode_runtime().unwrap();
    installer.install_lcs_runtime().unwrap();

    // Sadly, we have to manage numpy typeshed manually for now until the next release of numpy.
    let status = Command::new("cp")
        .arg("-r")
        .arg("runtime/python/typeshed")
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success());

    let mut pyre_config = File::create(dir.path().join(".pyre_configuration")).unwrap();
    writeln!(
        &mut pyre_config,
        r#"{{
  "source_directories": [
    "src"
  ],
  "search_path": [
    "typeshed"
  ]
}}"#,
    )
    .unwrap();

    let status = Command::new("pyre")
        .current_dir(dir.path())
        .arg("check")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_that_cpp_code_compiles() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    let header_path = dir.path().join("test.hpp");
    let mut header = File::create(&header_path).unwrap();
    let inner = CodegenConfig::new("testing".to_string());
    let config = cpp::CppCodegenConfig::new(&inner);
    config.output(&mut header, &registry).unwrap();

    let source_path = dir.path().join("test.cpp");
    let mut source = File::create(&source_path).unwrap();
    writeln!(
        source,
        r#"
#include "bincode.hpp"
#include "test.hpp"
"#
    )
    .unwrap();

    let status = Command::new("clang++")
        .arg("--std=c++17")
        .arg("-c")
        .arg("-o")
        .arg(dir.path().join("test.o"))
        .arg("-I")
        .arg("runtime/cpp")
        .arg(source_path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_that_cpp_code_links() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    let header_path = dir.path().join("test.hpp");
    let mut header = File::create(&header_path).unwrap();

    let inner = CodegenConfig::new("testing".to_string());
    let config = cpp::CppCodegenConfig::new(&inner);
    config.output(&mut header, &registry).unwrap();

    let source_path = dir.path().join("lib.cpp");
    let mut source = File::create(&source_path).unwrap();
    writeln!(
        source,
        r#"
#include "lcs.hpp"
#include "test.hpp"

using namespace serde;
using namespace testing;

std::vector<uint8_t> serialize_data(SerdeData data) {{
    auto serializer = LcsSerializer();
    Serializable<SerdeData>::serialize(data, serializer);
    return std::move(serializer).bytes();
}}

SerdeData deserialize_data(const std::vector<uint8_t> &input) {{
    auto deserializer = LcsDeserializer(input);
    return Deserializable<SerdeData>::deserialize(deserializer);
}}
"#
    )
    .unwrap();

    let source_path = dir.path().join("main.cpp");
    let mut source = File::create(&source_path).unwrap();
    writeln!(
        source,
        r#"
#include "test.hpp"

using namespace serde;
using namespace testing;

extern std::vector<uint8_t> serialize_data(SerdeData data);

extern SerdeData deserialize_data(const std::vector<uint8_t> &bytes);

bool test(const std::vector<uint8_t>& input) {{
    auto output = serialize_data(deserialize_data(input));
    return input == output;
}}

int main() {{
    // dummy
    return test({{}});
}}
"#
    )
    .unwrap();

    let status = Command::new("clang++")
        .arg("--std=c++17")
        .arg("-I")
        .arg("runtime/cpp")
        .arg("-o")
        .arg(dir.path().join("main"))
        .arg(dir.path().join("lib.cpp"))
        .arg(dir.path().join("main.cpp"))
        .status()
        .unwrap();
    assert!(status.success());
}

// Quick test using rustc directly.
#[test]
fn test_that_rust_code_compiles() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("test.rs");
    let mut source = File::create(&source_path).unwrap();

    let inner = CodegenConfig::new("testing".to_string()).serialization(false);
    let config = rust::RustCodegenConfig::new(&inner);
    config.output(&mut source, &registry).unwrap();

    let status = Command::new("rustc")
        .current_dir(dir.path())
        .arg("--crate-type")
        .arg("lib")
        .arg("--edition")
        .arg("2018")
        .arg(source_path)
        .status()
        .unwrap();
    assert!(status.success());
}

// Quick test using rustc directly.
#[test]
fn test_that_rust_code_with_comments_compiles() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("test.rs");
    let mut source = File::create(&source_path).unwrap();
    let comments = vec![(vec!["SerdeData".to_string()], "Some\ncomments".to_string())]
        .into_iter()
        .collect();
    let definitions = vec![
        ("foo".to_string(), vec!["Map".to_string()]),
        (String::new(), vec!["Bytes".into()]),
    ]
    .into_iter()
    .collect();

    let inner = CodegenConfig::new("testing".to_string())
        .comments(comments)
        .external_definitions(definitions)
        .serialization(false);
    let config = rust::RustCodegenConfig::new(&inner);
    config.output(&mut source, &registry).unwrap();

    // Comment was correctly generated.
    let content = std::fs::read_to_string(&source_path).unwrap();
    assert!(content.contains("/// Some\n/// comments\n"));

    let output = Command::new("rustc")
        .current_dir(dir.path())
        .arg("--crate-type")
        .arg("lib")
        .arg("--edition")
        .arg("2018")
        .arg(source_path.clone())
        .output()
        .unwrap();
    assert!(!output.status.success());

    // Externally defined names "Map" and "Bytes" have caused the usual imports to be
    // replaced by `use foo::Map` (and nothing, respectively), so we must add the definitions.
    writeln!(
        &mut source,
        r#"
type Bytes = Vec<u8>;

mod foo {{
    pub type Map<K, V> = std::collections::BTreeMap<K, V>;
}}
"#
    )
    .unwrap();

    let status = Command::new("rustc")
        .current_dir(dir.path())
        .arg("--crate-type")
        .arg("lib")
        .arg("--edition")
        .arg("2018")
        .arg(source_path)
        .status()
        .unwrap();
    assert!(status.success());
}

// Full test using cargo. This may take a while.
#[test]
fn test_that_rust_code_compiles_with_derive_macros() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "testing"
version = "0.1.0"
edition = "2018"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_bytes = "0.11"

[workspace]
"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();

    let inner = CodegenConfig::new("testing".to_string());
    let config = rust::RustCodegenConfig::new(&inner);

    let source_path = dir.path().join("src/lib.rs");
    let mut source = File::create(&source_path).unwrap();
    config.output(&mut source, &registry).unwrap();

    // Use a stable `target` dir to avoid downloading and recompiling crates everytime.
    let target_dir = std::env::current_dir().unwrap().join("../target");
    let status = Command::new("cargo")
        .current_dir(dir.path())
        .arg("build")
        .arg("--target-dir")
        .arg(target_dir)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_that_java_code_compiles() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();

    let inner = CodegenConfig::new("test".to_string());
    let config = java::JavaCodegenConfig::new(&inner);
    config
        .write_source_files(dir.path().to_path_buf(), &registry)
        .unwrap();

    let paths = std::iter::empty()
        .chain(std::fs::read_dir("runtime/java/com/facebook/serde").unwrap())
        .chain(std::fs::read_dir("runtime/java/com/facebook/bincode").unwrap())
        .chain(std::fs::read_dir(dir.path().join("test")).unwrap())
        .map(|e| e.unwrap().path());
    let status = Command::new("javac")
        .arg("-Xlint")
        .arg("-d")
        .arg(dir.path())
        .args(paths)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn test_that_java_code_with_comments_compiles() {
    let registry = test_utils::get_registry().unwrap();
    let dir = tempdir().unwrap();

    let inner = CodegenConfig::new("test".to_string()).comments(
        vec![(
            vec!["test".to_string(), "SerdeData".to_string()],
            "Some\ncomments".to_string(),
        )]
        .into_iter()
        .collect(),
    );
    let config = java::JavaCodegenConfig::new(&inner);
    config
        .write_source_files(dir.path().to_path_buf(), &registry)
        .unwrap();

    // Comment was correctly generated.
    let content = std::fs::read_to_string(dir.path().join("test/SerdeData.java")).unwrap();
    assert!(content.contains(
        r#"
/**
 * Some
 * comments
 */
"#
    ));

    // Files compile.
    let paths = std::iter::empty()
        .chain(std::fs::read_dir("runtime/java/com/facebook/serde").unwrap())
        .chain(std::fs::read_dir("runtime/java/com/facebook/bincode").unwrap())
        .chain(std::fs::read_dir(dir.path().join("test")).unwrap())
        .map(|e| e.unwrap().path());
    let status = Command::new("javac")
        .arg("-Xlint")
        .arg("-d")
        .arg(dir.path())
        .args(paths)
        .status()
        .unwrap();
    assert!(status.success());

    // (wrongly) Declare TraitHelpers as external.
    let mut definitions = BTreeMap::new();
    definitions.insert("foo".to_string(), vec!["TraitHelpers".to_string()]);
    let inner = CodegenConfig::new("test".to_string()).external_definitions(definitions);
    let config = java::JavaCodegenConfig::new(&inner);

    config
        .write_source_files(dir.path().to_path_buf(), &registry)
        .unwrap();

    // References were updated.
    let content = std::fs::read_to_string(dir.path().join("test/SerdeData.java")).unwrap();
    assert!(content.contains("foo.TraitHelpers."));
}

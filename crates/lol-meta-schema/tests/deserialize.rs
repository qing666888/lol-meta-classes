use lol_meta_schema::MetaDump;
use std::fs::File;
use std::path::Path;

#[test]
fn test_deserialize_real_dump() {
    let dump_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("dumps/14.24.6442327.json");

    if !dump_path.exists() {
        eprintln!("Skipping test: dump file not found at {:?}", dump_path);
        return;
    }

    let file = File::open(&dump_path).unwrap();
    let dump: MetaDump = serde_json::from_reader(file).unwrap();

    assert!(!dump.version.is_empty());
    assert!(!dump.classes.is_empty());

    println!("Version: {}", dump.version);
    println!("Classes: {}", dump.classes.len());

    // Verify we can access nested structures
    for (hash, class) in dump.classes.iter().take(5) {
        println!("Class {}: size={}, props={}", hash, class.size, class.properties.len());
    }
}

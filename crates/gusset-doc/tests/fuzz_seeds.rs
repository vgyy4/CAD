//! The extra `doc_text` fuzz seeds (`fuzz/seeds-src/doc_text`, copied into the seed corpus by
//! `fuzz/examples/make_seeds.rs` next to the grammar.md examples) are valid documents whose
//! printed text is a fixed point, and together they use every v0.2 statement and face key
//! (review v0.2 L10). A seed that stops parsing would only start the fuzzer from garbage.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use gusset_doc::Document;
use std::path::Path;

#[test]
fn seeds_are_valid_fixed_points_and_cover_v02() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds-src/doc_text");
    let mut all = String::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    assert!(files.len() >= 3, "{files:?}");
    for p in &files {
        let text = std::fs::read_to_string(p).unwrap();
        let doc = Document::from_text(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let printed = doc.to_text();
        assert_eq!(Document::from_text(&printed).unwrap().to_text(), printed, "{}", p.display());
        all.push_str(&text);
    }
    // With the grammar examples (seeded too), every v0.2 statement and face key appears.
    all.push_str(gusset_doc::TEXT_GRAMMAR);
    for want in [
        "\nshell ",
        "\nmirror ",
        "\npattern_linear ",
        "\npattern_circular ",
        "\nplane ",
        "axis: edge(",
        "\nmove ",
        "copy: true",
        "\ncombine ",
        "\nsweep ",
        "twist: ",
        "\nloft ",
        "\nhole ",
        "type: countersink",
        "type: counterbore",
        "type: tapped",
        "at: [[",
        " spline(",
        ") closed",
        " pattern_linear(of: [",
        " pattern_circular(of: [",
        "/inner(",
        "/mirror(",
        "/instance(",
        "/wall(c8_2)",
        ", gear: ",
    ] {
        assert!(all.contains(want), "no seed uses {want:?}");
    }
}

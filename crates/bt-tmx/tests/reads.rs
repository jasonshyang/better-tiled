//! Every Tiled file of the corpora reads.

#![cfg(test)]

mod corpus;

use std::collections::BTreeMap;

use bt_tmx::{Loader, WarningKind, load_project};
use corpus::{DIRS, Kind, corpus};

#[test]
fn every_corpus_file_reads() {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    let mut count = 0;
    for file in corpus(&DIRS) {
        count += 1;
        let mut loader = Loader::default();
        let warnings = match file.kind {
            Kind::Map => loader.load_map(&file.path).map(|l| l.warnings),
            Kind::Tileset => loader.load_tileset(&file.path).map(|l| l.warnings),
            Kind::Template => loader.load_template(&file.path).map(|l| l.warnings),
            Kind::World => loader.load_world(&file.path).map(|l| l.warnings),
            Kind::Project => load_project(&file.path).map(|l| l.warnings),
        };
        match warnings {
            Ok(warnings) => {
                for warning in warnings {
                    *kinds.entry(format!("{:?}", warning.kind)).or_default() += 1;
                    if warning.kind != WarningKind::MissingFile {
                        println!("{}: {}", file.shown, warning);
                    }
                }
            }
            Err(error) => failures.push(format!("{}: {error}", file.shown)),
        }
    }
    println!("{count} files read; warnings by kind: {kinds:?}");
    assert!(
        failures.is_empty(),
        "files that do not read:\n{}",
        failures.join("\n")
    );
}

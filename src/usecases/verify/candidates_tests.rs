use std::collections::BTreeSet;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::super::content_diff::{basename, split_stem_ext};
use super::CandidateIndex;
use crate::adapters::backend::fake::FakeBackend;
use crate::entities::file_index::Index;
use crate::entities::file_info::Info;
use crate::entities::uri::Location;

const NAMES: &[&str] = &[
    "photo_2021.jpg",
    "photo_2021_2.jpg",
    "photo_2021_12.JPG",
    "photo_2021_.jpg",
    "photo_2021_abc.jpg",
    "photo_2021_1_2.jpg",
    "photo_2021_1.jpg",
    "photo.jpg",
    "photo_7.jpg",
    "photo__1.jpg",
    "a.b_1.jpg",
    "a.b.jpg",
    "noext",
    "noext_3",
    "x.y_1",
    "x.y",
    "IMG_0001.HEIC",
    "img_0001_1.heic",
    "unrelated.jpg",
];

fn stem_digit_variant(name_stem: &str, base_stem: &str) -> bool {
    let Some(digits) = name_stem
        .strip_prefix(base_stem)
        .and_then(|rest| rest.strip_prefix('_'))
    else {
        return false;
    };
    if digits.is_empty() {
        return false;
    }
    digits.chars().all(|c| c.is_ascii_digit())
}

fn linear_oracle(out: &Index, base: &str) -> BTreeSet<Utf8PathBuf> {
    let base_lower = base.to_ascii_lowercase();
    let (stem, ext) = split_stem_ext(&base_lower);
    out.iter()
        .filter(|entry| {
            let name_lower = basename(entry.key().as_str()).to_ascii_lowercase();
            if name_lower == base_lower {
                return true;
            }
            let (nstem, next) = split_stem_ext(&name_lower);
            if next == ext {
                stem_digit_variant(&nstem, &stem)
            } else {
                false
            }
        })
        .map(|entry| entry.key().clone())
        .collect()
}

fn output_index() -> Index {
    let be = Arc::new(FakeBackend::new("local"));
    let idx = Index::new();
    for (i, name) in NAMES.iter().enumerate() {
        let loc = Location::Local(Utf8PathBuf::from(format!("/out/d{i}/{name}")));
        be.add_file(loc.clone(), vec![u8::try_from(i).unwrap(); 8]);
        idx.add(Info::open(&loc, be.clone()).unwrap());
    }
    idx
}

#[test]
fn oracle_detects_numeric_suffix_only() {
    assert!(stem_digit_variant("photo_2021_12", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021_abc", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021", "photo_2021"));
    assert!(!stem_digit_variant("photo_2021_", "photo_2021"));
    assert!(!stem_digit_variant("other_2021_12", "photo_2021"));
}

#[test]
fn find_matches_linear_oracle_for_every_name() {
    let out = output_index();
    let cands = CandidateIndex::build(&out);
    for base in NAMES
        .iter()
        .chain(&["PHOTO_2021.JPG", "missing.jpg", "a.jpg"])
    {
        let got: BTreeSet<Utf8PathBuf> = cands.find(base).into_iter().collect();
        assert_eq!(got, linear_oracle(&out, base), "base={base}");
    }
}

#[test]
fn find_returns_empty_when_nothing_matches() {
    let cands = CandidateIndex::build(&output_index());
    assert_eq!(cands.find("zzz.png"), Vec::<Utf8PathBuf>::new());
}

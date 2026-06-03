#[test]
fn hash() {
    const XXH3SUM: u64 = 0x59d5_aae4_ebec_cc24;

    let contents = "# tidymedia\nTidy Media\n";

    let hash = wyhash::wyhash(contents.as_bytes(), 0);
    assert_eq!(14_067_286_713_656_012_073, hash);

    let hash = xxhash_rust::xxh3::xxh3_64(contents.as_bytes());

    assert_eq!(XXH3SUM, hash);
}

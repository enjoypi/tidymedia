#[test]
fn hash() {
    let contents = "# tidymedia\nTidy Media\n";

    let hash = wyhash::wyhash(contents.as_bytes(), 0);
    assert_eq!(14067286713656012073, hash);

    const XXH3SUM: u64 = 0x59d5aae4ebeccc24;
    let hash = xxhash_rust::xxh3::xxh3_64(contents.as_bytes());

    assert_eq!(XXH3SUM, hash);
}

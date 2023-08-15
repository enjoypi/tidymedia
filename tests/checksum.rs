#[cfg(test)]
mod tests {

    #[test]
    fn wyhash() {
        let contents = "# tidymedia\nTidy Media\n";

        use wyhash::wyhash;

        let checksum = wyhash(contents.as_bytes(), 0);
        assert_eq!(14067286713656012073, checksum);
    }
}

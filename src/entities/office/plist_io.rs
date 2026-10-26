pub(super) fn read_value(buf: &[u8]) -> Option<plist::Value> {
    plist::Value::from_reader(std::io::Cursor::new(buf)).ok()
}

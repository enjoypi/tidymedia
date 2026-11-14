use super::*;

struct Fixed {
    data: Vec<u8>,
    unchanged: bool,
}

impl MappedBytes for Fixed {
    fn bytes(&self) -> &[u8] {
        &self.data
    }

    fn unchanged(&self) -> bool {
        self.unchanged
    }
}

#[test]
fn heap_bytes_deref_and_never_change() {
    let bytes = FileBytes::Heap(b"abc".to_vec());
    assert_eq!(&*bytes, b"abc");
    assert!(bytes.unchanged());
}

#[test]
fn mapped_bytes_deref_and_report_change() {
    for unchanged in [true, false] {
        let bytes = FileBytes::Mapped(MappedFile::new(Box::new(Fixed {
            data: b"xyz".to_vec(),
            unchanged,
        })));
        assert_eq!(&*bytes, b"xyz");
        assert_eq!(bytes.unchanged(), unchanged);
    }
}

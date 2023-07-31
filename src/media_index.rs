use crate::file_checksum::Checksum;
use crate::FileChecksum;
use std::collections::HashMap;

pub struct MediaIndex {
    files: HashMap<Checksum, FileChecksum>,
    crc32: HashMap<u32, HashMap<Checksum, ()>>,
}

impl MediaIndex {
    // pub fn new() -> MediaIndex {
    //     MediaIndex {
    //         files: HashMap::new(),
    //         crc32: HashMap::new(),
    //     }
    // }
    //
    // pub fn get(&self, checksum: &Checksum) -> Option<&FileChecksum> {
    //     self.files.get(checksum)
    // }
    //
    // pub fn insert(&mut self, mut media: FileChecksum) -> std::io::Result<&FileChecksum> {
    //     let crc32 = media.get_crc32()?;
    //     let checksum = media.get_sha256()?;
    //
    //     self.files.insert(checksum, media);
    //     self.crc32
    //         .entry(crc32)
    //         .or_insert_with(HashMap::new)
    //         .insert(checksum, ());
    //     Ok(self.get(&checksum).unwrap())
    // }
}

#[cfg(test)]
mod tests {
    use crate::file_checksum::FileChecksum;
    use crate::media_index::MediaIndex;
    #[test]
    fn new() {
        let mut index = MediaIndex::new();
        assert_eq!(index.files.len(), 0);
        assert_eq!(index.crc32.len(), 0);

        if let Ok(mut media) =
            FileChecksum::new("/Users/user/Movies/寰宇全視界20210512全球晶片荒產能急重組.mp4")
        {
            assert_eq!(0xc3ff178e, media.full_crc32().unwrap());
            let crc32 = media.get_crc32().unwrap();
            let checksum = media.get_sha256().unwrap();

            _ = index.insert(media);

            if let Some(media) = index.get(&checksum) {
                assert_eq!(crc32, media.crc32().unwrap());
                assert_eq!(checksum, media.sha256().unwrap());
            }
        }
        // mi.files.insert(0, Media::new("/tmp/test.mp4").unwrap());
    }
}

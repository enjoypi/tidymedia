pub fn initialize() -> [u32; 256] {
    let mut table: [u32; 256] = [0; 256];
    for i in 0..table.len() {
        let mut crc: u32 = i as u32;
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
        table[i] = crc;
    }
    table
}

pub fn calculate(table: &[u32; 256], data: &str) -> u32 {
    let mut crc: u32 = 0xffffffff;

    for b in data.bytes() {
        crc = table[((crc as u8) ^ b  & 0xff) as usize] ^ (crc >> 8)
    }
    crc
}

#[cfg(test)]
mod tests {

    #[test]
    fn one_result() {
        let contents = "abcdefghijkasdfasdfasdfsdf";

        use crc32fast::Hasher;

        let mut hasher = Hasher::new();
        hasher.update(contents.as_bytes());
        let checksum = hasher.finalize();
        assert_eq!(0xCCF8A3B6, checksum);
    }
}


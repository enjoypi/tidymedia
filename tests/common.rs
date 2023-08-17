use tidymedia::{decode_hex_string, SecureChecksum};

pub type Result = std::result::Result<(), Box<dyn std::error::Error>>;

pub const DATA0: &str = "tests/data0";
pub const DATA0_WYHASH: u64 = 13333046383594682858;
pub const DATA0_XXHASH: u64 = 0x1a5efdfdbd01a44c;
pub fn data0_sha512() -> SecureChecksum {
    decode_hex_string("c77d955d24f36057a2fc6eba10d9a386ef6b8a6568e73bb8f6a168b4e2adc65fa2ffdc6f6e479f42199b740b8e83af74caffa6f580d4b7351be20efa65b0fcd2")
}

pub const DATA1: &str = "tests/data1";
pub const DATA1_WYHASH: u64 = 0;
pub const DATA1_XXHASH: u64 = 0xca510fe9ebc09aa9;
pub fn data1_sha512() -> SecureChecksum {
    decode_hex_string("ceb20165fcc949aa93168dacd26c4e48d0460e3e48a5e6fdbecfeea28962d1f966fec7e2ff8f5091ae64d62b140ae3cb2ff5a8be132652294b6aa0a79b1475e6")
}

pub const DATA2: &str = "tests/data2";
pub const DATA2_WYHASH: u64 = 0;
pub const DATA2_XXHASH: u64 = 0x9dba53c59ea968e9;
pub fn data2_sha512() -> SecureChecksum {
    decode_hex_string("0f7fd3e44b860c33de83c19edb759edcad9c6e101910f765e86e2443f533f9c254ad544a84e4bb56b221620148c79b2b8619cfd8f611d30617c6c32f210bcea7")
}

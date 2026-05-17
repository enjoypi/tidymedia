// docs/media-time-detection.md 的可执行契约测试。
// 每个子模块对应 spec 一章；测试名映射到具体 §X.Y 规则。
// 此文件本身仅做模块装配，不放断言。

#[path = "media_time/common.rs"]
mod common;

#[path = "media_time/p0_authoritative.rs"]
mod p0;

#[path = "media_time/p1_digitized.rs"]
mod p1;

#[path = "media_time/p2_filename.rs"]
mod p2;

#[path = "media_time/p3_sidecar.rs"]
mod p3;

#[path = "media_time/p4_filesystem.rs"]
mod p4;

#[path = "media_time/priority_ordering.rs"]
mod priority_ordering;

#[path = "media_time/timezone.rs"]
mod timezone;

#[path = "media_time/traps.rs"]
mod traps;

#[path = "media_time/cross_validation.rs"]
mod cross_validation;

#[path = "media_time/result_fields.rs"]
mod result_fields;

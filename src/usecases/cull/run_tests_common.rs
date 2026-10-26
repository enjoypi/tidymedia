use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use camino::Utf8PathBuf;

use super::ScannedFile;
use crate::adapters::backend::local::LocalBackend;
use crate::entities::backend::Backend;
use crate::entities::backend::factory::BackendFactory;
use crate::entities::common::{Error, Result};
use crate::entities::uri::Location;

pub(super) fn local_loc(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

pub(super) fn smb_loc(path: &str) -> Location {
    Location::Smb {
        user: None,
        host: "nas".into(),
        port: None,
        share: "x".into(),
        path: Utf8PathBuf::from(path),
    }
}

pub(super) struct MapFactory {
    by_scheme: HashMap<&'static str, Arc<dyn Backend>>,
}

impl MapFactory {
    pub(super) fn new() -> Self {
        Self {
            by_scheme: HashMap::new(),
        }
    }
    pub(super) fn insert(&mut self, scheme: &'static str, b: Arc<dyn Backend>) {
        self.by_scheme.insert(scheme, b);
    }
}

impl BackendFactory for MapFactory {
    fn for_location(&self, loc: &Location) -> Result<Arc<dyn Backend>> {
        if let Some(b) = self.by_scheme.get(loc.scheme()) {
            return Ok(Arc::clone(b));
        }
        if matches!(loc, Location::Local(_)) {
            return Ok(LocalBackend::arc());
        }
        Err(Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("no fake backend for scheme {}", loc.scheme()),
        )))
    }
}

pub(super) fn write_png(path: &std::path::Path, fill: [u8; 3]) {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let mut pixels = Vec::with_capacity(64 * 64 * 3);
    for i in 0_u32..(64 * 64) {
        let noise = noise_byte(i);
        pixels.push(fill[0].wrapping_add(noise));
        pixels.push(fill[1].wrapping_add(noise));
        pixels.push(fill[2].wrapping_add(noise));
    }
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, 64, 64, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

fn noise_byte(i: u32) -> u8 {
    let v = i.wrapping_mul(37) ^ (i >> 3);
    (v & 0xff) as u8
}

pub(super) fn write_random_png(path: &std::path::Path, side: u32) {
    use image::ImageEncoder;
    let total = (side as usize) * (side as usize) * 3;
    let mut pixels = vec![0_u8; total];
    for (i, p) in pixels.iter_mut().enumerate() {
        *p = u8::try_from((i.wrapping_mul(37) ^ (i >> 3)) & 0xff).expect("internal: & 0xff < 256");
    }
    let mut out = Vec::with_capacity(total);
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&pixels, side, side, image::ExtendedColorType::Rgb8)
        .unwrap();
    std::fs::write(path, out).unwrap();
}

pub(super) fn make_scanned(sharpness: f32) -> ScannedFile {
    ScannedFile {
        src_loc: local_loc("/x"),
        src_backend: LocalBackend::arc(),
        source_root: local_loc("/"),
        hash: 0,
        sharpness,
    }
}

pub(super) fn call_analyze_image(
    item: &ScannedFile,
    failures: &mut Vec<(String, io::Error)>,
) -> Option<()> {
    use crate::adapters::face::fake::{
        FakeEyeStateClassifier, FakeFaceEmbedder, FakeFaceMeshDetector,
    };

    let scrfd = crate::FakeFaceDetector::new(vec![]);
    let facenet = FakeFaceEmbedder::new([0.0; 128]);
    let facemesh = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468]);
    let eyestate = FakeEyeStateClassifier::new(0.0);
    let face_cfg = &crate::usecases::config::config().backend.face;
    super::analyze_image(
        item, &scrfd, &facenet, &facemesh, &eyestate, face_cfg, failures,
    )
    .map(|_| ())
}

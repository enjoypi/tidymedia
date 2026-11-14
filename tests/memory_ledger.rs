use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use camino::Utf8PathBuf;
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma, Rgb, RgbImage};
use tidymedia::{
    Backend, BackendFactory, CommandResult, Commands, DefaultDetectorFactory, FakeBackend,
    Location, Result, calibrate_decode, tidy_with,
};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn peak_during<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let result = f();
    (result, PEAK.load(Ordering::Relaxed) - base)
}

fn noise_rgb(width: u32, height: u32) -> RgbImage {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    ImageBuffer::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let b = state.to_le_bytes();
        Rgb([b[0], b[1], b[2]])
    })
}

fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    image.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

fn decode_samples() -> Vec<(&'static str, Vec<u8>)> {
    let rgb = DynamicImage::ImageRgb8(noise_rgb(2000, 1500));
    let gray = DynamicImage::ImageLuma8(ImageBuffer::from_fn(2000, 2000, |x, y| {
        Luma([u8::try_from((x ^ y) & 0xFF).unwrap()])
    }));
    let wide = DynamicImage::ImageRgb16(rgb.to_rgb16());
    vec![
        ("jpeg", encode(&rgb, ImageFormat::Jpeg)),
        ("png-gray", encode(&gray, ImageFormat::Png)),
        ("png-rgb16", encode(&wide, ImageFormat::Png)),
        ("webp", encode(&rgb, ImageFormat::WebP)),
        ("tiff", encode(&rgb, ImageFormat::Tiff)),
    ]
}

struct LocalFake(Arc<FakeBackend>);

impl BackendFactory for LocalFake {
    fn for_location(&self, _loc: &Location) -> Result<Arc<dyn Backend>> {
        Ok(self.0.clone())
    }
}

fn local(path: &str) -> Location {
    Location::Local(Utf8PathBuf::from(path))
}

const FILES: usize = 2000;
const PATH_LEN: usize = "/src/d00/f000000.bin".len();
const SOURCE_ENTRY_BYTES: usize = 3584;
const OUTPUT_ENTRY_BYTES: usize = 2048;
const PATH_COPIES: usize = 5;

fn bytes_per_file(command: Commands) -> usize {
    let fake = Arc::new(FakeBackend::new("local"));
    fake.add_dir(local("/src"));
    for i in 0..FILES {
        fake.add_file(
            local(&format!("/src/d{:02}/f{i:06}.bin", i % 50)),
            i.to_le_bytes().to_vec(),
        );
    }
    let factory = LocalFake(fake);
    let (result, peak) =
        peak_during(|| tidy_with(&factory, &DefaultDetectorFactory, command).unwrap());
    let scanned = match result {
        CommandResult::Copy(report) => report.copied,
        CommandResult::Find(report) => report.scanned,
        CommandResult::MoveTextShot(_) | CommandResult::Verify(_) => 0,
    };
    assert_eq!(scanned, FILES);
    peak / FILES
}

#[test]
fn memory_estimates_cover_measured_peaks() {
    for (name, bytes) in decode_samples() {
        let (estimate, peak) = peak_during(|| calibrate_decode(&bytes));
        let estimate = usize::try_from(estimate.unwrap()).unwrap();
        println!("decode {name}: peak {peak} estimate {estimate}");
        assert!(
            peak <= estimate,
            "{name}: peak {peak} > estimate {estimate}"
        );
    }
    let copy = bytes_per_file(Commands::Copy {
        dry_run: true,
        include_non_media: true,
        sources: vec![local("/src")],
        output: local("/out"),
        archive_template: None,
        report: None,
    });
    let copy_budget = SOURCE_ENTRY_BYTES + PATH_LEN * PATH_COPIES;
    println!("copy dry-run: {copy} bytes per file, budget {copy_budget}");
    assert!(copy <= copy_budget, "{copy} > {copy_budget}");
    let find = bytes_per_file(Commands::Find {
        secure: false,
        sources: vec![local("/src")],
        output: None,
        report: None,
    });
    let find_budget = OUTPUT_ENTRY_BYTES + PATH_LEN * PATH_COPIES;
    println!("find: {find} bytes per file, budget {find_budget}");
    assert!(find <= find_budget, "{find} > {find_budget}");
}

#[cfg(test)]
#[path = "support/tracing_fixture_tests.rs"]
mod tracing_fixture;

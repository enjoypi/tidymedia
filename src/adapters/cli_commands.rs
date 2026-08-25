// Commands 枚举拆分文件：原 cli.rs 307 行拆为两份各 ≤300。
// 本文件承接 `Commands` 枚举与各子命令字段定义；
// 公开 API（`Cli` / `Commands` / `run_cli`）留在 cli.rs，`Commands` 经
// cli.rs `pub use commands::Commands` 原样暴露，签名与行为完全不变。

use clap::Subcommand;

use crate::entities::uri::Location;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Copy non-duplicate media files (images / videos recognized via magic-bytes MIME) from sources to the output directory. Pass --include-non-media to also copy everything else. Duplicate detection uses SHA-512. No source files are modified. Sources / output accept URI form: `smb://[user@]host[:port]/share/path`, `mtp://device/storage/path` or plain local path.
    Copy {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// Also copy files that magic-bytes MIME does not classify as image/video (e.g. documents, archives, unknown formats)
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Find duplicate files under the sources and print a shell script (batch syntax on Windows) that deletes the duplicates. Default uses a fast non-cryptographic hash (xxh3-64); pass --secure to use SHA-512 instead. If --output is given, deletions for files under that directory are commented out.
    Find {
        /// Use the cryptographic hash (SHA-512) instead of the default fast non-cryptographic hash (xxh3-64). Slower but eliminates the (already astronomically small) collision risk.
        #[arg(short, long)]
        secure: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory; deletions for files under it are commented out
        #[arg(short, long)]
        output: Option<Location>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Move images whose content contains detectable text (OCR text detection) from sources into the output directory, preserving each file's path relative to its source root. Requires a configured `PaddleOCR` `DBNet` `det.onnx` model (`backend.ocr.det_model_path` / `TIDYMEDIA_OCR_DET_MODEL`). Non-image files are skipped.
    MoveTextShot {
        /// Dry run, do not move files
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Cull similar/burst photos: keep the best one in source and move lower-quality copies to `output/<relative-path>/group-NNN/`, with a `BEST_<basename>` copy of the best photo placed alongside for side-by-side review. Uses perceptual hashing for grouping plus 4 ONNX models (`SCRFD`/`MobileFaceNet`/`FaceMesh`/`EyeState`) configured under `backend.face.*` for face quality scoring.
    Cull {
        /// Dry run, do not move files or create output directories
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Maximum pHash Hamming distance for grouping similar photos (overrides `backend.face.phash_hamming_max`)
        #[arg(long)]
        phash_max: Option<u8>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Move non-duplicate media files from sources into the output directory. Sources that duplicate something already in output are physically deleted; duplicate detection uses SHA-512. Pass --include-non-media to also move everything else.
    Move {
        /// Dry run, do not move or delete files
        #[arg(short, long)]
        dry_run: bool,

        /// Also move files that magic-bytes MIME does not classify as image/video
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Copy non-duplicate document files from sources to the output directory, archived by document creation time and content category. Documents cover pdf, doc/xls/ppt, docx/xlsx/pptx, odt/ods/odp, rtf, epub, pages/numbers/key, mind maps and plain text (txt/md/rst/csv/tsv). Media and unknown formats are skipped (use `copy` for those). Duplicate detection uses SHA-512; no source files are modified.
    CopyDoc {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{category}` `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Move non-duplicate document files from sources into the output directory, archived by document creation time and content category (same document formats as `copy-doc`). Sources that duplicate something already in output are physically deleted; duplicate detection uses SHA-512. Media and unknown formats are left untouched.
    MoveDoc {
        /// Dry run, do not move or delete files
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{category}` `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Verify pending archive decisions without moving anything: report each source file's predicted archive bucket, `media_time` decision and its conflicts, and (with `--exif-tsv`) cross-check against a second implementation's expected bucket to catch container times that tidymedia's own reader missed. Diagnostic only; writes no files.
    Verify {
        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path) where files are/will be archived
        #[arg(short, long)]
        output: Location,

        /// Also verify files that magic-bytes MIME does not classify as image/video
        #[arg(long)]
        include_non_media: bool,

        /// TAB-separated EXIF table (the 8-column exiftool `-p` contract of `.claude/skills/tidy-verify/config.yaml`) injected for cross-checking expected buckets
        #[arg(long)]
        exif_tsv: Option<String>,

        /// Maximum pHash Hamming distance for pixel-level duplicate comparison (defaults to `backend.face.phash_hamming_max`)
        #[arg(long)]
        phash_max: Option<u8>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },
}

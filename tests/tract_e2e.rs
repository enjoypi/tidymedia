use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;

use camino::Utf8Path;

use tidymedia::{
    CategoryDef, ClassifyConfig, FaceConfig, OcrConfig, build_classifier, build_detector,
    build_eyestate_classifier, build_facemesh, build_facenet_embedder, build_scrfd_detector,
};

fn models() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models")
}

fn photo_bytes() -> Vec<u8> {
    let p = format!(
        "{}/tests/data/sample-with-exif.jpg",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read(p).expect("sample photo fixture exists")
}

fn face_cfg() -> FaceConfig {
    face_cfg_with_paths(
        "scrfd_10g_bnkps.onnx",
        "mobilefacenet.onnx",
        "face_mesh_192x192.onnx",
        "eyestate_yolov8.onnx",
    )
}

fn face_cfg_with_paths(scrfd: &str, facenet: &str, facemesh: &str, eyestate: &str) -> FaceConfig {
    FaceConfig {
        scrfd_model_path: models().join(scrfd).display().to_string(),
        scrfd_score_threshold: 0.5,
        scrfd_nms_iou: 0.4,
        facenet_model_path: models().join(facenet).display().to_string(),
        facemesh_model_path: models().join(facemesh).display().to_string(),
        eyestate_model_path: models().join(eyestate).display().to_string(),
        ..FaceConfig::default()
    }
}

#[test]
fn scrfd_real_model_detects() {
    let det = build_scrfd_detector(&face_cfg()).unwrap();
    let faces = det
        .detect_faces(Utf8Path::new("/photo.jpg"), &photo_bytes())
        .unwrap();
    for f in &faces {
        assert_eq!(f.bbox.len(), 4);
        assert_eq!(f.landmarks_5pt.len(), 5);
    }
}

#[test]
fn eyestate_real_model_classifies() {
    let det = build_eyestate_classifier(&face_cfg()).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let p = det.classify_eye(Utf8Path::new("/x"), &img).unwrap();
    assert!((0.0..=1.0).contains(&p), "got {p}");
}

#[test]
fn facemesh_real_model_meshes() {
    let det = build_facemesh(&face_cfg()).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let mesh = det.detect_mesh(Utf8Path::new("/x"), &img).unwrap();
    assert!(mesh.len() <= 478, "got {}", mesh.len());
}

#[test]
fn facenet_real_model_embeds() {
    let det = build_facenet_embedder(&face_cfg()).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let v = det.embed_face(Utf8Path::new("/x"), &img).unwrap();
    assert_eq!(v.len(), 128);
}

#[test]
fn embed_real_model_classifies() {
    let cfg = ClassifyConfig {
        embed_model_path: models()
            .join("bge_small_zh_v15_int8.onnx")
            .display()
            .to_string(),
        tokenizer_path: models()
            .join("bge_small_zh_v15_tokenizer.json")
            .display()
            .to_string(),
        categories: vec![CategoryDef {
            name: "趣味".to_string(),
            description: "日常记录与趣味内容".to_string(),
        }],
        score_min: 0.0,
        max_text_bytes: 4096,
    };
    let det = build_classifier(&cfg).unwrap();
    let c = det
        .classify(Utf8Path::new("/t.md"), "周末去公园散步拍的照片")
        .unwrap();
    assert!(
        c.category.contains("趣味") || c.score.is_finite(),
        "got {c:?}"
    );
}

#[test]
fn dbnet_missing_model_propagates_load_error() {
    let cfg = OcrConfig {
        det_model_path: models().join("missing-det.onnx").display().to_string(),
        ..OcrConfig::default()
    };
    let det = build_detector(&cfg).unwrap();
    let err = det
        .has_text(Utf8Path::new("/x.jpg"), &photo_bytes())
        .unwrap_err();
    assert!(err.to_string().contains("load"), "unexpected: {err}");
}

#[test]
fn embed_missing_paths_propagate_load_error() {
    let mk = |model: &str, tok: &str| ClassifyConfig {
        embed_model_path: models().join(model).display().to_string(),
        tokenizer_path: models().join(tok).display().to_string(),
        categories: vec![CategoryDef {
            name: "a".to_string(),
            description: "b".to_string(),
        }],
        score_min: 0.0,
        max_text_bytes: 64,
    };
    // tokenizer 缺失 → `Tokenizer::from_file` Err（load_raw_embedder 首步）。
    let det = build_classifier(&mk("bge_small_zh_v15_int8.onnx", "no-tokenizer.json")).unwrap();
    let err = det.classify(Utf8Path::new("/t"), "你好").unwrap_err();
    assert!(err.to_string().contains("load"), "tokenizer: {err}");
    // 模型缺失 → `model_for_path` Err。
    let det = build_classifier(&mk("no-model.onnx", "bge_small_zh_v15_tokenizer.json")).unwrap();
    let err = det.classify(Utf8Path::new("/t"), "你好").unwrap_err();
    assert!(err.to_string().contains("load"), "model: {err}");
}

#[test]
fn face_models_missing_path_propagate_load_error() {
    let missing = "no-such-model.onnx";

    let scrfd = build_scrfd_detector(&face_cfg_with_paths(missing, "", "", "")).unwrap();
    let err = scrfd
        .detect_faces(Utf8Path::new("/x"), &photo_bytes())
        .unwrap_err();
    assert!(err.to_string().contains("load"), "scrfd: {err}");

    let eye = build_eyestate_classifier(&face_cfg_with_paths("", "", "", missing)).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let err = eye.classify_eye(Utf8Path::new("/x"), &img).unwrap_err();
    assert!(err.to_string().contains("load"), "eyestate: {err}");

    let mesh = build_facemesh(&face_cfg_with_paths("", "", missing, "")).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let err = mesh.detect_mesh(Utf8Path::new("/x"), &img).unwrap_err();
    assert!(err.to_string().contains("load"), "facemesh: {err}");

    let emb = build_facenet_embedder(&face_cfg_with_paths("", missing, "", "")).unwrap();
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let err = emb.embed_face(Utf8Path::new("/x"), &img).unwrap_err();
    assert!(err.to_string().contains("load"), "facenet: {err}");
}

fn run_concurrent<F>(f: F)
where
    F: Fn(Arc<Barrier>) + Send + Sync + Clone + 'static,
{
    let barrier = Arc::new(Barrier::new(8));
    let hs: Vec<_> = (0..8)
        .map(|_| {
            let b = Arc::clone(&barrier);
            let f = f.clone();
            thread::spawn(move || {
                f(b);
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
}

#[test]
fn face_ensure_raw_concurrent_load() {
    let img = image::load_from_memory(&photo_bytes()).unwrap().to_rgb8();
    let bytes = photo_bytes();

    let scrfd = Arc::new(build_scrfd_detector(&face_cfg()).unwrap());
    let eye = Arc::new(build_eyestate_classifier(&face_cfg()).unwrap());
    let mesh = Arc::new(build_facemesh(&face_cfg()).unwrap());
    let emb = Arc::new(build_facenet_embedder(&face_cfg()).unwrap());

    let d = scrfd.clone();
    run_concurrent(move |b| {
        b.wait();
        d.detect_faces(Utf8Path::new("/x"), &bytes).unwrap();
    });
    let d = eye.clone();
    let img2 = img.clone();
    run_concurrent(move |b| {
        b.wait();
        d.classify_eye(Utf8Path::new("/x"), &img2).unwrap();
    });
    let d = mesh.clone();
    let img3 = img.clone();
    run_concurrent(move |b| {
        b.wait();
        d.detect_mesh(Utf8Path::new("/x"), &img3).unwrap();
    });
    let d = emb.clone();
    let img4 = img;
    run_concurrent(move |b| {
        b.wait();
        d.embed_face(Utf8Path::new("/x"), &img4).unwrap();
    });
}

#[test]
fn face_aligned_input_uses_borrowed_cow() {
    let mesh = build_facemesh(&face_cfg()).unwrap();
    let mesh_img = image::RgbImage::new(192, 192);
    mesh.detect_mesh(Utf8Path::new("/x"), &mesh_img).unwrap();

    let emb = build_facenet_embedder(&face_cfg()).unwrap();
    let emb_img = image::RgbImage::new(112, 112);
    emb.embed_face(Utf8Path::new("/x"), &emb_img).unwrap();
}

#[test]
fn embed_empty_categories_returns_empty_classification() {
    let cfg = ClassifyConfig {
        embed_model_path: models()
            .join("bge_small_zh_v15_int8.onnx")
            .display()
            .to_string(),
        tokenizer_path: models()
            .join("bge_small_zh_v15_tokenizer.json")
            .display()
            .to_string(),
        categories: vec![],
        score_min: 0.0,
        max_text_bytes: 64,
    };
    let det = build_classifier(&cfg).unwrap();
    let c = det.classify(Utf8Path::new("/t"), "任意文本").unwrap();
    assert!(c.category.is_empty() && c.score.is_infinite());
}

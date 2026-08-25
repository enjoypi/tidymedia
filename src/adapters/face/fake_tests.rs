use super::*;

fn tiny_rgb() -> image::RgbImage {
    image::RgbImage::from_pixel(2, 2, image::Rgb([0, 0, 0]))
}

fn sample_detection() -> FaceDetection {
    FaceDetection {
        bbox: [0.0, 0.0, 10.0, 10.0],
        score: 0.9,
        landmarks_5pt: [[1.0, 1.0]; 5],
    }
}

#[test]
fn fake_face_detector_returns_default_on_miss() {
    let d = FakeFaceDetector::new(vec![sample_detection()]);
    let faces = d.detect_faces(Utf8Path::new("/x.jpg"), b"").unwrap();
    assert_eq!(faces.len(), 1);
}

#[test]
fn fake_face_detector_returns_explicit_result_then_error() {
    let d = FakeFaceDetector::new(Vec::new())
        .with_result("/a.jpg", vec![sample_detection(), sample_detection()])
        .with_error("/e.jpg");
    let faces = d.detect_faces(Utf8Path::new("/a.jpg"), b"").unwrap();
    assert_eq!(faces.len(), 2);
    let e = d.detect_faces(Utf8Path::new("/e.jpg"), b"").unwrap_err();
    assert!(e.to_string().contains("injected error"));
}

#[test]
fn fake_face_detector_debug_shows_counts() {
    let d = FakeFaceDetector::new(vec![sample_detection()])
        .with_result("/a", vec![sample_detection()])
        .with_error("/b");
    let s = format!("{d:?}");
    assert!(s.contains("default_count: 1"), "got: {s}");
    assert!(s.contains("results_count: 1"), "got: {s}");
    assert!(s.contains("errors_count: 1"), "got: {s}");
}

#[test]
fn fake_face_embedder_path_resolution_and_error_precedence() {
    let zero = [0.0; 128];
    let mut one = [0.0; 128];
    one[0] = 1.0;
    let d = FakeFaceEmbedder::new(zero)
        .with_result("/a", one)
        .with_error("/e");
    let img = tiny_rgb();
    assert!(
        d.embed_face(Utf8Path::new("/miss"), &img).unwrap()[0].abs() < f32::EPSILON,
        "miss → default 0"
    );
    assert!(
        (d.embed_face(Utf8Path::new("/a"), &img).unwrap()[0] - 1.0).abs() < f32::EPSILON,
        "hit → with_result 1"
    );
    let err = d.embed_face(Utf8Path::new("/e"), &img).unwrap_err();
    assert!(err.to_string().contains("injected error"));
}

#[test]
fn fake_face_embedder_debug_shows_counts() {
    let d = FakeFaceEmbedder::new([0.0; 128])
        .with_result("/a", [0.0; 128])
        .with_error("/b");
    let s = format!("{d:?}");
    assert!(s.contains("results_count: 1"), "got: {s}");
    assert!(s.contains("errors_count: 1"), "got: {s}");
}

#[test]
fn fake_face_mesh_path_resolution_and_error_precedence() {
    let default_mesh = vec![[0.0; 3]; 468];
    let custom_mesh = vec![[1.0; 3]; 468];
    let d = FakeFaceMeshDetector::new(default_mesh)
        .with_result("/a", custom_mesh)
        .with_error("/e");
    let img = tiny_rgb();
    let m = d.detect_mesh(Utf8Path::new("/miss"), &img).unwrap();
    assert!(m[0].iter().all(|v| v.abs() < f32::EPSILON));
    let m = d.detect_mesh(Utf8Path::new("/a"), &img).unwrap();
    assert!(m[0].iter().all(|v| (v - 1.0).abs() < f32::EPSILON));
    let err = d.detect_mesh(Utf8Path::new("/e"), &img).unwrap_err();
    assert!(err.to_string().contains("injected error"));
}

#[test]
fn fake_face_mesh_debug_shows_counts() {
    let d = FakeFaceMeshDetector::new(vec![[0.0; 3]; 468])
        .with_result("/a", vec![[1.0; 3]; 468])
        .with_error("/b");
    let s = format!("{d:?}");
    assert!(s.contains("default_len: 468"), "got: {s}");
    assert!(s.contains("results_count: 1"), "got: {s}");
    assert!(s.contains("errors_count: 1"), "got: {s}");
}

#[test]
fn fake_eye_state_path_resolution_and_error_precedence() {
    let d = FakeEyeStateClassifier::new(0.2)
        .with_result("/closed", 0.9)
        .with_error("/e");
    let img = tiny_rgb();
    assert!((d.classify_eye(Utf8Path::new("/miss"), &img).unwrap() - 0.2).abs() < f32::EPSILON);
    assert!((d.classify_eye(Utf8Path::new("/closed"), &img).unwrap() - 0.9).abs() < f32::EPSILON);
    let err = d.classify_eye(Utf8Path::new("/e"), &img).unwrap_err();
    assert!(err.to_string().contains("injected error"));
}

#[test]
fn fake_eye_state_debug_shows_counts() {
    let d = FakeEyeStateClassifier::new(0.5)
        .with_result("/a", 0.9)
        .with_error("/b");
    let s = format!("{d:?}");
    assert!(s.contains("default: 0.5"), "got: {s}");
    assert!(s.contains("results_count: 1"), "got: {s}");
    assert!(s.contains("errors_count: 1"), "got: {s}");
}

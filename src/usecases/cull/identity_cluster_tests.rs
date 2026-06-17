//! `identity_cluster` 单测：覆盖空输入、单图、同人、不同人、链式传递、阈值边界、
//! NaN 输入、零向量、同图多脸去重。

use super::*;

fn embedding(seed: f32) -> [f32; EMBED_DIM] {
    let mut e = [0.0_f32; EMBED_DIM];
    e[0] = seed;
    e
}

#[test]
fn cluster_identities_returns_empty_when_no_faces() {
    let out = cluster_identities(&[], 0.5);
    assert!(out.is_empty());
}

#[test]
fn cluster_identities_returns_empty_when_all_images_have_no_faces() {
    let out = cluster_identities(&[vec![], vec![], vec![]], 0.5);
    assert!(out.is_empty());
}

#[test]
fn cluster_identities_single_face_yields_single_cluster() {
    let out = cluster_identities(&[vec![embedding(1.0)]], 0.5);
    assert_eq!(out, vec![vec![0_usize]]);
}

#[test]
fn cluster_identities_merges_two_images_with_same_embedding() {
    let e = embedding(1.0);
    let out = cluster_identities(&[vec![e], vec![e]], 0.5);
    assert_eq!(out, vec![vec![0_usize, 1]]);
}

#[test]
fn cluster_identities_separates_orthogonal_embeddings() {
    let mut a = [0.0_f32; EMBED_DIM];
    let mut b = [0.0_f32; EMBED_DIM];
    a[0] = 1.0;
    b[1] = 1.0; // cosine = 0
    let out = cluster_identities(&[vec![a], vec![b]], 0.5);
    assert_eq!(out.len(), 2);
}

#[test]
fn cluster_identities_propagates_transitively() {
    // 构造 A-B 与 B-C 都过 threshold=0.5，但 A-C 不过 → 传递闭包合一簇。
    // A=[1,0], B=[1,0.5], C=[0.5,1]：
    //   A·B = 1 / (1·√1.25) ≈ 0.894 (≥0.5)
    //   B·C = (0.5+0.5)/(√1.25·√1.25) = 0.8 (≥0.5)
    //   A·C = 0.5 / (1·√1.25) ≈ 0.447 (<0.5)
    let mut a = [0.0_f32; EMBED_DIM];
    let mut b = [0.0_f32; EMBED_DIM];
    let mut c = [0.0_f32; EMBED_DIM];
    a[0] = 1.0;
    b[0] = 1.0;
    b[1] = 0.5;
    c[0] = 0.5;
    c[1] = 1.0;
    let out = cluster_identities(&[vec![a], vec![b], vec![c]], 0.5);
    assert_eq!(out.len(), 1, "A-B-C 全部因传递闭包合一簇: {out:?}");
    assert_eq!(out[0], vec![0_usize, 1, 2]);
}

#[test]
fn cluster_identities_threshold_at_exact_boundary_includes() {
    // cosine == threshold 命中 `>=` 入同簇（边界包含）。
    let e = embedding(1.0);
    // 两份相同向量 cosine = 1.0；threshold = 1.0 仍 >=。
    let out = cluster_identities(&[vec![e], vec![e]], 1.0);
    assert_eq!(out.len(), 1);
}

#[test]
fn cluster_identities_threshold_above_max_keeps_all_separate() {
    let e = embedding(1.0);
    let out = cluster_identities(&[vec![e], vec![e]], 1.1);
    assert_eq!(out.len(), 2, "threshold > 1 让任何对都不连通");
}

#[test]
fn cluster_identities_zero_norm_embedding_does_not_merge() {
    // 零向量与任何向量 cosine = 0.0（NaN/0 兜底返 0.0），不连通。
    let zero = [0.0_f32; EMBED_DIM];
    let one = embedding(1.0);
    let out = cluster_identities(&[vec![zero], vec![one]], 0.1);
    assert_eq!(out.len(), 2);
}

#[test]
fn cluster_identities_same_image_multiple_faces_dedup() {
    // 同图 2 张脸 + 另一图 1 张脸全相似 → 1 簇 [0, 1]（图索引去重）。
    let e = embedding(1.0);
    let out = cluster_identities(&[vec![e, e], vec![e]], 0.5);
    assert_eq!(out, vec![vec![0_usize, 1]]);
}

#[test]
fn cluster_identities_handles_nan_input_without_panic() {
    let mut nan = [0.0_f32; EMBED_DIM];
    nan[0] = f32::NAN;
    let e = embedding(1.0);
    let out = cluster_identities(&[vec![nan], vec![e]], 0.5);
    assert_eq!(out.len(), 2, "NaN cosine 不连通");
}

#[test]
fn cosine_similarity_identical_normalized_vectors_is_one() {
    let mut e = [0.0_f32; EMBED_DIM];
    e[0] = 1.0;
    let sim = cosine_similarity(&e, &e);
    assert!((sim - 1.0).abs() < 1e-5, "got: {sim}");
}

#[test]
fn cosine_similarity_orthogonal_is_zero() {
    let mut a = [0.0_f32; EMBED_DIM];
    let mut b = [0.0_f32; EMBED_DIM];
    a[0] = 1.0;
    b[1] = 1.0;
    let sim = cosine_similarity(&a, &b);
    assert!(sim.abs() < 1e-5);
}

#[test]
fn cosine_similarity_zero_vector_returns_zero() {
    let zero = [0.0_f32; EMBED_DIM];
    let one = embedding(1.0);
    assert!(cosine_similarity(&zero, &one).abs() < f32::EPSILON);
    assert!(cosine_similarity(&one, &zero).abs() < f32::EPSILON);
}

#[test]
fn cosine_similarity_nan_returns_zero() {
    let mut nan = [0.0_f32; EMBED_DIM];
    nan[0] = f32::NAN;
    let one = embedding(1.0);
    assert!(cosine_similarity(&nan, &one).abs() < f32::EPSILON);
}

#[test]
fn union_find_finds_self_root_for_singleton() {
    let mut uf = UnionFind::new(3);
    assert_eq!(uf.find(0), 0);
    assert_eq!(uf.find(1), 1);
    assert_eq!(uf.find(2), 2);
}

#[test]
fn union_find_union_then_same_root() {
    let mut uf = UnionFind::new(4);
    uf.union(0, 1);
    uf.union(2, 3);
    assert_eq!(uf.find(0), uf.find(1));
    assert_eq!(uf.find(2), uf.find(3));
    assert_ne!(uf.find(0), uf.find(2));
    uf.union(1, 2);
    assert_eq!(uf.find(0), uf.find(3), "传递闭包合一");
}

#[test]
fn union_find_union_already_connected_is_noop() {
    let mut uf = UnionFind::new(3);
    uf.union(0, 1);
    let root_before = uf.find(0);
    uf.union(1, 0); // 已连通
    assert_eq!(uf.find(0), root_before);
}

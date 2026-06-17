//! 跨图人脸身份聚类：余弦相似度 + Union-Find 把所有图片中检测到的人脸按 embedding
//! 聚成「同一人」的簇，每个簇含原图索引列表。
//!
//! 算法：
//! 1. 扁平化所有 `(img_idx, face_embedding)` 到全局索引；
//! 2. O(N²) 两两比较余弦相似度，≥ threshold 合入同一 Union-Find 集合（传递闭包）；
//! 3. 按 root 聚合，每簇取去重后的图索引列表（同图多脸不重复计数）。
//!
//! 不依赖 embedding 已归一化——内部按 L2 norm 再除一次让 Fake 输入也稳定。

// pick_best 接入 4 模型印证流水线前本模块仅被单测调用。commit 5 把
// cluster_identities 接入 run.rs 后此 allow 删除。
#![allow(dead_code, reason = "占位实现：pick_best 接入 4 模型印证后启用")]

use std::collections::{BTreeMap, BTreeSet};

/// `MobileFaceNet` 128 维 embedding；同 `FaceEmbedder` trait 维度。
pub(crate) const EMBED_DIM: usize = 128;

/// 把 `embeddings`（外层每张图，内层每脸 128 维向量）按余弦相似度 `>= threshold` 聚类。
///
/// 返回每簇含的原图索引列表（去重 + 升序）；簇间按首图索引升序排列。
pub(crate) fn cluster_identities(
    embeddings: &[Vec<[f32; EMBED_DIM]>],
    threshold: f32,
) -> Vec<Vec<usize>> {
    let mut flat: Vec<(usize, [f32; EMBED_DIM])> = Vec::new();
    for (img_idx, faces) in embeddings.iter().enumerate() {
        for face in faces {
            flat.push((img_idx, *face));
        }
    }
    let n = flat.len();
    if n == 0 {
        return Vec::new();
    }
    let mut uf = UnionFind::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            if cosine_similarity(&flat[i].1, &flat[j].1) >= threshold {
                uf.union(i, j);
            }
        }
    }
    let mut by_root: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (k, (img_idx, _)) in flat.iter().enumerate() {
        let r = uf.find(k);
        by_root.entry(r).or_default().insert(*img_idx);
    }
    by_root
        .into_values()
        .map(|s| s.into_iter().collect())
        .collect()
}

/// 余弦相似度 = 点积 / (‖a‖·‖b‖)。零向量或非有限输入返 0.0（视为不相似）。
pub(crate) fn cosine_similarity(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !dot.is_finite() || norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Path-compression Union-Find。父指针扁平化让 find 摊还接近 O(α(n))。
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        if self.parent[i] == i {
            return i;
        }
        let r = self.find(self.parent[i]);
        self.parent[i] = r;
        r
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

#[cfg(test)]
#[path = "identity_cluster_tests.rs"]
mod tests;

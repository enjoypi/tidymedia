//! 真实 bge-small-zh embedding 加载 + 推理（需用户备模型 + tokenizer 文件）。
//!
//! **覆盖率**：本文件走 `--ignore-filename-regex='adapters/(ocr|face|classify)/tract_[a-z_]+\.rs'`
//! 排除——加载真实 ONNX/tokenizer 在 CI 不可触发（模型不入 git），stub 测试在
//! `tract_embed::tests` 通过 `RawEmbedder` trait 注入覆盖 normalize/cosine/argmax。
//!
//! 加载序列（spike 验证）：`into_typed()` → `set_symbols(batch_size=1,
//! sequence_length=SEQ)` → `into_optimized()` → `into_runnable()`。BERT 图内
//! position-embedding Slice 引用 symbolic `sequence_length`，tract run 不会从
//! 输入形状自动绑定 symbol，必须显式 concretize；固定 shape 后可 optimize。

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use tract_onnx::prelude::*;

use super::tract_embed::RawEmbedder;

/// 固定输入序列长度：encode 后 truncate/pad 到此值。bge-small-zh `max_position=512`，
/// 分类只吃文档前几百 token，256 平衡精度与推理耗时。
const SEQ: usize = 256;

/// tract runnable（`run` 的接收者是 `self: &Arc<Self>`，必须 Arc 包装）。
type EmbedModel = Arc<TypedRunnableModel>;

struct TractRawEmbedder {
    model: EmbedModel,
    tokenizer: tokenizers::Tokenizer,
}

impl RawEmbedder for TractRawEmbedder {
    fn embed(&self, text: &str) -> io::Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| io::Error::other(format!("tokenize: {e}")))?;
        let ids = pad_i64(enc.get_ids());
        let mask = pad_i64(enc.get_attention_mask());
        let types = pad_i64(enc.get_type_ids());
        let outputs = self
            .model
            .run(tvec!(to_tvalue(ids), to_tvalue(mask), to_tvalue(types)))
            .map_err(|e| io::Error::other(format!("tract embed run failed: {e}")))?;
        let first = outputs
            .into_iter()
            .next()
            .expect("模型 run 成功必有输出 tensor");
        // CLS pooling：[1, seq, hidden] 取 [0, 0, :]（bge 官方口径，非 mean-pooling）。
        let hidden = *first.shape().last().expect("tensor shape 恒非空");
        let cast = first.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
        let view = cast.view();
        let slice = view
            .as_slice::<f32>()
            .expect("cast 成功后 view 恒 contiguous");
        // CLS 向量长度 = hidden 维；`into_runnable` 已固化输出 shape
        // `[1, seq, hidden]`，`slice.len() = seq*hidden ≥ hidden` 恒成立。
        Ok(slice[..hidden].to_vec())
    }
}

/// truncate/pad token 序列到固定 [`SEQ`]（pad token id = 0，attention mask 同步为 0）。
fn pad_i64(v: &[u32]) -> Vec<i64> {
    let mut out: Vec<i64> = v.iter().take(SEQ).map(|&x| i64::from(x)).collect();
    out.resize(SEQ, 0);
    out
}

fn to_tvalue(v: Vec<i64>) -> TValue {
    tract_ndarray::Array2::from_shape_vec((1, SEQ), v)
        .expect("internal: padded vec sized exactly 1*SEQ")
        .into_tensor()
        .into_tvalue()
}

/// 读 ONNX + tokenizer.json → 固定 shape 的 runnable embedding 推理器。
///
/// # Errors
///
/// 文件不存在、ONNX/tokenizer 解析失败、symbol 绑定或 runnable 装配失败时返回 `Err`。
pub(crate) fn load_raw_embedder(
    model_path: &str,
    tokenizer_path: &str,
) -> io::Result<Box<dyn RawEmbedder>> {
    let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
        .map_err(|e| io::Error::other(format!("load tokenizer {tokenizer_path}: {e}")))?;
    let typed = tract_onnx::onnx()
        .model_for_path(model_path)
        .map_err(|e| io::Error::other(format!("load embed ONNX {model_path}: {e}")))?
        .into_typed()
        .map_err(|e| io::Error::other(format!("type embed model: {e}")))?;
    let seq = i64::try_from(SEQ).expect("internal: SEQ fits i64");
    let subs: HashMap<Symbol, TDim> = [
        (typed.sym("batch_size"), TDim::Val(1)),
        (typed.sym("sequence_length"), TDim::Val(seq)),
    ]
    .into_iter()
    .collect();
    let model = typed
        .set_symbols(&subs)
        .map_err(|e| io::Error::other(format!("bind embed model symbols: {e}")))?
        .into_optimized()
        .map_err(|e| io::Error::other(format!("optimize embed model: {e}")))?
        .into_runnable()
        .map_err(|e| io::Error::other(format!("make embed model runnable: {e}")))?;
    Ok(Box::new(TractRawEmbedder { model, tokenizer }))
}

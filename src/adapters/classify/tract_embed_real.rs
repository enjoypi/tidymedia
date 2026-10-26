use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use tract_onnx::prelude::*;

use super::tract_embed::{RawEmbedder, SEQ, pad_i64};
use crate::usecases::config::ClassifyConfig;

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
        let hidden = *first.shape().last().expect("tensor shape 恒非空");
        let cast = first.cast_to::<f32>().expect("numeric→f32 cast 恒成功");
        let slice = cast
            .view()
            .as_slice::<f32>()
            .expect("cast 成功后 view 恒 contiguous");
        Ok(slice[..hidden].to_vec())
    }
}

fn to_tvalue(v: Vec<i64>) -> TValue {
    tract_ndarray::Array2::from_shape_vec((1, SEQ), v)
        .expect("internal: padded vec sized exactly 1*SEQ")
        .into_tensor()
        .into_tvalue()
}

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

pub(crate) fn load_classifier(cfg: &ClassifyConfig) -> io::Result<Box<dyn RawEmbedder>> {
    load_raw_embedder(&cfg.embed_model_path, &cfg.tokenizer_path)
}

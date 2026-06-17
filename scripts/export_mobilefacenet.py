#!/usr/bin/env python3
"""下载 foamliu/MobileFaceNet 的 mobilefacenet_scripted.pt，导出 1×3×112×112 静态 ONNX。

ArcFace 标准 preprocess：112×112 RGB、(v-127.5)/127.5 归一化、NCHW。tract 适配器
src/adapters/face/tract_mobilefacenet*.rs 与此 layout 对齐。

依赖：torch 2.x（uv run --with torch 临时拉取）。
"""
from __future__ import annotations

import sys
import urllib.request
from pathlib import Path

import torch

PT_URL = "https://github.com/foamliu/MobileFaceNet/raw/master/pretrained_model/mobilefacenet_scripted.pt"
MODELS = Path(__file__).resolve().parent.parent / "models"
PT_PATH = MODELS / "mobilefacenet.pt"
ONNX_PATH = MODELS / "mobilefacenet.onnx"


def main() -> int:
    MODELS.mkdir(parents=True, exist_ok=True)
    if not PT_PATH.exists():
        print(f"    下载 {PT_URL}")
        urllib.request.urlretrieve(PT_URL, PT_PATH)
    model = torch.jit.load(str(PT_PATH), map_location="cpu").eval()
    dummy = torch.randn(1, 3, 112, 112)
    # torch 2.x 默认 dynamo 导出不支持 ScriptModule，强制 dynamo=False 走旧导出器
    torch.onnx.export(
        model,
        dummy,
        str(ONNX_PATH),
        input_names=["input"],
        output_names=["embedding"],
        opset_version=17,
        dynamic_axes=None,
        dynamo=False,
    )
    print(f"    导出 {ONNX_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

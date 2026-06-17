#!/usr/bin/env python3
"""onnxsim 化简 + 静态输入固化（项目内 4 模型路径硬编码，避免 CLI 字段心算）。

避坑：SCRFD/EyeState 等模型导出时输入 dim 是动态（`['batch', 3, 'h', 'w']` 等），
tract 加载效率低；`overwrite_input_shapes` 在化简同时固化输入 shape。
"""
from __future__ import annotations

import sys
from pathlib import Path

import onnx
from onnxsim import simplify

# (相对路径, 输入名 → 固定 shape)
MODELS = [
    ("models/scrfd_10g_bnkps.onnx", {"input.1": [1, 3, 640, 640]}),
    ("models/eyestate_yolov8.onnx", {"images": [1, 3, 640, 640]}),
    ("models/face_mesh_192x192.onnx", {}),  # 已是静态 [1,3,192,192]
    ("models/mobilefacenet.onnx", {}),       # 导出时已 [1,3,112,112]
]


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    for rel, overwrite in MODELS:
        fpath = repo_root / rel
        if not fpath.exists():
            print(f"SKIP {fpath.name}: not found")
            continue
        model = onnx.load(str(fpath))
        sim, ok = (
            simplify(model, overwrite_input_shapes=overwrite)
            if overwrite
            else simplify(model)
        )
        if not ok:
            print(f"ERROR {fpath.name}: simplify returned ok=False")
            return 1
        onnx.save(sim, str(fpath))
        print(f"    简化 {fpath.name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

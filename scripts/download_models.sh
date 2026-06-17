#!/usr/bin/env bash
# 拉 cull 子命令所需 4 个 ONNX 模型到 models/ 目录。
# 模型由 git-lfs 跟踪：clone 后只需 `git lfs pull` 即获全部；本脚本仅在初次
# 装配（或刷新模型版本）时跑。所有 ONNX 跑 onnxsim 静态化算子，提升 tract
# 推理速度并降低解析出错概率。
#
# 模型来源与对应 tract 适配器：
#   SCRFD-10G   → src/adapters/face/tract_scrfd*.rs        (face 检测)
#   EyeState    → src/adapters/face/tract_eyestate*.rs     (YOLOv8 检测头)
#   FaceMesh    → src/adapters/face/tract_facemesh*.rs     (468 点 landmark)
#   MobileFaceNet → src/adapters/face/tract_mobilefacenet*.rs (512 维 embedding)
#
# 用法：bash scripts/download_models.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODELS_DIR="$REPO_ROOT/models"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$MODELS_DIR"

echo "==> [1/4] SCRFD-10G (antelopev2)"
curl --silent --show-error --fail --location \
    -o "$MODELS_DIR/scrfd_10g_bnkps.onnx" \
    "https://huggingface.co/DIAMONIK7777/antelopev2/resolve/main/scrfd_10g_bnkps.onnx"

echo "==> [2/4] EyeState YOLOv8 (MichalMlodawski/open-closed-eye-detection)"
curl --silent --show-error --fail --location \
    -o "$MODELS_DIR/eyestate_yolov8.onnx" \
    "https://huggingface.co/MichalMlodawski/open-closed-eye-detection/resolve/main/model.onnx"

echo "==> [3/4] FaceMesh (PINTO_model_zoo 032_FaceMesh)"
# PINTO 的 tarball 是嵌套两层：外层按 framework 分目录，每目录内 resources.tar.gz
# 才是真正模型；选 20_new_onnx_postprocess_N-batch 子集（含 1-batch 静态 onnx）。
curl --silent --show-error --fail --location \
    -o "$TMP_DIR/032_FaceMesh.tar.gz" \
    "https://s3.ap-northeast-2.wasabisys.com/pinto-model-zoo/032_FaceMesh/032_FaceMesh.tar.gz"
tar -xzf "$TMP_DIR/032_FaceMesh.tar.gz" -C "$TMP_DIR"
INNER_TGZ="$TMP_DIR/032_FaceMesh/20_new_onnx_postprocess_N-batch/resources_post.tar.gz"
if [[ ! -f "$INNER_TGZ" ]]; then
    echo "ERROR: PINTO 内层 tarball 缺失：$INNER_TGZ" >&2
    exit 1
fi
mkdir -p "$TMP_DIR/facemesh_inner"
tar -xzf "$INNER_TGZ" -C "$TMP_DIR/facemesh_inner"
FACEMESH_SRC="$TMP_DIR/facemesh_inner/face_mesh_192x192.onnx"
if [[ ! -f "$FACEMESH_SRC" ]]; then
    echo "ERROR: PINTO 内层缺 face_mesh_192x192.onnx" >&2
    find "$TMP_DIR/facemesh_inner" -name '*.onnx' >&2
    exit 1
fi
cp "$FACEMESH_SRC" "$MODELS_DIR/face_mesh_192x192.onnx"
echo "    抽取自 $FACEMESH_SRC"

echo "==> [4/4] MobileFaceNet (foamliu/MobileFaceNet pt → onnx)"
uv run --no-project --quiet --with torch --with numpy --with onnxscript \
    "$REPO_ROOT/scripts/export_mobilefacenet.py"

echo
echo "==> onnxsim 静态化算子简化"
uv run --no-project --quiet --with onnx --with onnxsim --with onnxruntime \
    "$REPO_ROOT/scripts/simplify_onnx.py"

echo
echo "==> 模型清单"
ls -lh "$MODELS_DIR"/*.onnx

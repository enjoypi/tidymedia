# TODO

## 真实样本替换合成 fixture

以下增补当前用合成 fixture 验证字节级解析路径；累积真实样本后替换 `tests/data/` 内合成 fixture（fallback 逻辑不必动）：

- PNG `eXIf` chunk 自解析（`entities/png.rs` + `entities/exif/image_png.rs`）：合成 fixture `tests/data/sample-png-exif.png`。累积真实样本（如 `LKIT3149.png` DTO=2017:02 类）后替换。
- Canon EOS 7D `MakerNotes` JPEG APP1 fallback（`entities/exif/image_jpeg.rs`）：合成 fixture `tests/data/sample-jpeg-app1-broken.jpg` 模拟 nom-exif `parse_exif` 失败但裸 IFD 可读。累积真实 Canon 7D 样本（exiftool 报 `Adjusted MakerNotes base by -126`）后替换。

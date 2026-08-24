// exiftool（Windows Perl）对含中文的入口路径按 ANSI(GBK) 输出文件名字节，
// 不保证 UTF-8。统一规范化为 UTF-8：先按 UTF-8 严格解码（macOS/正常路径直接
// 通过），遇非法 UTF-8 才按 GBK 解码兜底。GBK 解码几乎永不失败，故仅以
// UTF-8 失败为切换条件——正常 UTF-8 输入零开销。
export function decodeExifText(bytes: Uint8Array): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return new TextDecoder("gbk").decode(bytes);
  }
}

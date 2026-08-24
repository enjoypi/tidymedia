// 极简 YAML 子集解析：flat `key: value` + `key: >-` 多行折叠 + `#` 注释。
// 仅服务 skill 内 config.yaml；嵌套/数组/锚点等复杂 YAML 不在支持范围。
// 保留字面 `\t` 等反斜杠序列不解释，由消费方自行替换。
export function parseYaml(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  const lines = text.split(/\r?\n/);
  let blockKey: string | null = null;
  const block: string[] = [];

  for (const line of lines) {
    if (blockKey !== null) {
      const t = line.trim();
      // 空行（段落分隔）、注释、缩进行均属块内；其余非缩进行结束块。
      const isBlank = t === "";
      const isComment = t.startsWith("#");
      const isIndented = !isBlank && /^\s/.test(line);
      if (isBlank || isComment || isIndented) {
        if (!isBlank && !isComment) {
          block.push(t);
        }
        continue;
      }
      out[blockKey] = block.join(" ");
      blockKey = null;
      block.length = 0;
    }
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) {
      continue;
    }
    const idx = line.indexOf(":");
    if (idx < 0) {
      continue;
    }
    const key = line.slice(0, idx).trim();
    let value = line.slice(idx + 1).trim();
    if (value === ">-") {
      blockKey = key;
      continue;
    }
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    out[key] = value;
  }
  if (blockKey !== null) {
    out[blockKey] = block.join(" ");
  }
  return out;
}

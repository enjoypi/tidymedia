import { mkdirSync, statSync } from "node:fs";
import { resolve } from "node:path";

const MODELS_DIR = resolve(import.meta.dir, "..", "models");
const HF = process.env.HF_ENDPOINT || "https://huggingface.co";
const REPO = "Xenova/bge-small-zh-v1.5/resolve/main";
const FILES: [string, string][] = [
  ["bge_small_zh_v15_int8.onnx", "onnx/model_quantized.onnx"],
  ["bge_small_zh_v15_tokenizer.json", "tokenizer.json"],
];

async function download(name: string, remote: string): Promise<void> {
  const url = `${HF}/${REPO}/${remote}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const target = resolve(MODELS_DIR, name);
  await Bun.write(target, response);
  console.log(`${name}\t${statSync(target).size} bytes`);
}

mkdirSync(MODELS_DIR, { recursive: true });
console.log("==> bge-small-zh-v1.5 int8 + tokenizer");
for (const [name, remote] of FILES) await download(name, remote);

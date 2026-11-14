import { appendFileSync, constants, copyFileSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { extname, join, relative, resolve } from "node:path";
import { deflateSync } from "node:zlib";
import { pathUnder, realish } from "./perf-collect.ts";

const MUTATE_MAX_BYTES = 50 << 20;
const PHOTO_SEED_EXTS = [".jpg", ".png", ".mp4"];
const PHOTOS_PER_DIR = 500;
const TAIL_LENGTHS = [64, 64, 256, 1024, 4096];
const DUPLICATE_EVERY = 10;

export function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomBytes(rand: () => number, len: number): Buffer {
  const buf = Buffer.alloc(len);
  for (let i = 0; i < len; i++) buf[i] = Math.floor(rand() * 256);
  return buf;
}

export function tailFor(ext: string, payload: Buffer): Buffer {
  if (ext !== ".mp4") return payload;
  const header = Buffer.alloc(8);
  header.writeUInt32BE(payload.length + 8, 0);
  header.write("free", 4, "latin1");
  return Buffer.concat([header, payload]);
}

export type PhotoPlan = { name: string; seed: number; tail: number; duplicateOf: number | null };

export function planPhotos(count: number, seedExts: string[], rand: () => number): PhotoPlan[] {
  const plans: PhotoPlan[] = [];
  for (let i = 0; i < count; i++) {
    const dir = String(Math.floor(i / PHOTOS_PER_DIR)).padStart(4, "0");
    const duplicateOf = i > 0 && i % DUPLICATE_EVERY === 0 ? Math.floor(rand() * i) : null;
    const source = duplicateOf === null ? null : plans[duplicateOf];
    const seed = source ? source.seed : Math.floor(rand() * seedExts.length);
    const tail = source ? source.tail : TAIL_LENGTHS[Math.floor(rand() * TAIL_LENGTHS.length)];
    plans.push({ name: `${dir}/p${String(i).padStart(6, "0")}${seedExts[seed]}`, seed, tail, duplicateOf });
  }
  return plans;
}

function listFiles(dir: string): string[] {
  const files: string[] = [];
  const pending = [dir];
  while (pending.length) {
    const current = pending.pop() as string;
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const path = join(current, entry.name);
      if (entry.isDirectory()) pending.push(path);
      if (entry.isFile()) files.push(path);
    }
  }
  return files.sort();
}

function writeAt(root: string, name: string, bytes: Buffer): void {
  const path = join(root, name);
  mkdirSync(resolve(path, ".."), { recursive: true });
  writeFileSync(path, bytes);
}

function clone(from: string, to: string, mutateTail: boolean): number {
  const files = listFiles(from);
  for (const src of files) {
    const dst = join(to, relative(from, src));
    mkdirSync(resolve(dst, ".."), { recursive: true });
    copyFileSync(src, dst, constants.COPYFILE_FICLONE);
    if (mutateTail && statSync(dst).size <= MUTATE_MAX_BYTES) appendFileSync(dst, "tidymedia-perf-tail");
  }
  return files.length;
}

function photos(seedDir: string, to: string, count: number): number {
  const seeds = listFiles(seedDir).filter((path) => PHOTO_SEED_EXTS.includes(extname(path).toLowerCase()));
  const bodies = seeds.map((path) => readFileSync(path));
  const exts = seeds.map((path) => extname(path).toLowerCase());
  const rand = mulberry32(count);
  const written: Buffer[] = [];
  for (const plan of planPhotos(count, exts, rand)) {
    const bytes =
      plan.duplicateOf === null
        ? Buffer.concat([bodies[plan.seed], tailFor(exts[plan.seed], randomBytes(rand, plan.tail))])
        : written[plan.duplicateOf];
    written.push(bytes);
    writeAt(to, plan.name, bytes);
  }
  return count;
}

function chunk(type: string, data: Buffer): Buffer {
  const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
  const frame = Buffer.alloc(body.length + 8);
  frame.writeUInt32BE(data.length, 0);
  body.copy(frame, 4);
  frame.writeUInt32BE(Bun.hash.crc32(body), body.length + 4);
  return frame;
}

export function png(width: number, height: number, colorType: number, bitDepth: number, fill: (raw: Buffer) => void): Buffer {
  const channels = colorType === 2 ? 3 : 1;
  const stride = 1 + width * channels * (bitDepth / 8);
  const raw = Buffer.alloc(stride * height);
  fill(raw);
  for (let y = 0; y < height; y++) raw[y * stride] = 0;
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = bitDepth;
  header[9] = colorType;
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  return Buffer.concat([signature, chunk("IHDR", header), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

function sips(args: string[]): void {
  const proc = spawnSync("/usr/bin/sips", args, { stdio: ["ignore", "ignore", "inherit"] });
  if (proc.status !== 0) throw new Error(`sips ${args.join(" ")} failed`);
}

function stress(to: string): number {
  const src = join(to, "src");
  const lib = join(to, "lib");
  const pair = (name: string, width: number, height: number, colorType: number, bitDepth: number) => {
    writeAt(src, name, png(width, height, colorType, bitDepth, () => {}));
    writeAt(lib, name, png(width, height, colorType, bitDepth, (raw) => raw.fill(1, raw.length >> 1)));
  };
  pair("huge-gray.png", 20000, 20000, 0, 8);
  pair("huge-rgb16.png", 8000, 8000, 2, 16);
  if (process.platform !== "darwin") return 4;
  const rand = mulberry32(24);
  const noise = join(to, "noise.png");
  writeFileSync(noise, png(6000, 4000, 2, 8, (raw) => randomBytes(rand, raw.length).copy(raw)));
  mkdirSync(src, { recursive: true });
  sips(["-s", "format", "jpeg", noise, "--out", join(src, "noise-24mp.jpg")]);
  sips(["-r", "90", join(src, "noise-24mp.jpg"), "--out", join(lib, "noise-24mp.jpg")]);
  return 6;
}

export function validate(mode: string | undefined, opts: Record<string, string>): string | null {
  if (!mode || !["clone", "photos", "stress"].includes(mode)) return "mode";
  if (!opts.to) return "--to";
  if (mode !== "stress" && !opts.from) return "--from";
  if (mode === "photos" && !/^[1-9]\d*$/.test(opts.count ?? "")) return "--count";
  return null;
}

function main(): void {
  const [mode, ...rest] = process.argv.slice(2);
  const opts: Record<string, string> = {};
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === "--mutate-tail") opts["mutate-tail"] = "1";
    else if (rest[i].startsWith("--")) opts[rest[i].slice(2)] = rest[++i] ?? "";
  }
  const invalid = validate(mode, opts);
  if (invalid) {
    console.error(`error: missing or invalid ${invalid}`);
    console.error("用法：bun scripts/perf-dataset.ts <clone --from <dir> [--mutate-tail]|photos --from <种子目录> --count <n>|stress> --to <dir>");
    process.exit(2);
  }
  const to = realish(opts.to);
  if (opts.from && pathUnder(to, realish(opts.from))) {
    console.error(`error: ${to} is inside --from`);
    process.exit(2);
  }
  mkdirSync(to, { recursive: true });
  const written =
    mode === "clone"
      ? clone(realish(opts.from), to, opts["mutate-tail"] === "1")
      : mode === "photos"
        ? photos(realish(opts.from), to, parseInt(opts.count, 10))
        : stress(to);
  console.error(`[perf-dataset] ${mode}: wrote ${written} files to ${to}`);
}

if (import.meta.main) main();

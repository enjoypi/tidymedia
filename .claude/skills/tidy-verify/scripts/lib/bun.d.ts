// 最小 Bun API 类型声明（自包含，避免 @types/bun 外部依赖）。
// 仅覆盖本 skill 脚本用到的 API 面；新增用法需同步扩展。

interface BunFile {
  readonly size: number;
  text(): Promise<string>;
  arrayBuffer(): Promise<ArrayBuffer>;
  exists(): Promise<boolean>;
}

interface BunSubprocess {
  readonly pid: number;
  readonly stdout: ReadableStream<Uint8Array>;
  readonly stderr: ReadableStream<Uint8Array>;
  readonly exited: Promise<number>;
}

interface BunSpawnOptions {
  stdout?: "pipe" | "inherit" | "ignore" | BunFile;
  stderr?: "pipe" | "inherit" | "ignore" | BunFile;
}

interface BunSyncSubprocess {
  readonly stdout: Uint8Array;
  readonly exitCode: number;
}

interface BunGlobScanOptions {
  cwd?: string;
  onlyFiles?: boolean;
  dot?: boolean;
}

declare namespace Bun {
  function file(path: string): BunFile;
  function write(path: string, data: string | Uint8Array): Promise<number>;
  function spawn(cmd: string[], opts?: BunSpawnOptions): BunSubprocess;
  function spawnSync(cmd: string[], opts?: BunSpawnOptions): BunSyncSubprocess;
  function which(name: string): string | null;
  class Glob {
    constructor(pattern: string);
    scan(opts?: BunGlobScanOptions): AsyncIterable<string>;
  }
}

declare module "node:fs" {
  export function mkdirSync(path: string, opts?: { recursive?: boolean }): void;
  export function mkdtempSync(prefix: string): string;
  export function appendFileSync(path: string, data: string): void;
  export function writeFileSync(path: string, data: string): void;
  export function existsSync(path: string): boolean;
  export function rmSync(path: string, opts?: { recursive?: boolean; force?: boolean }): void;
}

declare module "node:path" {
  export function dirname(path: string): string;
}

declare module "node:os" {
  export function tmpdir(): string;
  export function freemem(): number;
}

declare namespace process {
  const argv: string[];
  const platform: string;
  const execPath: string;
  function exit(code?: number): never;
  function kill(pid: number, signal?: string): void;
}

declare module "bun:test" {
  export interface Expect {
    toBe(expected: unknown): void;
    toEqual(expected: unknown): void;
    toBeNull(): void;
    toBeTruthy(): void;
    toBeGreaterThan(n: number): void;
    toContain(expected: unknown): void;
    readonly not: Expect;
  }
  export function test(name: string, fn: () => void | Promise<void>): void;
  export function expect<T>(actual: T): Expect;
}

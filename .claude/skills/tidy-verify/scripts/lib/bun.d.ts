// 最小 Bun API 类型声明（自包含，避免 @types/bun 外部依赖）。
// 仅覆盖本 skill 脚本用到的 API 面；新增用法需同步扩展。

interface BunFile {
  readonly size: number;
  text(): Promise<string>;
  arrayBuffer(): Promise<ArrayBuffer>;
  exists(): Promise<boolean>;
}

interface BunSubprocess {
  readonly stdout: ReadableStream<Uint8Array>;
  readonly stderr: ReadableStream<Uint8Array>;
  readonly exited: Promise<number>;
}

interface BunSpawnOptions {}

declare namespace Bun {
  function file(path: string): BunFile;
  function write(path: string, data: string | Uint8Array): Promise<number>;
  function spawn(cmd: string[], opts?: BunSpawnOptions): BunSubprocess;
  function which(name: string): string | null;
}

declare namespace process {
  const argv: string[];
  const platform: string;
  function exit(code?: number): never;
}

declare module "bun:test" {
  export interface Expect {
    toBe(expected: unknown): void;
    toEqual(expected: unknown): void;
    toBeNull(): void;
    toBeTruthy(): void;
    toBeGreaterThan(n: number): void;
    toContain(expected: unknown): void;
  }
  export function test(name: string, fn: () => void | Promise<void>): void;
  export function expect<T>(actual: T): Expect;
}

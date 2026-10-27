/// <reference path="./bun.d.ts" />

import { appendFileSync, existsSync, writeFileSync } from "node:fs";

export function pickConcurrency(max: number, freeBytes: number, perProcBytes: number): number {
  return Math.max(1, Math.min(max, Math.floor(freeBytes / perProcBytes)));
}

export function recordPid(file: string, pid: number): void {
  appendFileSync(file, `${pid}\n`);
}

export async function reapStale(
  file: string,
  isTarget: (pid: number) => boolean,
  kill: (pid: number) => void,
): Promise<number[]> {
  if (!existsSync(file)) {
    return [];
  }
  const pids = (await Bun.file(file).text()).split("\n").filter((l) => l !== "").map(Number);
  const stale = pids.filter(isTarget);
  stale.forEach(kill);
  writeFileSync(file, "");
  return stale;
}

function stdoutOf(cmd: string[]): string {
  return new TextDecoder().decode(Bun.spawnSync(cmd, { stdout: "pipe", stderr: "ignore" }).stdout);
}

export function isImageRunning(pid: number, image: string): boolean {
  const out = process.platform === "win32"
    ? stdoutOf(["tasklist", "/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"])
    : stdoutOf(["ps", "-p", String(pid), "-o", "comm="]);
  return out.toLowerCase().includes(image.toLowerCase());
}

export function killTree(pid: number): void {
  if (process.platform === "win32") {
    Bun.spawnSync(["taskkill", "/F", "/T", "/PID", String(pid)], { stdout: "ignore", stderr: "ignore" });
  } else {
    process.kill(pid, "SIGKILL");
  }
}

export interface ExecResult {
  out: Uint8Array;
  err: Uint8Array;
  exit: number;
}

export type Runner = (cmd: string[]) => Promise<ExecResult>;

export async function spawnRunner(cmd: string[]): Promise<ExecResult> {
  const proc = Bun.spawn(cmd, { stdout: "pipe", stderr: "pipe" });
  const [out, err] = await Promise.all([
    new Response(proc.stdout).arrayBuffer(),
    new Response(proc.stderr).arrayBuffer(),
  ]);
  return { out: new Uint8Array(out), err: new Uint8Array(err), exit: await proc.exited };
}

export async function runPool<T, R>(
  items: readonly T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(items.length);
  let next = 0;
  let failed = false;
  let failure: unknown;
  async function lane(): Promise<void> {
    while (!failed && next < items.length) {
      const index = next;
      next += 1;
      try {
        results[index] = await fn(items[index] as T, index);
      } catch (e) {
        failed = true;
        failure = e;
      }
    }
  }
  const lanes = Math.min(Math.max(limit, 1), items.length);
  await Promise.all(Array.from({ length: lanes }, lane));
  if (failed) {
    throw failure;
  }
  return results;
}

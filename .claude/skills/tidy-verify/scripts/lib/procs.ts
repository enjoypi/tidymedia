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

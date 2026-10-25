/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { isImageRunning, killTree, pickConcurrency, reapStale, recordPid } from "../scripts/lib/procs.ts";

const MB = 1024 * 1024;

test("pickConcurrency 按空闲内存折算并以配置为上限", () => {
  expect(pickConcurrency(8, 16000 * MB, 300 * MB)).toBe(8);
  expect(pickConcurrency(8, 1300 * MB, 300 * MB)).toBe(4);
});

test("pickConcurrency 内存不足一个进程时仍保底 1", () => {
  expect(pickConcurrency(8, 100 * MB, 300 * MB)).toBe(1);
});

function withDir(fn: (dir: string) => Promise<void>): Promise<void> {
  const dir = mkdtempSync(`${tmpdir()}/tv-procs-`).replaceAll("\\", "/");
  return fn(dir).finally(() => rmSync(dir, { recursive: true, force: true }));
}

test("reapStale 无 pid 文件返回空且不调用 kill", () =>
  withDir(async (dir) => {
    const killed: number[] = [];
    const r = await reapStale(`${dir}/p.txt`, () => true, (pid) => killed.push(pid));
    expect(r).toEqual([]);
    expect(killed).toEqual([]);
  }));

test("reapStale 只结束仍是 exiftool 的残留 pid 并清空文件", () =>
  withDir(async (dir) => {
    const file = `${dir}/p.txt`;
    recordPid(file, 11);
    recordPid(file, 22);
    recordPid(file, 33);
    const killed: number[] = [];
    const r = await reapStale(file, (pid) => pid !== 22, (pid) => killed.push(pid));
    expect(r).toEqual([11, 33]);
    expect(killed).toEqual([11, 33]);
    expect(await Bun.file(file).text()).toBe("");
  }));

test("isImageRunning 按映像名识别进程", async () => {
  const proc = Bun.spawn([process.execPath, "-e", "setTimeout(() => {}, 30000)"], { stdout: "ignore", stderr: "ignore" });
  try {
    const name = process.execPath.replaceAll("\\", "/").split("/").pop() ?? "";
    expect(isImageRunning(proc.pid, name)).toBe(true);
    expect(isImageRunning(proc.pid, "exiftool")).toBe(false);
  } finally {
    killTree(proc.pid);
  }
  expect(await proc.exited).not.toBe(0);
});

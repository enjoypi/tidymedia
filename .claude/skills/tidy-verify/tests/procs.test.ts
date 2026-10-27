/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { isImageRunning, killTree, pickConcurrency, reapStale, recordPid, runPool } from "../scripts/lib/procs.ts";

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

test("runPool 空输入直接返回且不调用 fn", async () => {
  let called = 0;
  const r = await runPool([], 3, async () => {
    called += 1;
    return 1;
  });
  expect(r).toEqual([]);
  expect(called).toBe(0);
});

test("runPool 并发不超过上限且结果顺序与输入一致", async () => {
  let active = 0;
  let maxActive = 0;
  const items = Array.from({ length: 10 }, (_, i) => i);
  const r = await runPool(items, 3, async (n, i) => {
    expect(i).toBe(n);
    active += 1;
    maxActive = Math.max(maxActive, active);
    await new Promise((res) => setTimeout(res, (10 - n) * 4));
    active -= 1;
    return n * 2;
  });
  expect(maxActive).toBeLessThanOrEqual(3);
  expect(maxActive).toBeGreaterThan(1);
  expect(r).toEqual(items.map((n) => n * 2));
});

test("runPool 失败后停止派发新任务并等待在途任务结束", async () => {
  const started: number[] = [];
  const finished: number[] = [];
  const boom = new Error("boom");
  let caught: unknown = null;
  try {
    await runPool([0, 1, 2, 3, 4], 2, async (n) => {
      started.push(n);
      if (n === 1) {
        throw boom;
      }
      await new Promise((res) => setTimeout(res, 30));
      finished.push(n);
      return n;
    });
  } catch (e) {
    caught = e;
  }
  expect(caught).toBe(boom);
  expect([...started].sort((x, y) => x - y)).toEqual([0, 1]);
  expect(finished).toEqual([0]);
});

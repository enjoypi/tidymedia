/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { startVerify, verifyCommand, verifyExitCode } from "../scripts/lib/verify.ts";

test("verifyCommand 有 tsv 时注入 --exif-tsv", () => {
  expect(verifyCommand("tm", "S", "O", "W/exif.tsv", "W/verify.json")).toEqual([
    "tm", "--log-level=debug", "verify", "S", "-o", "O",
    "--exif-tsv", "W/exif.tsv", "--report", "W/verify.json",
  ]);
});

test("verifyCommand 无 tsv 时省略 --exif-tsv", () => {
  expect(verifyCommand("tm", "S", "O", null, "W/verify.json")).toEqual([
    "tm", "--log-level=debug", "verify", "S", "-o", "O", "--report", "W/verify.json",
  ]);
});

test("startVerify 发送 stdin 并收集输出", async () => {
  const running = startVerify([
    process.execPath,
    "-e",
    "await Bun.write(Bun.stdout, await Bun.stdin.text())",
  ]);
  expect(await running.send(new TextEncoder().encode("tsv-data"))).toBe(true);
  const result = await running.finish();
  expect(result.exit).toBe(0);
  expect(new TextDecoder().decode(result.out)).toBe("tsv-data");
});

test("startVerify 目标提前退出时发送失败", async () => {
  const running = startVerify([process.execPath, "-e", "process.exit(7)"]);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(await running.send(new TextEncoder().encode("tsv-data"))).toBe(false);
  expect((await running.finish()).exit).toBe(7);
});

test("verifyExitCode 不允许未送达输入伪装成功", () => {
  expect(verifyExitCode(0, true)).toBe(0);
  expect(verifyExitCode(0, false)).toBe(1);
  expect(verifyExitCode(7, false)).toBe(7);
});

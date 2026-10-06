/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { verifyCommand } from "../scripts/lib/verify.ts";

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

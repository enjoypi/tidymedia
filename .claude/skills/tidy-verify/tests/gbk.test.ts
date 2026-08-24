/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { decodeExifText } from "../scripts/lib/gbk.ts";

test("UTF-8 输入原样通过", () => {
  const bytes = new TextEncoder().encode("中文路径/D:/文件夹/IMG_0001.jpg");
  expect(decodeExifText(bytes)).toBe("中文路径/D:/文件夹/IMG_0001.jpg");
});

test("GBK 字节按 GBK 解码（Windows 中文路径场景）", () => {
  // "你好" 的 GBK 编码：你=C4E3 好=BAC3
  const gbk = new Uint8Array([0xc4, 0xe3, 0xba, 0xc3]);
  expect(decodeExifText(gbk)).toBe("你好");
});

test("含非法 UTF-8 字节触发 GBK 兜底不抛异常", () => {
  const bytes = new Uint8Array([0x80, 0x81, 0x82]);
  const r = decodeExifText(bytes);
  expect(typeof r).toBe("string");
});

test("ASCII 字节保持原样", () => {
  const bytes = new TextEncoder().encode("D:/Pics/2010/IMG_1.JPG\t2024:05:01 10:00:00");
  expect(decodeExifText(bytes)).toBe("D:/Pics/2010/IMG_1.JPG\t2024:05:01 10:00:00");
});

test("空输入返回空串", () => {
  expect(decodeExifText(new Uint8Array(0))).toBe("");
});

/// <reference path="../scripts/lib/bun.d.ts" />

import { expect, test } from "bun:test";
import { parseYaml } from "../scripts/lib/yaml.ts";

test("解析 flat key value", () => {
  const y = parseYaml("a: 1\nb: hello\n");
  expect(y.a).toBe("1");
  expect(y.b).toBe("hello");
});

test(">- 多行折叠", () => {
  const y = parseYaml("p: >-\n  line1\n  line2\n");
  expect(y.p).toBe("line1 line2");
});

test("跳过注释与空行", () => {
  const y = parseYaml("# 注释\n\na: x\n");
  expect(y.a).toBe("x");
});

test("剥单双引号", () => {
  const y = parseYaml('a: "hello"\nb: \'world\'\n');
  expect(y.a).toBe("hello");
  expect(y.b).toBe("world");
});

test("多行块后跟新键", () => {
  const y = parseYaml("p: >-\n  l1\n  l2\nnext: v\n");
  expect(y.p).toBe("l1 l2");
  expect(y.next).toBe("v");
});

test("块内注释与空行不进入折叠内容", () => {
  const y = parseYaml("p: >-\n  l1\n\n  # 注释\n  l2\nk: v\n");
  expect(y.p).toBe("l1 l2");
  expect(y.k).toBe("v");
});

test("CRLF 行尾", () => {
  const y = parseYaml("a: 1\r\nb: 2\r\n");
  expect(y.a).toBe("1");
  expect(y.b).toBe("2");
});

import { expect, test } from "bun:test";
import { inflateSync } from "node:zlib";
import { compareMetrics, stableReport, verdict } from "../../scripts/perf-compare.ts";
import { mulberry32, planPhotos, png, tailFor, validate } from "../../scripts/perf-dataset.ts";

test("verdict 仅在分布不重叠时判改进或回归", () => {
  expect(verdict([5, 6, 7], [1, 2, 4])).toBe("improved");
  expect(verdict([1, 2, 3], [4, 5, 6])).toBe("regressed");
  expect(verdict([1, 2, 5], [4, 5, 6])).toBe("noise");
  expect(verdict([], [1])).toBe("noise");
});

test("compareMetrics 对缺失指标判噪声", () => {
  const result = compareMetrics([{ elapsed_sec: 5 }, { elapsed_sec: 6 }], [{ elapsed_sec: 1 }, { elapsed_sec: 2 }]);
  expect(result.elapsed_sec).toBe("improved");
  expect(result.peak_private_kb).toBe("noise");
});

test("stableReport 忽略耗时与读字节并与键序无关", () => {
  const a = stableReport({ scanned: 3, duration_ms: 10, bytes_read: 99, copied: 1 });
  const b = stableReport({ copied: 1, bytes_read: 1, scanned: 3, duration_ms: 20 });
  expect(a).toBe(b);
  expect(a).not.toBe(stableReport({ scanned: 4, copied: 1 }));
});

test("mulberry32 同种子序列一致", () => {
  const a = mulberry32(7);
  const b = mulberry32(7);
  expect([a(), a(), a()]).toEqual([b(), b(), b()]);
  expect(mulberry32(8)()).not.toBe(mulberry32(7)());
});

test("tailFor 给 mp4 包 free box，其余原样", () => {
  const payload = Buffer.from("abcd");
  expect(tailFor(".jpg", payload)).toBe(payload);
  const box = tailFor(".mp4", payload);
  expect(box.readUInt32BE(0)).toBe(12);
  expect(box.subarray(4, 8).toString("latin1")).toBe("free");
});

test("planPhotos 分目录并按固定间隔安排完全重复", () => {
  const plans = planPhotos(1001, [".jpg", ".png"], mulberry32(1));
  expect(plans[0].name.startsWith("0000/p000000")).toBe(true);
  expect(plans[1000].name.startsWith("0002/")).toBe(true);
  const dup = plans[10];
  expect(dup.duplicateOf).not.toBeNull();
  expect(dup.seed).toBe(plans[dup.duplicateOf as number].seed);
  expect(dup.tail).toBe(plans[dup.duplicateOf as number].tail);
  expect(plans[11].duplicateOf).toBeNull();
});

test("png 生成合法头与可解压像素", () => {
  const bytes = png(3, 2, 2, 8, (raw) => raw.fill(9));
  expect(bytes.subarray(1, 4).toString("latin1")).toBe("PNG");
  expect(bytes.readUInt32BE(16)).toBe(3);
  expect(bytes.readUInt32BE(20)).toBe(2);
  const idatLen = bytes.readUInt32BE(33);
  const raw = inflateSync(bytes.subarray(41, 41 + idatLen));
  expect(raw.length).toBe(2 * (1 + 3 * 3));
  expect(raw[0]).toBe(0);
  expect(raw[1]).toBe(9);
});

test("validate 校验模式与必填参数", () => {
  expect(validate(undefined, {})).toBe("mode");
  expect(validate("clone", {})).toBe("--to");
  expect(validate("clone", { to: "T" })).toBe("--from");
  expect(validate("photos", { to: "T", from: "F", count: "0" })).toBe("--count");
  expect(validate("photos", { to: "T", from: "F", count: "20" })).toBeNull();
  expect(validate("stress", { to: "T" })).toBeNull();
});

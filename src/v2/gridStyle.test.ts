import { test } from "node:test";
import assert from "node:assert/strict";
import { justify, ratio, objectFit } from "./gridStyle.ts";

const r = (x: number) => x; // 비를 그대로 쓰는 항목

test("가로세로비 — 없거나 이상하면 정사각형", () => {
  assert.equal(ratio(null, null), 1);
  assert.equal(ratio(0, 100), 1);
  assert.equal(ratio(300, 200), 1.5);
});

test("파노라마 한 장이 줄을 통째로 먹지 않게 막는다", () => {
  assert.equal(ratio(10000, 500), 4, "20:1도 4:1로 제한");
  assert.equal(ratio(500, 10000), 0.25);
});

test("양쪽 맞춤 — 각 줄이 폭에 딱 맞는다", () => {
  const files = [1.5, 1.5, 1.5, 0.75, 1.5, 1.5];
  const rows = justify(files, r, 1000, 200, 10);
  // 마지막 줄은 늘리지 않으므로 뺀다
  for (const row of rows.slice(0, -1)) {
    const w =
      row.items.reduce((a, i) => a + i.width, 0) + 10 * (row.items.length - 1);
    assert.ok(Math.abs(w - 1000) < 0.5, `줄 폭 ${w}`);
  }
});

test("줄 높이는 목표 근처에 머문다", () => {
  const files = Array.from({ length: 40 }, () => 1.5);
  const rows = justify(files, r, 1200, 220, 10);
  for (const row of rows.slice(0, -1)) {
    assert.ok(row.height > 100 && row.height <= 240, `높이 ${row.height}`);
  }
});

test("마지막 줄은 늘리지 않는다 — 두 장이 화면을 가로지르면 흉하다", () => {
  const rows = justify([1.5, 1.5], r, 2000, 200, 10);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].height, 200, "목표 높이 그대로");
  const w = rows[0].items.reduce((a, i) => a + i.width, 0);
  assert.ok(w < 2000, "폭을 다 채우지 않는다");
});

test("모든 사진이 정확히 한 번씩 나온다", () => {
  const files = Array.from({ length: 37 }, (_, i) => 0.6 + (i % 5) * 0.4);
  const rows = justify(files, r, 900, 180, 8);
  assert.equal(
    rows.reduce((a, x) => a + x.items.length, 0),
    37,
  );
});

test("빈 목록과 잘못된 폭", () => {
  assert.deepEqual(justify([], r, 1000, 200, 10), []);
  assert.deepEqual(justify([1.5], r, 0, 200, 10), []);
});

test("채우기 방식이 클래스로 매핑된다", () => {
  assert.equal(objectFit("cover"), "object-cover");
  assert.equal(objectFit("contain"), "object-contain");
  assert.equal(objectFit("fill"), "object-fill");
});

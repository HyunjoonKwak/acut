import { test } from "node:test";
import assert from "node:assert/strict";
import { areaLabel, groupByArea, nextArea } from "./areaItems.ts";

test("영역 이름과 다음 칸", () => {
  assert.equal(areaLabel(0), "작업대");
  assert.equal(areaLabel(2), "공용");
  assert.equal(areaLabel(9), "기타");
  assert.equal(nextArea(0), 1);
  assert.equal(nextArea(1), 2);
  assert.equal(nextArea(2), null);
});

test("트리 행을 영역별로 묶고 작업대→내사진→공용 순으로 세운다", () => {
  const rows = [
    { path: "a", depth: 0, library_id: 10 },
    { path: "a/x", depth: 1, library_id: 10 },
    { path: "b", depth: 0, library_id: 20 },
    { path: "c", depth: 0, library_id: 30 },
    { path: "c/y", depth: 1, library_id: 30 },
  ];
  const area = (id: number) => ({ 10: 2, 20: 0, 30: 2 })[id] ?? 3;
  const g = groupByArea(rows, area);
  assert.deepEqual(
    g.map((x) => [x.area, x.rows.map((r) => r.path)]),
    [
      [0, ["b"]],
      [2, ["a", "a/x", "c", "c/y"]],
    ],
  );
});

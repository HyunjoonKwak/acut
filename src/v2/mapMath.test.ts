import { test } from "node:test";
import assert from "node:assert/strict";
import {
  bboxString,
  cellBounds,
  isFinest,
  parseBbox,
  precisionForZoom,
  safeMapBbox,
} from "./mapMath.ts";

test("멀리서는 굵게, 가까이서는 잘게", () => {
  assert.equal(precisionForZoom(3), 1);
  assert.equal(precisionForZoom(7), 0.1);
  assert.equal(precisionForZoom(11), 0.01);
  assert.equal(precisionForZoom(15), 0.001);
  assert.equal(isFinest(0.001), true);
  assert.equal(isFinest(0.01), false);
});

test("칸의 테두리는 격자에 맞고 꼬리가 없다", () => {
  assert.deepEqual(
    cellBounds(37.5512, 126.9882, 0.1),
    [37.5, 126.9, 37.6, 127],
  );
  assert.deepEqual(cellBounds(-33.87, 151.21, 1), [-34, 151, -33, 152]);
});

test("영역 글자는 Filter.bbox가 읽는 꼴로 오간다", () => {
  const b = bboxString([37.5, 126.9, 37.6, 127]);
  assert.equal(b, "37.5,126.9,37.6,127");
  assert.deepEqual(parseBbox(b), [37.5, 126.9, 37.6, 127]);
  assert.equal(parseBbox(null), null);
  assert.equal(parseBbox("a,b,c,d"), null);
  assert.equal(parseBbox("0,,1,2"), null);
  assert.equal(parseBbox("NaN,0,1,1"), null);
  assert.equal(parseBbox("-91,0,1,1"), null);
  assert.equal(parseBbox("1,1,0,2"), null);
});

test("세계 밖으로 감긴 화면은 안전하게 전체 경도로 묻는다", () => {
  assert.deepEqual(safeMapBbox(30, 120, 40, 140), [30, 120, 40, 140]);
  assert.deepEqual(safeMapBbox(-100, -220, 100, 220), [-90, -180, 90, 180]);
  assert.deepEqual(safeMapBbox(-10, 170, 10, 190), [-10, -180, 10, 180]);
});

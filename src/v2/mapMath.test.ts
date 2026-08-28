import { test } from "node:test";
import assert from "node:assert/strict";
import {
  bboxString,
  cellBounds,
  isFinest,
  parseBbox,
  precisionForZoom,
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
});

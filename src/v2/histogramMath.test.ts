import { test } from "node:test";
import assert from "node:assert/strict";
import { bins, polyline } from "./histogramMath.ts";

/** RGBA 픽셀 목록을 바이트 배열로 */
const px = (...list: [number, number, number][]) =>
  new Uint8ClampedArray(list.flatMap(([r, g, b]) => [r, g, b, 255]));

test("픽셀을 밝기 칸에 넣는다", () => {
  const h = bins(px([0, 128, 255], [0, 128, 255]));
  assert.equal(h.r[0], 2);
  assert.equal(h.g[128], 2);
  assert.equal(h.b[255], 2);
});

/** 하늘이 조금만 날아가도 255 칸이 치솟아 나머지가 바닥에 깔린다 */
test("봉우리는 양 끝 칸을 빼고 잰다", () => {
  const list: [number, number, number][] = [];
  for (let i = 0; i < 100; i++) list.push([255, 255, 255]);
  for (let i = 0; i < 5; i++) list.push([128, 128, 128]);
  const h = bins(px(...list));
  assert.equal(h.peak, 5, "가운데 봉우리가 기준이어야 한다");
});

test("날아간 픽셀의 비율을 센다", () => {
  const h = bins(px([255, 255, 255], [0, 0, 0], [128, 128, 128], [128, 0, 0]));
  assert.equal(h.clippedHighlight, 0.25);
  assert.equal(h.clippedShadow, 0.25);
});

/** 한 채널만 끝에 붙은 건 날아간 게 아니다 — 빨간 꽃잎이 그렇다 */
test("세 채널이 다 끝에 붙어야 날아간 것으로 본다", () => {
  const h = bins(px([255, 10, 10], [10, 10, 255]));
  assert.equal(h.clippedHighlight, 0);
  assert.equal(h.clippedShadow, 0);
});

test("빈 그림도 터지지 않는다", () => {
  const h = bins(new Uint8ClampedArray(0));
  assert.equal(h.peak, 1);
  assert.equal(h.clippedShadow, 0);
  assert.equal(h.clippedHighlight, 0);
});

test("폴리라인은 256점이고 화면 안에 들어온다", () => {
  const h = bins(px([128, 128, 128]));
  const pts = polyline(h.r, h.peak, 100, 40).split(" ");
  assert.equal(pts.length, 256);
  for (const p of pts) {
    const [x, y] = p.split(",").map(Number);
    assert.ok(x >= 0 && x <= 100, p);
    assert.ok(y >= 0 && y <= 40, p);
  }
});

/** 봉우리를 넘는 칸(양 끝)이 천장 위로 삐져나가면 안 된다 */
test("봉우리를 넘는 칸은 천장에 붙는다", () => {
  const ch = new Uint32Array(256);
  ch[255] = 1000;
  const pts = polyline(ch, 10, 100, 40).split(" ");
  const [, y] = pts[255].split(",").map(Number);
  assert.equal(y, 0);
});

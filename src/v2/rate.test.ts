import { test } from "node:test";
import assert from "node:assert/strict";
import { etaSec, fmtEta, pushSample, rateOf, WINDOW_MS } from "./rate.ts";

const walk = (steps: [number, number][]) =>
  steps.reduce<ReturnType<typeof pushSample>>(
    (acc, [t, n]) => pushSample(acc, { t, n }),
    [],
  );

test("표본 하나로는 못 잰다", () => {
  assert.equal(rateOf(walk([[0, 0]]), 1000), null);
});

test("2초 안 되는 창으로도 못 잰다 — 첫 알림 사이는 크게 튄다", () => {
  assert.equal(
    rateOf(
      walk([
        [0, 0],
        [500, 16],
      ]),
      600,
    ),
    null,
  );
});

test("최근 창의 초당 처리량", () => {
  const s = walk([
    [0, 0],
    [5_000, 100],
    [10_000, 300],
  ]);
  assert.equal(rateOf(s, 10_000), 30);
});

test("창 밖의 옛 표본은 속도에 안 끼친다 — 처음 느렸던 구간에 끌리지 않는다", () => {
  // 처음 60초는 초당 10장, 그 뒤 30초는 초당 60장
  const s = walk([
    [0, 0],
    [60_000, 600],
    [75_000, 1500],
    [90_000, 2400],
  ]);
  const r = rateOf(s, 90_000);
  assert.ok(r !== null && r >= 55 && r <= 65, `${r}`);
});

test("한동안 소식이 없으면 멎은 것 — null", () => {
  const s = walk([
    [0, 0],
    [5_000, 100],
  ]);
  assert.equal(rateOf(s, 5_000 + WINDOW_MS + 1), null);
});

test("pushSample은 원본을 두고 창 밖은 버린다", () => {
  const a = walk([
    [0, 0],
    [1_000, 10],
  ]);
  const b = pushSample(a, { t: 100_000, n: 500 });
  assert.equal(a.length, 2);
  // 창 경계 바깥의 하나(1_000)는 남고 그보다 옛것(0)은 버린다
  assert.deepEqual(
    b.map((x) => x.t),
    [1_000, 100_000],
  );
});

test("남은 시간과 글", () => {
  assert.equal(etaSec(600, 30), 20);
  assert.equal(etaSec(600, null), null);
  assert.equal(etaSec(600, 0), null);
  assert.equal(fmtEta(null), "");
  assert.equal(fmtEta(20), "1분 안");
  assert.equal(fmtEta(200), "약 3분");
  assert.equal(fmtEta(3600), "약 1시간");
  assert.equal(fmtEta(4800), "약 1시간 20분");
});

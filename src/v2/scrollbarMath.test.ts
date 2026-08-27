import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MIN_LABEL,
  MIN_THUMB,
  bucketAt,
  cumulative,
  thinMarks,
  thumbGeometry,
  topToIndex,
  yToIndex,
  type Bucket,
} from "./scrollbarMath.ts";

const b = (year: number, month: number, count: number): Bucket => ({
  year,
  month,
  count,
  top: 0,
});

/** 2015~2024, 매달 100장 — 120개월 */
const tenYears = () => {
  const out: Bucket[] = [];
  for (let y = 2024; y >= 2015; y--)
    for (let m = 12; m >= 1; m--) out.push(b(y, m, 100));
  return out;
};

test("달마다 시작 순번이 누적된다", () => {
  const { items, total } = cumulative([
    b(2024, 3, 10),
    b(2024, 2, 5),
    b(2024, 1, 7),
  ]);
  assert.deepEqual(
    items.map((i) => i.start),
    [0, 10, 15],
  );
  assert.equal(total, 22);
});

test("빈 목록도 터지지 않는다", () => {
  const { items, total } = cumulative([]);
  assert.equal(total, 0);
  assert.equal(bucketAt(items, 0), null);
  assert.deepEqual(thinMarks(items, total, 600), []);
});

test("눈금 간격이 장수에 비례한다", () => {
  // 첫 달이 900장, 나머지가 100장씩이면 첫 달이 대부분을 차지해야 한다
  const { items, total } = cumulative([
    b(2024, 3, 900),
    b(2024, 2, 50),
    b(2024, 1, 50),
  ]);
  const h = 1000;
  assert.equal(total, 1000);
  assert.equal((items[1].start / total) * h, 900, "두 번째 눈금은 90% 지점");
  assert.equal((items[2].start / total) * h, 950);
});

test("순번으로 달을 찾는다 — 경계 포함", () => {
  const { items } = cumulative([b(2024, 3, 10), b(2024, 2, 5), b(2024, 1, 7)]);
  assert.equal(bucketAt(items, 0)!.month, 3);
  assert.equal(bucketAt(items, 9)!.month, 3);
  assert.equal(bucketAt(items, 10)!.month, 2, "경계는 다음 달의 것");
  assert.equal(bucketAt(items, 14)!.month, 2);
  assert.equal(bucketAt(items, 15)!.month, 1);
  assert.equal(bucketAt(items, 9999)!.month, 1, "끝을 넘어가면 마지막 달");
});

test("세로 위치를 순번으로 — 열 밖은 양 끝에 붙인다", () => {
  assert.equal(yToIndex(0, 500, 1000), 0);
  assert.equal(yToIndex(250, 500, 1000), 500);
  assert.equal(yToIndex(500, 500, 1000), 999, "마지막 순번을 넘지 않는다");
  assert.equal(yToIndex(-40, 500, 1000), 0);
  assert.equal(yToIndex(9999, 500, 1000), 999);
  assert.equal(yToIndex(100, 0, 1000), 0, "높이를 아직 모를 때");
});

test("라벨이 겹치지 않는다", () => {
  const { items, total } = cumulative(tenYears());
  const marks = thinMarks(items, total, 600);
  const labels = marks.filter((m) => m.label);
  for (let i = 1; i < labels.length; i++) {
    assert.ok(
      labels[i].y - labels[i - 1].y >= MIN_LABEL,
      `${labels[i - 1].label}(${labels[i - 1].y}) 와 ${labels[i].label}(${labels[i].y})`,
    );
  }
  assert.ok(labels.length > 0, "적어도 몇 개는 남아야 한다");
});

test("좁아지면 연도만 남는다", () => {
  const { items, total } = cumulative(tenYears());
  const wide = thinMarks(items, total, 900).filter((m) => m.label);
  const narrow = thinMarks(items, total, 130).filter((m) => m.label);
  assert.ok(narrow.length < wide.length, "높이가 줄면 라벨도 준다");
  assert.ok(
    narrow.every((m) => m.isYear),
    `좁을 때 남은 라벨: ${narrow.map((m) => m.label).join(",")}`,
  );
});

test("눈금은 항상 위에서 아래로 정렬돼 있다", () => {
  const { items, total } = cumulative(tenYears());
  const marks = thinMarks(items, total, 600);
  for (let i = 1; i < marks.length; i++) {
    assert.ok(marks[i].y >= marks[i - 1].y);
    assert.ok(marks[i].y <= 600);
  }
});

test("손잡이 크기는 한 화면 비율 — 다만 최소 높이가 있다", () => {
  const h = 600;
  assert.equal(
    thumbGeometry(1000, 500, 0, h).height,
    300,
    "절반이 보이면 절반",
  );
  assert.equal(
    thumbGeometry(140_000, 60, 0, h).height,
    MIN_THUMB,
    "6만 장에서도 잡힌다",
  );
  assert.equal(
    thumbGeometry(50, 100, 0, h).height,
    h,
    "다 보이면 트랙을 꽉 채운다",
  );
});

test("손잡이는 트랙을 벗어나지 않는다", () => {
  const h = 600;
  const g = (at: number) => thumbGeometry(140_000, 60, at, h);
  assert.equal(g(0).top, 0);
  assert.equal(g(-100).top, 0, "음수도 맨 위");
  const bottom = g(139_940);
  assert.ok(
    Math.abs(bottom.top + bottom.height - h) < 0.001,
    "끝은 트랙 바닥에 딱 붙는다",
  );
  assert.equal(g(999_999).top, bottom.top, "넘겨도 더 내려가지 않는다");
});

test("끌어 놓은 자리와 손잡이 위치가 서로 맞는다", () => {
  const h = 600;
  const total = 140_000;
  const pageSize = 60;
  for (const top of [0, 37, 150, 400, 572]) {
    const index = topToIndex(top, total, pageSize, h);
    const back = thumbGeometry(total, pageSize, index, h).top;
    assert.ok(Math.abs(back - top) < 1.5, `top=${top} -> ${index} -> ${back}`);
  }
});

test("전부 한 화면에 들어오면 끌어도 제자리", () => {
  assert.equal(topToIndex(300, 40, 100, 600), 0);
  assert.equal(thumbGeometry(40, 100, 0, 600).maxTop, 0);
});

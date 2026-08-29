import { test } from "node:test";
import assert from "node:assert/strict";
import {
  justify,
  ratio,
  fitOf,
  hasCaption,
  masonry,
  metrics,
  GAP,
  PAD,
  CAPTION_H,
  visibleRange,
} from "./gridStyle.ts";

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

test("카드만 사진 전체, 나머지는 채운다 — 이름줄도 카드뿐", () => {
  assert.equal(fitOf("card"), "object-contain");
  assert.equal(fitOf("tile"), "object-cover");
  assert.equal(fitOf("justified"), "object-cover");
  assert.equal(fitOf("masonry"), "object-cover");
  assert.equal(hasCaption("card"), true);
  assert.equal(hasCaption("tile"), false);
});

/** 줄이 겹쳐 이름이 묻히고 그림이 붙던 버그. 줄 높이는 반드시 그림+이름+틈이다. */
test("줄 높이가 실제 칸 높이를 덮는다 — 어떤 폭·크기에서도", () => {
  for (const w of [400, 777, 1000, 1100, 1439, 2560]) {
    for (const t of [100, 140, 180, 240, 320]) {
      for (const style of ["card", "tile"] as const) {
        const m = metrics(w, t, style, true);
        const cellH = m.imageH + CAPTION_H;
        assert.ok(
          m.rowH >= cellH + GAP - 1e-6,
          `${w}px/${t}/${style}: ${m.rowH} < ${cellH + GAP}`,
        );
      }
    }
  }
});

test("칸은 썸네일 크기보다 작지 않다", () => {
  for (const w of [400, 777, 1000, 1100, 2560]) {
    for (const t of [100, 180, 320]) {
      const m = metrics(w, t, "card", true);
      assert.ok(m.cellW >= t - 1e-6, `${w}/${t}: ${m.cellW}`);
    }
  }
});

test("칸들이 안쪽 폭을 정확히 채운다", () => {
  const m = metrics(1100, 180, "card", true);
  assert.equal(m.contentW, 1100 - PAD * 2);
  assert.ok(
    Math.abs(m.cols * m.cellW + (m.cols - 1) * GAP - m.contentW) < 1e-6,
  );
});

/** 바깥 여백을 안 빼면 칸이 한 개 더 들어가 오른쪽이 잘린다 */
test("바깥 여백을 뺀 폭으로 센다", () => {
  // 안쪽 1080 = 180×5 + 10×4 → 정확히 5칸. 여백을 안 빼면 (1100+10)/190=5.8→5 로 같지만
  // 안쪽 1090이면 (1090+10)/190 = 5.79 → 5, 여백 무시 1110 → 5.89 → 5. 경계에서 갈린다:
  const tight = metrics(180 * 6 + 10 * 5 + PAD * 2, 180, "card", true); // 안쪽 1130
  assert.equal(tight.cols, 6);
  const under = metrics(180 * 6 + 10 * 5 + PAD * 2 - 1, 180, "card", true);
  assert.equal(under.cols, 5, "1px 모자라면 한 칸 줄어야 한다");
});

test("타일은 높이가 썸네일 크기로 고정이다 — 칸이 넓어져도", () => {
  const a = metrics(1000, 180, "tile", false);
  const b = metrics(1900, 180, "tile", false);
  assert.equal(a.imageH, 180);
  assert.equal(b.imageH, 180);
  assert.ok(b.cellW > a.cellW, "폭만 달라진다");
});

test("이름줄을 끄면 그만큼 낮아진다", () => {
  const a = metrics(1000, 180, "card", true);
  const b = metrics(1000, 180, "card", false);
  assert.equal(a.rowH - b.rowH, CAPTION_H);
});

test("폭이 0이거나 너무 좁아도 터지지 않는다", () => {
  const z = metrics(0, 180, "card", true);
  assert.equal(z.cols, 1);
  assert.equal(z.cellW, 0);
  const n = metrics(50, 180, "card", true);
  assert.equal(n.cols, 1);
  assert.equal(n.cellW, 30, "한 칸이 안쪽 폭 전부를 갖는다");
});

// ── 메이슨리 ────────────────────────────────────────────────────────────
type M = { id: number; r: number; g: string | null };
const m = (id: number, r: number, g: string | null = null): M => ({ id, r, g });
const lay = (files: M[], cols = 3) =>
  masonry(
    files,
    (f) => f.g,
    (f) => f.r,
    3 * 100 + 2 * 10,
    cols,
    10,
    30,
  );

test("메이슨리 — 열 폭은 같고 높이는 비율대로", () => {
  const L = lay([m(1, 2), m(2, 0.5), m(3, 1)]);
  assert.equal(L.boxes.length, 3);
  for (const b of L.boxes) assert.equal(b.w, 100);
  assert.equal(L.boxes[0].h, 50, "2:1은 절반 높이");
  assert.equal(L.boxes[1].h, 200, "1:2는 두 배");
  assert.equal(L.boxes[2].h, 100);
  // 첫 셋은 각자 열 하나씩
  assert.deepEqual(
    L.boxes.map((b) => b.x),
    [0, 110, 220],
  );
  assert.deepEqual(
    L.boxes.map((b) => b.y),
    [0, 0, 0],
  );
});

test("다음 장은 가장 짧은 열로 간다", () => {
  const L = lay([m(1, 2), m(2, 0.5), m(3, 1), m(4, 1)]);
  // 열 높이: 50, 200, 100 → 넷째는 첫 열(50+10) 아래로
  assert.equal(L.boxes[3].x, 0);
  assert.equal(L.boxes[3].y, 60);
  assert.equal(L.height, 200, "가장 긴 열");
});

test("묶기가 켜져 있으면 묶음마다 머리글을 놓고 열을 새로 시작한다", () => {
  const L = lay([m(1, 1, "A"), m(2, 0.5, "A"), m(3, 1, "B")]);
  assert.equal(L.headers.length, 2);
  assert.equal(L.headers[0].y, 0);
  assert.equal(L.boxes[0].y, 30, "머리글 아래");
  // A의 가장 긴 열은 1:2(200) → 30+200 = 230. B 머리글은 그 뒤 (gap 포함)
  assert.equal(L.headers[1].y, 30 + 200 + 10);
  assert.equal(L.boxes[2].x, 0, "B의 첫 장은 첫 열부터");
  assert.equal(L.boxes[2].y, L.headers[1].y + 30);
});

test("메이슨리 — rows 안 위치를 안다", () => {
  const L = lay([m(7, 1), m(8, 1)]);
  assert.deepEqual(
    L.boxes.map((b) => b.index),
    [0, 1],
  );
});

test("메이슨리 — 빈 목록·폭 0", () => {
  assert.equal(lay([]).boxes.length, 0);
  assert.equal(
    masonry(
      [m(1, 1)],
      () => null,
      () => 1,
      0,
      3,
      10,
      30,
    ).boxes.length,
    0,
  );
});

test("보이는 구간 — y 오름차순 상자에서 이분 탐색으로 자른 구간이 전수 검사와 같다", () => {
  // 높이가 들쭉날쭉한 상자 2,000개, y 는 줄지 않는다(메이슨리의 «가장 짧은 열» 성질)
  const boxes: { y: number; h: number }[] = [];
  let y = 0;
  for (let i = 0; i < 2000; i++) {
    const h = 50 + ((i * 37) % 200);
    boxes.push({ y, h });
    if (i % 3 === 2) y += 40 + ((i * 11) % 90);
  }
  const maxH = Math.max(...boxes.map((b) => b.h));
  for (const [top, bottom] of [
    [0, 800],
    [1234, 2034],
    [30000, 30800],
    [-500, 100],
    [10 ** 9, 10 ** 9 + 800],
  ]) {
    const [s, e] = visibleRange(boxes, top, bottom, maxH);
    const got = boxes.slice(s, e).filter((b) => b.y + b.h >= top && b.y <= bottom);
    const want = boxes.filter((b) => b.y + b.h >= top && b.y <= bottom);
    assert.deepEqual(got, want, `[${top}, ${bottom}]`);
  }
  assert.deepEqual(visibleRange([], 0, 100, 10), [0, 0]);
});

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  justify,
  ratio,
  objectFit,
  metrics,
  GAP,
  PAD,
  CAPTION_H,
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

test("채우기 방식이 클래스로 매핑된다", () => {
  assert.equal(objectFit("cover"), "object-cover");
  assert.equal(objectFit("contain"), "object-contain");
  assert.equal(objectFit("fill"), "object-fill");
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

test("타일은 4:3이라 줄이 낮다", () => {
  const card = metrics(1000, 180, "card", false);
  const tile = metrics(1000, 180, "tile", false);
  assert.ok(tile.rowH < card.rowH);
  assert.ok(Math.abs(tile.imageH - (tile.cellW * 3) / 4) < 1e-6);
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

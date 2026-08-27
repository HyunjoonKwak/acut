import { test } from "node:test";
import assert from "node:assert/strict";
import { layout, headerLabel } from "./gridLayout.ts";

type F = { id: number; g: string | null };
const f = (id: number, g: string | null): F => ({ id, g });
const key = (x: F) => x.g;

test("묶지 않으면 그냥 cols개씩 자른다", () => {
  const rows = layout([f(1, null), f(2, null), f(3, null)], key, 2);
  assert.equal(rows.length, 2);
  assert.deepEqual(
    rows.map((r) => (r.kind === "photos" ? r.items.length : -1)),
    [2, 1],
  );
});

test("그룹마다 머리글이 하나씩", () => {
  const rows = layout([f(1, "A"), f(2, "A"), f(3, "B")], key, 4);
  const heads = rows.filter((r) => r.kind === "header");
  assert.equal(heads.length, 2);
  assert.deepEqual(
    heads.map((h) => (h.kind === "header" ? [h.label, h.count] : null)),
    [
      ["A", 2],
      ["B", 1],
    ],
  );
});

test("그룹은 새 줄에서 시작한다 — 섞이면 머리글이 거짓말이 된다", () => {
  // cols=3, A가 2장이면 남은 한 칸에 B를 채우면 안 된다
  const rows = layout([f(1, "A"), f(2, "A"), f(3, "B")], key, 3);
  const photoRows = rows.filter((r) => r.kind === "photos");
  assert.equal(photoRows.length, 2, "A줄 하나 + B줄 하나");
  for (const r of photoRows) {
    if (r.kind !== "photos") continue;
    const gs = new Set(r.items.map((i) => i.g));
    assert.equal(gs.size, 1, "한 줄에 두 그룹이 섞이면 안 된다");
  }
});

test("한 그룹이 여러 줄로 나뉜다", () => {
  const rows = layout(
    [1, 2, 3, 4, 5].map((n) => f(n, "A")),
    key,
    2,
  );
  assert.equal(rows.filter((r) => r.kind === "header").length, 1);
  assert.equal(rows.filter((r) => r.kind === "photos").length, 3);
});

test("start 는 전체에서의 위치 — 뷰어를 열 때 쓴다", () => {
  const rows = layout([f(1, "A"), f(2, "B"), f(3, "B")], key, 2);
  const photos = rows.filter((r) => r.kind === "photos");
  assert.deepEqual(
    photos.map((r) => (r.kind === "photos" ? r.start : -1)),
    [0, 1],
  );
});

test("빈 목록과 잘못된 cols", () => {
  assert.deepEqual(layout([], key, 4), []);
  assert.deepEqual(layout([f(1, "A")], key, 0), []);
});

test("머리글은 사람이 읽는 말로", () => {
  assert.equal(headerLabel("2024-08", "month"), "2024년 8월");
  assert.equal(headerLabel("2024", "year"), "2024년");
  assert.equal(headerLabel("2024-08-27", "day"), "2024년 8월 27일");
  assert.equal(headerLabel("4", "rating"), "★★★★");
  assert.equal(headerLabel("0", "rating"), "평점 없음");
  assert.equal(headerLabel("", "folder"), "(최상단)");
  assert.equal(headerLabel("사진", "file_type"), "사진");
});

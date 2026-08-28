import { test } from "node:test";
import assert from "node:assert/strict";
import { EMPTY, isEmpty, picksFrom, type Picks } from "./picks.ts";

/**
 * 스마트 앨범은 저장한 조건을 그대로 되돌려야 한다. 한 자리라도 빠지면
 * 눌렀을 때 저장할 때와 다른 사진이 뜬다 — 그런데 화면에는 아무 표시도
 * 없어서 알아채기가 어렵다.
 */
test("저장한 조건이 하나도 안 빠지고 돌아온다", () => {
  // EMPTY의 모든 열쇠에 기본값이 아닌 값을 채운다
  const full: Picks = {
    kind: 1,
    culling_flag: 2,
    min_rating: 4,
    favorite_only: true,
    name_like: "IMG",
    year: "2024",
    month: "2024-08",
    day: "2024-08-27",
    camera: "ILCE-7M4",
    lens: "FE 24-70",
    tag_id: 7,
    place: "37.5,126.9",
    no_thumb: true,
    person_id: 3,
    bbox: "37.5,126.9,37.6,127",
  };
  // 조건이 늘어나면 이 시험도 같이 늘어나야 한다
  assert.deepEqual(
    Object.keys(full).sort(),
    Object.keys(EMPTY).sort(),
    "Picks에 조건이 늘었다면 이 시험의 full에도 채워야 한다",
  );

  const back = picksFrom({ ...full, library_id: 3, sort: { by: "size" } });
  assert.deepEqual(back, full);
});

test("빈 조건은 EMPTY 그대로", () => {
  assert.deepEqual(picksFrom({}), EMPTY);
  assert.deepEqual(picksFrom(null), EMPTY);
  assert.deepEqual(picksFrom(undefined), EMPTY);
});

test("모르는 열쇠는 흘려보낸다", () => {
  const p = picksFrom({ kind: 2, 알수없음: "x", library_id: 9 });
  assert.equal(p.kind, 2);
  assert.equal(Object.keys(p).length, Object.keys(EMPTY).length);
});

/** 손으로 고친 JSON이 와도 그 조건만 버리고 나머지는 살린다 */
test("형이 안 맞는 값은 그것만 버린다", () => {
  const p = picksFrom({ kind: "영상", min_rating: 3 });
  assert.equal(p.kind, null, "숫자 자리에 글자가 오면 안 쓴다");
  assert.equal(p.min_rating, 3, "옆의 멀쩡한 조건은 살아야 한다");
});

test("자리 없음(빈 문자열)은 조건으로 살아남는다", () => {
  // ""는 "GPS가 없는 것만"이라는 뜻이다. 거짓값이라고 버리면 안 된다.
  assert.equal(picksFrom({ place: "" }).place, "");
  assert.equal(isEmpty(picksFrom({ place: "" })), false);
});

test("isEmpty는 아무것도 안 고른 상태만 참", () => {
  assert.equal(isEmpty(EMPTY), true);
  assert.equal(isEmpty({ ...EMPTY, tag_id: 1 }), false);
  assert.equal(isEmpty({ ...EMPTY, favorite_only: true }), false);
  assert.equal(isEmpty({ ...EMPTY, min_rating: 0 }), false);
});

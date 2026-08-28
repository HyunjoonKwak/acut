import { test } from "node:test";
import assert from "node:assert/strict";
import { chips, formatPlace, without } from "./chips.ts";
import { EMPTY, isEmpty, type Picks } from "./picks.ts";

const none = () => undefined;
const labels = (p: Partial<Picks>) =>
  chips({ ...EMPTY, ...p }, none).map((c) => c.label);

test("아무것도 안 걸리면 비어 있다", () => {
  assert.deepEqual(chips(EMPTY, none), []);
});

test("걸린 것만 나온다", () => {
  assert.deepEqual(labels({ kind: 1, favorite_only: true }), [
    "영상",
    "♥ 즐겨찾기",
  ]);
});

/** 「2024년 8월」 옆에 「2024년」이 또 뜨면 같은 말을 두 번 하는 셈이다 */
test("달을 고르면 연도는 따로 안 뜬다", () => {
  assert.deepEqual(labels({ year: "2024", month: "2024-08" }), ["2024년 8월"]);
  assert.deepEqual(labels({ year: "2024" }), ["2024년"]);
});

test("날을 고르면 하나만 뜨고, 떼면 달로 돌아간다", () => {
  assert.deepEqual(
    labels({ year: "2024", month: "2024-08", day: "2024-08-27" }),
    ["2024년 8월 27일"],
  );
  const p = without(
    { ...EMPTY, year: "2024", month: "2024-08", day: "2024-08-27" },
    "day",
  );
  assert.equal(p.day, null);
  assert.equal(p.month, "2024-08", "달은 남는다");
});

test("태그는 이름으로 뜬다", () => {
  const byId = (id: number) => (id === 3 ? "가족" : undefined);
  assert.deepEqual(
    chips({ ...EMPTY, tag_id: 3 }, byId).map((c) => c.label),
    ["가족"],
  );
  // 이름을 아직 못 읽었어도 조건이 걸린 건 보여야 한다
  assert.deepEqual(labels({ tag_id: 9 }), ["태그 9"]);
});

test("자리는 사이드바와 같은 말로 뜬다", () => {
  assert.equal(formatPlace("37.5,126.9"), "북위 37.5° 동경 126.9°");
  assert.equal(formatPlace("-33.9,-70.7"), "남위 33.9° 서경 70.7°");
  assert.equal(formatPlace(""), "위치 없음");
  assert.equal(formatPlace("이상한값"), "이상한값");
});

/** 값이 0이거나 빈 문자열인 조건도 "걸린 것"이다. 거짓값이라고 빠지면 안 된다 */
test("0과 빈 문자열도 조건이다", () => {
  assert.deepEqual(labels({ kind: 0 }), ["사진"]);
  assert.deepEqual(labels({ culling_flag: 0 }), ["미판정"]);
  assert.deepEqual(labels({ min_rating: 0 }), ["평점 없음"]);
  assert.deepEqual(labels({ place: "" }), ["위치 없음"]);
  assert.deepEqual(labels({ camera: "" }), ["카메라 없음"]);
});

test("떼면 그 조건만 사라진다", () => {
  const p: Picks = { ...EMPTY, kind: 1, tag_id: 3 };
  assert.deepEqual(without(p, "kind"), { ...EMPTY, tag_id: 3 });
});

test("달을 떼면 연도도 같이 떨어진다", () => {
  const p: Picks = { ...EMPTY, year: "2024", month: "2024-08" };
  assert.equal(isEmpty(without(p, "month")), true);
});

/** 칩을 하나씩 다 떼면 아무 조건도 안 남아야 한다 */
test("전부 떼면 EMPTY로 돌아온다", () => {
  let p: Picks = {
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
  // 날을 떼면 달이 드러나고, 달을 떼면 연도까지 떨어진다 — 사람이 ✕를
  // 계속 누르는 것과 같다. 스무 번 안에 다 없어져야 한다.
  for (let i = 0; i < 20 && chips(p, none).length > 0; i++) {
    p = without(p, chips(p, none)[0].key);
  }
  assert.equal(isEmpty(p), true, JSON.stringify(p));
});

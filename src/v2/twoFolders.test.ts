import { test } from "node:test";
import assert from "node:assert/strict";
import { doneSide, droppable, overlaps, verdict, type FolderHit, type PairRow } from "./twoFoldersLogic.ts";

const hit = (volume_uuid: string, vol_rel: string): FolderHit => ({
  id: null,
  library_id: 1,
  library: "T7",
  path: vol_rel,
  volume_uuid,
  vol_rel,
  abs: "/Volumes/T7/" + vol_rel,
  file_count: 1,
});
const folder = (folder_id: number) => ({ folder_id, library_id: 1, library: "T7", folder: "x", area: 0 });
const pair = (o: Partial<PairRow>): PairRow => ({
  a: folder(1),
  b: folder(2),
  files_a: 3,
  files_b: 3,
  same: true,
  common: 3,
  bytes: 30,
  flagged_a: 0,
  flagged_b: 0,
  b_in_a: true,
  a_in_b: true,
  a_ids: [1],
  b_ids: [2],
  ...o,
});

test("한쪽이 다른 쪽에 다 들어 있으면 그쪽만 지울 수 있다 — 하위 폴더까지 합쳐 본 결과", () => {
  const r = pair({ same: false, b_in_a: true, a_in_b: false, files_a: 207, files_b: 191 });
  assert.equal(droppable(r, "b"), true);
  assert.equal(droppable(r, "a"), false);
  assert.equal(verdict(r).kind, "b_in_a");
  assert.equal(verdict(pair({})).kind, "same");
  assert.equal(droppable(pair({}), "a"), true, "똑같으면 어느 쪽이든");
  const partial = pair({ same: false, b_in_a: false, a_in_b: false, common: 3 });
  assert.equal(verdict(partial).text, "3장 똑같음");
  assert.equal(droppable(partial, "b"), false, "부분만 겹치면 지울 수 없다");
  assert.equal(verdict(pair({ b: null })).kind, "a_only");
});

test("한쪽이 다른 쪽의 위 폴더면 겹친다", () => {
  assert.equal(overlaps(hit("v", "통합전후보"), hit("v", "통합전후보/후보1번")), true);
  assert.equal(overlaps(hit("v", "통합전후보/후보1번"), hit("v", "통합전후보")), true);
  assert.equal(overlaps(hit("v", "a"), hit("v", "a")), true);
});

test("이름 앞만 같은 것은 겹치지 않는다 — «후보1» 과 «후보10»", () => {
  assert.equal(overlaps(hit("v", "후보1"), hit("v", "후보10")), false);
  assert.equal(overlaps(hit("v", "통합전후보/후보1번"), hit("v", "통합전후보/후보2번")), false);
});

test("볼륨이 다르면 경로가 같아도 겹치지 않고, 볼륨 뿌리는 전부를 품는다", () => {
  assert.equal(overlaps(hit("v1", "사진"), hit("v2", "사진")), false);
  assert.equal(overlaps(hit("v", ""), hit("v", "아무데나")), true);
});

test("한쪽이 전부 지우기 표시됐으면 그쪽이 처리됨", () => {
  assert.equal(doneSide(pair({ flagged_b: 3 })), "b");
  assert.equal(doneSide(pair({ flagged_a: 3 })), "a");
});

test("일부만 표시됐거나 지울 수 없는 짝이면 처리되지 않았다", () => {
  assert.equal(doneSide(pair({ flagged_b: 2 })), null);
  // 부분만 겹치는 짝은 표시가 있어도 «처리됨»이 아니다
  assert.equal(doneSide(pair({ same: false, b_in_a: false, a_in_b: false, flagged_b: 3 })), null);
  assert.equal(doneSide(pair({ b: null, flagged_a: 3 })), null);
});

test("«B쪽이 A에 다 있음» 짝도 B가 전부 표시되면 처리됨이다", () => {
  assert.equal(doneSide(pair({ same: false, b_in_a: true, a_in_b: false, files_a: 9, files_b: 6, flagged_b: 6 })), "b");
  assert.equal(doneSide(pair({ same: false, b_in_a: true, a_in_b: false, files_a: 9, files_b: 6, flagged_b: 5 })), null);
});

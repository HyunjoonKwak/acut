import { test } from "node:test";
import assert from "node:assert/strict";
import { doneSide, overlaps, type FolderHit, type PairRow } from "./twoFoldersLogic.ts";

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
  ...o,
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

test("일부만 표시됐거나 같은 짝이 아니면 처리되지 않았다", () => {
  assert.equal(doneSide(pair({ flagged_b: 2 })), null);
  assert.equal(doneSide(pair({ same: false, flagged_b: 3 })), null);
  assert.equal(doneSide(pair({ b: null, flagged_a: 3 })), null);
});

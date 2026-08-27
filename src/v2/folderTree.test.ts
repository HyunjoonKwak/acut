import { test } from "node:test";
import assert from "node:assert/strict";
import { visible } from "./folderTree.ts";

const row = (path: string) => ({
  path,
  depth: path === "" ? 0 : path.split("/").length - 1,
});

test("접힌 라이브러리의 폴더는 안 보인다", () => {
  const rows = [row("#3"), row("#3/연도별"), row("#3/연도별/2001")];
  assert.deepEqual(
    visible(rows, new Set()).map((r) => r.path),
    ["#3"],
  );
});

test("펴면 바로 아래만 보인다 — 손자는 아직", () => {
  const rows = [row("#3"), row("#3/연도별"), row("#3/연도별/2001")];
  assert.deepEqual(
    visible(rows, new Set(["#3"])).map((r) => r.path),
    ["#3", "#3/연도별"],
  );
});

/** 할아버지가 접혔는데 아버지만 펴져 있으면 손자가 떠오르면 안 된다 */
test("조상이 하나라도 접혀 있으면 안 보인다", () => {
  const rows = [row("#3"), row("#3/연도별"), row("#3/연도별/2001")];
  assert.deepEqual(
    visible(rows, new Set(["#3/연도별"])).map((r) => r.path),
    ["#3"],
  );
});

test("라이브러리가 둘이면 서로 간섭하지 않는다", () => {
  const rows = [row("#1"), row("#1/행사"), row("#2"), row("#2/행사")];
  assert.deepEqual(
    visible(rows, new Set(["#1"])).map((r) => r.path),
    ["#1", "#1/행사", "#2"],
  );
});

test("빈 목록도 터지지 않는다", () => {
  assert.deepEqual(visible([], new Set()), []);
});

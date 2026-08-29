import { test } from "node:test";
import assert from "node:assert/strict";
import { undoLabel } from "./undoLabel.ts";

test("되돌리기 단추는 작업 종류별로 «무엇이 어떻게 되는지»를 적는다", () => {
  assert.equal(undoLabel("trash", 3912), "휴지통 보낸 3,912장 되살리기");
  assert.equal(undoLabel("restore", 5), "되살린 5장 다시 휴지통으로");
  assert.equal(undoLabel("move", 12), "정리 되돌리기 (12장)");
  assert.equal(undoLabel("rename", 1), "이름 바꾸기 되돌리기");
  assert.equal(undoLabel("import", 40), "가져온 40장 되돌리기");
  assert.equal(undoLabel("whatever", 2), "되돌리기 (2장)");
});

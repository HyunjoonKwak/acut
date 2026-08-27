import { test } from "node:test";
import assert from "node:assert/strict";
import { useJob } from "./jobStore.ts";

const reset = () => useJob.getState().clear();

test("같은 일의 알림은 앞으로만 간다", () => {
  reset();
  const s = useJob.getState();
  s.progress({ label: "썸네일", done: 100, total: 1000 });
  s.progress({ label: "썸네일", done: 105, total: 1000 });
  s.progress({ label: "썸네일", done: 101, total: 1000 }); // 늦게 온 것
  assert.equal(useJob.getState().job?.done, 105);
});

test("다른 일이 시작되면 0부터 다시", () => {
  reset();
  const s = useJob.getState();
  s.progress({ label: "스캔", done: 78857, total: 78857 });
  s.progress({ label: "썸네일", done: 3, total: 78857 });
  assert.equal(useJob.getState().job?.label, "썸네일");
  assert.equal(useJob.getState().job?.done, 3);
});

/** 대상 수가 달라졌으면 같은 이름이어도 새 일이다 (다른 라이브러리) */
test("대상 수가 바뀌면 새 일로 본다", () => {
  reset();
  const s = useJob.getState();
  s.progress({ label: "썸네일", done: 500, total: 1000 });
  s.progress({ label: "썸네일", done: 10, total: 2000 });
  assert.equal(useJob.getState().job?.done, 10);
});

test("같은 값이 또 오면 상태 객체가 그대로다 — 다시 그리지 않는다", () => {
  reset();
  const s = useJob.getState();
  s.progress({ label: "썸네일", done: 7, total: 10 });
  const a = useJob.getState().job;
  s.progress({ label: "썸네일", done: 7, total: 10 });
  assert.equal(useJob.getState().job, a);
});

test("끝나면 비운다", () => {
  reset();
  useJob.getState().progress({ label: "썸네일", done: 1, total: 2 });
  useJob.getState().clear();
  assert.equal(useJob.getState().job, null);
});

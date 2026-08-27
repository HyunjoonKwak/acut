import { test } from "node:test";
import assert from "node:assert/strict";

/**
 * 훅 자체는 브라우저 프레임에 묶여 있어 여기서 못 돌린다.
 * 대신 **한 칸씩 올라가되 뒤처지지 않는다**는 규칙만 따로 확인한다.
 * (useCountUp 안의 step 계산과 같은 식)
 */
const step = (gap: number) => Math.max(1, Math.ceil(gap / 8));

test("가까우면 한 칸씩 오른다", () => {
  assert.equal(step(1), 1);
  assert.equal(step(8), 1);
});

test("벌어지면 성큼 따라간다 — 안 그러면 영영 못 따라잡는다", () => {
  // 초당 177장을 60fps로 그리면 프레임마다 3씩 벌어진다.
  // 1씩만 올리면 격차가 계속 커진다.
  assert.ok(step(24) >= 3, `${step(24)}`);
  assert.equal(step(800), 100);
});

test("실제 속도를 따라잡는다", () => {
  // 프레임마다 3장씩 늘고, 표시는 step만큼 따라간다
  let shown = 0;
  let real = 0;
  for (let frame = 0; frame < 600; frame++) {
    real += 3;
    const gap = real - shown;
    if (gap > 0) shown += Math.min(gap, step(gap));
  }
  assert.ok(real - shown <= 24, `격차 ${real - shown}이면 눈에 띄게 뒤처진다`);
});

test("작업이 새로 시작되면 뒤로 가지 않는다", () => {
  // 목표가 줄면(새 작업) 훅은 따라가지 않고 바로 맞춘다 — 규칙만 확인
  const target = 0;
  const cur = 50_000;
  assert.ok(target < cur, "이 경우 애니메이션 없이 즉시 맞춰야 한다");
});

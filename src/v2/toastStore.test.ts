import { test } from "node:test";
import assert from "node:assert/strict";
import { useToasts } from "./toastStore.ts";

const reset = () => useToasts.setState({ toasts: [] });

test("띄우면 쌓이고 지우면 빠진다", () => {
  reset();
  const a = useToasts.getState().push("하나", "plain", 0);
  useToasts.getState().push("둘", "ok", 0);
  assert.equal(useToasts.getState().toasts.length, 2);
  useToasts.getState().dismiss(a);
  assert.deepEqual(
    useToasts.getState().toasts.map((t) => t.text),
    ["둘"],
  );
});

/** 같은 결과가 연달아 오면(⌘Z 두 번) 같은 말이 두 줄 쌓이지 않는다 */
test("같은 글은 한 번만 뜬다", () => {
  reset();
  const a = useToasts.getState().push("3장 옮겼습니다", "plain", 0);
  const b = useToasts.getState().push("3장 옮겼습니다", "plain", 0);
  assert.equal(a, b);
  assert.equal(useToasts.getState().toasts.length, 1);
});

test("시간이 지나면 저절로 사라진다", async () => {
  reset();
  useToasts.getState().push("잠깐", "plain", 20);
  assert.equal(useToasts.getState().toasts.length, 1);
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(useToasts.getState().toasts.length, 0);
});

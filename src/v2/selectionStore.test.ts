import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { useSelection } from "./selectionStore.ts";
import { useUi } from "./uiStore.ts";

const reset = () =>
  useSelection.setState({ selected: null, picked: new Set() });
const order = [10, 20, 30, 40, 50];
const S = () => useSelection.getState();

afterEach(() => {
  useUi.setState({ organizing: false, organizeSelection: null });
});

test("그냥 누르면 그것 하나만", () => {
  reset();
  S().pick(20, { meta: false, shift: false }, order);
  S().pick(30, { meta: false, shift: false }, order);
  assert.deepEqual([...S().picked], [30]);
  assert.equal(S().selected, 30);
});

test("⌘는 하나씩 더하고 다시 누르면 뺀다", () => {
  reset();
  S().pick(20, { meta: false, shift: false }, order);
  S().pick(40, { meta: true, shift: false }, order);
  assert.deepEqual([...S().picked].sort(), [20, 40]);
  S().pick(20, { meta: true, shift: false }, order);
  assert.deepEqual([...S().picked], [40]);
});

test("⇧는 기준점부터 여기까지 — 거꾸로 골라도 된다", () => {
  reset();
  S().pick(40, { meta: false, shift: false }, order);
  S().pick(20, { meta: false, shift: true }, order);
  assert.deepEqual(
    [...S().picked].sort((a, b) => a - b),
    [20, 30, 40],
  );
  assert.equal(S().selected, 20);
});

/** 기준점이 목록에 없으면(다른 쪽으로 넘어감) 그냥 하나만 고른다 */
test("⇧인데 기준점이 목록 밖이면 하나만", () => {
  reset();
  S().setSelected(999);
  S().pick(20, { meta: false, shift: true }, order);
  assert.deepEqual([...S().picked], [20]);
});

test("키보드로 옮기면 하나만, ⇧를 잡으면 늘어난다", () => {
  reset();
  S().moveTo(10, false);
  S().moveTo(20, true);
  S().moveTo(30, true);
  assert.deepEqual(
    [...S().picked].sort((a, b) => a - b),
    [10, 20, 30],
  );
  S().moveTo(40, false);
  assert.deepEqual([...S().picked], [40]);
});

/** 상태바가 늘 한 장을 가리키게 — 목록이 바뀌어 초점이 사라졌을 때만 첫 장 */
test("초점이 목록 안에 있으면 그대로, 없으면 첫 장으로", () => {
  reset();
  S().focusWithin(order);
  assert.equal(S().selected, 10);
  S().setSelected(30);
  S().focusWithin([30, 40]);
  assert.equal(S().selected, 30, "있으면 안 건드린다");
  S().focusWithin([70, 80]);
  assert.equal(S().selected, 70, "없으면 첫 장");
  S().focusWithin([]);
  assert.equal(S().selected, 70, "빈 목록은 아무것도 안 한다");
});

test("목록 안에 남은 선택은 그대로, 목록 밖으로 나간 것은 떨어진다", () => {
  reset();
  S().setPicked([10, 20]);
  S().focusWithin([10, 20, 30]);
  assert.deepEqual([...S().picked].sort(), [10, 20], "다 있으면 안 건드린다");
  // 다른 폴더로 갔다 — 보이지 않는 사진에 «제외»가 찍히면 안 된다 (리뷰 H8)
  S().focusWithin([30, 40]);
  assert.deepEqual([...S().picked], []);
  S().setPicked([30, 40]);
  S().focusWithin([]);
  assert.deepEqual(
    [...S().picked].sort(),
    [30, 40],
    "빈 목록(새로 읽는 중)은 아무것도 안 한다",
  );
});

test("이벤트 자동 발견의 정리 대상은 그리드 쪽 전환으로 잘리지 않는다", () => {
  reset();
  useUi.setState({
    organizing: true,
    organizeSelection: { ids: [10, 20, 30], libraryId: 7 },
  });
  S().setPicked([10, 20, 30]);
  S().focusWithin([10]);
  assert.deepEqual(
    [...S().picked],
    [10],
    "보이는 선택은 안전을 위해 현재 쪽으로 줄인다",
  );
  assert.deepEqual(
    useUi.getState().organizeSelection?.ids,
    [10, 20, 30],
    "그리드에서 빠진 사진도 고정 정리 대상에는 남는다",
  );
  assert.deepEqual(useUi.getState().organizeSelection, {
    ids: [10, 20, 30],
    libraryId: 7,
  });
});

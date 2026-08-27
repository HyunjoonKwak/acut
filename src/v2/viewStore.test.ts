import { test } from "node:test";
import assert from "node:assert/strict";
import { useView, facetOf, type Filter } from "./viewStore.ts";
import { usePrefs, DEFAULT_PREFS } from "./prefs.ts";
import { EMPTY } from "./picks.ts";

const reset = () => {
  useView.setState({ sel: null, picks: EMPTY, viewTrash: false });
  usePrefs.setState({ ...DEFAULT_PREFS });
};

test("스마트 앨범을 열면 라이브러리·폴더·정렬·조건이 다 돌아온다", () => {
  reset();
  useView.getState().applySmart(
    {
      kind: 1,
      min_rating: 4,
      library_id: 3,
      folder_path: "2024/여름",
      trashed: false,
      알수없음: "x",
    },
    { by: "size", desc: false },
  );
  const v = useView.getState();
  assert.equal(v.picks.kind, 1);
  assert.equal(v.picks.min_rating, 4);
  assert.deepEqual(v.sel, { libId: 3, path: "2024/여름", rel: "2024/여름" });
  assert.equal(usePrefs.getState().libId, 3);
  assert.deepEqual(usePrefs.getState().sort, { by: "size", desc: false });
});

/** 라이브러리 없이 폴더만 저장된 것은 폴더를 살릴 수 없다 — 폴더는 라이브러리에 속한다 */
test("묶기도 저장돼 있으면 되살린다", () => {
  reset();
  useView.getState().applySmart({ kind: 0, group: "month" }, null);
  assert.equal(usePrefs.getState().group, "month");
  useView.getState().applySmart({ kind: 0 }, null);
  assert.equal(usePrefs.getState().group, "month", "없으면 지금 것을 둔다");
});

test("라이브러리가 없으면 폴더는 살리지 않는다", () => {
  reset();
  useView.getState().applySmart({ folder_path: "a/b", library_id: null }, null);
  assert.equal(useView.getState().sel, null);
});

test("정렬이 없으면 지금 것을 둔다", () => {
  reset();
  usePrefs.getState().set("sort", { by: "name", desc: true });
  useView.getState().applySmart({ kind: 2 }, null);
  assert.deepEqual(usePrefs.getState().sort, { by: "name", desc: true });
});

test("「모든 사진」은 라이브러리·폴더·조건·휴지통을 다 푼다", () => {
  reset();
  usePrefs.getState().set("libId", 5);
  useView.setState({
    sel: { libId: 5, path: "x", rel: "x" },
    picks: { ...EMPTY, kind: 1 },
    viewTrash: true,
  });
  useView.getState().showAll();
  const v = useView.getState();
  assert.equal(v.sel, null);
  assert.deepEqual(v.picks, EMPTY);
  assert.equal(v.viewTrash, false);
  assert.equal(usePrefs.getState().libId, null);
});

test("갈래를 셀 때는 그 갈래 조건들을 뺀다", () => {
  const f: Filter = {
    ...EMPTY,
    kind: 1,
    year: "2024",
    month: "2024-08",
    camera: "X",
    min_rating: 3,
    tag_id: 7,
    place: "1,2",
    sort: { by: "taken_at", desc: true },
    library_id: 1,
    folder_path: null,
    trashed: false,
  };
  const g = facetOf(f);
  assert.equal(g.kind, 1, "종류는 갈래가 아니라 남는다");
  assert.equal(g.library_id, 1);
  for (const k of [
    "year",
    "month",
    "camera",
    "lens",
    "min_rating",
    "tag_id",
    "place",
  ] as const)
    assert.equal(g[k], null, k);
});

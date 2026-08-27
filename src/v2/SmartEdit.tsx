import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import GroupMenu from "./GroupMenu";
import SearchPanel from "./SearchPanel";
import SortMenu from "./SortMenu";
import { useData } from "./dataStore";
import type { GroupBy } from "./groupItems";
import { EMPTY, picksFrom, type Picks } from "./picks";
import { DEFAULT_SORT, type Sort } from "./sortItems";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import type { SmartAlbum } from "./SmartPanel";

/**
 * 스마트 앨범 편집 — 이름·조건·정렬·묶기·라이브러리를 한 상자에서.
 *
 * 저장만 되고 고칠 수 없었다. 조건은 찾기 패널을 그대로 안에 넣는다 —
 * 같은 화면이라 여기서 고른 것이 사이드바에서 고른 것과 같은 뜻이다.
 *
 * 이름을 바꾸면 옛 줄을 지우고 새로 넣는다 (저장은 이름으로 덮어쓰기라).
 */
export default function SmartEdit({
  initial,
  onClose,
  onSaved,
}: {
  /** 고칠 것. 없으면 새로 만든다. */
  initial: SmartAlbum | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const libs = useData((s) => s.libs);
  const init = useMemo(() => {
    const f = (initial?.filter ?? {}) as Partial<Picks> & {
      library_id?: number | null;
      group?: GroupBy;
    };
    return {
      picks: picksFrom(initial?.filter ?? EMPTY),
      libId: f.library_id ?? null,
      group: f.group ?? ("none" as GroupBy),
      sort: (initial?.sort as Sort | null) ?? DEFAULT_SORT,
    };
  }, [initial]);

  const [name, setName] = useState(initial?.name ?? "");
  const [picks, setPicks] = useState<Picks>(init.picks);
  const [libId, setLibId] = useState<number | null>(init.libId);
  const [group, setGroup] = useState<GroupBy>(init.group);
  const [sort, setSort] = useState<Sort>(init.sort);
  const [saving, setSaving] = useState(false);

  /// 평점 갈래가 «이 조건 안에서» 세도록 — 평점 자신은 뺀다
  const facetFilter = useMemo(
    () => ({
      ...picks,
      min_rating: null,
      sort,
      library_id: libId,
      folder_path: null,
      trashed: false,
    }),
    [picks, sort, libId],
  );

  const save = async () => {
    const n = name.trim();
    if (!n) return;
    setSaving(true);
    try {
      const filter = {
        ...picks,
        sort,
        library_id: libId,
        folder_path: null,
        trashed: false,
        group,
      };
      if (initial && initial.id !== 0 && initial.name !== n)
        await invoke("smart_delete", { id: initial.id });
      await invoke("smart_save", { name: n, filter, sort });
      toast(`「${n}」 저장했습니다`, "ok");
      onSaved();
      onClose();
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center bg-black/50"
      onPointerDown={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        onPointerDown={(e) => e.stopPropagation()}
        className="w-[420px] max-w-[92vw] max-h-[86vh] overflow-y-auto rounded-xl bg-chrome ring-1 ring-line-strong shadow-2xl"
      >
        <div className="px-5 pt-5">
          <div className="text-[15px] font-semibold text-fg">
            {initial && initial.id !== 0
              ? "스마트 앨범 고치기"
              : "스마트 앨범 만들기"}
          </div>
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && save()}
            placeholder="이름"
            aria-label="이름"
            className="mt-3 w-full h-control px-2 rounded-md bg-canvas text-[13px] text-fg
              placeholder:text-fg-faint outline-none ring-1 ring-line focus:ring-accent"
          />
        </div>

        <Head>어디서</Head>
        <div className="px-4 flex flex-wrap gap-1">
          <Chip on={libId === null} onClick={() => setLibId(null)}>
            모든 라이브러리
          </Chip>
          {libs.map((l) => (
            <Chip key={l.id} on={libId === l.id} onClick={() => setLibId(l.id)}>
              {l.name}
            </Chip>
          ))}
        </div>

        <Head>조건</Head>
        <div className="px-2">
          <SearchPanel
            value={picks}
            onChange={setPicks}
            facetFilter={facetFilter}
          />
        </div>

        <Head>정렬 · 묶기</Head>
        <div className="px-4 flex items-center gap-2">
          <SortMenu value={sort} onChange={setSort} />
          <GroupMenu value={group} onChange={setGroup} />
        </div>

        <div className="px-5 py-4 mt-2 flex justify-end gap-2 border-t border-line">
          <Btn onClick={onClose}>취소</Btn>
          <Btn tone="accent" disabled={saving || !name.trim()} onClick={save}>
            {initial && initial.id !== 0 ? "저장" : "만들기"}
          </Btn>
        </div>
      </div>
    </div>
  );
}

function Head({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-5 pt-4 pb-1.5 text-[10.5px] uppercase tracking-wider text-fg-mute">
      {children}
    </div>
  );
}

function Chip({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`h-control px-2.5 rounded-md text-[12px] ${
        on ? "bg-accent text-accent-fg" : "bg-raised text-fg-dim hover:text-fg"
      }`}
    >
      {children}
    </button>
  );
}

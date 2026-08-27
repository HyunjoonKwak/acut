import { useMemo } from "react";
import { Btn } from "./ui";
import { useData } from "./dataStore";
import { visible } from "./folderTree";
import { usePref } from "./prefs";
import { useUi } from "./uiStore";
import { useView } from "./viewStore";
import type { Library } from "./types";

/**
 * 앨범 — 라이브러리를 뿌리로 한 폴더 트리.
 *
 * 라이브러리 줄에는 다시 스캔(⟳)과 「⋯」가 붙는다. ⟳ 옆에 지우기를 두지
 * 않는다 — 실제로 잘못 눌려 라이브러리가 통째로 날아갔다. 지우기는 「⋯」
 * 안으로.
 */
export default function AlbumTree({
  rescan,
  addLibrary,
  dropLibrary,
}: {
  rescan: (ids: number[]) => void;
  addLibrary: () => void;
  dropLibrary: (l: Library) => void;
}) {
  const libs = useData((s) => s.libs);
  const folders = useData((s) => s.folders);
  const open = useData((s) => s.open);
  const toggleOpen = useData((s) => s.toggleOpen);
  const sel = useView((s) => s.sel);
  const setSel = useView((s) => s.setSel);
  const setViewTrash = useView((s) => s.setViewTrash);
  const [libId, setLibId] = usePref("libId");
  const menuFor = useUi((s) => s.menuFor);
  const setUi = useUi((s) => s.set);
  const openImport = () => setUi({ importing: true });

  /// 접힌 마디의 자식은 그리지 않는다. 셈은 folderTree.ts에 있다.
  const rows = useMemo(() => visible(folders, open), [folders, open]);

  return (
    <>
      {folders.length === 0 ? (
        <div className="px-3 py-2 text-[12px] text-fg-mute leading-relaxed">
          등록된 라이브러리가 없습니다.
          <br />
          아래 「라이브러리 추가」를 누르세요.
        </div>
      ) : (
        rows.map((f) => {
          const root = f.is_library;
          const on = root
            ? sel === null && libId === f.library_id
            : sel?.path === f.path;
          const lib = root
            ? libs.find((l) => l.id === f.library_id)
            : undefined;
          return (
            <div
              key={f.path}
              className={`group relative flex items-center pr-1 ${
                on ? "bg-raised" : "hover:bg-chrome"
              }`}
              style={{ paddingLeft: 4 + f.depth * 11 }}
            >
              {/* 펼침 삼각형 — 자식이 없으면 자리만 차지한다 */}
              <button
                onClick={() => f.has_children && toggleOpen(f.path)}
                className={`w-4 shrink-0 text-[9px] ${
                  f.has_children
                    ? "text-fg-mute hover:text-fg"
                    : "text-transparent"
                }`}
              >
                {open.has(f.path) ? "▼" : "▶"}
              </button>
              <button
                onClick={() => {
                  setLibId(f.library_id);
                  // 라이브러리 마디를 누르면 그 라이브러리 전체
                  setSel(
                    root
                      ? null
                      : { libId: f.library_id, path: f.path, rel: f.rel_path },
                  );
                  setViewTrash(false);
                }}
                title={f.rel_path || f.name}
                className={`flex-1 min-w-0 text-left py-1 truncate ${
                  root ? "font-semibold" : ""
                } ${on ? "text-fg" : "text-fg-dim"} ${lib && !lib.online ? "opacity-50" : ""}`}
              >
                {root && (
                  <span
                    className="inline-block w-1.5 h-1.5 rounded-full mr-1.5 align-middle"
                    style={{
                      background: lib?.online
                        ? "var(--color-accent)"
                        : "var(--color-fg-faint)",
                    }}
                  />
                )}
                {f.name}
              </button>
              <span
                className={`text-fg-mute tabular-nums text-[11px] shrink-0 pl-1.5 ${
                  root ? "group-hover:invisible" : ""
                }`}
              >
                {f.file_count.toLocaleString()}
              </span>

              {root && lib && (
                <div className="absolute right-1 hidden group-hover:flex bg-raised rounded">
                  <button
                    onClick={() => rescan([lib.id])}
                    disabled={!lib.online}
                    title="이 라이브러리 다시 스캔"
                    className="px-1.5 text-fg-mute hover:text-accent disabled:opacity-30"
                  >
                    ⟳
                  </button>
                  <button
                    onClick={() =>
                      setUi({ menuFor: menuFor === lib.id ? null : lib.id })
                    }
                    title="더 보기"
                    className="px-1.5 text-fg-mute hover:text-fg"
                  >
                    ⋯
                  </button>
                </div>
              )}
              {root && lib && menuFor === lib.id && (
                <div className="absolute right-1 top-7 z-20 bg-raised rounded-md ring-1 ring-line-strong shadow-lg py-1">
                  <button
                    onClick={() => {
                      setUi({ menuFor: null });
                      dropLibrary(lib);
                    }}
                    className="block w-full text-left px-3 py-1.5 text-[12px] text-drop hover:bg-hover whitespace-nowrap"
                  >
                    목록에서 빼기
                  </button>
                </div>
              )}
            </div>
          );
        })
      )}
      <div className="px-2 pt-2 flex flex-wrap gap-1">
        <Btn onClick={openImport}>↓ 가져오기…</Btn>
        <Btn onClick={addLibrary}>＋ 라이브러리 추가…</Btn>
      </div>
    </>
  );
}

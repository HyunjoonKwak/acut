import { useMemo } from "react";
import { Btn } from "./ui";
import { useData } from "./dataStore";
import { visible } from "./folderTree";
import { usePref } from "./prefs";
import { useUi } from "./uiStore";
import { useJob } from "./jobStore";
import { useView } from "./viewStore";
import type { Library } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { AREAS, areaLabel, groupByArea } from "./areaItems";

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
  const folderMenu = useUi((s) => s.folderMenu);
  const busy = useJob((s) => s.job !== null);
  const setUi = useUi((s) => s.set);
  const openImport = () => setUi({ importing: true });

  /// 접힌 마디의 자식은 그리지 않는다. 셈은 folderTree.ts에 있다.
  const rows = useMemo(() => visible(folders, open), [folders, open]);
  /// 영역별로 — 작업대 → 내사진 → 공용 → 기타. 흐름이 곧 순서다.
  const groups = useMemo(
    () => groupByArea(rows, (id) => libs.find((l) => l.id === id)?.area ?? 3),
    [rows, libs],
  );
  const refreshLibs = useData((s) => s.refreshLibs);
  const setArea = async (lib: Library, area: number) => {
    setUi({ menuFor: null });
    await invoke("library_set_area", { id: lib.id, area });
    await refreshLibs();
  };

  return (
    <>
      {folders.length === 0 ? (
        <div className="px-3 py-2 text-[13px] text-fg-mute leading-relaxed">
          등록된 라이브러리가 없습니다.
          <br />
          아래 「라이브러리 추가」를 누르세요.
        </div>
      ) : (
        groups.map((g) => (
          <div key={g.area}>
            <div className="px-3 pt-3 pb-1 text-[11.5px] uppercase tracking-wider text-fg-mute">
              {areaLabel(g.area)}
            </div>
            {g.rows.map((f) => {
              const root = f.is_library;
              const on = root
                ? sel === null && libId === f.library_id
                : sel?.path === f.path;
              const lib = root
                ? libs.find((l) => l.id === f.library_id)
                : undefined;
              const folderRel = root
                ? ""
                : f.path.split("/").slice(1).join("/");
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
                    className={`w-4 shrink-0 text-[10px] ${
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
                          : {
                              libId: f.library_id,
                              path: f.path,
                              rel: f.rel_path,
                            },
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
                    className={`text-fg-mute tabular-nums text-[12px] shrink-0 pl-1.5 ${
                      root ? "pr-14" : ""
                    }`}
                  >
                    {f.file_count.toLocaleString()}
                  </span>

                  {/* 라이브러리 줄의 ⟳·⋯ — 늘 보이고 누를 자리가 넉넉하게(«너무 작고 누르기 힘들다» 2026-08-30) */}
                  {root && lib && (
                    <div className="absolute right-1 flex items-center gap-0.5">
                      <button
                        onClick={() => rescan([lib.id])}
                        disabled={!lib.online || busy}
                        title={
                          busy
                            ? "스캔이 도는 중입니다"
                            : `«${lib.name}» 다시 스캔 — 새 사진을 넣고 사라진 사진의 기록을 정리합니다`
                        }
                        className="h-6 w-7 rounded-md flex items-center justify-center text-[16px] leading-none text-fg-mute hover:text-accent hover:bg-hover disabled:opacity-30"
                      >
                        ⟳
                      </button>
                      <button
                        onClick={() =>
                          setUi({ menuFor: menuFor === lib.id ? null : lib.id })
                        }
                        title="더 보기"
                        className="h-6 w-6 rounded-md flex items-center justify-center text-fg-mute hover:text-fg hover:bg-hover"
                      >
                        ⋯
                      </button>
                    </div>
                  )}
                  {/* 폴더 줄 — 다른 디스크로 옮기기. 라이브러리 자체는 못 옮긴다 */}
                  {!root && (
                    <div className="absolute right-1 hidden group-hover:flex bg-raised rounded">
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: folderMenu === f.path ? null : f.path,
                          })
                        }
                        title="더 보기"
                        className="px-1.5 text-fg-mute hover:text-fg"
                      >
                        ⋯
                      </button>
                    </div>
                  )}
                  {!root && folderMenu === f.path && (
                    <div className="absolute right-1 top-7 z-20 bg-raised rounded-md ring-1 ring-line-strong shadow-lg py-1">
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: null,
                            folderOperation: {
                              action: "create",
                              sourceLibraryId: f.library_id,
                              sourceDir: folderRel,
                              sourceName: f.name,
                            },
                          })
                        }
                        disabled={busy}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        안에 새 폴더…
                      </button>
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: null,
                            folderOperation: {
                              action: "rename",
                              sourceLibraryId: f.library_id,
                              sourceDir: folderRel,
                              sourceName: f.name,
                            },
                          })
                        }
                        disabled={busy}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        이름 변경…
                      </button>
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: null,
                            folderOperation: {
                              action: "move",
                              sourceLibraryId: f.library_id,
                              sourceDir: folderRel,
                              sourceName: f.name,
                            },
                          })
                        }
                        disabled={busy}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        이동·복사…
                      </button>
                      <div className="my-1 border-t border-line" />
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: null,
                            captureDate: {
                              ids: [],
                              libraryId: f.library_id,
                              relPath: f.path.split("/").slice(1).join("/"),
                            },
                          })
                        }
                        disabled={busy}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        촬영일 감사·교정…
                      </button>
                      {f.id !== null && (
                        <button
                          onClick={() =>
                            setUi({
                              folderMenu: null,
                              offload: {
                                folderId: f.id!,
                                name: f.name,
                                libraryId: f.library_id,
                              },
                            })
                          }
                          disabled={busy}
                          className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                        >
                          다른 디스크로 옮기기…
                        </button>
                      )}
                      <button
                        onClick={() =>
                          setUi({
                            folderMenu: null,
                            folderOperation: {
                              action: "trash",
                              sourceLibraryId: f.library_id,
                              sourceDir: folderRel,
                              sourceName: f.name,
                            },
                          })
                        }
                        disabled={busy}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-drop hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        폴더를 휴지통으로…
                      </button>
                    </div>
                  )}
                  {root && lib && menuFor === lib.id && (
                    <div className="absolute right-1 top-7 z-20 bg-raised rounded-md ring-1 ring-line-strong shadow-lg py-1">
                      <div className="px-3 pt-1 pb-0.5 text-[11px] uppercase tracking-wider text-fg-faint">
                        역할
                      </div>
                      {AREAS.map((a) => (
                        <button
                          key={a.v}
                          onClick={() => setArea(lib, a.v)}
                          title={a.hint}
                          className={`block w-full text-left px-3 py-1 text-[13px] hover:bg-hover whitespace-nowrap ${
                            lib.area === a.v
                              ? "text-fg font-semibold"
                              : "text-fg-dim"
                          }`}
                        >
                          {lib.area === a.v ? "✓ " : "\u2007\u2007"}
                          {a.label}
                        </button>
                      ))}
                      <div className="my-1 border-t border-line" />
                      <button
                        onClick={() =>
                          setUi({
                            menuFor: null,
                            folderOperation: {
                              action: "create",
                              sourceLibraryId: lib.id,
                              sourceDir: "",
                              sourceName: lib.name,
                            },
                          })
                        }
                        disabled={busy || !lib.online}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap disabled:opacity-40"
                      >
                        새 폴더…
                      </button>
                      <button
                        onClick={() => setUi({ menuFor: null, husks: { libraryId: lib.id, name: lib.name } })}
                        title="사진을 다 치운 뒤 메모·썸네일·zip 만 남은 폴더들을 찾아 휴지통으로"
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-fg-dim hover:bg-hover whitespace-nowrap"
                      >
                        사진 없는 폴더 정리…
                      </button>
                      <button
                        onClick={() => {
                          setUi({ menuFor: null });
                          dropLibrary(lib);
                        }}
                        className="block w-full text-left px-3 py-1.5 text-[13px] text-drop hover:bg-hover whitespace-nowrap"
                      >
                        목록에서 빼기
                      </button>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        ))
      )}
      <div className="px-2 pt-2 flex flex-wrap gap-1">
        <Btn onClick={openImport}>↓ 가져오기…</Btn>
        <Btn onClick={addLibrary}>＋ 라이브러리 추가…</Btn>
      </div>
    </>
  );
}

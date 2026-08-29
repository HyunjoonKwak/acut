import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import type { Library } from "./types";

/**
 * 정리 — «NAS에 이미 있는 것은 지우고, 없는 것은 옮긴다».
 *
 * 완전 중복 무리를 폴더 기준으로 다시 보여 준다. 왼쪽은 폴더 목록(몇 %가 이미
 * NAS에 있나), 오른쪽은 고른 폴더의 사진을 두 묶음으로: 🔴 NAS에 이미 있음(지워도
 * 됨) / 🟢 NAS에 없음(옮겨야 함). 무리·대표·제외 같은 말은 여기 없다.
 */

type Folder = {
  folder_id: number;
  folder: string;
  total: number;
  have: number;
  have_bytes: number;
  inner: number;
  inner_bytes: number;
  keeper_library: string | null;
  keeper_folder: string | null;
  keeper_copies: number;
};
type File = {
  file_id: number;
  name: string;
  size: number;
  kind: number;
  library_id: number | null;
  thumb: string | null;
  cat: "have" | "inner" | "none";
  keeper: string | null;
};
type Summary = {
  have: number;
  have_bytes: number;
  inner: number;
  inner_bytes: number;
  settled_groups: number;
  settled_files: number;
};
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };

const thumbUrl = (f: File) =>
  f.thumb && f.library_id !== null
    ? `thumb://localhost/${f.library_id}/${f.thumb.split("/").map(encodeURIComponent).join("/")}`
    : null;

export default function Cleanup({
  libs,
  onOrganize,
  onChanged,
}: {
  /** 정리할 수 있는 곳 — 정착 구역이 아닌 라이브러리들 */
  libs: Library[];
  /** «공용으로 정리…» — 격자로 나가 이 사진들을 골라 정리 대화상자를 연다 */
  onOrganize: (ids: number[], libraryId: number) => void;
  /** 지우기 표시가 바뀌었다 — 바깥의 숫자를 새로 읽으라고 */
  onChanged: () => void;
}) {
  const ask = useConfirm();
  const [libId, setLibId] = useState<number | null>(libs[0]?.id ?? null);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [folders, setFolders] = useState<Folder[] | null>(null);
  const [sel, setSel] = useState<number | null>(null);
  const [files, setFiles] = useState<File[] | null>(null);
  const [tick, setTick] = useState(0);

  // 라이브러리가 바뀌거나 표시를 하고 나면 전부 새로 읽는다
  useEffect(() => {
    if (libId === null) return;
    let live = true;
    invoke<Summary>("cleanup_summary", { libraryId: libId }).then(
      (s) => live && setSummary(s),
    );
    invoke<Folder[]>("cleanup_folders", { libraryId: libId }).then((f) => {
      if (!live) return;
      setFolders(f);
      setSel((cur) => (cur !== null && f.some((x) => x.folder_id === cur) ? cur : (f[0]?.folder_id ?? null)));
    });
    return () => {
      live = false;
    };
  }, [libId, tick]);

  useEffect(() => {
    if (sel === null) return;
    let live = true;
    invoke<File[]>("cleanup_files", { folderId: sel }).then(
      (f) => live && setFiles(f),
    );
    return () => {
      live = false;
    };
  }, [sel, tick]);

  const folder = folders?.find((f) => f.folder_id === sel) ?? null;
  const have = useMemo(() => files?.filter((f) => f.cat === "have") ?? [], [files]);
  const inner = useMemo(() => files?.filter((f) => f.cat === "inner") ?? [], [files]);
  const none = useMemo(() => files?.filter((f) => f.cat === "none") ?? [], [files]);

  /// 지우기 표시 — 폴더 하나 또는 라이브러리 전부. 먼저 세어 묻는다.
  const mark = useCallback(
    async (folderId: number | null, what: string) => {
      if (libId === null) return;
      const args = { kind: 0, skipSettled: true, folderId, libraryId: libId };
      const dry = await invoke<ApplyAll>("cull_apply_all", { ...args, dryRun: true });
      if (dry.groups === 0) {
        toast("지우기 표시할 것이 없습니다");
        return;
      }
      const ok = await ask({
        title: `${what} — ${dry.rejected.toLocaleString()}장에 지우기 표시`,
        lines: [
          "원본(NAS 또는 다른 폴더의 것)은 그대로 둡니다",
          "파일은 아직 옮기지 않습니다 — 격자의 «제외 N장 치우기»로 휴지통에 보냅니다",
          ...(dry.skipped > 0
            ? [`공용·내사진 안에서 겹치는 ${dry.skipped.toLocaleString()}무리는 건너뜁니다 — «공용 안 겹침»에서 하나씩`]
            : []),
        ],
        confirmLabel: "지우기 표시",
      });
      if (!ok) return;
      const r = await invoke<ApplyAll>("cull_apply_all", { ...args, dryRun: false });
      toast(`${r.rejected.toLocaleString()}장에 지우기 표시 — 격자에서 «치우기»로 휴지통에 보냅니다`, "ok");
      setTick((t) => t + 1);
      onChanged();
    },
    [libId, ask, onChanged],
  );

  const lib = libs.find((l) => l.id === libId) ?? null;
  const gb = (n: number) => fmtBytes(n);

  if (libs.length === 0)
    return (
      <div className="h-full flex items-center justify-center text-fg-mute">
        정리할 라이브러리가 없습니다 — 작업대·기타 역할의 라이브러리를 등록하세요
      </div>
    );

  return (
    <div className="h-full flex flex-col">
      {/* 위 — 어디를, 얼마나 */}
      <div className="h-11 shrink-0 flex items-center gap-3 px-4 border-b border-line text-[12.5px]">
        <span className="text-fg-mute">정리할 곳</span>
        <select
          value={libId ?? ""}
          onChange={(e) => {
            setLibId(Number(e.target.value));
            setFolders(null);
            setFiles(null);
            setSel(null);
          }}
          aria-label="정리할 라이브러리"
          className="h-7 px-2 rounded-md bg-raised text-fg ring-1 ring-line outline-none"
        >
          {libs.map((l) => (
            <option key={l.id} value={l.id}>
              {l.name}
            </option>
          ))}
        </select>
        {summary && (
          <span className="text-fg-dim tabular-nums">
            NAS에 이미 있는 사진{" "}
            <b className="text-keep">
              {summary.have.toLocaleString()}장 · {gb(summary.have_bytes)}
            </b>
            {summary.inner > 0 && (
              <span className="text-fg-mute">
                {" "}· 안에서 겹치는 사본 {summary.inner.toLocaleString()}장 · {gb(summary.inner_bytes)}
              </span>
            )}
          </span>
        )}
        <div className="flex-1" />
        {summary && summary.have + summary.inner > 0 && lib && (
          <button
            onClick={() => mark(null, `${lib.name} 전부`)}
            className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px]"
          >
            이 라이브러리 전부 지우기 표시
          </button>
        )}
      </div>

      <div className="flex-1 min-h-0 flex">
        {/* 왼쪽 — 폴더 목록 */}
        <div className="w-[420px] shrink-0 border-r border-line overflow-y-auto">
          <div className="grid grid-cols-[1fr_92px_64px_60px] gap-2 px-3 py-1.5 text-[10.5px] uppercase tracking-wider text-fg-mute border-b border-line">
            <span>폴더 (지울 게 많은 순)</span>
            <span className="text-right">있음 / 전체</span>
            <span />
            <span className="text-right">비는 용량</span>
          </div>
          {folders === null ? (
            <div className="px-3 py-6 text-fg-mute text-[12px] flex items-center gap-2">
              <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 폴더별로 세는 중…
            </div>
          ) : folders.length === 0 ? (
            <div className="px-3 py-6 text-fg-mute text-[12px]">
              지워도 되는 사본이 있는 폴더가 없습니다
            </div>
          ) : (
            folders.map((f) => {
              const dup = f.have + f.inner;
              const pct = f.total ? Math.round((dup / f.total) * 100) : 0;
              const on = f.folder_id === sel;
              return (
                <button
                  key={f.folder_id}
                  onClick={() => setSel(f.folder_id)}
                  className={`w-full grid grid-cols-[1fr_92px_64px_60px] gap-2 items-center px-3 py-2 text-left text-[12.5px] tabular-nums border-b border-line ${
                    on ? "bg-raised shadow-[inset_3px_0_0_var(--color-keep)]" : "hover:bg-hover"
                  }`}
                >
                  <span className="truncate text-fg" title={f.folder || "/"}>
                    {f.folder || "/"}
                  </span>
                  <span className="text-right text-fg-dim">
                    {dup.toLocaleString()} / {f.total.toLocaleString()}
                  </span>
                  <span className="flex items-center gap-1">
                    <span className="flex-1 h-1.5 rounded bg-line overflow-hidden">
                      <i className="block h-full bg-drop/85" style={{ width: `${pct}%` }} />
                    </span>
                    {dup === f.total && (
                      <span className="text-[9.5px] font-bold px-1 rounded bg-drop/20 text-drop">
                        전부
                      </span>
                    )}
                  </span>
                  <span className="text-right text-fg-mute">{gb(f.have_bytes + f.inner_bytes)}</span>
                </button>
              );
            })
          )}
        </div>

        {/* 오른쪽 — 고른 폴더의 사진, 두 묶음 */}
        <div className="flex-1 min-w-0 overflow-y-auto p-4">
          {folder === null ? (
            <div className="h-full flex items-center justify-center text-fg-mute">
              왼쪽에서 폴더를 고르세요
            </div>
          ) : (
            <>
              <div className="text-[15px] font-semibold mb-3">
                <span className="text-fg-mute font-medium">{lib?.name}</span> · {folder.folder || "/"}
                <span className="text-fg-mute font-medium text-[12.5px] ml-2">
                  {folder.total.toLocaleString()}장
                </span>
              </div>

              <Section
                tone="del"
                title={
                  <>
                    NAS에 이미 있음 <b>{folder.have.toLocaleString()}장</b> · {gb(folder.have_bytes)}
                  </>
                }
                hint={
                  folder.have > 0
                    ? `지워도 됩니다 — 원본: ${folder.keeper_library ?? ""} · ${folder.keeper_folder ?? "/"} (${folder.keeper_copies.toLocaleString()}장)${
                        folder.inner > 0 ? ` · 그 밖에 ${lib?.name} 안에서 겹치는 사본 ${folder.inner.toLocaleString()}장` : ""
                      }`
                    : `${lib?.name} 안에서만 겹치는 사본 ${folder.inner.toLocaleString()}장 — 원본은 다른 폴더에`
                }
                action={
                  folder.have + folder.inner > 0 ? (
                    <button
                      onClick={() => mark(folder.folder_id, `«${folder.folder || "/"}» 폴더`)}
                      className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px]"
                    >
                      {(folder.have + folder.inner).toLocaleString()}장 지우기 표시
                    </button>
                  ) : null
                }
              >
                {files === null ? (
                  <Loading />
                ) : (
                  <Grid files={[...have, ...inner]} dim />
                )}
              </Section>

              <Section
                tone="ok"
                title={
                  <>
                    NAS에 없음 <b>{none.length.toLocaleString()}장</b>
                  </>
                }
                hint={
                  none.length > 0
                    ? `${lib?.name}에만 있습니다 — 옮겨야 합니다`
                    : "옮길 것이 없습니다 — 지우기 표시 뒤 이 폴더는 비워도 됩니다"
                }
                action={
                  none.length > 0 && libId !== null ? (
                    <button
                      onClick={() => onOrganize(none.map((f) => f.file_id), libId)}
                      className="h-7 px-3 rounded-md bg-accent text-accent-fg font-semibold text-[12.5px]"
                    >
                      공용으로 정리…
                    </button>
                  ) : null
                }
              >
                {files === null ? <Loading /> : none.length > 0 ? <Grid files={none} /> : null}
              </Section>
            </>
          )}
        </div>
      </div>

      <div className="h-9 shrink-0 flex items-center gap-3 px-4 border-t border-line text-[12px] text-fg-mute">
        지우기 표시는 도장일 뿐입니다 — 격자로 나가 «제외 N장 치우기»를 눌러야 휴지통(같은 디스크
        안, 되돌리기 가능)으로 갑니다.
        <span className="ml-auto">공용·내사진 안에서 겹치는 것은 «공용 안 겹침» 탭에서 하나씩</span>
      </div>
    </div>
  );
}

function Section({
  tone,
  title,
  hint,
  action,
  children,
}: {
  tone: "del" | "ok";
  title: React.ReactNode;
  hint: string;
  action: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded-lg border border-line bg-chrome p-3 mb-3">
      <header className="flex items-center gap-3 flex-wrap mb-2">
        <i className={`w-2.5 h-2.5 rounded-full ${tone === "del" ? "bg-drop" : "bg-ok"}`} />
        <h3 className="text-[13.5px] font-semibold m-0">{title}</h3>
        <span className="text-[12px] text-fg-mute flex-1 min-w-[200px] truncate" title={hint}>
          {hint}
        </span>
        {action}
      </header>
      {children}
    </section>
  );
}

function Loading() {
  return (
    <div className="py-4 text-[12px] text-fg-mute flex items-center gap-2">
      <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 읽는 중…
    </div>
  );
}

/** 사진 격자 — 지워질 것은 흐리게, 아래에 원본이 있는 곳 */
function Grid({ files, dim }: { files: File[]; dim?: boolean }) {
  const [limit, setLimit] = useState(120);
  const shown = files.slice(0, limit);
  return (
    <>
      <div className="grid gap-2.5" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(130px, 1fr))" }}>
        {shown.map((f) => {
          const u = thumbUrl(f);
          return (
            <figure key={f.file_id} className="m-0 relative">
              {u ? (
                <img
                  src={u}
                  loading="lazy"
                  alt=""
                  className="w-full rounded-md object-cover bg-canvas"
                  style={{ aspectRatio: "4/3", opacity: dim ? 0.45 : 1 }}
                />
              ) : (
                <div className="w-full rounded-md bg-canvas flex items-center justify-center text-fg-faint text-[10px]" style={{ aspectRatio: "4/3" }}>
                  {f.kind === 1 ? "영상" : "…"}
                </div>
              )}
              {dim && (
                <span className="absolute top-1.5 left-1.5 text-[10px] font-bold px-1.5 rounded bg-drop/90 text-drop-fg">
                  지움
                </span>
              )}
              <figcaption
                className={`text-[10.5px] mt-1 truncate ${dim ? "text-keep" : "text-fg-mute"}`}
                title={f.keeper ? `${f.name} → ${f.keeper}` : f.name}
              >
                {f.keeper ? `→ ${f.keeper}` : f.name}
              </figcaption>
            </figure>
          );
        })}
      </div>
      {files.length > limit && (
        <button
          onClick={() => setLimit((n) => n + 240)}
          className="mt-3 h-7 px-3 rounded-md text-[12px] text-fg-dim ring-1 ring-line-strong"
        >
          {(files.length - limit).toLocaleString()}장 더 보기
        </button>
      )}
    </>
  );
}

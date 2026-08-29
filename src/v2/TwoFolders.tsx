import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { useData } from "./dataStore";
import type { FolderRow } from "./types";

/**
 * 두 폴더 비교 — «후보1번/연도별»과 «후보2번»처럼 내가 고른 두 폴더 아래를 견준다.
 *
 * 줄마다 A쪽 폴더 ⇔ B쪽 폴더. 내용이 완전히 같으면 ✓ 똑같음, 이름만 같으면
 * «n/m 똑같음», 한쪽에만 있으면 그대로. 같은 것은 어느 쪽을 지울지 골라 한 번에.
 */

type FolderIn = {
  folder_id: number;
  library_id: number;
  library: string;
  folder: string;
  area: number;
};
type PairRow = {
  a: FolderIn | null;
  b: FolderIn | null;
  files_a: number;
  files_b: number;
  same: boolean;
  common: number;
  bytes: number;
};
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };

const settled = (f: FolderIn) => f.area === 1 || f.area === 2;

export default function TwoFolders({ onChanged }: { onChanged: () => void }) {
  const ask = useConfirm();
  const libs = useData((s) => s.libs);
  const [all, setAll] = useState<FolderRow[]>([]);
  const [a, setA] = useState<FolderRow | null>(null);
  const [b, setB] = useState<FolderRow | null>(null);
  const [rows, setRows] = useState<PairRow[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    invoke<FolderRow[]>("folders_list", { libraryId: null }).then(
      (f) => live && setAll(f.filter((x) => x.id !== null)),
    );
    return () => {
      live = false;
    };
  }, []);

  const libName = useCallback(
    (id: number) => libs.find((l) => l.id === id)?.name ?? "",
    [libs],
  );

  const compare = useCallback(async () => {
    if (!a?.id || !b?.id) return;
    setBusy(true);
    try {
      setRows(
        await invoke<PairRow[]>("cull_compare_folders", { aFolderId: a.id, bFolderId: b.id }),
      );
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setBusy(false);
    }
  }, [a, b]);

  /// 같은 폴더 짝에서 한쪽에 지우기 표시. side = 지울 쪽
  const mark = useCallback(
    async (targets: PairRow[], side: "a" | "b") => {
      const pairs = targets.filter((r) => r.same && r.a && r.b);
      if (pairs.length === 0) {
        toast("똑같은 폴더 짝이 없습니다");
        return;
      }
      const drop = (r: PairRow) => (side === "a" ? r.a! : r.b!);
      const keep = (r: PairRow) => (side === "a" ? r.b! : r.a!);
      const risky = pairs.filter((r) => settled(drop(r)));
      const bytes = pairs.reduce((s, r) => s + r.bytes, 0);
      const files = pairs.reduce((s, r) => s + r.common, 0);
      const ok = await ask({
        title: `${side === "a" ? "A" : "B"}쪽 폴더 ${pairs.length.toLocaleString()}개의 사진 ${files.toLocaleString()}장에 지우기 표시`,
        lines: [
          `${side === "a" ? "B" : "A"}쪽 똑같은 폴더는 그대로 둡니다 · ${fmtBytes(bytes)} 빔`,
          ...(risky.length
            ? [`주의: ${risky.length}개는 NAS 동기화 폴더입니다 — 치우면 NAS에서도 지워집니다`]
            : []),
          "파일은 아직 옮기지 않습니다 — 격자의 «제외 N장 치우기»로 휴지통에 보냅니다",
        ],
        confirmLabel: "지우기 표시",
        danger: risky.length > 0,
      });
      if (!ok) return;
      let failed = 0;
      for (const r of pairs) {
        try {
          await invoke<ApplyAll>("cull_folder_set_apply", {
            keepFolderId: keep(r).folder_id,
            dropFolderIds: [drop(r).folder_id],
          });
        } catch {
          failed += 1;
        }
      }
      toast(
        failed
          ? `${pairs.length - failed}개 처리 · ${failed}개 실패`
          : `${pairs.length.toLocaleString()}개 폴더에 지우기 표시했습니다 — 격자에서 «치우기»`,
        failed ? "drop" : "ok",
      );
      onChanged();
      void compare();
    },
    [ask, onChanged, compare],
  );

  const same = useMemo(() => rows?.filter((r) => r.same) ?? [], [rows]);
  const sameBytes = same.reduce((s, r) => s + r.bytes, 0);

  return (
    <div className="h-full flex flex-col">
      <div className="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-line text-[12.5px] flex-wrap">
        <Picker label="A" value={a} all={all} libName={libName} onPick={setA} exclude={b} />
        <span className="text-fg-mute">⇔</span>
        <Picker label="B" value={b} all={all} libName={libName} onPick={setB} exclude={a} />
        <button
          onClick={compare}
          disabled={!a || !b || busy}
          className="h-7 px-3 rounded-md bg-accent text-accent-fg font-semibold text-[12.5px] disabled:opacity-40"
        >
          {busy ? "견주는 중…" : "비교"}
        </button>
        {rows && (
          <span className="text-fg-dim tabular-nums">
            똑같은 폴더 <b className="text-fg">{same.length.toLocaleString()}</b>쌍
            {same.length > 0 && (
              <>
                {" "}· 한쪽을 지우면 <b className="text-keep">{fmtBytes(sameBytes)}</b> 빔
              </>
            )}
          </span>
        )}
        <div className="flex-1" />
        {same.length > 0 && (
          <>
            <button
              onClick={() => mark(same, "b")}
              className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px]"
            >
              B쪽 똑같은 폴더 전부 지우기 표시
            </button>
            <button
              onClick={() => mark(same, "a")}
              className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px]"
            >
              A쪽 전부
            </button>
          </>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        {rows === null ? (
          <div className="h-full flex items-center justify-center text-fg-mute text-[13px]">
            위에서 두 폴더를 고르고 «비교»를 누르세요 — 예: A 통합전후보 · 후보1번/연도별, B 통합전후보 · 후보2번
          </div>
        ) : rows.length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-mute">
            두 폴더 아래에 사진이 없습니다
          </div>
        ) : (
          <table className="w-full text-[12.5px] tabular-nums">
            <thead className="text-[10.5px] text-fg-mute uppercase tracking-wider sticky top-0 bg-canvas">
              <tr className="text-left">
                <th className="py-1.5 px-4 font-medium">A · {a && `${libName(a.library_id)} · ${a.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium">B · {b && `${libName(b.library_id)} · ${b.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium text-right">장수</th>
                <th className="py-1.5 pr-3 font-medium">상태</th>
                <th className="py-1.5 pr-4 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i} className="border-t border-line align-middle">
                  <td className="py-1.5 px-4 max-w-[380px]">
                    <Cell f={r.a} sub={a} />
                  </td>
                  <td className="py-1.5 pr-3 max-w-[380px]">
                    <Cell f={r.b} sub={b} />
                  </td>
                  <td className="py-1.5 pr-3 text-right whitespace-nowrap text-fg-dim">
                    {r.same
                      ? r.files_a.toLocaleString()
                      : r.a && r.b
                        ? `${r.files_a.toLocaleString()} / ${r.files_b.toLocaleString()}`
                        : (r.files_a || r.files_b).toLocaleString()}
                  </td>
                  <td className="py-1.5 pr-3 whitespace-nowrap">
                    {r.same ? (
                      <span className="text-ok font-semibold">✓ 똑같음 · {fmtBytes(r.bytes)}</span>
                    ) : r.a && r.b ? (
                      <span className="text-keep">
                        {r.common.toLocaleString()}장 똑같음{" "}
                        <span className="text-fg-mute">— 나머지는 개별 비교에서</span>
                      </span>
                    ) : r.a ? (
                      <span className="text-fg-mute">A에만 있음</span>
                    ) : (
                      <span className="text-fg-mute">B에만 있음</span>
                    )}
                  </td>
                  <td className="py-1.5 pr-4 text-right whitespace-nowrap">
                    {r.same && (
                      <>
                        <button
                          onClick={() => mark([r], "b")}
                          className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong mr-1"
                          title="B쪽 폴더의 사진에 지우기 표시"
                        >
                          B쪽 지우기
                        </button>
                        <button
                          onClick={() => mark([r], "a")}
                          className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong"
                          title="A쪽 폴더의 사진에 지우기 표시"
                        >
                          A쪽 지우기
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

/** 폴더 칸 — 고른 뿌리 아래 경로만 보인다 */
function Cell({ f, sub }: { f: FolderIn | null; sub: FolderRow | null }) {
  if (!f) return <span className="text-fg-faint">—</span>;
  const rootPath = sub?.path ?? "";
  const shown =
    rootPath && f.folder.startsWith(rootPath)
      ? f.folder.slice(rootPath.length).replace(/^\//, "") || "(이 폴더)"
      : f.folder || "/";
  return (
    <span className="truncate block" title={`${f.library} / ${f.folder || "/"}`}>
      {settled(f) && <span className="text-keep text-[10px] mr-1">NAS</span>}
      {shown}
    </span>
  );
}

/** 폴더 고르기 — 글자로 걸러 고른다. 폴더가 수천 개라 목록 전체는 못 보여 준다 */
function Picker({
  label,
  value,
  all,
  libName,
  onPick,
  exclude,
}: {
  label: string;
  value: FolderRow | null;
  all: FolderRow[];
  libName: (id: number) => string;
  onPick: (f: FolderRow) => void;
  exclude: FolderRow | null;
}) {
  const [q, setQ] = useState("");
  const [open, setOpen] = useState(false);
  const hits = useMemo(() => {
    const t = q.trim().toLowerCase();
    return all
      .filter((f) => f.id !== exclude?.id)
      .filter((f) => !t || `${libName(f.library_id)} ${f.path}`.toLowerCase().includes(t))
      .slice(0, 40);
  }, [q, all, exclude, libName]);
  return (
    <div className="relative flex items-center gap-1.5">
      <span className="text-fg-mute font-semibold">{label}</span>
      <input
        value={open ? q : value ? `${libName(value.library_id)} · ${value.path || "/"}` : q}
        placeholder="폴더 이름으로 찾기…"
        onFocus={() => {
          setOpen(true);
          setQ("");
        }}
        onBlur={() => window.setTimeout(() => setOpen(false), 150)}
        onChange={(e) => setQ(e.target.value)}
        className="h-7 w-[300px] px-2 rounded-md bg-raised text-fg ring-1 ring-line outline-none focus:ring-accent"
      />
      {open && (
        <ul className="absolute left-4 top-8 z-40 w-[420px] max-h-72 overflow-y-auto scroll-thin bg-raised rounded-lg ring-1 ring-line-strong shadow-2xl py-1">
          {hits.length === 0 && <li className="px-3 py-1.5 text-fg-mute">없음</li>}
          {hits.map((f) => (
            <li key={`${f.library_id}-${f.id}`}>
              <button
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  onPick(f);
                  setOpen(false);
                }}
                className="w-full text-left px-3 py-1.5 hover:bg-hover"
              >
                <span className="text-fg-mute">{libName(f.library_id)} · </span>
                {f.path || "/"}
                <span className="text-fg-mute"> ({f.file_count.toLocaleString()})</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { doneSide, overlaps, type FolderHit, type PairRow } from "./twoFoldersLogic";

/**
 * 두 폴더 비교 — «후보1번/연도별»과 «후보2번»처럼 내가 고른 두 폴더 아래를 견준다.
 *
 * 줄마다 A쪽 폴더 ⇔ B쪽 폴더. 내용이 완전히 같으면 ✓ 똑같음, 이름만 같으면
 * «n/m 똑같음», 한쪽에만 있으면 그대로. 같은 것은 어느 쪽을 지울지 골라 한 번에.
 */

type PairsApplied = { applied: number; failed: number; first_error: string | null; kept: number; rejected: number };
type Outcome = { moved: number; failed: number; first_error: string | null; bytes: number; folders_removed?: number };
type FolderIn = NonNullable<PairRow["a"]>;

const settled = (f: FolderIn) => f.area === 1 || f.area === 2;

export default function TwoFolders({ onChanged }: { onChanged: () => void }) {
  const ask = useConfirm();
  const [a, setA] = useState<FolderHit | null>(null);
  const [b, setB] = useState<FolderHit | null>(null);
  const [rows, setRows] = useState<PairRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  /** 표시 진행 — n/총. null 이면 표시 중이 아니다 */
  const [marking, setMarking] = useState<{ total: number } | null>(null);
  const [tick, setTick] = useState(0);

  // 두 폴더가 정해지면 바로 견준다 — «비교» 단추를 따로 누를 이유가 없다.
  // 표시한 뒤에는 tick 으로 같은 길을 다시 태운다 — invoke 경로는 이 하나뿐
  useEffect(() => {
    if (!a || !b) return;
    let live = true;
    setBusy(true);
    setRows(null);
    invoke<PairRow[]>("cull_compare_folders", {
      aVolume: a.volume_uuid,
      aRel: a.vol_rel,
      bVolume: b.volume_uuid,
      bRel: b.vol_rel,
    })
      .then((r) => live && setRows(r))
      .catch((e) => live && toast(String(e), "drop"))
      .finally(() => live && setBusy(false));
    return () => {
      live = false;
    };
  }, [a, b, tick]);

  /// 같은 폴더 짝에서 한쪽에 제외 표시. side = 제외할 쪽
  const mark = useCallback(
    async (targets: PairRow[], side: "a" | "b") => {
      // 처리된 짝은 뺀다 — B쪽을 지운 줄에서 A쪽을 또 누르면 방금 남긴 B가 뒤집힌다
      const pairs = targets.filter((r) => r.same && r.a && r.b && doneSide(r) === null);
      if (pairs.length === 0) {
        toast("제외 표시할 똑같은 폴더 짝이 없습니다");
        return;
      }
      const drop = (r: PairRow) => (side === "a" ? r.a! : r.b!);
      const keep = (r: PairRow) => (side === "a" ? r.b! : r.a!);
      const risky = pairs.filter((r) => settled(drop(r)));
      const bytes = pairs.reduce((s, r) => s + r.bytes, 0);
      const files = pairs.reduce((s, r) => s + r.common, 0);
      const ok = await ask({
        title: `${side === "a" ? "A" : "B"}쪽 폴더 ${pairs.length.toLocaleString()}개의 사진 ${files.toLocaleString()}장에 제외 표시`,
        lines: [
          `${side === "a" ? "B" : "A"}쪽 똑같은 폴더는 그대로 둡니다 · ${fmtBytes(bytes)} 빔`,
          ...(risky.length
            ? [`주의: ${risky.length}개는 NAS 동기화 폴더입니다 — 휴지통으로 옮기면 NAS에서도 지워집니다`]
            : []),
          "파일은 아직 옮기지 않습니다 — 표시한 뒤 위의 «제외한 N장 휴지통으로»를 누르면 옮깁니다",
        ],
        confirmLabel: "제외 표시",
        danger: risky.length > 0,
      });
      if (!ok) return;
      // 잠그고 한 명령으로 — 짝마다 보내면 도는 동안 «A쪽 전부»를 또 눌러 두 루프가 얽힌다
      setMarking({ total: pairs.length });
      let r: PairsApplied;
      try {
        r = await invoke<PairsApplied>("cull_folder_pairs_apply", {
          pairs: pairs.map((p) => [keep(p).folder_id, drop(p).folder_id]),
        });
      } catch (e) {
        setMarking(null);
        toast(String(e), "drop");
        return;
      } finally {
        setMarking(null);
      }
      toast(
        r.failed
          ? `${r.applied}개 처리 · ${r.failed}개 실패 (${r.first_error ?? ""})`
          : `${r.applied.toLocaleString()}개 폴더 · ${r.rejected.toLocaleString()}장에 제외 표시했습니다 — 위의 «휴지통으로»로 옮깁니다`,
        r.failed ? "drop" : "ok",
      );
      onChanged();
      setTick((t) => t + 1);
    },
    [ask, onChanged],
  );

  const pickSide = useCallback(
    (side: "a" | "b") => (hit: FolderHit) => {
      const other = side === "a" ? b : a;
      if (other && overlaps(hit, other)) {
        toast("두 폴더가 서로를 품고 있습니다 — 겹치지 않는 두 폴더를 고르세요", "drop");
        return;
      }
      (side === "a" ? setA : setB)(hit);
    },
    [a, b],
  );

  const same = useMemo(() => rows?.filter((r) => r.same) ?? [], [rows]);
  const todo = useMemo(() => same.filter((r) => doneSide(r) === null), [same]);
  const sameBytes = todo.reduce((s, r) => s + r.bytes, 0);
  /// 이 비교에 나온 폴더들 안에서 제외 표시된 장수 — 표시했으면 여기서 바로 치운다
  const flagged = useMemo(() => (rows ?? []).reduce((s, r) => s + r.flagged_a + r.flagged_b, 0), [rows]);
  const [sweeping, setSweeping] = useState(false);
  const locked = busy || marking !== null || sweeping;

  /// 표시한 것을 휴지통으로 — 세 화면을 건너다니지 않게 비교 화면 안에서 끝낸다.
  /// 라이브러리 전체가 아니라 이 비교의 폴더들만
  const sweep = useCallback(async () => {
    if (!rows || flagged === 0) return;
    const folderIds = [...new Set(rows.flatMap((r) => [r.a?.folder_id, r.b?.folder_id]).filter((x): x is number => x != null))];
    const ok = await ask({
      title: `제외한 ${flagged.toLocaleString()}장을 휴지통으로 옮깁니다`,
      lines: [
        "이 비교에 나온 폴더 안의 제외 표시된 사진만 — 라이브러리의 다른 폴더는 건드리지 않습니다",
        "사진이 다 나간 폴더는 디스크에서도 지웁니다",
        "라이브러리 안 .acut/휴지통 으로 옮기는 것이라 되돌릴 수 있습니다 — 영구 삭제는 휴지통 화면에서",
      ],
      confirmLabel: "휴지통으로",
    });
    if (!ok) return;
    setSweeping(true);
    try {
      const r = await invoke<Outcome>("trash_apply", { libraryId: null, folderIds });
      const dirs = r.folders_removed ?? 0;
      toast(
        r.failed
          ? `${r.moved.toLocaleString()}장 옮김 · ${r.failed}장 실패 (${r.first_error ?? ""})`
          : `${r.moved.toLocaleString()}장을 휴지통으로 옮겼습니다 (${fmtBytes(r.bytes)})${dirs ? ` · 빈 폴더 ${dirs}개 지움` : ""} — 휴지통에서 되돌릴 수 있습니다`,
        r.failed ? "drop" : "ok",
      );
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      setSweeping(false);
    }
    onChanged();
    setTick((t) => t + 1);
  }, [rows, flagged, ask, onChanged]);

  return (
    <div className="h-full flex flex-col">
      <div className="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-line text-[12.5px] flex-wrap">
        <Picker label="A" value={a} onPick={pickSide("a")} startAt={b?.abs ?? null} disabled={locked} />
        <span className="text-fg-mute">⇔</span>
        <Picker label="B" value={b} onPick={pickSide("b")} startAt={a?.abs ?? null} disabled={locked} />
        {busy && (
          <span className="flex items-center gap-2 text-keep">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 견주는 중…
          </span>
        )}
        {marking && (
          <span className="flex items-center gap-2 text-keep tabular-nums">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> {marking.total.toLocaleString()}쌍에 표시 중…
          </span>
        )}
        {sweeping && (
          <span className="flex items-center gap-2 text-keep">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 휴지통으로 옮기는 중…
          </span>
        )}
        {rows && !marking && (
          <span className="text-fg-dim tabular-nums">
            똑같은 폴더 <b className="text-fg">{same.length.toLocaleString()}</b>쌍
            {same.length > todo.length && (
              <> · 처리됨 {(same.length - todo.length).toLocaleString()}</>
            )}
            {todo.length > 0 && (
              <>
                {" "}· 한쪽을 지우면 <b className="text-keep">{fmtBytes(sameBytes)}</b> 빔
              </>
            )}
          </span>
        )}
        <div className="flex-1" />
        {flagged > 0 && (
          <button
            onClick={sweep}
            disabled={locked}
            title="이 비교에 나온 폴더 안에서 제외 표시한 사진을 라이브러리 안 휴지통으로 옮깁니다 — 되돌릴 수 있습니다"
            className="h-7 px-3 rounded-md bg-drop text-drop-fg font-semibold text-[12.5px] disabled:opacity-40"
          >
            제외한 {flagged.toLocaleString()}장 휴지통으로
          </button>
        )}
        {todo.length > 0 && (
          <>
            <button
              onClick={() => mark(todo, "b")}
              disabled={locked}
              className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px] disabled:opacity-40"
            >
              B쪽 똑같은 폴더 전부 제외 표시
            </button>
            <button
              onClick={() => mark(todo, "a")}
              disabled={locked}
              className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px] disabled:opacity-40"
            >
              A쪽 전부
            </button>
          </>
        )}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        {rows === null ? (
          <div className="h-full flex flex-col items-center justify-center gap-2 text-fg-mute text-[13px]">
            <div>«폴더 고르기…»로 견줄 두 폴더를 Finder 에서 고르세요 — 등록한 라이브러리 안의 폴더면 됩니다.</div>
            <div className="text-fg-faint text-[12px]">예: A = T7 › 통합전후보 › 후보1번 › 연도별, B = T7 › 통합전후보 › 후보2번</div>
          </div>
        ) : rows.length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-mute">
            두 폴더 아래에 사진이 없습니다
          </div>
        ) : (
          <table className="w-full text-[12.5px] tabular-nums">
            <thead className="text-[10.5px] text-fg-mute uppercase tracking-wider sticky top-0 bg-canvas">
              <tr className="text-left">
                <th className="py-1.5 px-4 font-medium">A · {a && `${a.library} · ${a.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium">B · {b && `${b.library} · ${b.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium text-right">장수</th>
                <th className="py-1.5 pr-3 font-medium">상태</th>
                <th className="py-1.5 pr-4 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const done = doneSide(r);
                return (
                <tr
                  key={`${r.a?.folder_id ?? "x"}-${r.b?.folder_id ?? "x"}`}
                  className={`border-t border-line align-middle ${done ? "opacity-45" : ""}`}
                >
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
                    {done ? (
                      <span className="text-ok text-[11.5px]">처리됨 — {done === "a" ? "A" : "B"}쪽 제외</span>
                    ) : (
                      r.same && (
                        <>
                          <button
                            onClick={() => mark([r], "b")}
                            disabled={locked}
                            className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong mr-1 disabled:opacity-40"
                            title="B쪽 폴더의 사진에 제외 표시"
                          >
                            B쪽 제외
                          </button>
                          <button
                            onClick={() => mark([r], "a")}
                            disabled={locked}
                            className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
                            title="A쪽 폴더의 사진에 제외 표시"
                          >
                            A쪽 제외
                          </button>
                        </>
                      )
                    )}
                  </td>
                </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

/** 폴더 칸 — 고른 뿌리 아래 경로만 보인다 */
function Cell({ f, sub }: { f: FolderIn | null; sub: FolderHit | null }) {
  if (!f) return <span className="text-fg-faint">—</span>;
  // 고른 뿌리 아래 경로만 — 뿌리가 라이브러리 자체(경로 "")면 전체를 보인다
  const rootPath = sub?.path ?? "";
  const shown =
    sub && f.library_id === sub.library_id && (rootPath === "" || f.folder === rootPath || f.folder.startsWith(rootPath + "/"))
      ? f.folder.slice(rootPath.length).replace(/^\//, "") || "(이 폴더)"
      : `${f.library} · ${f.folder || "/"}`;
  return (
    <span className="truncate block" title={`${f.library} / ${f.folder || "/"}`}>
      {settled(f) && <span className="text-keep text-[10px] mr-1">NAS</span>}
      {shown}
    </span>
  );
}

/** 폴더 고르기 — Finder 창으로. 라이브러리 안의 폴더를 DB 의 폴더 행으로 바꾼다 */
function Picker({
  label,
  value,
  onPick,
  startAt,
  disabled,
}: {
  label: string;
  value: FolderHit | null;
  onPick: (f: FolderHit) => void;
  /** 창이 열릴 자리 — 다른 쪽에서 고른 폴더의 위 폴더. 없으면 /Volumes */
  startAt: string | null;
  disabled?: boolean;
}) {
  const pick = async () => {
    const from = value?.abs ?? startAt;
    const defaultPath = from ? from.replace(/\/[^/]+$/, "") || "/Volumes" : "/Volumes";
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: `${label} 폴더 고르기 — 등록한 라이브러리 안의 폴더`,
      defaultPath,
    });
    if (typeof picked !== "string") return;
    try {
      const hit = await invoke<FolderHit | null>("folder_by_path", { path: picked });
      if (!hit) {
        toast("등록된 라이브러리 안의 폴더가 아닙니다 — 왼쪽 앨범에 있는 폴더를 고르세요", "drop");
        return;
      }
      if (hit.file_count === 0) {
        toast("이 폴더 아래에 아직 훑은 사진이 없습니다 — 라이브러리를 먼저 다시 훑으세요", "drop");
        return;
      }
      onPick(hit);
    } catch (e) {
      toast(String(e), "drop");
    }
  };
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-fg-mute font-semibold">{label}</span>
      <button
        onClick={pick}
        disabled={disabled}
        title="Finder 에서 폴더 고르기"
        className={`h-7 min-w-[260px] max-w-[420px] px-2.5 rounded-md text-left truncate ring-1 disabled:opacity-40 ${
          value ? "bg-raised text-fg ring-line" : "bg-raised text-fg-mute ring-line-strong"
        }`}
      >
        {value ? (
          <>
            <span className="text-fg-mute">{value.library} · </span>
            {value.path || "/"}
            <span className="text-fg-mute"> ({value.file_count.toLocaleString()})</span>
          </>
        ) : (
          "폴더 고르기…"
        )}
      </button>
    </div>
  );
}

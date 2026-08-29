import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { doneSide, droppable, overlaps, verdict, type FolderHit, type PairRow } from "./twoFoldersLogic";

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
  /// 디스크에 없어 뺀 폴더 수 — Finder 에서 지운 폴더의 행이 남은 것
  const [missing, setMissing] = useState(0);
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
    invoke<{ rows: PairRow[]; missing: number }>("cull_compare_folders", {
      aVolume: a.volume_uuid,
      aRel: a.vol_rel,
      bVolume: b.volume_uuid,
      bRel: b.vol_rel,
    })
      .then((r) => {
        if (!live) return;
        setRows(r.rows);
        setMissing(r.missing);
      })
      .catch((e) => live && toast(String(e), "drop"))
      .finally(() => live && setBusy(false));
    return () => {
      live = false;
    };
  }, [a, b, tick]);

  /// 짝에서 한쪽에 제외 표시. side = 제외할 쪽 — 그쪽 사진이 반대쪽에 다 있는 짝만
  const mark = useCallback(
    async (targets: PairRow[], side: "a" | "b") => {
      // 처리된 짝은 뺀다 — B쪽을 지운 줄에서 A쪽을 또 누르면 방금 남긴 B가 뒤집힌다
      const pairs = targets.filter((r) => droppable(r, side) && doneSide(r) === null);
      if (pairs.length === 0) {
        toast(`${side === "a" ? "A" : "B"}쪽을 지워도 되는 짝이 없습니다`);
        return;
      }
      const drop = (r: PairRow) => (side === "a" ? r.a! : r.b!);
      const dropIds = (r: PairRow) => (side === "a" ? r.a_ids : r.b_ids);
      const keepIds = (r: PairRow) => (side === "a" ? r.b_ids : r.a_ids);
      const risky = pairs.filter((r) => settled(drop(r)));
      const bytes = pairs.reduce((s, r) => s + r.bytes, 0);
      const files = pairs.reduce((s, r) => s + (side === "a" ? r.files_a : r.files_b), 0);
      const ok = await ask({
        title: `${side === "a" ? "A" : "B"}쪽 폴더 ${pairs.length.toLocaleString()}개의 사진 ${files.toLocaleString()}장에 제외 표시`,
        lines: [
          `${side === "a" ? "B" : "A"}쪽은 그대로 둡니다(하위 폴더 포함) · ${fmtBytes(bytes)} 빔`,
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
          pairs: pairs.map((p) => ({ keep: keepIds(p), drop: dropIds(p) })),
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
  /// 지워도 되는 짝 — B 쪽 / A 쪽
  const todoB = useMemo(() => (rows ?? []).filter((r) => droppable(r, "b") && doneSide(r) === null), [rows]);
  const todoA = useMemo(() => (rows ?? []).filter((r) => droppable(r, "a") && doneSide(r) === null), [rows]);
  const sameBytes = todoB.reduce((s, r) => s + r.bytes, 0);
  /// 폴더 비교로 붙인 표시(남김·제외)를 되돌린다 — 휴지통에 가기 전이면 언제든
  const unmark = useCallback(
    async (targets: PairRow[]) => {
      const ids = [...new Set(targets.flatMap((r) => [...r.a_ids, ...r.b_ids]))];
      const n = targets.reduce((s, r) => s + r.flagged_a + r.flagged_b, 0);
      if (ids.length === 0 || n === 0) return;
      const ok = await ask({
        title: `제외 표시 ${n.toLocaleString()}장을 되돌립니다`,
        lines: ["이 폴더들의 남김·제외 표시를 미판정으로 돌리고, 닫았던 무리는 개별 비교에 다시 나옵니다", "파일은 그대로입니다"],
        confirmLabel: "표시 취소",
      });
      if (!ok) return;
      try {
        const [files] = await invoke<[number, number]>("cull_folder_set_unapply", { folderIds: ids });
        toast(`${files.toLocaleString()}장의 표시를 되돌렸습니다`, "ok");
      } catch (e) {
        toast(String(e), "drop");
      }
      onChanged();
      setTick((t) => t + 1);
    },
    [ask, onChanged],
  );

  /// 이 비교에 나온 폴더들 안에서 제외 표시된 장수 — 표시했으면 여기서 바로 치운다
  const flagged = useMemo(() => (rows ?? []).reduce((s, r) => s + r.flagged_a + r.flagged_b, 0), [rows]);
  const [sweeping, setSweeping] = useState(false);
  const locked = busy || marking !== null || sweeping;

  /// 표시한 것을 휴지통으로 — 세 화면을 건너다니지 않게 비교 화면 안에서 끝낸다.
  /// 라이브러리 전체가 아니라 이 비교의 폴더들만
  const sweep = useCallback(async () => {
    if (!rows || flagged === 0) return;
    const folderIds = [...new Set(rows.flatMap((r) => [...r.a_ids, ...r.b_ids]))];
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
            B쪽을 지워도 되는 짝 <b className="text-fg">{todoB.length.toLocaleString()}</b>
            {same.length > 0 && <> (그중 똑같음 {same.length.toLocaleString()})</>}
            {todoA.length > 0 && <> · A쪽을 지워도 되는 짝 {todoA.length.toLocaleString()}</>}
            {todoB.length > 0 && (
              <>
                {" "}· B쪽을 지우면 <b className="text-keep">{fmtBytes(sameBytes)}</b> 빔
              </>
            )}
          </span>
        )}
        <div className="flex-1" />
        {flagged > 0 && (
          <>
            <button
              onClick={() => unmark(rows ?? [])}
              disabled={locked}
              title="이 비교에서 붙인 남김·제외 표시를 전부 미판정으로 되돌립니다 — 파일은 그대로"
              className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px] disabled:opacity-40"
            >
              표시 취소
            </button>
            <button
              onClick={sweep}
              disabled={locked}
              title="이 비교에 나온 폴더 안에서 제외 표시한 사진을 라이브러리 안 휴지통으로 옮깁니다 — 되돌릴 수 있습니다"
              className="h-7 px-3 rounded-md bg-drop text-drop-fg font-semibold text-[12.5px] disabled:opacity-40"
            >
              제외한 {flagged.toLocaleString()}장 휴지통으로
            </button>
          </>
        )}
        {todoB.length > 0 && (
          <button
            onClick={() => mark(todoB, "b")}
            disabled={locked}
            title="B쪽 사진이 전부 A쪽에 있는 짝 — B쪽(하위 폴더 포함)에 제외 표시"
            className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px] disabled:opacity-40"
          >
            B쪽 전부 제외 표시 ({todoB.length.toLocaleString()}짝)
          </button>
        )}
        {todoA.length > 0 && (
          <button
            onClick={() => mark(todoA, "a")}
            disabled={locked}
            title="A쪽 사진이 전부 B쪽에 있는 짝 — A쪽(하위 폴더 포함)에 제외 표시"
            className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px] disabled:opacity-40"
          >
            A쪽 전부 ({todoA.length.toLocaleString()}짝)
          </button>
        )}
      </div>

      {missing > 0 && (
        <div className="shrink-0 px-4 py-1.5 text-[12px] text-drop bg-drop/10 border-b border-line">
          디스크에 없는 폴더 {missing.toLocaleString()}개는 뺐습니다 — Finder 에서 지운 폴더의 기록이 남은 것입니다. 왼쪽 앨범에서 라이브러리의 ⟳(다시 스캔)을 누르면 정리됩니다.
        </div>
      )}
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
                    <Cell f={r.a} sub={a} tree={r.a_ids.length > 1} />
                  </td>
                  <td className="py-1.5 pr-3 max-w-[380px]">
                    <Cell f={r.b} sub={b} tree={r.b_ids.length > 1} />
                  </td>
                  <td className="py-1.5 pr-3 text-right whitespace-nowrap text-fg-dim">
                    {r.same
                      ? r.files_a.toLocaleString()
                      : r.a && r.b
                        ? `${r.files_a.toLocaleString()} / ${r.files_b.toLocaleString()}`
                        : (r.files_a || r.files_b).toLocaleString()}
                  </td>
                  <td className="py-1.5 pr-3 whitespace-nowrap">
                    {(() => {
                      const v = verdict(r);
                      if (v.kind === "same")
                        return <span className="text-ok font-semibold">{v.text} · {fmtBytes(r.bytes)}</span>;
                      if (v.kind === "b_in_a" || v.kind === "a_in_b")
                        return <span className="text-ok">{v.text} · {fmtBytes(r.bytes)}</span>;
                      if (v.kind === "partial")
                        return (
                          <span className="text-keep">
                            {v.text} <span className="text-fg-mute">— 나머지는 개별 비교에서</span>
                          </span>
                        );
                      return <span className="text-fg-mute">{v.text}</span>;
                    })()}
                  </td>
                  <td className="py-1.5 pr-4 text-right whitespace-nowrap">
                    {done ? (
                      <span className="text-[11.5px]">
                        <span className="text-ok">처리됨 — {done === "a" ? "A" : "B"}쪽 제외</span>
                        <button
                          onClick={() => unmark([r])}
                          disabled={locked}
                          className="ml-2 h-6 px-2 rounded text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
                          title="이 짝의 표시를 되돌립니다"
                        >
                          취소
                        </button>
                      </span>
                    ) : (
                      <>
                        {droppable(r, "b") && (
                          <button
                            onClick={() => mark([r], "b")}
                            disabled={locked}
                            className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong mr-1 disabled:opacity-40"
                            title="B쪽 폴더(하위 포함)의 사진에 제외 표시 — 전부 A쪽에 있습니다"
                          >
                            B쪽 제외
                          </button>
                        )}
                        {droppable(r, "a") && (
                          <button
                            onClick={() => mark([r], "a")}
                            disabled={locked}
                            className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
                            title="A쪽 폴더(하위 포함)의 사진에 제외 표시 — 전부 B쪽에 있습니다"
                          >
                            A쪽 제외
                          </button>
                        )}
                      </>
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
function Cell({ f, sub, tree }: { f: FolderIn | null; sub: FolderHit | null; tree?: boolean }) {
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
      {tree && <span className="text-fg-mute text-[10.5px] ml-1" title="하위 폴더까지 합쳐서 본 줄">/…</span>}
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

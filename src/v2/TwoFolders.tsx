import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes } from "./format";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { doneSide, droppable, overlaps, verdict, type FolderHit, type PairRow } from "./twoFoldersLogic";
import PairView from "./PairView";

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
/** 줄의 열쇠 — 폴더 id 조합 */
const rowKey = (r: PairRow) => `${r.a?.folder_id ?? "x"}-${r.b?.folder_id ?? "x"}`;

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
    async (targets: PairRow[], side: "a" | "b", confirmed = false, mode: "drop" | "twins" = "drop") => {
      // 처리된 짝은 뺀다 — B쪽을 지운 줄에서 A쪽을 또 누르면 방금 남긴 B가 뒤집힌다.
      // twins 는 «부분만 겹치는» 폴더에서 반대쪽에도 있는 사진만 — 백엔드가 같은 내용만 골라 표시한다
      const pairs = targets.filter((r) =>
        mode === "twins" ? !!r.a && !!r.b && r.common > 0 : droppable(r, side) && doneSide(r) === null,
      );
      if (pairs.length === 0) {
        toast(`${side === "a" ? "A쪽 사진이 B쪽에" : "B쪽 사진이 A쪽에"} 다 있는 폴더가 없습니다`);
        return;
      }
      const drop = (r: PairRow) => (side === "a" ? r.a! : r.b!);
      const dropIds = (r: PairRow) => (side === "a" ? r.a_ids : r.b_ids);
      const keepIds = (r: PairRow) => (side === "a" ? r.b_ids : r.a_ids);
      const risky = pairs.filter((r) => settled(drop(r)));
      const bytes = pairs.reduce((s, r) => s + r.bytes, 0);
      const files = pairs.reduce((s, r) => s + (mode === "twins" ? r.common : side === "a" ? r.files_a : r.files_b), 0);
      // 검토 목록에서 «제외 표시»를 눌러 온 것이면 확인 창을 한 번 더 띄우지 않는다
      const ok = confirmed || (await ask({
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
      }));
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
  /// 이미 제외 표시가 다 붙은 폴더 수 — 쪽별
  const doneB = useMemo(() => (rows ?? []).filter((r) => doneSide(r) === "b").length, [rows]);
  const doneA = useMemo(() => (rows ?? []).filter((r) => doneSide(r) === "a").length, [rows]);
  /// 검토 단계 — «B쪽 폴더 N개 제외 표시»를 누르면 먼저 그 폴더들만 보여 주고, 여기서 확인한다.
  /// 체크를 풀면 그 폴더는 이번에 빠진다 (사용자 요청 2026-08-30)
  const [review, setReview] = useState<{ side: "a" | "b"; mode: "drop" | "twins"; unchecked: Set<string> } | null>(null);
  /// «부분만 겹치는» 폴더에서 B쪽 사진 중 A쪽에도 있는 것 — 폴더째는 못 지워도 그 사진들은 뺄 수 있다.
  /// 병합 전에 이걸 빼야 같은 폴더에 사본이 «이름 (2)»로 쌓이지 않는다 (실측 2026-08-30: 5,697장)
  const twinRows = useMemo(
    () => (rows ?? []).filter((r) => r.a && r.b && !droppable(r, "b") && r.kept_b === 0 && r.common > 0 && r.flagged_b < r.common),
    [rows],
  );
  const twinCount = useMemo(() => twinRows.reduce((s, r) => s + r.common, 0), [twinRows]);
  /// 폴더 짝 «보기» — 열려 있는 줄
  const [viewing, setViewing] = useState<PairRow | null>(null);
  const reviewRows = useMemo(
    () => (review ? (review.mode === "twins" ? twinRows : review.side === "b" ? todoB : todoA) : []),
    [review, todoA, todoB, twinRows],
  );
  const reviewPicked = useMemo(
    () => reviewRows.filter((r) => !review?.unchecked.has(rowKey(r))),
    [reviewRows, review],
  );
  /// 폴더 합치기 — 사본을 다 뺀 뒤 B 나무를 A 안으로. 끝나면 목록을 새로 읽는다
  const [merging, setMerging] = useState(false);
  useEffect(() => {
    let alive = true;
    const subs = [
      listen("merge-done", () => {
        if (!alive) return;
        setMerging(false);
        onChanged();
        setTick((t) => t + 1);
      }),
      listen("merge-error", () => alive && setMerging(false)),
    ];
    return () => {
      alive = false;
      subs.forEach((p) => p.then((f) => f()));
    };
  }, [onChanged]);
  const merge = useCallback(async () => {
    if (!a || !b || !rows) return;
    if (a.library_id !== b.library_id) {
      toast("같은 라이브러리 안의 폴더끼리만 합칠 수 있습니다", "drop");
      return;
    }
    const bFiles = rows.reduce((s, r) => s + (r.b ? r.files_b : 0), 0);
    const ok = await ask({
      title: `«${b.path || "/"}» 폴더를 «${a.path || "/"}» 안으로 합칩니다`,
      lines: [
        `· B 의 사진 ${bFiles.toLocaleString()}장을 A 의 같은 자리 폴더로 옮깁니다 (하위 폴더 구조 그대로)`,
        "· 같은 이름의 사진이 있으면 «이름 (2)»로 두고 덮어쓰지 않습니다",
        "· 비어 버린 B 폴더는 디스크에서 지웁니다 · ⌘Z 로 되돌릴 수 있습니다",
        ...(twinCount > 0
          ? [`주의: B 에는 A 에도 있는 사진 ${twinCount.toLocaleString()}장이 아직 남아 있습니다 — 그대로 합치면 사본이 «(2)»로 쌓입니다. 먼저 ②에서 빼는 게 좋습니다`]
          : []),
      ],
      confirmLabel: "합치기",
      danger: twinCount > 0,
    });
    if (!ok) return;
    setMerging(true);
    try {
      await invoke("folder_merge", { libraryId: a.library_id, srcRel: b.vol_rel, dstRel: a.vol_rel });
    } catch (e) {
      setMerging(false);
      toast(String(e), "drop");
    }
  }, [a, b, rows, ask, twinCount]);

  /// 한쪽에만 있는 폴더는 비교할 것이 없다 — 기본은 양쪽에 있는 폴더만 (사용자 요청 2026-08-30)
  const [onlyBoth, setOnlyBoth] = useState(true);
  const bothRows = useMemo(() => (rows ?? []).filter((r) => r.a && r.b), [rows]);
  const shownRows = review ? reviewRows : onlyBoth ? bothRows : rows;
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
  const locked = busy || marking !== null || sweeping || merging;

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
    <div className="h-full flex flex-col relative">
      {viewing && viewing.a && viewing.b && (
        <PairView
          a={viewing.a}
          b={viewing.b}
          aIds={viewing.a_ids}
          bIds={viewing.b_ids}
          onClose={() => {
            setViewing(null);
            onChanged();
            setTick((t) => t + 1);
          }}
        />
      )}
      <div className="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-line text-[12.5px] flex-wrap">
        <Picker label="A" value={a} onPick={pickSide("a")} startAt={b?.abs ?? null} disabled={locked} />
        <span className="text-fg-mute">⇔</span>
        <Picker label="B" value={b} onPick={pickSide("b")} startAt={a?.abs ?? null} disabled={locked} />
        {busy && (
          <span className="flex items-center gap-2 text-keep">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 비교하는 중…
          </span>
        )}
        {rows && !busy && !review && (
          <span className="flex items-center gap-3 text-[12px]" role="radiogroup" aria-label="보이는 줄">
            <label className="flex items-center gap-1 cursor-pointer">
              <input type="radio" name="tf-rows" checked={onlyBoth} onChange={() => setOnlyBoth(true)} className="accent-accent" />
              양쪽에 있는 폴더만 <span className="text-fg-mute tabular-nums">({bothRows.length.toLocaleString()})</span>
            </label>
            <label className="flex items-center gap-1 cursor-pointer">
              <input type="radio" name="tf-rows" checked={!onlyBoth} onChange={() => setOnlyBoth(false)} className="accent-accent" />
              전부 <span className="text-fg-mute tabular-nums">({rows.length.toLocaleString()})</span>
            </label>
          </span>
        )}
        {rows && !busy && (
          <span className="text-fg-dim tabular-nums">
            폴더 {rows.length.toLocaleString()}개 비교함 — 똑같음 <b className="text-fg">{same.length.toLocaleString()}</b>
            {" "}· B쪽 사진이 A쪽에 다 있음 <b className="text-fg">{rows.filter((r) => r.b_in_a && !r.same).length.toLocaleString()}</b>
            {rows.some((r) => r.a_in_b && !r.same) && (
              <> · A쪽 사진이 B쪽에 다 있음 {rows.filter((r) => r.a_in_b && !r.same).length.toLocaleString()}</>
            )}
          </span>
        )}
        {rows && !review && (
          <>
            <div className="flex-1" />
            <button
              onClick={merge}
              disabled={locked}
              title="B 나무의 사진을 A 의 같은 자리 폴더로 옮깁니다 — 사본을 먼저 뺀 뒤에. ⌘Z 로 되돌릴 수 있습니다"
              className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px] disabled:opacity-40"
            >
              {merging ? "합치는 중…" : "B 폴더를 A 로 합치기"}
            </button>
          </>
        )}
      </div>

      {/* 검토 단계 — 제외 표시할 폴더만 보이고, 여기서 확인한다 */}
      {rows && review && (
        <div className="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-line bg-keep/10 text-[12.5px] flex-wrap">
          <span className="text-fg">
            {review.mode === "twins" ? (
              <>
                <b>B쪽 폴더 {reviewRows.length.toLocaleString()}개 — A쪽에도 있는 사진만 제외 표시</b>
                <span className="text-fg-dim"> — 폴더는 남고, A에 없는 사진도 남습니다. 체크를 풀면 그 폴더는 이번에 빠집니다.</span>
              </>
            ) : (
              <>
                <b>{review.side === "a" ? "A" : "B"}쪽 제외 표시할 폴더 {reviewRows.length.toLocaleString()}개</b>
                <span className="text-fg-dim"> — 아래 목록을 확인하세요. 체크를 풀면 그 폴더는 이번에 빠집니다.</span>
              </>
            )}
          </span>
          <div className="flex-1" />
          <span className="text-fg-dim tabular-nums">
            {reviewPicked.length.toLocaleString()}개 폴더 ·{" "}
            {reviewPicked
              .reduce((s, r) => s + (review.mode === "twins" ? r.common : review.side === "a" ? r.files_a : r.files_b), 0)
              .toLocaleString()}
            장
          </span>
          <button
            onClick={() => setReview(null)}
            disabled={locked}
            className="h-7 px-3 rounded-md text-fg-dim ring-1 ring-line-strong text-[12.5px] disabled:opacity-40"
          >
            돌아가기
          </button>
          <button
            onClick={async () => {
              const { side, mode } = review;
              const picked = reviewPicked;
              setReview(null);
              await mark(picked, side, true, mode);
            }}
            disabled={locked || reviewPicked.length === 0}
            className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px] disabled:opacity-40"
          >
            제외 표시 ({reviewPicked.length.toLocaleString()})
          </button>
        </div>
      )}

      {/* 다음에 할 일 — 세 단계를 순서대로. 지금 어디까지 왔는지가 문장에 보인다 */}
      {rows && !review && (
        <div className="shrink-0 flex items-center gap-x-4 gap-y-1 px-4 py-1.5 border-b border-line bg-raised/40 text-[12.5px] flex-wrap">
          <span className="text-fg-mute">① 비교 끝</span>
          <span className="text-fg-faint">→</span>
          <span className="flex items-center gap-2">
            <span className={todoB.length + todoA.length > 0 ? "text-fg" : "text-fg-mute"}>② 제외 표시</span>
            {marking ? (
              <span className="flex items-center gap-2 text-keep tabular-nums">
                <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 폴더 {marking.total.toLocaleString()}개에 표시 중…
              </span>
            ) : (
              <>
                {todoB.length > 0 && (
                  <button
                    onClick={() => setReview({ side: "b", mode: "drop", unchecked: new Set() })}
                    disabled={locked}
                    title="B쪽 사진이 전부 A쪽에 있는 폴더 — B쪽(하위 폴더 포함)의 사진에 제외 표시를 붙입니다. 파일은 아직 그대로"
                    className="h-7 px-3 rounded-md bg-keep text-keep-fg font-semibold text-[12.5px] disabled:opacity-40"
                  >
                    B쪽 폴더 {todoB.length.toLocaleString()}개 제외 표시
                  </button>
                )}
                {todoA.length > 0 && (
                  <button
                    onClick={() => setReview({ side: "a", mode: "drop", unchecked: new Set() })}
                    disabled={locked}
                    title="A쪽 사진이 전부 B쪽에 있는 폴더 — A쪽(하위 폴더 포함)의 사진에 제외 표시를 붙입니다. 반대 방향이니 A를 남기는 중이면 누르지 마세요"
                    className="h-7 px-3 rounded-md bg-accent text-accent-fg font-semibold text-[12.5px] disabled:opacity-40"
                  >
                    A쪽 폴더 {todoA.length.toLocaleString()}개 제외 표시
                  </button>
                )}
                {twinRows.length > 0 && (
                  <button
                    onClick={() => setReview({ side: "b", mode: "twins", unchecked: new Set() })}
                    disabled={locked}
                    title="부분만 겹치는 폴더에서, B쪽 사진 가운데 A쪽에도 같은 내용이 있는 것만 제외 표시합니다 — 폴더는 남고 A에 없는 사진도 남습니다"
                    className="h-7 px-3 rounded-md ring-1 ring-keep text-keep font-semibold text-[12.5px] disabled:opacity-40"
                  >
                    B쪽 사진 중 A쪽에도 있는 {twinCount.toLocaleString()}장 제외 표시 ({twinRows.length.toLocaleString()}개 폴더)
                  </button>
                )}
                {flagged > 0 && (
                  <span className="text-fg-dim tabular-nums">
                    제외 표시된 사진 <b className="text-fg">{flagged.toLocaleString()}</b>장
                    {doneB > 0 && <> (B쪽 폴더 {doneB.toLocaleString()}개)</>}
                    {doneA > 0 && <> (A쪽 폴더 {doneA.toLocaleString()}개)</>}
                  </span>
                )}
                {flagged > 0 && (
                  <button
                    onClick={() => unmark(rows)}
                    disabled={locked}
                    title="이 비교에서 붙인 남김·제외 표시를 전부 미판정으로 되돌립니다 — 파일은 그대로"
                    className="h-7 px-2.5 rounded-md text-fg-dim ring-1 ring-line-strong text-[12px] disabled:opacity-40"
                  >
                    표시 취소
                  </button>
                )}
                {todoB.length + todoA.length === 0 && flagged === 0 && (
                  <span className="text-fg-mute">제외할 폴더가 없습니다</span>
                )}
              </>
            )}
          </span>
          <span className="text-fg-faint">→</span>
          <span className="flex items-center gap-2">
            <span className={flagged > 0 ? "text-fg" : "text-fg-mute"}>③ 휴지통으로</span>
            {sweeping ? (
              <span className="flex items-center gap-2 text-keep">
                <i className="w-2 h-2 rounded-full bg-keep animate-pulse" /> 옮기는 중…
              </span>
            ) : flagged > 0 ? (
              <button
                onClick={sweep}
                disabled={locked}
                title="②에서 제외 표시한 사진을 라이브러리 안 휴지통으로 옮깁니다. 파일이 실제로 움직이지만 휴지통에서 되돌릴 수 있습니다. 영구 삭제는 휴지통 화면의 «영구히 비우기»"
                className="h-7 px-3 rounded-md bg-drop text-drop-fg font-semibold text-[12.5px] disabled:opacity-40"
              >
                제외 표시된 {flagged.toLocaleString()}장 휴지통으로 보내기
              </button>
            ) : (
              <span className="text-fg-mute">②를 먼저</span>
            )}
          </span>
        </div>
      )}

      {missing > 0 && (
        <div className="shrink-0 px-4 py-1.5 text-[12px] text-drop bg-drop/10 border-b border-line">
          디스크에 없는 폴더 {missing.toLocaleString()}개는 뺐습니다 — Finder 에서 지운 폴더의 기록이 남은 것입니다. 왼쪽 앨범에서 라이브러리의 ⟳(다시 스캔)을 누르면 정리됩니다.
        </div>
      )}
      <div className="flex-1 min-h-0 overflow-y-auto scroll-thin">
        {rows === null ? (
          <div className="h-full flex flex-col items-center justify-center gap-2 text-fg-mute text-[13px]">
            <div>«폴더 고르기…»로 비교할 두 폴더를 Finder 에서 고르세요 — 등록한 라이브러리 안의 폴더면 됩니다.</div>
            <div className="text-fg-faint text-[12px]">예: A = T7 › 통합전후보 › 후보1번 › 연도별, B = T7 › 통합전후보 › 후보2번</div>
          </div>
        ) : rows.length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-mute">
            두 폴더 아래에 사진이 없습니다
          </div>
        ) : (shownRows ?? []).length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-mute">
            양쪽에 다 있는 폴더가 없습니다 — «전부»를 고르면 한쪽에만 있는 폴더도 보입니다
          </div>
        ) : (
          <table className="w-full text-[12.5px] tabular-nums">
            <thead className="text-[10.5px] text-fg-mute uppercase tracking-wider sticky top-0 bg-canvas">
              <tr className="text-left">
                {review && <th className="py-1.5 pl-4 w-8 font-medium"></th>}
                <th className="py-1.5 px-4 font-medium">A · {a && `${a.library} · ${a.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium">B · {b && `${b.library} · ${b.path || "/"}`}</th>
                <th className="py-1.5 pr-3 font-medium text-right">장수</th>
                <th className="py-1.5 pr-3 font-medium">상태</th>
                <th className="py-1.5 pr-4 font-medium"></th>
              </tr>
            </thead>
            <tbody>
              {(shownRows ?? []).map((r) => {
                const done = doneSide(r);
                const key = rowKey(r);
                const checked = review ? !review.unchecked.has(key) : false;
                return (
                <tr
                  key={key}
                  className={`border-t border-line align-middle ${done ? "opacity-45" : ""} ${review && !checked ? "opacity-40" : ""}`}
                >
                  {review && (
                    <td className="py-1.5 pl-4">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() =>
                          setReview((cur) => {
                            if (!cur) return cur;
                            const next = new Set(cur.unchecked);
                            if (next.has(key)) next.delete(key);
                            else next.add(key);
                            return { ...cur, unchecked: next };
                          })
                        }
                        className="accent-keep w-3.5 h-3.5"
                        title="체크를 풀면 이번 제외 표시에서 뺍니다"
                      />
                    </td>
                  )}
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
                    {r.a && r.b && !review && (
                      <button
                        onClick={() => setViewing(r)}
                        disabled={locked}
                        className="h-6 px-2 rounded text-[11.5px] text-fg-dim ring-1 ring-line-strong mr-1 disabled:opacity-40"
                        title="두 폴더의 사진을 나란히 놓고 직접 골라 표시합니다"
                      >
                        보기
                      </button>
                    )}
                    {done ? (
                      <span className="text-[11.5px]">
                        <span className="text-ok">처리됨 — {done === "a" ? "A" : "B"}쪽 제외</span>
                        <button
                          onClick={() => unmark([r])}
                          disabled={locked}
                          className="ml-2 h-6 px-2 rounded text-fg-dim ring-1 ring-line-strong disabled:opacity-40"
                          title="이 폴더의 표시를 되돌립니다"
                        >
                          취소
                        </button>
                      </span>
                    ) : review ? null : (
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

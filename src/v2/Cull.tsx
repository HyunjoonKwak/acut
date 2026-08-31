import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useData } from "./dataStore";
import { thumbUrlPath } from "./types";
import CullTile from "./CullTile";
import { listen } from "@tauri-apps/api/event";
import Viewer from "./Viewer";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";
import { usePrefs } from "./prefs";
import { useViewportW } from "./useViewportW";
import FolderSets from "./FolderSets";
import TwoFolders from "./TwoFolders";

type Group = {
  id: number;
  kind: number;
  reason: string | null;
  size_bytes: number;
  member_count: number;
  cover: string | null;
  name: string | null;
  folder: string | null;
};
type Member = {
  file_id: number;
  library_id: number | null;
  name: string;
  size: number;
  taken_at: number;
  is_best: boolean;
  score: number | null;
  thumb: string | null;
  culling_flag: number;
  library: string;
  folder: string;
  folder_id: number;
  area: number;
};
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };
/** 무리 목록 한 쪽 — 끝에 가까워지면 다음 쪽을 이어 읽는다 */
const PAGE_GROUPS = 200;
/** 정착 구역(내사진 1 · 공용 2) — 여기서 지우면 Drive 가 NAS 에서도 지운다 */
const settledArea = (area: number | null | undefined) => area === 1 || area === 2;
type ScopeCount = { library_id: number; groups: number };

type Summary = {
  kind: number;
  groups: number;
  photos: number;
  reclaimable: number;
  /** 처리된(확정한) 무리 — «처리됨 보기» */
  done: number;
};

const KINDS = [
  { id: -3, label: "폴더 비교", hint: "내용이 완전히 같은 폴더들 — 하나만 남기고 나머지는 제외" },
  { id: -4, label: "두 폴더 비교", hint: "내가 고른 두 폴더 아래를 견준다 — 후보1번/연도별 ⇔ 후보2번" },
  { id: 0, label: "개별 비교", hint: "같은 사진 무리(메타데이터만 다른 사본 포함) — 한 장씩 보며" },
  { id: 2, label: "같은 순간", hint: "연달아 찍은 것" },
  { id: 1, label: "잡동사니", hint: "스크린샷·다운로드본" },
  { id: 3, label: "비슷한 장면", hint: "AI가 본 닮은 사진 (벡터 필요)" },
];

export default function Cull({
  onClose,
  onChanged,
  cleanExcluded,
}: {
  onClose: () => void;
  /** 판정 수가 바뀌었다 — 격자 쪽 «확정 (N)» 수를 다시 세게 */
  onChanged: () => void;
  /** 모든 라이브러리의 제외 표시를 각 휴지통으로 — 확정한 것을 여기서 바로 정리한다 */
  cleanExcluded: () => Promise<void>;
}) {
  const [kind, setKind] = useState(-3); // 폴더 비교가 먼저 — 가장 크게 비운다
  const [groups, setGroups] = useState<Group[]>([]);
  const [idx, setIdx] = useState(0);
  /// 구성원 — 어느 그룹의 것인지와 함께. 그룹이 바뀌면 안 맞아 빈 목록이 된다.
  const [got, setGot] = useState<{ groupId: number; list: Member[] } | null>(
    null,
  );
  const [summary, setSummary] = useState<Summary[]>([]);
  const [busy, setBusy] = useState("");
  const ask = useConfirm();
  // 상태바의 잡 — 고르기 화면을 닫았다 열어도 «도는 중»을 안다
  const job = useJob((s) => s.job);
  const jobRunning = job?.label.startsWith("고르기") ?? false;
  const scanning = busy !== "" || jobRunning;
  const scanText =
    busy ||
    (job
      ? job.total > 0
        ? `${job.label} ${job.done.toLocaleString()}/${job.total.toLocaleString()}`
        : job.label
      : "찾는 중…");
  // 경과 시간 — 숫자가 안 바뀌는 단계(전체 해시)에서도 «살아 있음»을 보인다.
  // 1초마다 센다. 화면을 다시 열면 그때부터 센다 — 시작 시각은 뒷단이 모른다.
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!scanning) return;
    const t0 = Date.now();
    const id = window.setInterval(
      () => setElapsed(Math.floor((Date.now() - t0) / 1000)),
      1000,
    );
    return () => window.clearInterval(id);
  }, [scanning]);
  /// 크게 보기 — 어느 그룹의 몇 번째. 그룹이 바뀌면 저절로 닫힌다.
  const [viewer, setViewer] = useState<{ groupId: number; at: number } | null>(
    null,
  );
  const [viewerFull, setViewerFull] = useState(false);


  /// 범위 — «지워질 사본이 이 라이브러리에 있는 무리»만 본다. 목록·머리 숫자·모두 확정이
  /// 모두 이 잣대를 쓴다 (2026-08-30: 모두 확정에만 걸려 있어 넘겨 보는 무리와 어긋났다)
  const [scopeLib, setScopeLib] = useState<number | null>(null);
  const scopeRef = useRef(scopeLib);
  useEffect(() => {
    scopeRef.current = scopeLib;
  }, [scopeLib]);
  /// 범위 선택지 옆 숫자 — 라이브러리마다 «지워질 사본이 거기 있는» 미결 무리 수
  const [scopeCounts, setScopeCounts] = useState<Map<number, number>>(new Map());
  /// 방금 확정한 무리들 — «↩ 확정 취소»가 하나씩 되돌린다 (2026-08-31 «취소하고 다시 선택할 방법이 없어»)
  const [undoStack, setUndoStack] = useState<Group[]>([]);
  /// 처리됨 보기 — 확정한 무리를 최근 순으로 다시 넘겨 보며 무리 단위로 취소한다. 껐다 켜도 남는다
  const [viewDone, setViewDone] = useState(false);
  const viewDoneRef = useRef(viewDone);
  useEffect(() => {
    viewDoneRef.current = viewDone;
  }, [viewDone]);
  // 접히는 순서 사다리(2026-08-31) — 격자와 같은 규칙을 고르기에도.
  // c2(<1080) 탭 숫자·사유·안내문 접힘 → c3(<880) 진행 막대 축소·부제 접힘
  const w = useViewportW();
  const c2 = w < 1080;
  const c3 = w < 880;
  const loadSummary = useCallback(async () => {
    try {
      setSummary(await invoke<Summary[]>("cull_summary", { libraryId: scopeRef.current }));
    } catch (e) {
      toast(String(e), "drop");
    }
    // 확정할 때마다 «제외 표시 N장» 이 머리와 상태바에서 바로 늘어나야 한다 —
    // 안 그러면 «확정했는데 아무 데도 안 보인다» (2026-08-30)
    void useData.getState().refreshTrash(usePrefs.getState().libId);
    const k = kindRef.current;
    if (k === 0 || k === 1) {
      invoke<ScopeCount[]>("cull_scope_counts", { kind: k })
        .then((sc) => setScopeCounts(new Map(sc.map((x) => [x.library_id, x.groups]))))
        .catch(() => {});
    }
  }, []);
  const toCleanAll = useData((s) => s.toCleanAll);

  // 지금 탭 — 이벤트 경로에서 «어느 갈래를 새로 읽나»를 최신으로 본다
  const kindRef = useRef(kind);
  useEffect(() => {
    kindRef.current = kind;
  }, [kind]);
  // 응답 세대 — 늦게 온 다른 갈래의 무리가 지금 탭에 들어오지 않게 (리뷰 H3)
  const groupsGen = useRef(0);
  /// 무리는 200개씩 — 끝에 가까워지면 다음 200개를 이어 붙인다 (처리한 무리는 목록에서 빠지니
  /// 다음 쪽의 offset 은 «지금 들고 있는 미결 수»다)
  const [groupsDone, setGroupsDone] = useState(false);
  const libs = useData((s) => s.libs);
  const loadingMore = useRef(false);
  const loadGroups = useCallback(async (k: number) => {
    const gen = ++groupsGen.current;
    if (k === -3 || k === -4) {
      setGroups([]); // 폴더 비교는 무리가 아니라 폴더로 본다
      setIdx(0);
      return;
    }
    let g: Group[];
    try {
      g = await invoke<Group[]>("cull_groups", {
        kind: k,
        limit: PAGE_GROUPS,
        offset: 0,
        libraryId: scopeRef.current,
        done: viewDoneRef.current,
      });
    } catch (e) {
      toast(String(e), "drop");
      return;
    }
    if (gen !== groupsGen.current || k !== kindRef.current) return;
    setGroups(g);
    setGroupsDone(g.length < PAGE_GROUPS);
    setIdx(0);
  }, []);
  const loadMoreGroups = useCallback(async () => {
    if (loadingMore.current || groupsDone) return;
    const k = kindRef.current;
    if (k === -3 || k === -4) return;
    loadingMore.current = true;
    const gen = groupsGen.current;
    try {
      const more = await invoke<Group[]>("cull_groups", {
        kind: k,
        limit: PAGE_GROUPS,
        offset: groups.length,
        libraryId: scopeRef.current,
        done: viewDoneRef.current,
      });
      if (gen !== groupsGen.current || k !== kindRef.current) return;
      setGroupsDone(more.length < PAGE_GROUPS);
      setGroups((prev) => {
        const have = new Set(prev.map((g) => g.id));
        return [...prev, ...more.filter((g) => !have.has(g.id))];
      });
    } catch (e) {
      toast(String(e), "drop");
    } finally {
      loadingMore.current = false;
    }
  }, [groups.length, groupsDone]);
  useEffect(() => {
    if (groups.length > 0 && idx >= groups.length - 20) void loadMoreGroups();
  }, [idx, groups.length, loadMoreGroups]);


  /// 무리를 한꺼번에 확정한다 — 먼저 세어 보여 주고 묻는다. 정착 구역(내사진·
  /// 공용)에 제외될 사본이 있는 무리는 건너뛴다: 거기서 지우면 NAS에서도 지워진다.
  const applyAll = useCallback(
    async (folderId: number | null, what: string, libraryId: number | null = null) => {
      let dry: ApplyAll;
      try {
        dry = await invoke<ApplyAll>("cull_apply_all", {
          kind,
          skipSettled: true,
          dryRun: true,
          folderId,
          libraryId,
        });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      // 잡동사니의 skipped 는 «건너뛴 사진 수», 나머지 갈래는 «건너뛴 무리 수»
      const unit = kind === 1 ? "장" : "무리";
      if (dry.groups === 0 || (kind === 1 && dry.rejected === 0)) {
        toast(
          dry.skipped > 0
            ? `확정할 것이 없습니다 — 공용·내사진 안의 ${dry.skipped.toLocaleString()}${unit}는 하나씩 봐야 합니다`
            : "확정할 것이 없습니다",
        );
        return;
      }
      const ok = await ask({
        title: `${what} ${dry.groups.toLocaleString()}무리를 확정`,
        lines: [
          `남김 ${dry.kept.toLocaleString()}장 · 제외 표시 ${dry.rejected.toLocaleString()}장`,
          ...(dry.skipped > 0
            ? [
                kind === 1
                  ? `공용·내사진 안의 ${dry.skipped.toLocaleString()}장은 건너뜁니다 — 나중에 하나씩`
                  : `공용·내사진 안에 제외될 사본이 있는 ${dry.skipped.toLocaleString()}무리는 건너뜁니다 — 나중에 하나씩`,
              ]
            : []),
          "파일은 옮기지 않습니다 — 위의 «N장 휴지통으로»가 옮깁니다",
        ],
        confirmLabel: "확정",
      });
      if (!ok) return;
      let r: ApplyAll;
      try {
        r = await invoke<ApplyAll>("cull_apply_all", {
        kind,
        skipSettled: true,
        dryRun: false,
        folderId,
        libraryId,
      });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      toast(
        `${r.groups.toLocaleString()}무리 확정 — 제외 ${r.rejected.toLocaleString()}장`,
        "ok",
      );
      loadSummary();
      loadGroups(kind);
    },
    [kind, ask, loadSummary, loadGroups],
  );

  // 갈래나 범위를 바꾸면 요약과 그룹을 새로 읽는다. 다른 데서 쓰는 loadSummary·
  // loadGroups를 여기서 부르지 않는 이유: 컴파일러가 그 안의 setState를
  // «효과 안에서 바로»로 본다. 여기서는 then으로 풀어 쓴다.
  useEffect(() => {
    let live = true;
    const gen = ++groupsGen.current;
    setUndoStack([]);
    invoke<Summary[]>("cull_summary", { libraryId: scopeLib })
      .then((s) => live && setSummary(s))
      .catch((e) => live && toast(String(e), "drop"));
    // 폴더 탭은 무리를 안 쓴다 — 비우되, 효과 안에서 바로 setState 하지 않는다(컴파일러 규칙)
    const load =
      kind === -3 || kind === -4
        ? Promise.resolve([] as Group[])
        : invoke<Group[]>("cull_groups", {
            kind,
            limit: PAGE_GROUPS,
            offset: 0,
            libraryId: scopeLib,
            done: viewDone,
          });
    load.then(
      (g) => {
        if (!live || gen !== groupsGen.current) return;
        setGroups(g);
        setGroupsDone(g.length < PAGE_GROUPS);
        setIdx(0);
      },
    );
    return () => {
      live = false;
    };
  }, [kind, scopeLib, viewDone]);

  // 현재 그룹의 구성원
  const current = groups[idx];
  useEffect(() => {
    if (!current) return;
    let live = true;
    invoke<Member[]>("cull_members", { groupId: current.id }).then(
      (list) => live && setGot({ groupId: current.id, list }),
    ).catch((e) => toast(String(e), "drop"));
    return () => {
      live = false;
    };
  }, [current]);
  const members = useMemo(
    () => (current && got?.groupId === current.id ? got.list : []),
    [current, got],
  );
  const viewerAt = current && viewer?.groupId === current.id ? viewer.at : null;
  const setViewerAt = useCallback(
    (at: number | null) =>
      setViewer(at === null || !current ? null : { groupId: current.id, at }),
    [current],
  );

  // 스캔 진행
  // 지금 탭 — 구독은 한 번만 하고 갈래는 ref 로 본다. 탭마다 다시 구독하면 listen 이
  // 끝나기 전에 바뀐 효과의 정리가 빈 배열을 돌아 구독이 새고, 낡은 클로저가 다른 탭에
  // 무리를 채웠다 (리뷰 H3)
  useEffect(() => {
    let alive = true;
    const subs: Promise<() => void>[] = [];
    const on = <T,>(name: string, f: (payload: T) => void) => {
      subs.push(listen<T>(name, (e) => alive && f(e.payload)));
    };
    // 갈래 하나가 끝날 때마다 위의 숫자와 목록을 새로 읽는다 — 안 그러면 전체
    // 해시를 읽는 한 시간 동안 «같은 순간 0»으로 보여 아무것도 못 찾은 줄 안다.
    const stage = (msg: string) => {
      setBusy(msg);
      loadSummary();
      loadGroups(kindRef.current);
    };
    on("cull-junk", () => stage("잡동사니 완료 — 같은 순간 찾는 중"));
    on("cull-burst", () => stage("같은 순간 완료 — 중복 확인 중"));
    on<{
      phase: string;
      hashed: number;
      candidates: number;
      full_total: number;
      full_done: number;
      full_bytes: number;
      image_total: number;
      image_done: number;
    }>("cull-dedup-progress", (p) => {
      // 전체 해시는 파일을 끝까지 읽어 오래 걸린다 — 장수와 읽은 양을 같이 보인다
      setBusy(
        p.phase === "image"
          ? `중복 확인 — 메타데이터만 다른 사본 ${p.image_done.toLocaleString()}/${p.image_total.toLocaleString()}`
          : p.phase === "full"
            ? `중복 확인 — 전체 해시 ${p.full_done.toLocaleString()}/${p.full_total.toLocaleString()} · ${fmtBytes(p.full_bytes)}`
            : `중복 확인 — 빠른 해시 ${p.hashed.toLocaleString()}/${p.candidates.toLocaleString()}`,
      );
    });
    on("cull-dedup", () => stage("중복 완료 — 비슷한 장면 찾는 중"));
    on<{ photos: number; groups: number }>("cull-scene", (p) =>
      setBusy(
        p.photos === 0
          ? "비슷한 장면은 벡터가 있어야 찾습니다 — 설정 › AI"
          : `비슷한 장면 ${p.groups}그룹`,
      ),
    );
    on("cull-done", () => {
      setBusy("");
      // 다시 찾기가 무리를 새로 만들면 id 가 재사용될 수 있다 — 옛 «확정 취소»는 버린다
      setUndoStack([]);
      loadSummary();
      loadGroups(kindRef.current);
    });
    return () => {
      alive = false;
      // 아직 안 끝난 listen 도 끝나는 대로 푼다
      subs.forEach((p) => p.then((f) => f()));
    };
  }, [loadSummary, loadGroups]);

  const advance = useCallback(() => {
    setGroups((prev) => prev.filter((_, i) => i !== idx));
    setIdx((i) => Math.max(0, Math.min(i, groups.length - 2)));
  }, [idx, groups.length]);

  /// 확정 하나를 골라 되돌린다 — 표시를 지우고 그 무리를 목록 맨 앞에 되살린다
  const unapplyOne = useCallback(
    async (g: Group) => {
      try {
        await invoke("cull_unapply", { groupIds: [g.id] });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      setUndoStack((s) => s.filter((x) => x.id !== g.id));
      setGroups((prev) =>
        viewDoneRef.current ? prev.filter((x) => x.id !== g.id) : [g, ...prev.filter((x) => x.id !== g.id)],
      );
      setIdx(0);
      loadSummary();
    },
    [loadSummary],
  );

  const apply = useCallback(async () => {
    const g = groups[idx];
    if (!g) return;
    let r: ApplyAll;
    try {
      r = await invoke<ApplyAll>("cull_apply", { groupIds: [g.id] });
    } catch (e) {
      toast(String(e), "drop");
      return;
    }
    if (r.rejected === 0) toast("이 무리엔 제외할 사본이 없습니다 — 이미 휴지통에 있거나 지워진 사진뿐", "drop");
    setUndoStack((s) => [g, ...s].slice(0, 20));
    advance();
    loadSummary();
  }, [groups, idx, advance, loadSummary]);

  const skip = useCallback(async () => {
    const g = groups[idx];
    if (!g) return;
    try {
      await invoke("cull_skip", { groupIds: [g.id] });
    } catch (e) {
      toast(String(e), "drop");
      return;
    }
    advance();
  }, [groups, idx, advance]);

  const pick = useCallback(
    async (fileId: number) => {
      const g = groups[idx];
      if (!g) return;
      try {
        await invoke("cull_set_best", { groupId: g.id, fileId });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      setGot((cur) =>
        cur && cur.groupId === g.id
          ? {
              ...cur,
              list: cur.list.map((x) => ({
                ...x,
                is_best: x.file_id === fileId,
              })),
            }
          : cur,
      );
    },
    [groups, idx],
  );

  /// 공용 안 겹침 — 이 사진을 남기고 같은 무리의 나머지에 제외 표시
  const keepThis = useCallback(
    async (m: Member) => {
      const g = groups[idx];
      if (!g) return;
      try {
        await invoke("cull_set_best", { groupId: g.id, fileId: m.file_id });
        await invoke("cull_apply", { groupIds: [g.id] });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      setUndoStack((s) => [g, ...s].slice(0, 20));
      advance();
      loadSummary();
    },
    [groups, idx, advance, loadSummary],
  );
  /// 같은 두 폴더 사이의 무리 전부를 이 방향으로
  const pairAll = useCallback(
    async (m: Member) => {
      const other = members.find((x) => x.file_id !== m.file_id);
      if (!other) return;
      // 먼저 세어 보여 준다 — 몇 쌍·몇 장인지 모른 채 «전부 이렇게»를 누르지 않게
      let dry: ApplyAll;
      try {
        dry = await invoke<ApplyAll>("cull_apply_pair", {
          keepFolderId: m.folder_id,
          dropFolderId: other.folder_id,
          dryRun: true,
        });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      if (dry.groups === 0) {
        toast("이 두 폴더 사이에서만 얽힌 무리가 없습니다 — 다른 폴더까지 얽힌 것은 하나씩");
        return;
      }
      const risky = settledArea(other.area);
      const ok = await ask({
        title: `«${m.folder || "/"}»을 남기고 «${other.folder || "/"}» 것에 제외 표시`,
        lines: [
          `${dry.groups.toLocaleString()}쌍 — 남김 ${dry.kept.toLocaleString()}장 · 제외 표시 ${dry.rejected.toLocaleString()}장`,
          "두 폴더 사이에서만 겹치는 무리에 적용합니다 — 다른 폴더까지 얽힌 무리는 건너뜁니다",
          ...(risky ? ["주의: 제외될 쪽이 NAS 동기화 폴더입니다 — 휴지통으로 옮기면 NAS에서도 지워집니다"] : []),
          "파일은 아직 옮기지 않습니다 — 위의 «N장 휴지통으로»가 옮깁니다",
        ],
        confirmLabel: "전부 적용",
        danger: risky,
      });
      if (!ok) return;
      let r: ApplyAll;
      try {
        r = await invoke<ApplyAll>("cull_apply_pair", {
          keepFolderId: m.folder_id,
          dropFolderId: other.folder_id,
        });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      toast(`${r.groups.toLocaleString()}쌍 처리 — 제외 표시 ${r.rejected.toLocaleString()}장`, "ok");
      loadSummary();
      loadGroups(kind);
    },
    [members, ask, loadSummary, loadGroups, kind],
  );

  // 키보드 — 손이 마우스로 가지 않게
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (viewerAt !== null) return; // 크게 보기가 열려 있으면 뷰어가 키를 가져간다
      if ((kind === -3 || kind === -4) && e.key !== "Escape") return; // 폴더 비교는 제 손잡이가 있다
      if (viewDone && e.key !== "Escape") return; // 처리됨 보기는 구경 + 취소뿐
      if (e.key === " ") {
        e.preventDefault();
        apply();
      } else if (e.key === "s" || e.key === "S") {
        skip();
      } else if (e.key === "Escape") {
        onClose();
      } else if (/^[1-9]$/.test(e.key)) {
        const m = members[+e.key - 1];
        if (m) pick(m.file_id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [apply, skip, pick, members, onClose, viewerAt, kind, viewDone]);

  /// 크게 본 상태에서 P(남김)를 누르면 그 사진이 이 그룹의 남길 것이 된다
  const viewerMark = useCallback(
    async (
      fileId: number,
      patch: { rating?: number; cullingFlag?: number; favorite?: boolean },
    ) => {
      if (patch.cullingFlag === 1) {
        await pick(fileId);
        return;
      }
      try {
        await invoke("files_mark", {
          ids: [fileId],
          rating: patch.rating ?? null,
          cullingFlag: patch.cullingFlag ?? null,
          favorite: patch.favorite ?? null,
        });
      } catch (e) {
        toast(String(e), "drop");
        return;
      }
      // 타일의 배지도 같이 — X 로 제외한 것이 «★ 남김»으로 남아 있지 않게
      if (patch.cullingFlag !== undefined) {
        const flag = patch.cullingFlag;
        setGot((cur) =>
          cur
            ? { ...cur, list: cur.list.map((x) => (x.file_id === fileId ? { ...x, culling_flag: flag } : x)) }
            : cur,
        );
      }
    },
    [pick],
  );


  const cur = groups[idx];
  const total = summary.reduce((a, s) => a + s.reclaimable, 0);

  return (
    <div className="fixed inset-0 bg-canvas text-fg flex flex-col z-50">
      {/* 헤더 */}
      <div className="h-12 shrink-0 flex items-center gap-3 px-4 bg-chrome border-b border-line">
        <span className="font-semibold shrink-0">고르기</span>
        {/* 좁아지면 단추가 찌그러지는 대신 이 안이 가로로 밀린다 — 오른쪽 «확보 가능·닫기»는 늘 제자리 */}
        <div className="flex-1 min-w-0 flex items-center gap-3 bar-scroll">
        {KINDS.map((k) => {
          const s = summary.find((x) => x.kind === k.id);
          return (
            <button
              key={k.id}
              onClick={() => setKind(k.id)}
              title={c2 && s ? `${k.hint} — 미결 ${(s.groups ?? 0).toLocaleString()}` : k.hint}
              className={`h-control px-3 rounded-md text-[13.5px] ${
                kind === k.id
                  ? "bg-raised text-white ring-1 ring-line-strong"
                  : "text-fg-dim"
              }`}
            >
              {k.label}
              {k.id !== -3 && k.id !== -4 && !c2 && (
                <span className="tabular-nums text-fg-mute">
                  {" "}
                  {s?.groups ?? 0}
                </span>
              )}
            </button>
          );
        })}
        {scanning ? (
          // 찾는 중에는 멈출 수 있어야 한다. 해시를 읽느라 오래 걸린다.
          <button
            onClick={async () => {
              await invoke("scan_cancel");
              setBusy("");
            }}
            className="h-control px-3 rounded-md text-[13.5px] text-drop ring-1 ring-drop"
          >
            멈추기
          </button>
        ) : (
          <button
            onClick={() => {
              setElapsed(0);
              setBusy("찾는 중…");
              invoke("cull_scan").catch((e) => setBusy(String(e)));
            }}
            className="h-control px-3 rounded-md text-[13.5px] text-fg-dim ring-1 ring-line-strong"
          >
            다시 찾기
          </button>
        )}
        {scanning && (
          <span className="flex items-center gap-2 text-keep text-[13px] tabular-nums">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" />
            {scanText}
            <span className="text-fg-mute">· {fmtElapsed(elapsed)}</span>
          </span>
        )}
        </div>
        <span className="shrink-0 whitespace-nowrap text-[13px] text-fg-mute tabular-nums">
          확보 가능 <b className="text-keep">{fmtBytes(total)}</b>
        </span>
        <button onClick={onClose} className="shrink-0 whitespace-nowrap text-fg-dim px-2">
          닫기 <span className="text-[11px]">Esc</span>
        </button>
      </div>

      {kind === -3 || kind === -4 ? (
        <div className="flex-1 min-h-0">
          {kind === -3 ? (
            <FolderSets
              onChanged={() => {
                loadSummary();
                onChanged();
              }}
            />
          ) : (
            <TwoFolders
              onChanged={() => {
                loadSummary();
                onChanged();
              }}
            />
          )}
        </div>
      ) : (
        <>
      {/* 진행 */}
      {/* 조작 줄 — 범위·모두 확정·휴지통으로는 여기(늘 같은 자리). 머리는 탭·닫기만 (2026-08-31 좁은 창) */}
      <div className="h-10 shrink-0 flex items-center gap-3 px-4 bg-chrome border-b border-line text-[13.5px] bar-scroll">
            {!scanning && (
              <label className="flex items-center gap-1.5 text-[13px] text-fg-dim" style={{ flex: "none" }}>
                {!c3 && "범위"}
                <select
                  value={scopeLib ?? ""}
                  style={{ flex: "none" }}
                  onChange={(e) => setScopeLib(e.target.value === "" ? null : Number(e.target.value))}
                  title="지워질 사본이 이 라이브러리에 있는 무리만 봅니다 — 목록·숫자·모두 확정에 모두 걸립니다"
                  className="h-control rounded-md bg-raised text-fg text-[13px] px-2 ring-2 ring-accent/70 hover:ring-accent focus:ring-accent outline-none"
                >
                  <option value="">전체 라이브러리</option>
                  {libs.map((l) => (
                    <option key={l.id} value={l.id}>
                      {l.name} 쪽을 지울 것만 ({(scopeCounts.get(l.id) ?? 0).toLocaleString()})
                    </option>
                  ))}
                </select>
              </label>
            )}
            {!scanning && (kind === 0 || kind === 1) && !viewDone && groups.length > 0 && (
              <button
                onClick={() => applyAll(null, KINDS.find((k) => k.id === kind)?.label ?? "", scopeLib)}
                title={`${scopeLib === null ? "전체 라이브러리" : `${libs.find((l) => l.id === scopeLib)?.name ?? ""} 쪽을 지울 무리만`} — 한꺼번에 확정합니다. 공용·내사진 안의 사본이 있는 무리는 건너뜁니다`}
                className="h-control px-3 rounded-md text-[13.5px] bg-keep text-keep-fg font-semibold"
              >
                모두 확정
              </button>
            )}
            {!scanning && (toCleanAll?.files ?? 0) > 0 && (
              <button
                onClick={() => void cleanExcluded()}
                title={`지금까지 확정해 제외 표시한 ${toCleanAll?.files.toLocaleString()}장(${fmtBytes(toCleanAll?.bytes ?? 0)}, 모든 라이브러리)을 각 라이브러리의 휴지통으로 옮깁니다 — 휴지통에서 되돌릴 수 있습니다`}
                className="h-control px-3 rounded-md text-[13.5px] bg-keep text-keep-fg font-semibold"
              >
                {toCleanAll?.files.toLocaleString()}장 휴지통으로
              </button>
            )}
            <span className="w-px h-4 bg-line" style={{ flex: "none" }}></span>
            {/* 분모는 이 갈래의 미결 무리 전체 — 목록은 200개씩 읽어 두지만 그건 화면 사정이다 */}
            <span
              className="tabular-nums text-fg-dim"
              title="지금 무리 번호 / 이 갈래에서 아직 안 한 무리 전체. 처리한 만큼 줄어듭니다 — 언제 닫아도 한 것은 저장돼 있습니다"
            >
              {groups.length === 0
                ? "0 / 0"
                : `${idx + 1} / ${Math.max(
                    groups.length,
                    (viewDone
                      ? summary.find((x) => x.kind === kind)?.done
                      : summary.find((x) => x.kind === kind)?.groups) ?? 0,
                  ).toLocaleString()}`}
            </span>
            <div className={`${c3 ? "w-24" : c2 ? "w-36" : "w-56"} h-1.5 rounded bg-raised overflow-hidden`}>
              <i
                className="block h-full bg-accent"
                style={{
                  width: groups.length
                    ? `${((idx + 1) / groups.length) * 100}%`
                    : "0%",
                }}
              />
            </div>
            {viewDone && (
              <span className="h-6 px-2 rounded bg-keep/15 text-keep text-[12.5px] font-semibold flex items-center">
                처리됨 보기 — 취소만 할 수 있습니다
              </span>
            )}
            {((summary.find((x) => x.kind === kind)?.done ?? 0) > 0 || viewDone) && (
              <button
                onClick={() => setViewDone((v) => !v)}
                title="확정한 무리를 최근 순으로 다시 봅니다 — 앱을 껐다 켜도 남고, «↩ 확정 취소»로 무리 단위로 되돌립니다"
                className={`h-6 px-2 rounded text-[12.5px] ring-1 ${viewDone ? "bg-raised text-fg ring-line-strong" : "text-fg-dim ring-line-strong"}`}
              >
                {viewDone
                  ? "미결로 돌아가기"
                  : `처리됨 ${(summary.find((x) => x.kind === kind)?.done ?? 0).toLocaleString()}개 보기`}
              </button>
            )}
            {cur && !c2 && (
              <span className="text-fg-mute">
                {cur.reason} · {cur.member_count}장 · 확보{" "}
                {fmtBytes(cur.size_bytes)}
              </span>
            )}
      </div>

      {/* 후보들 — 크게 보기는 이 안만 덮는다. 위아래 막대는 남는다. */}
      {/* 스크롤은 안쪽 div가 맡는다. 바깥이 스크롤하면 덮개가 같이 밀려 올라간다. */}
      <div className="flex-1 relative min-h-0">
        <div className="absolute inset-0 overflow-y-auto p-4 scroll-thin">
          {!cur && (
            <div className="h-full flex flex-col items-center justify-center gap-2 text-fg-mute">
              {scanning ? (
                <>
                  <i className="w-3 h-3 rounded-full bg-keep animate-pulse" />
                  <div className="text-fg-dim tabular-nums">{scanText}</div>
                  <div className="text-[13px] tabular-nums">
                    {fmtElapsed(elapsed)} 지남 — 디스크를 읽는 동안은 숫자가
                    단계마다 갱신됩니다. 멈춰도 읽은 해시는 남습니다.
                  </div>
                </>
              ) : (
                "정리할 그룹이 없습니다 — 「다시 찾기」를 눌러보세요"
              )}
            </div>
          )}
          {cur && (
            <div
              className="grid gap-3"
              style={{
                gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))",
              }}
            >
              {members.map((m, i) => (
                <CullTile key={m.file_id} m={m} i={i} kind={kind} onPick={pick} onView={setViewerAt} onKeep={keepThis} />
              ))}
            </div>
          )}
        </div>

        {viewerAt !== null && members[viewerAt] && (
          <Viewer
            ids={members.map((m) => m.file_id)}
            index={viewerAt}
            onIndex={setViewerAt}
            onClose={() => {
              setViewerAt(null);
              setViewerFull(false);
            }}
            onMark={viewerMark}
            fullScreen={viewerFull}
            onToggleFullScreen={() => setViewerFull((f) => !f)}
          />
        )}
      </div>

      {/* 액션 */}
      <div className="h-14 shrink-0 flex items-center gap-2 px-4 bg-chrome border-t border-line bar-fixed">
        {viewDone && (
          <>
            <button
              onClick={() => cur && void unapplyOne(cur)}
              disabled={!cur}
              title="이 무리의 남김·제외 표시를 지우고 미결로 되돌립니다 — 이미 휴지통으로 옮긴 사진은 그대로"
              className="h-control px-3.5 rounded-lg text-fg-dim text-[14px] ring-1 ring-line-strong disabled:opacity-40 flex items-center gap-2"
            >
              ↩ 확정 취소
            </button>
            <span className="text-[13px] text-fg-mute">최근에 확정한 것부터 — 취소하면 미결 목록 맨 앞으로 돌아갑니다</span>
          </>
        )}
        {!viewDone && (
        <>
        <button
          onClick={apply}
          disabled={!cur}
          title="★ 대표는 남기고 나머지에 제외 표시 — 파일은 아직 옮기지 않습니다. 위의 «N장 휴지통으로»가 옮깁니다"
          className="h-control px-3.5 rounded-lg bg-keep text-keep-fg font-semibold text-[14px] disabled:opacity-40 flex items-center gap-2"
        >
          확정
          <span className="text-[11.5px] bg-black/20 px-1.5 py-0.5 rounded font-mono">
            Space
          </span>
        </button>
        <button
          onClick={skip}
          disabled={!cur}
          title="이 무리는 건너뜁니다 — 판정하지 않고 «보류»로 남깁니다"
          className="h-control px-3.5 rounded-lg text-fg-dim text-[14px] ring-1 ring-line-strong disabled:opacity-40 flex items-center gap-2"
        >
          나중에
          <span className="text-[11.5px] bg-white/8 px-1.5 py-0.5 rounded font-mono">
            S
          </span>
        </button>
        {kind === 0 && cur && members.length === 2 && members.some((m) => m.is_best) && (() => {
          const best = members.find((m) => m.is_best)!;
          const other = members.find((m) => !m.is_best)!;
          return (
            <button
              onClick={() => pairAll(best)}
              title={`«${best.folder || "/"}»를 남기고 «${other.folder || "/"}»의 사본을 제외 — 같은 두 폴더 사이에서 겹치는 다른 무리들에도 한꺼번에. 먼저 몇 쌍·몇 장인지 보여 주고 묻습니다`}
              className="h-control px-3.5 rounded-lg bg-accent text-accent-fg font-semibold text-[14px] flex items-center gap-2"
            >
              두 폴더 전체
              {!c3 && (
              <span className="text-[12px] font-normal opacity-80 truncate max-w-[280px]">
                {best.folder.split("/").pop() || "/"} 남김 · {other.folder.split("/").pop() || "/"} 제외
              </span>
              )}
            </button>
          );
        })()}
        </>
        )}
        {/* 긴 설명은 단추의 풍선(title)으로 — 막대엔 핵심 낱말만 (2026-08-30) */}
        {!viewDone && !c2 && (
        <span className="text-[13px] text-fg-mute ml-2" title="숫자키를 누르면 그 번호의 사진이 남길 쪽이 됩니다. 두 번 누르면 크게 봅니다">
          숫자키 <span className="font-mono">1–9</span> 남길 쪽 · 두 번 누르면 크게
        </span>
        )}
        <div className="flex-1" />
      </div>
      {/* 확정한 무리 띠 — 몇 쌍 지나간 뒤에도 특정 쌍만 골라 취소할 수 있게 (2026-08-31).
          이 세션에서 확정한 최근 20개. «다시 찾기» 뒤에는 무리 id 를 못 믿어 비운다 */}
      {undoStack.length > 0 && (
        <div className="shrink-0 flex items-center gap-2 px-4 py-2 bg-chrome/70 border-t border-line overflow-x-auto scroll-thin">
          <span className="shrink-0 text-[13px] text-fg-mute whitespace-nowrap">
            확정한 무리 <b className="text-fg-dim">{undoStack.length}</b>개 — ↩ 로 그것만 취소
          </span>
          {undoStack.map((g) => (
            <span
              key={g.id}
              className="shrink-0 flex items-center gap-1.5 pl-1.5 pr-1 h-12 rounded-md bg-raised ring-1 ring-line"
              title={`${g.reason ?? ""} · ${g.member_count}장 · ${fmtBytes(g.size_bytes)} 확보${g.name ? `\n${g.name}` : ""}${g.folder ? `\n${g.folder}` : ""}`}
            >
              {g.cover ? (
                <img src={thumbUrlPath(g.cover)} className="w-9 h-9 object-cover rounded" alt="" />
              ) : (
                <span className="w-9 h-9 rounded bg-canvas" />
              )}
              <span className="text-[12px] text-fg-dim tabular-nums">{g.member_count}장</span>
              <button
                onClick={() => void unapplyOne(g)}
                title="이 무리의 남김·제외 표시를 지우고 목록 맨 앞에 다시 보여 줍니다 — 이미 휴지통으로 옮긴 사진은 그대로"
                className="h-7 w-7 rounded text-fg-dim hover:text-fg hover:bg-hover text-[14px]"
              >
                ↩
              </button>
            </span>
          ))}
        </div>
      )}
        </>
      )}
    </div>
  );
}

/** 경과 시간 — «3분 12초» */
function fmtElapsed(sec: number): string {
  const m = Math.floor(sec / 60);
  return m > 0 ? `${m}분 ${sec % 60}초` : `${sec}초`;
}

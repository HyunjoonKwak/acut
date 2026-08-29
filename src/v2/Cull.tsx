import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Viewer from "./Viewer";
import { fmtBytes } from "./format";
import { useJob } from "./jobStore";
import { useConfirm } from "./confirmContext";
import { toast } from "./toastStore";

type Group = {
  id: number;
  kind: number;
  reason: string | null;
  size_bytes: number;
  member_count: number;
  cover: string | null;
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
  area: number;
};
type ApplyAll = { groups: number; kept: number; rejected: number; skipped: number };
type DupFolder = {
  folder_id: number;
  library: string;
  folder: string;
  area: number;
  files: number;
  copies: number;
  bytes: number;
  keeper_library: string | null;
  keeper_folder: string | null;
  keeper_copies: number;
};
type Summary = {
  kind: number;
  groups: number;
  photos: number;
  reclaimable: number;
};

const KINDS = [
  { id: 0, label: "완전 중복", hint: "바이트가 같은 파일" },
  { id: 2, label: "같은 순간", hint: "연달아 찍은 것" },
  { id: 1, label: "잡동사니", hint: "스크린샷·다운로드본" },
  { id: 3, label: "비슷한 장면", hint: "AI가 본 닮은 사진 (벡터 필요)" },
];

export default function Cull({ onClose }: { onClose: () => void }) {
  const [kind, setKind] = useState(2); // 같은 순간이 가장 많다
  const [groups, setGroups] = useState<Group[]>([]);
  const [idx, setIdx] = useState(0);
  /// 구성원 — 어느 그룹의 것인지와 함께. 그룹이 바뀌면 안 맞아 빈 목록이 된다.
  const [got, setGot] = useState<{ groupId: number; list: Member[] } | null>(
    null,
  );
  const [summary, setSummary] = useState<Summary[]>([]);
  const [busy, setBusy] = useState("");
  const ask = useConfirm();
  // 폴더별 보기 — 폴더 통째로 사본인 것을 한 번에
  const [byFolder, setByFolder] = useState(false);
  /// null 이면 아직 세는 중 — 실측 8초(무리 7만 개의 자기 결합)라 «없음»과 구분해야 한다
  const [folders, setFolders] = useState<DupFolder[] | null>(null);
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

  /// 캐시가 라이브러리마다 따로 있어 주소 앞에 라이브러리 id가 붙는다
  const url = (rel: string | null, libraryId: number | null) =>
    rel && libraryId !== null
      ? `thumb://localhost/${libraryId}/${rel.split("/").map(encodeURIComponent).join("/")}`
      : null;

  const loadSummary = useCallback(async () => {
    setSummary(await invoke<Summary[]>("cull_summary"));
  }, []);

  const loadGroups = useCallback(async (k: number) => {
    const g = await invoke<Group[]>("cull_groups", {
      kind: k,
      limit: 200,
      offset: 0,
    });
    setGroups(g);
    setIdx(0);
  }, []);

  const loadFolders = useCallback(async (k: number) => {
    setFolders(await invoke<DupFolder[]>("cull_dup_folders", { kind: k }));
  }, []);

  /// 무리를 한꺼번에 확정한다 — 먼저 세어 보여 주고 묻는다. 정착 구역(내사진·
  /// 공용)에 제외될 사본이 있는 무리는 건너뛴다: 거기서 지우면 NAS에서도 지워진다.
  const applyAll = useCallback(
    async (folderId: number | null, what: string) => {
      const dry = await invoke<ApplyAll>("cull_apply_all", {
        kind,
        skipSettled: true,
        dryRun: true,
        folderId,
      });
      if (dry.groups === 0) {
        toast(
          dry.skipped > 0
            ? `확정할 것이 없습니다 — 공용·내사진 안의 사본이 있는 ${dry.skipped.toLocaleString()}무리는 하나씩 봐야 합니다`
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
                `공용·내사진 안에 제외될 사본이 있는 ${dry.skipped.toLocaleString()}무리는 건너뜁니다 — 나중에 하나씩`,
              ]
            : []),
          "파일은 옮기지 않습니다 — 격자의 «제외 N장 치우기»로 휴지통에 보냅니다",
        ],
        confirmLabel: "확정",
      });
      if (!ok) return;
      const r = await invoke<ApplyAll>("cull_apply_all", {
        kind,
        skipSettled: true,
        dryRun: false,
        folderId,
      });
      toast(
        `${r.groups.toLocaleString()}무리 확정 — 제외 ${r.rejected.toLocaleString()}장`,
        "ok",
      );
      loadSummary();
      loadGroups(kind);
      if (byFolder) loadFolders(kind);
    },
    [kind, ask, loadSummary, loadGroups, loadFolders, byFolder],
  );

  // 갈래를 바꾸면 요약과 그룹을 새로 읽는다. 다른 데서 쓰는 loadSummary·
  // loadGroups를 여기서 부르지 않는 이유: 컴파일러가 그 안의 setState를
  // «효과 안에서 바로»로 본다. 여기서는 then으로 풀어 쓴다.
  useEffect(() => {
    let live = true;
    invoke<Summary[]>("cull_summary").then((s) => live && setSummary(s));
    invoke<Group[]>("cull_groups", { kind, limit: 200, offset: 0 }).then(
      (g) => {
        if (!live) return;
        setGroups(g);
        setIdx(0);
      },
    );
    return () => {
      live = false;
    };
  }, [kind]);

  // 현재 그룹의 구성원
  const current = groups[idx];
  useEffect(() => {
    if (!current) return;
    let live = true;
    invoke<Member[]>("cull_members", { groupId: current.id }).then(
      (list) => live && setGot({ groupId: current.id, list }),
    );
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
  useEffect(() => {
    const un: Array<() => void> = [];
    // 갈래 하나가 끝날 때마다 위의 숫자와 목록을 새로 읽는다 — 안 그러면 전체
    // 해시를 읽는 한 시간 동안 «같은 순간 0»으로 보여 아무것도 못 찾은 줄 안다.
    const stage = (msg: string) => {
      setBusy(msg);
      loadSummary();
      loadGroups(kind);
    };
    listen<{ found: number; bytes: number }>("cull-junk", () =>
      stage("잡동사니 완료 — 같은 순간 찾는 중"),
    ).then((f) => un.push(f));
    listen("cull-burst", () => stage("같은 순간 완료 — 중복 확인 중")).then(
      (f) => un.push(f),
    );
    listen<{
      phase: string;
      hashed: number;
      candidates: number;
      full_total: number;
      full_done: number;
      full_bytes: number;
    }>("cull-dedup-progress", (e) => {
      const p = e.payload;
      // 전체 해시는 파일을 끝까지 읽어 오래 걸린다 — 장수와 읽은 양을 같이 보인다
      setBusy(
        p.phase === "full"
          ? `중복 확인 — 전체 해시 ${p.full_done.toLocaleString()}/${p.full_total.toLocaleString()} · ${fmtBytes(p.full_bytes)}`
          : `중복 확인 — 빠른 해시 ${p.hashed.toLocaleString()}/${p.candidates.toLocaleString()}`,
      );
    }).then((f) => un.push(f));
    listen("cull-dedup", () => stage("중복 완료 — 비슷한 장면 찾는 중")).then(
      (f) => un.push(f),
    );
    listen<{ photos: number; groups: number }>("cull-scene", (e) =>
      setBusy(
        e.payload.photos === 0
          ? "비슷한 장면은 벡터가 있어야 찾습니다 — 설정 › AI"
          : `비슷한 장면 ${e.payload.groups}그룹`,
      ),
    ).then((f) => un.push(f));
    listen("cull-done", () => {
      setBusy("");
      loadSummary();
      loadGroups(kind);
    }).then((f) => un.push(f));
    return () => un.forEach((f) => f());
  }, [kind, loadSummary, loadGroups]);

  const advance = useCallback(() => {
    setGroups((prev) => prev.filter((_, i) => i !== idx));
    setIdx((i) => Math.min(i, groups.length - 2));
  }, [idx, groups.length]);

  const apply = useCallback(async () => {
    const g = groups[idx];
    if (!g) return;
    await invoke("cull_apply", { groupIds: [g.id] });
    advance();
    loadSummary();
  }, [groups, idx, advance, loadSummary]);

  const skip = useCallback(async () => {
    const g = groups[idx];
    if (!g) return;
    await invoke("cull_skip", { groupIds: [g.id] });
    advance();
  }, [groups, idx, advance]);

  const pick = useCallback(
    async (fileId: number) => {
      const g = groups[idx];
      if (!g) return;
      await invoke("cull_set_best", { groupId: g.id, fileId });
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

  // 키보드 — 손이 마우스로 가지 않게
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (viewerAt !== null) return; // 크게 보기가 열려 있으면 뷰어가 키를 가져간다
      if (byFolder && e.key !== "Escape") return; // 폴더별 보기에는 무리가 없다
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
  }, [apply, skip, pick, members, onClose, viewerAt, byFolder]);

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
      await invoke("files_mark", {
        ids: [fileId],
        rating: patch.rating ?? null,
        cullingFlag: patch.cullingFlag ?? null,
        favorite: patch.favorite ?? null,
      });
    },
    [pick],
  );

  // 폴더별 보기를 켜면 읽는다. loadFolders를 안 부르는 이유는 위 loadGroups와 같다.
  useEffect(() => {
    if (!byFolder) return;
    let live = true;
    invoke<DupFolder[]>("cull_dup_folders", { kind }).then(
      (r) => live && setFolders(r),
    );
    return () => {
      live = false;
    };
  }, [byFolder, kind]);

  const cur = groups[idx];
  const total = summary.reduce((a, s) => a + s.reclaimable, 0);

  return (
    <div className="fixed inset-0 bg-canvas text-fg flex flex-col z-50">
      {/* 헤더 */}
      <div className="h-12 shrink-0 flex items-center gap-3 px-4 bg-chrome border-b border-line">
        <span className="font-semibold">고르기</span>
        {KINDS.map((k) => {
          const s = summary.find((x) => x.kind === k.id);
          return (
            <button
              key={k.id}
              onClick={() => setKind(k.id)}
              title={k.hint}
              className={`h-control px-3 rounded-md text-[12.5px] ${
                kind === k.id
                  ? "bg-raised text-white ring-1 ring-line-strong"
                  : "text-fg-dim"
              }`}
            >
              {k.label}{" "}
              <span className="tabular-nums text-fg-mute">
                {s?.groups ?? 0}
              </span>
            </button>
          );
        })}
        {!scanning && (kind === 0 || kind === 1) && groups.length > 0 && (
          <button
            onClick={() => applyAll(null, KINDS.find((k) => k.id === kind)?.label ?? "")}
            title="미결 무리를 한꺼번에 확정 — 공용·내사진 안의 사본이 있는 무리는 건너뜁니다"
            className="h-control px-3 rounded-md text-[12.5px] bg-keep text-keep-fg font-semibold"
          >
            모두 확정
          </button>
        )}
        {kind === 0 && (
          <button
            onClick={() => setByFolder((v) => !v)}
            aria-pressed={byFolder}
            className={`h-control px-3 rounded-md text-[12.5px] ${
              byFolder ? "bg-raised text-white ring-1 ring-line-strong" : "text-fg-dim"
            }`}
          >
            폴더별
          </button>
        )}
        {scanning ? (
          // 찾는 중에는 멈출 수 있어야 한다. 해시를 읽느라 오래 걸린다.
          <button
            onClick={async () => {
              await invoke("scan_cancel");
              setBusy("");
            }}
            className="h-control px-3 rounded-md text-[12.5px] text-drop ring-1 ring-drop"
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
            className="h-control px-3 rounded-md text-[12.5px] text-fg-dim ring-1 ring-line-strong"
          >
            다시 찾기
          </button>
        )}
        {scanning && (
          <span className="flex items-center gap-2 text-keep text-[12px] tabular-nums">
            <i className="w-2 h-2 rounded-full bg-keep animate-pulse" />
            {scanText}
            <span className="text-fg-mute">· {fmtElapsed(elapsed)}</span>
          </span>
        )}
        <div className="flex-1" />
        <span className="text-[12px] text-fg-mute tabular-nums">
          확보 가능 <b className="text-keep">{fmtBytes(total)}</b>
        </span>
        <button onClick={onClose} className="text-fg-dim px-2">
          닫기 <span className="text-[10px]">Esc</span>
        </button>
      </div>

      {/* 진행 */}
      <div className="h-9 shrink-0 flex items-center gap-3 px-4 bg-chrome border-b border-line text-[12.5px]">
        {byFolder ? (
          // 폴더별 보기에는 «지금 무리»가 없다 — 표를 읽는 법을 적는다
          <span className="text-fg-dim truncate">
            왼쪽 폴더의 사진은 오른쪽 폴더에 원본이 있어 지워도 됩니다. 버튼을 누르면
            왼쪽 사본에 «지울 것(제외)» 도장을 찍습니다 — 실제로 지우는 건 나중에
            격자의 «제외 N장 치우기»입니다.
          </span>
        ) : (
          <>
            <span className="tabular-nums text-fg-dim">
              {groups.length === 0 ? "0 / 0" : `${idx + 1} / ${groups.length}`}
            </span>
            <div className="w-56 h-1.5 rounded bg-raised overflow-hidden">
              <i
                className="block h-full bg-accent"
                style={{
                  width: groups.length
                    ? `${((idx + 1) / groups.length) * 100}%`
                    : "0%",
                }}
              />
            </div>
            {cur && (
              <span className="text-fg-mute">
                {cur.reason} · {cur.member_count}장 · 확보{" "}
                {fmtBytes(cur.size_bytes)}
              </span>
            )}
          </>
        )}
      </div>

      {/* 후보들 — 크게 보기는 이 안만 덮는다. 위아래 막대는 남는다. */}
      {/* 스크롤은 안쪽 div가 맡는다. 바깥이 스크롤하면 덮개가 같이 밀려 올라간다. */}
      <div className="flex-1 relative min-h-0">
        <div className="absolute inset-0 overflow-y-auto p-4">
          {byFolder && (
            <FolderTable
              rows={folders}
              onApply={(f) =>
                applyAll(f.folder_id, `«${f.library} · ${f.folder || "/"}» 폴더의 사본`)
              }
            />
          )}
          {!byFolder && !cur && (
            <div className="h-full flex flex-col items-center justify-center gap-2 text-fg-mute">
              {scanning ? (
                <>
                  <i className="w-3 h-3 rounded-full bg-keep animate-pulse" />
                  <div className="text-fg-dim tabular-nums">{scanText}</div>
                  <div className="text-[12px] tabular-nums">
                    {fmtElapsed(elapsed)} 지남 — 디스크를 읽는 동안은 숫자가
                    단계마다 갱신됩니다. 멈춰도 읽은 해시는 남습니다.
                  </div>
                </>
              ) : (
                "정리할 그룹이 없습니다 — 「다시 찾기」를 눌러보세요"
              )}
            </div>
          )}
          {!byFolder && cur && (
            <div
              className="grid gap-3"
              style={{
                gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))",
              }}
            >
              {members.map((m, i) => {
                const u = url(m.thumb, m.library_id);
                return (
                  <button
                    key={m.file_id}
                    onClick={() => pick(m.file_id)}
                    onDoubleClick={() => setViewerAt(i)}
                    className="text-left"
                  >
                    <div
                      className="relative rounded-lg overflow-hidden bg-canvas"
                      style={{
                        aspectRatio: "3/2",
                        boxShadow: m.is_best
                          ? "0 0 0 2px var(--color-keep), 0 8px 22px -10px rgb(240 180 41 / 0.5)"
                          : "0 0 0 1px var(--color-line-strong)",
                      }}
                    >
                      {u ? (
                        <img
                          src={u}
                          loading="lazy"
                          className="w-full h-full object-cover"
                          style={{ opacity: m.is_best ? 1 : 0.55 }}
                        />
                      ) : (
                        <div className="w-full h-full flex items-center justify-center text-fg-faint">
                          …
                        </div>
                      )}
                      {m.is_best ? (
                        <span className="absolute top-2 left-2 h-5 px-2 rounded bg-keep text-keep-fg text-[11px] font-bold flex items-center">
                          ★ 남김
                        </span>
                      ) : (
                        <span className="absolute top-2 left-2 h-5 px-2 rounded bg-drop/90 text-drop-fg text-[11px] font-bold flex items-center">
                          ✕ 제외
                        </span>
                      )}
                      {i < 9 && (
                        <span className="absolute top-2 right-2 w-5 h-5 rounded bg-black/55 text-white text-[11px] flex items-center justify-center tabular-nums">
                          {i + 1}
                        </span>
                      )}
                    </div>
                    <div className="flex justify-between items-baseline mt-1.5 gap-2">
                      <span className="text-[11.5px] text-fg-dim truncate">
                        {m.name}
                      </span>
                      <span className="text-[11px] text-fg-mute tabular-nums shrink-0">
                        {fmtBytes(m.size)}
                      </span>
                    </div>
                    {/* 어디 있는 사본인가 — 어느 쪽을 남길지는 결국 폴더로 정한다 */}
                    <div
                      className={`text-[10.5px] truncate ${
                        m.area === 1 || m.area === 2 ? "text-keep" : "text-fg-mute"
                      }`}
                      title={`${m.library} / ${m.folder || "/"}`}
                    >
                      {m.library} · {m.folder || "/"}
                    </div>
                  </button>
                );
              })}
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
      <div className="h-14 shrink-0 flex items-center gap-2 px-4 bg-chrome border-t border-line">
        <button
          onClick={apply}
          disabled={!cur || byFolder}
          className="h-control px-3.5 rounded-lg bg-keep text-keep-fg font-semibold text-[13px] disabled:opacity-40 flex items-center gap-2"
        >
          이대로 확정
          <span className="text-[10.5px] bg-black/20 px-1.5 py-0.5 rounded font-mono">
            Space
          </span>
        </button>
        <button
          onClick={skip}
          disabled={!cur || byFolder}
          className="h-control px-3.5 rounded-lg text-fg-dim text-[13px] ring-1 ring-line-strong disabled:opacity-40 flex items-center gap-2"
        >
          나중에
          <span className="text-[10.5px] bg-white/8 px-1.5 py-0.5 rounded font-mono">
            S
          </span>
        </button>
        <span className="text-[12px] text-fg-mute ml-2">
          숫자키 <span className="font-mono">1–9</span> 로 남길 것을 바꿉니다 ·
          두 번 누르면 크게 봅니다
        </span>
        <div className="flex-1" />
        <span className="text-[12px] text-fg-mute">
          여기서는 판정만 합니다 — 닫으면 「휴지통으로 치우기」가 나옵니다
        </span>
      </div>
    </div>
  );
}

/** 경과 시간 — «3분 12초» */
function fmtElapsed(sec: number): string {
  const m = Math.floor(sec / 60);
  return m > 0 ? `${m}분 ${sec % 60}초` : `${sec}초`;
}

/** 폴더별 사본 표 — «이 폴더는 저 폴더의 사본이다»가 보이게 */
function FolderTable({
  rows,
  onApply,
}: {
  rows: DupFolder[] | null;
  onApply: (f: DupFolder) => void;
}) {
  if (rows === null)
    return (
      <div className="h-full flex items-center justify-center gap-2 text-fg-mute">
        <i className="w-2 h-2 rounded-full bg-keep animate-pulse" />
        폴더별로 세는 중… (몇 초 걸립니다)
      </div>
    );
  if (rows.length === 0)
    return (
      <div className="h-full flex items-center justify-center text-fg-mute">
        제외될 사본이 있는 폴더가 없습니다
      </div>
    );
  return (
    <table className="w-full text-[12px] tabular-nums">
      <thead className="text-[10.5px] text-fg-mute uppercase tracking-wider">
        <tr className="text-left">
          <th className="py-1.5 pr-3 font-medium">지워도 되는 사본이 있는 폴더</th>
          <th className="py-1.5 pr-3 font-medium text-right">사본 / 폴더 전체</th>
          <th className="py-1.5 pr-3 font-medium text-right">비는 용량</th>
          <th className="py-1.5 pr-3 font-medium">원본이 있는 폴더 (그대로 둠)</th>
          <th className="py-1.5 font-medium"></th>
        </tr>
      </thead>
      <tbody>
        {rows.map((f) => {
          const settled = f.area === 1 || f.area === 2;
          const whole = f.copies === f.files;
          return (
            <tr key={f.folder_id} className="border-t border-line align-top">
              <td className="py-2 pr-3 max-w-[420px]">
                <div className="text-fg truncate" title={`${f.library} / ${f.folder || "/"}`}>
                  <span className={settled ? "text-keep" : "text-fg-mute"}>{f.library}</span>
                  {" · "}
                  {f.folder || "/"}
                </div>
                {whole && (
                  <span className="inline-block mt-1 px-1.5 h-4 rounded bg-drop/20 text-drop text-[10px] font-semibold">
                    폴더 통째로 사본
                  </span>
                )}
                {settled && (
                  <span className="inline-block mt-1 ml-1 px-1.5 h-4 rounded bg-keep/20 text-keep text-[10px] font-semibold">
                    공용·내사진 — Drive로 NAS에서도 지워짐, 하나씩 봅니다
                  </span>
                )}
              </td>
              <td className="py-2 pr-3 text-right whitespace-nowrap">
                {f.copies.toLocaleString()} / {f.files.toLocaleString()}
              </td>
              <td className="py-2 pr-3 text-right whitespace-nowrap">{fmtBytes(f.bytes)}</td>
              <td className="py-2 pr-3 max-w-[420px]">
                {f.keeper_folder !== null ? (
                  <div
                    className="text-fg-dim truncate"
                    title={`${f.keeper_library} / ${f.keeper_folder || "/"}`}
                  >
                    {f.keeper_library} · {f.keeper_folder || "/"}
                    <span className="text-fg-mute"> ({f.keeper_copies.toLocaleString()}장)</span>
                  </div>
                ) : (
                  <span className="text-fg-faint">—</span>
                )}
              </td>
              <td className="py-2 text-right whitespace-nowrap">
                <button
                  onClick={() => onApply(f)}
                  disabled={settled}
                  title={
                    settled
                      ? "공용·내사진 안의 사본은 하나씩 봅니다"
                      : `${f.library}의 이 폴더 사본 ${f.copies.toLocaleString()}장에 «지울 것» 도장 — 오른쪽 원본은 그대로`
                  }
                  className="h-7 px-2.5 rounded-md text-[12px] bg-keep text-keep-fg font-semibold disabled:opacity-40"
                >
                  {f.library} 사본 지우기 표시
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

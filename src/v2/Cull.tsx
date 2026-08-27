import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Viewer from "./Viewer";

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
];

const fmtBytes = (n: number) => {
  if (n < 1024) return `${n} B`;
  const u = ["KB", "MB", "GB", "TB"];
  let v = n / 1024,
    i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${u[i]}`;
};

export default function Cull({ onClose }: { onClose: () => void }) {
  const [kind, setKind] = useState(2); // 같은 순간이 가장 많다
  const [groups, setGroups] = useState<Group[]>([]);
  const [idx, setIdx] = useState(0);
  const [members, setMembers] = useState<Member[]>([]);
  const [summary, setSummary] = useState<Summary[]>([]);
  const [busy, setBusy] = useState("");
  /// 크게 보기 — 구성원 목록 안 위치. null이면 닫힌 상태
  const [viewerAt, setViewerAt] = useState<number | null>(null);
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

  useEffect(() => {
    loadSummary();
    loadGroups(kind);
  }, [kind, loadSummary, loadGroups]);

  // 현재 그룹의 구성원
  useEffect(() => {
    const g = groups[idx];
    if (!g) {
      setMembers([]);
      return;
    }
    setViewerAt(null);
    invoke<Member[]>("cull_members", { groupId: g.id }).then(setMembers);
  }, [groups, idx]);

  // 스캔 진행
  useEffect(() => {
    const un: Array<() => void> = [];
    listen<{ found: number; bytes: number }>("cull-junk", () =>
      setBusy("잡동사니 완료 — 같은 순간 찾는 중"),
    ).then((f) => un.push(f));
    listen("cull-burst", () => setBusy("같은 순간 완료 — 중복 확인 중")).then(
      (f) => un.push(f),
    );
    listen<{ hashed: number; candidates: number }>("cull-dedup-progress", (e) =>
      setBusy(`중복 확인 ${e.payload.hashed}/${e.payload.candidates}`),
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
      setMembers((m) =>
        m.map((x) => ({ ...x, is_best: x.file_id === fileId })),
      );
    },
    [groups, idx],
  );

  // 키보드 — 손이 마우스로 가지 않게
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (viewerAt !== null) return; // 크게 보기가 열려 있으면 뷰어가 키를 가져간다
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
  }, [apply, skip, pick, members, onClose, viewerAt]);

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

  const cur = groups[idx];
  const total = summary.reduce((a, s) => a + s.reclaimable, 0);

  return (
    <div className="fixed inset-0 bg-[#15191A] text-[#EAEFEF] flex flex-col z-50">
      {/* 헤더 */}
      <div className="h-12 shrink-0 flex items-center gap-3 px-4 bg-[#1C2123] border-b border-[#242C2E]">
        <span className="font-semibold">고르기</span>
        {KINDS.map((k) => {
          const s = summary.find((x) => x.kind === k.id);
          return (
            <button
              key={k.id}
              onClick={() => setKind(k.id)}
              title={k.hint}
              className={`h-7 px-3 rounded-md text-[12.5px] ${
                kind === k.id
                  ? "bg-[#232A2C] text-white ring-1 ring-[#3B4649]"
                  : "text-[#8D9A9C]"
              }`}
            >
              {k.label}{" "}
              <span className="tabular-nums text-[#6D7B7E]">
                {s?.groups ?? 0}
              </span>
            </button>
          );
        })}
        <button
          onClick={() => invoke("cull_scan")}
          className="h-7 px-3 rounded-md text-[12.5px] text-[#A3B2B4] ring-1 ring-[#333C3F]"
        >
          다시 찾기
        </button>
        {busy && (
          <span className="text-[#F0B429] text-[12px] tabular-nums">
            {busy}
          </span>
        )}
        <div className="flex-1" />
        <span className="text-[12px] text-[#7C8A8D] tabular-nums">
          확보 가능 <b className="text-[#F0B429]">{fmtBytes(total)}</b>
        </span>
        <button onClick={onClose} className="text-[#8D9A9C] px-2">
          닫기 <span className="text-[10px]">Esc</span>
        </button>
      </div>

      {/* 진행 */}
      <div className="h-9 shrink-0 flex items-center gap-3 px-4 bg-[#1B2123] border-b border-[#242C2E] text-[12.5px]">
        <span className="tabular-nums text-[#A3B2B4]">
          {groups.length === 0 ? "0 / 0" : `${idx + 1} / ${groups.length}`}
        </span>
        <div className="w-56 h-1.5 rounded bg-[#282F31] overflow-hidden">
          <i
            className="block h-full bg-[#49B8B4]"
            style={{
              width: groups.length
                ? `${((idx + 1) / groups.length) * 100}%`
                : "0%",
            }}
          />
        </div>
        {cur && (
          <span className="text-[#6D7B7E]">
            {cur.reason} · {cur.member_count}장 · 확보{" "}
            {fmtBytes(cur.size_bytes)}
          </span>
        )}
      </div>

      {/* 후보들 — 크게 보기는 이 안만 덮는다. 위아래 막대는 남는다. */}
      {/* 스크롤은 안쪽 div가 맡는다. 바깥이 스크롤하면 덮개가 같이 밀려 올라간다. */}
      <div className="flex-1 relative min-h-0">
        <div className="absolute inset-0 overflow-y-auto p-4">
          {!cur && (
            <div className="h-full flex items-center justify-center text-[#6D7B7E]">
              {busy
                ? busy
                : "정리할 그룹이 없습니다 — 「다시 찾기」를 눌러보세요"}
            </div>
          )}
          {cur && (
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
                      className="relative rounded-lg overflow-hidden bg-[#0F1314]"
                      style={{
                        aspectRatio: "3/2",
                        boxShadow: m.is_best
                          ? "0 0 0 2px #F0B429, 0 8px 22px -10px rgba(240,180,41,.5)"
                          : "0 0 0 1px #2B3436",
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
                        <div className="w-full h-full flex items-center justify-center text-[#3A4547]">
                          …
                        </div>
                      )}
                      {m.is_best ? (
                        <span className="absolute top-2 left-2 h-5 px-2 rounded bg-[#F0B429] text-[#231A00] text-[11px] font-bold flex items-center">
                          ★ 남김
                        </span>
                      ) : (
                        <span className="absolute top-2 left-2 h-5 px-2 rounded bg-[#E2685C]/90 text-[#2A0D09] text-[11px] font-bold flex items-center">
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
                      <span className="text-[11.5px] text-[#A3B2B4] truncate">
                        {m.name}
                      </span>
                      <span className="text-[11px] text-[#6D7B7E] tabular-nums shrink-0">
                        {fmtBytes(m.size)}
                      </span>
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
      <div className="h-14 shrink-0 flex items-center gap-2 px-4 bg-[#1C2123] border-t border-[#242C2E]">
        <button
          onClick={apply}
          disabled={!cur}
          className="h-8 px-4 rounded-lg bg-[#F0B429] text-[#231A00] font-semibold text-[13px] disabled:opacity-40 flex items-center gap-2"
        >
          이대로 확정
          <span className="text-[10.5px] bg-black/20 px-1.5 py-0.5 rounded font-mono">
            Space
          </span>
        </button>
        <button
          onClick={skip}
          disabled={!cur}
          className="h-8 px-4 rounded-lg text-[#A3B2B4] text-[13px] ring-1 ring-[#333C3F] disabled:opacity-40 flex items-center gap-2"
        >
          나중에
          <span className="text-[10.5px] bg-white/8 px-1.5 py-0.5 rounded font-mono">
            S
          </span>
        </button>
        <span className="text-[12px] text-[#6D7B7E] ml-2">
          숫자키 <span className="font-mono">1–9</span> 로 남길 것을 바꿉니다 ·
          두 번 누르면 크게 봅니다
        </span>
        <div className="flex-1" />
        <span className="text-[12px] text-[#6D7B7E]">
          판정만 기록합니다 — 파일은 지워지지 않습니다
        </span>
      </div>
    </div>
  );
}

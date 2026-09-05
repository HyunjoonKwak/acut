import { useEffect, useId, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalFocus } from "./focus";

type Suggestion = { title: string; why: string; score: number };
type EventItem = { id: number; name: string; taken_at: number };
type Candidate = {
  key: string;
  date: string;
  start_at: number;
  end_at: number;
  count: number;
  items: EventItem[];
  suggestions: Suggestion[];
};

const clock = (at: number) =>
  new Date(at * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

export default function EventDiscoveryDialog({
  libraryId,
  libraryName,
  onChoose,
  onClose,
}: {
  libraryId: number;
  libraryName: string;
  onChoose: (ids: number[]) => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const [gapMinutes, setGapMinutes] = useState(240);
  const [minCount, setMinCount] = useState(8);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [selected, setSelected] = useState<Map<string, Set<number>>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = async () => {
    const safeGapMinutes = Math.max(
      1,
      Math.trunc(Number.isFinite(gapMinutes) ? gapMinutes : 1),
    );
    const safeMinCount = Math.max(
      2,
      Math.trunc(Number.isFinite(minCount) ? minCount : 2),
    );
    setBusy(true);
    setError(null);
    try {
      const found = await invoke<Candidate[]>("event_candidates", {
        libraryId,
        gapMinutes: safeGapMinutes,
        minCount: safeMinCount,
      });
      setCandidates(found);
      setExpanded(new Set());
      setSelected(
        new Map(
          found.map((candidate) => [
            candidate.key,
            new Set(candidate.items.map((item) => item.id)),
          ]),
        ),
      );
    } catch (caught) {
      setError(String(caught));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void search();
    // 첫 진입 기본값으로 한 번만 찾는다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libraryId]);

  useModalFocus(dialogRef, onClose);

  return (
    <div className="fixed inset-0 z-[68] bg-canvas/95 backdrop-blur-sm flex items-center justify-center p-6">
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="w-[820px] max-w-full max-h-[88vh] flex flex-col bg-chrome rounded-xl ring-1 ring-line shadow-2xl"
      >
        <div className="p-5 pb-3">
          <h2 id={titleId} className="text-[16px] font-semibold text-fg">
            이벤트 자동 발견
          </h2>
          <p className="mt-1 text-[12.5px] text-fg-mute">
            「{libraryName}」의 사진만 묶습니다. 후보 확인은 파일을 바꾸지
            않으며, 사진별로 제외한 뒤 기존 정리 화면에서 최종 목적지를
            확인합니다.
          </p>
          <div className="mt-3 flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1 text-[11.5px] text-fg-mute">
              시간 간격
              <select
                value={gapMinutes}
                onChange={(event) => setGapMinutes(Number(event.target.value))}
                className="h-control rounded bg-raised px-2 text-[13px] text-fg ring-1 ring-line"
              >
                <option value={60}>1시간</option>
                <option value={120}>2시간</option>
                <option value={240}>4시간</option>
                <option value={480}>8시간</option>
              </select>
            </label>
            <label className="flex flex-col gap-1 text-[11.5px] text-fg-mute">
              최소 장수
              <input
                type="number"
                min={2}
                max={500}
                value={minCount}
                onChange={(event) => setMinCount(Number(event.target.value))}
                className="h-control w-24 rounded bg-raised px-2 text-[13px] text-fg ring-1 ring-line"
              />
            </label>
            <button
              onClick={() => void search()}
              disabled={busy}
              className="h-control rounded px-3 text-[13px] text-fg-dim ring-1 ring-line hover:text-fg disabled:opacity-40"
            >
              {busy ? "찾는 중…" : "다시 찾기"}
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-auto px-5">
          {!busy && candidates.length === 0 && !error && (
            <div className="rounded bg-raised px-3 py-5 text-center text-[13px] text-fg-mute">
              이 조건에 맞는 이벤트 후보가 없습니다.
            </div>
          )}
          {candidates.map((candidate) => {
            const chosen = selected.get(candidate.key) ?? new Set<number>();
            const hint = candidate.suggestions[0];
            return (
              <section
                key={candidate.key}
                className="mb-3 rounded-lg bg-raised p-3 ring-1 ring-line"
              >
                <div className="flex items-start gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="text-[14px] font-semibold text-fg">
                      {candidate.date} · {clock(candidate.start_at)}–
                      {clock(candidate.end_at)}
                    </div>
                    <div className="mt-0.5 text-[12px] text-fg-mute">
                      {candidate.count.toLocaleString()}장
                      {hint && ` · 제안: ${hint.title} (${hint.why})`}
                    </div>
                  </div>
                  <button
                    onClick={() => onChoose([...chosen])}
                    disabled={chosen.size === 0}
                    className="h-control shrink-0 rounded bg-accent px-3 text-[13px] font-semibold text-accent-fg disabled:opacity-40"
                  >
                    선택 {chosen.size.toLocaleString()}장 정리…
                  </button>
                </div>
                <details
                  className="mt-2"
                  // 후보 키는 재검색에도 같아 <details> DOM 이 재사용된다. open 을
                  // 상태로 묶지 않으면 «열린 채 목록 없음»이 된다 (2차 리뷰 H-3)
                  open={expanded.has(candidate.key)}
                  onToggle={(event) => {
                    const open = event.currentTarget.open;
                    setExpanded((current) => {
                      if (current.has(candidate.key) === open) return current;
                      const next = new Set(current);
                      if (open) next.add(candidate.key);
                      else next.delete(candidate.key);
                      return next;
                    });
                  }}
                >
                  <summary className="cursor-pointer text-[12.5px] text-fg-dim">
                    사진별 검토·제외
                  </summary>
                  {expanded.has(candidate.key) && (
                    <div className="mt-2 max-h-44 overflow-auto rounded bg-canvas/60 p-2">
                      {candidate.items.map((item) => (
                        <label
                          key={item.id}
                          className="flex items-center gap-2 py-1 text-[12.5px] text-fg-dim"
                        >
                          <input
                            type="checkbox"
                            checked={chosen.has(item.id)}
                            onChange={(event) => {
                              const next = new Map(selected);
                              const ids = new Set(
                                next.get(candidate.key) ?? [],
                              );
                              if (event.target.checked) ids.add(item.id);
                              else ids.delete(item.id);
                              next.set(candidate.key, ids);
                              setSelected(next);
                            }}
                            className="accent-accent"
                          />
                          <span className="w-14 shrink-0 tabular-nums text-fg-mute">
                            {clock(item.taken_at)}
                          </span>
                          <span className="truncate" title={item.name}>
                            {item.name}
                          </span>
                        </label>
                      ))}
                    </div>
                  )}
                </details>
              </section>
            );
          })}
        </div>

        <div className="p-5 pt-3">
          {error && <div className="mb-3 text-[13px] text-drop">{error}</div>}
          <button
            onClick={onClose}
            className="h-control rounded-lg px-3 text-[14px] text-fg-dim ring-1 ring-line-strong"
          >
            닫기
          </button>
        </div>
      </div>
    </div>
  );
}

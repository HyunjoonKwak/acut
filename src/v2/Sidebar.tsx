import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import AlbumTree from "./AlbumTree";
import Calendar from "./Calendar";
import FacetList from "./FacetList";
import Rail from "./Rail";
import SearchPanel from "./SearchPanel";
import PeoplePanel from "./PeoplePanel";
import SettingsNav from "./SettingsNav";
import SmartPanel from "./SmartPanel";
import TagPanel from "./TagPanel";
import PlaceTree from "./PlaceTree";
import TrashPanel from "./TrashPanel";
import { useData } from "./dataStore";
import { EMPTY, isEmpty } from "./picks";
import { usePref } from "./prefs";
import { sourceTitle } from "./railItems";
import { useSelection } from "./selectionStore";
import { Label, QuickRow } from "./ui";
import { useView, type Filter } from "./viewStore";
import type { Bucket, GeoStats, Library } from "./types";
import { useViewportW } from "./useViewportW";

/**
 * 왼쪽 — 레일과 그 갈래의 패널, 그리고 폭 조절 손잡이.
 */
export default function Sidebar({
  filter,
  facetFilter,
  reload,
  rescan,
  addLibrary,
  dropLibrary,
}: {
  filter: Filter;
  facetFilter: Filter;
  /** 태그를 붙이거나 뗐을 때 목록을 다시 읽는다 */
  reload: () => void;
  rescan: (ids: number[]) => void;
  addLibrary: () => void;
  dropLibrary: (l: Library) => void;
}) {
  const [source, setSource] = usePref("source");
  const [panelOpen, setPanelOpen] = usePref("panelOpen");
  const [panelW, setPanelW] = usePref("panelW");
  const [libId] = usePref("libId");
  const [sort] = usePref("sort");
  const dragPanel = useRef(false);
  const viewportW = useViewportW();
  // 최소 창에서도 사진 영역과 타임라인이 쓸 폭을 남긴다.
  const panelMax = Math.max(160, Math.min(480, viewportW - 430));
  const visiblePanelW = Math.min(panelW, panelMax);

  const libs = useData((s) => s.libs);
  // 갈래 표시의 숫자는 모든 라이브러리 합 — 고른 것만 세면 다른 쪽 휴지통이 숨는다
  const trashTotal = useData((s) => s.trashByLib.reduce((a, r) => a + r.files, 0));
  const refreshTags = useData((s) => s.refreshTags);
  const sel = useView((s) => s.sel);
  const picks = useView((s) => s.picks);
  const setPicks = useView((s) => s.setPicks);
  const patchPicks = useView((s) => s.patchPicks);
  const setViewTrash = useView((s) => s.setViewTrash);
  const applySmart = useView((s) => s.applySmart);
  const showAll = useView((s) => s.showAll);
  const picked = useSelection((s) => s.picked);

  // `source`는 재실행 뒤에도 남지만 `viewTrash`는 세션 상태다. 마지막에
  // 휴지통을 보다가 앱을 닫은 경우 제목만 휴지통이고 일반 사진을 조회하지
  // 않도록, 저장된 갈래와 실제 필터를 처음부터 맞춘다.
  useEffect(() => {
    setViewTrash(source === "trash");
  }, [source, setViewTrash]);

  /// 처리 대기와 «서버에도 이름 없음»을 갈라 위치 갈래가 가능한 행동만 안내한다
  const geoRev = useData((s) => s.geoRev);
  const [geoState, setGeoState] = useState({ pending: 0, unavailable: 0 });
  useEffect(() => {
    if (source !== "location") return;
    let live = true;
    invoke<GeoStats>("geo_stats")
      .then((s) =>
        live && setGeoState({ pending: s.pending_files, unavailable: s.unavailable_files }),
      )
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [source, geoRev]);

  /// 달력이 쓸 눈금 — **날짜 조건을 뺀** 필터로 읽는다. 그리드용 buckets를
  /// 그대로 쓰면 2024년을 고른 순간 목록에 2024년만 남아 다른 해로 갈 수 없다.
  const [calendar, setCalendar] = useState<{
    forFilter: Filter | null;
    buckets: Bucket[];
  }>({ forFilter: null, buckets: [] });
  const calLoading = calendar.forFilter !== facetFilter;
  useEffect(() => {
    if (source !== "calendar") return;
    let live = true;
    invoke<Bucket[]>("files_timeline", { filter: facetFilter })
      .then((b) => {
        if (!live) return;
        setCalendar({ forFilter: facetFilter, buckets: b });
      })
      .catch(() => {
        if (!live) return;
        setCalendar({ forFilter: facetFilter, buckets: [] });
      });
    return () => {
      live = false;
    };
  }, [source, facetFilter]);

  return (
    <>
      <Rail
        value={source}
        open={panelOpen}
        trashCount={trashTotal}
        onPick={(s) => {
          // 같은 갈래를 다시 누르면 접힌다 — 사진을 넓게 보고 싶을 때
          if (s === source && panelOpen) {
            setPanelOpen(false);
            return;
          }
          setSource(s);
          setPanelOpen(true);
          setViewTrash(s === "trash");
        }}
      />

      {panelOpen && (
        <aside
          className="shrink-0 bg-chrome border-r border-line overflow-y-auto py-2"
          style={{ width: visiblePanelW }}
        >
          <Label>{sourceTitle(source)}</Label>

          {source === "all" && (
            <>
              <QuickRow
                label="모든 사진"
                count={libs.reduce((a, l) => a + l.file_count, 0)}
                on={libId === null && sel === null && isEmpty(picks)}
                onClick={showAll}
              />
              <QuickRow
                label="♥ 즐겨찾기"
                on={picks.favorite_only}
                onClick={() =>
                  setPicks({ ...EMPTY, favorite_only: !picks.favorite_only })
                }
              />
              <QuickRow
                label="영상"
                on={picks.kind === 1}
                onClick={() =>
                  setPicks({ ...EMPTY, kind: picks.kind === 1 ? null : 1 })
                }
              />
              <QuickRow
                label="RAW"
                on={picks.kind === 2}
                onClick={() =>
                  setPicks({ ...EMPTY, kind: picks.kind === 2 ? null : 2 })
                }
              />
              <QuickRow
                label="★ 4개 이상"
                on={picks.min_rating === 4}
                onClick={() =>
                  setPicks({
                    ...EMPTY,
                    min_rating: picks.min_rating === 4 ? null : 4,
                  })
                }
              />
            </>
          )}

          {source === "album" && (
            <AlbumTree
              rescan={rescan}
              addLibrary={addLibrary}
              dropLibrary={dropLibrary}
            />
          )}

          {source === "calendar" && (
            <Calendar
              buckets={calendar.buckets}
              loading={calLoading}
              year={picks.year}
              month={picks.month}
              day={picks.day}
              facetFilter={facetFilter}
              onPick={(y, m, d) => patchPicks({ year: y, month: m, day: d })}
            />
          )}
          {source === "camera" && (
            <>
              <FacetList
                kind="camera"
                filter={facetFilter}
                selected={picks.camera ?? null}
                onPick={(v) => patchPicks({ camera: v })}
              />
              <div className="px-3 pt-4 pb-1 text-[11.5px] uppercase tracking-wider text-fg-mute">
                렌즈
              </div>
              <FacetList
                kind="lens"
                filter={facetFilter}
                selected={picks.lens}
                onPick={(v) => patchPicks({ lens: v })}
              />
            </>
          )}
          {source === "location" && (
            <PlaceTree
              picks={picks}
              facetFilter={facetFilter}
              pending={geoState.pending}
              unavailable={geoState.unavailable}
              onPick={(p) => patchPicks({ ...p, place: null })}
            />
          )}
          {source === "tag" && (
            <TagPanel
              selected={picks.tag_id}
              onPick={(v) => patchPicks({ tag_id: v })}
              pickedIds={[...picked]}
              onChanged={() => {
                reload();
                refreshTags();
              }}
            />
          )}
          {source === "people" && (
            <PeoplePanel
              selected={picks.person_id}
              onPick={(v) => patchPicks({ person_id: v })}
            />
          )}
          {source === "smart" && (
            <SmartPanel
              current={filter}
              currentSort={sort}
              hasFilter={!isEmpty(picks)}
              onApply={applySmart}
            />
          )}
          {source === "search" && (
            <SearchPanel
              value={picks}
              onChange={setPicks}
              facetFilter={facetFilter}
            />
          )}
          {source === "settings" && <SettingsNav />}
          {source === "trash" && <TrashPanel />}
        </aside>
      )}

      {/* 폭 조절 — 잡고 끌면 사이드바가 넓어진다 */}
      {panelOpen && (
        <div
          role="separator"
          tabIndex={0}
          aria-label="사이드바 폭"
          aria-orientation="vertical"
          aria-valuemin={160}
          aria-valuemax={panelMax}
          aria-valuenow={visiblePanelW}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            dragPanel.current = true;
          }}
          onPointerMove={(e) => {
            if (!dragPanel.current) return;
            setPanelW(Math.max(160, Math.min(panelMax, e.clientX - 48)));
          }}
          onPointerUp={() => (dragPanel.current = false)}
          onPointerCancel={() => (dragPanel.current = false)}
          onLostPointerCapture={() => (dragPanel.current = false)}
          onKeyDown={(e) => {
            let next = visiblePanelW;
            if (e.key === "ArrowLeft") next -= 16;
            else if (e.key === "ArrowRight") next += 16;
            else if (e.key === "Home") next = 160;
            else if (e.key === "End") next = panelMax;
            else return;
            e.preventDefault();
            e.stopPropagation();
            setPanelW(Math.max(160, Math.min(panelMax, next)));
          }}
          className="w-1 shrink-0 cursor-col-resize hover:bg-accent focus:bg-focus focus:outline-none"
        />
      )}
    </>
  );
}

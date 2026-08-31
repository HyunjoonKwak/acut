import Breadcrumb from "./Breadcrumb";
import NasBadge from "./NasBadge";
import FilterButton from "./FilterBar";
import FilterChips from "./FilterChips";
import GroupMenu from "./GroupMenu";
import SelectMenu from "./SelectMenu";
import SortMenu from "./SortMenu";
import ViewBar, { ViewToggle } from "./ViewBar";
import { IconInfo } from "./icons";
import { useData } from "./dataStore";
import { usePref } from "./prefs";
import { Btn, Sep } from "./ui";
import { useUi } from "./uiStore";
import { useView } from "./viewStore";
import { useViewportW } from "./useViewportW";
import { useJob } from "./jobStore";
import type { FileRow, Mark } from "./types";

/**
 * 배경 작업 칩 — 스캔·해시·썸네일·받기가 돌 때 툴바에 보인다.
 *
 * 진행 표시가 아래 상태바(12.5px)에만 있어 «뭔가 도는지» 모르고 지나쳤다
 * (2026-08-31 «백그라운드로 뭔가 작업이 돌고 있으면 상황을 알 수 있게»).
 * 숫자 없는 상태(busy 문장)도 함께 보인다. 진행 색은 accent(규격: 진행·주 행동).
 */
function JobChip({ narrow }: { narrow: boolean }) {
  const job = useJob((st) => st.job);
  const busy = useData((st) => st.busy);
  if (!job && !busy) return null;
  const pct =
    job && job.total > 0 ? Math.min(100, (job.done / job.total) * 100) : null;
  const label = job?.label ?? busy;
  return (
    <div
      title={`${label}${job ? ` — ${job.done.toLocaleString()} / ${job.total.toLocaleString()}` : ""} · 아래 상태바에서 멈출 수 있습니다`}
      className="shrink-0 flex items-center gap-2 h-control px-2.5 rounded-md bg-raised ring-1 ring-accent/40"
    >
      <i className="w-2 h-2 rounded-full bg-accent animate-pulse shrink-0" />
      {!narrow && (
        <span className="text-[12.5px] text-fg-dim whitespace-nowrap max-w-[220px] overflow-hidden text-ellipsis">
          {label}
        </span>
      )}
      {job && job.total > 0 && (
        <>
          <span className="text-[12px] text-fg-mute tabular-nums whitespace-nowrap">
            {narrow
              ? `${Math.round(pct ?? 0)}%`
              : `${job.done.toLocaleString()} / ${job.total.toLocaleString()}`}
          </span>
          <span className="w-16 h-1 rounded bg-canvas overflow-hidden shrink-0">
            <i className="block h-full bg-accent" style={{ width: `${pct}%` }} />
          </span>
        </>
      )}
    </div>
  );
}

/**
 * 툴바 — 한 줄로 모은다.
 * 왼쪽은 «지금 어디를 보고 있나», 오른쪽은 «어떻게 볼까».
 * 진행 상황과 판정 처리는 아래 상태바로 내렸다 — 위에 줄이 셋이나 쌓여
 * 사진이 밀려나 있었다.
 */
export default function Toolbar({
  matched,
  rows,
  compareIds,
  markPicked,
  onTrash,
}: {
  /** 지금 조건에 걸린 장수 */
  matched: number;
  /** 선택 메뉴가 고를 수 있는 것 — 지금 불러온 목록 */
  rows: FileRow[];
  compareIds: number[];
  markPicked: (patch: Mark) => void;
  onTrash: (ids: number[]) => Promise<boolean>;
}) {
  const libs = useData((s) => s.libs);
  const tags = useData((s) => s.tags);
  const sel = useView((s) => s.sel);
  const viewTrash = useView((s) => s.viewTrash);
  const picks = useView((s) => s.picks);
  const setPicks = useView((s) => s.setPicks);
  const setUi = useUi((s) => s.set);
  const [libId] = usePref("libId");
  const [sort, setSort] = usePref("sort");
  const [group, setGroup] = usePref("group");
  const [gridStyle, setGridStyle] = usePref("gridStyle");
  const [filmstrip, setFilmstrip] = usePref("filmstrip");
  const [infoPanel, setInfoPanel] = usePref("infoPanel");
  const [thumbSize, setThumbSize] = usePref("thumbSize");
  // 접히는 순서 사다리(2026-08-31): 장식부터 접고 위치는 끝까지 남긴다.
  // s1(<1280) 단추 라벨 → s2(<1080) 슬라이더·보기 라벨·브레드크럼 압축 → s3(<880) 필터 칩 접힘
  const w = useViewportW();
  const s1 = w < 1280;
  const s2 = w < 1080;
  const s3 = w < 880;

  return (
    <div className="h-12 shrink-0 flex items-center gap-2 px-3 bg-chrome border-b border-line">
      {/* 줄어들고 잘리는 건 이 왼쪽 묶음뿐 — 툴바 자체가 overflow-hidden이면
          정렬·묶기 메뉴(absolute)가 사진 밑으로 잘려 안 보인다 */}
      <div className="flex-1 min-w-0 flex items-center gap-2 overflow-hidden">
        <Breadcrumb
          libs={libs}
          libId={libId}
          folder={sel?.path ?? null}
          viewTrash={viewTrash}
          matched={matched}
          compact={s2}
        />

        <FilterChips
          value={picks}
          onChange={setPicks}
          tagName={(id) => tags.get(id)}
          collapsed={s3}
        />
      </div>

      <JobChip narrow={s2} />

      <NasBadge />

      {libs.length > 0 && (
        // 창이 좁아져도 조작부는 줄어들지 않는다 — 먼저 줄어들 것은
        // 왼쪽의 빵부스러기와 조건 칩이다
        <div className="flex items-center gap-2 shrink-0">
          <FilterButton value={picks} onChange={setPicks} compact={s1} />
          <SortMenu value={sort} onChange={setSort} compact={s1} />
          <GroupMenu value={group} onChange={setGroup} compact={s1} />
          {!s2 && (
            <input
              type="range"
              min={100}
              max={320}
              value={thumbSize}
              onChange={(e) => setThumbSize(+e.target.value)}
              title="썸네일 크기"
              className="w-20 accent-accent"
            />
          )}
          <ViewBar
            style={gridStyle}
            onStyle={setGridStyle}
            filmstrip={filmstrip}
            onFilmstrip={setFilmstrip}
            compact={s2}
          />
          {/* 경계선 오른쪽은 «사진을 다루는 일» — 고르고, 골라내고, 들여다본다 (Lap의 배치) */}
          <Sep />
          <SelectMenu
            rows={rows}
            compareIds={compareIds}
            markPicked={markPicked}
            onTrash={onTrash}
          />
          <Btn tone="keep" onClick={() => setUi({ culling: true })}>
            고르기
          </Btn>
          <ViewToggle
            label="정보 패널"
            on={infoPanel}
            onClick={() => setInfoPanel(!infoPanel)}
          >
            <IconInfo className="w-[17px] h-[17px]" />
          </ViewToggle>
        </div>
      )}
    </div>
  );
}

import Breadcrumb from "./Breadcrumb";
import FilterButton from "./FilterBar";
import FilterChips from "./FilterChips";
import GroupMenu from "./GroupMenu";
import SortMenu from "./SortMenu";
import ViewBar from "./ViewBar";
import { useData } from "./dataStore";
import { fmtBytes } from "./format";
import { usePref } from "./prefs";
import { Btn, IconBtn, Menu, MenuItem, MenuSep, Sep } from "./ui";
import { useUi } from "./uiStore";
import { useView } from "./viewStore";

/**
 * 툴바 — 한 줄로 모은다.
 * 왼쪽은 «지금 어디를 보고 있나», 오른쪽은 «어떻게 볼까».
 * 진행 상황과 판정 처리는 아래 상태바로 내렸다 — 위에 줄이 셋이나 쌓여
 * 사진이 밀려나 있었다.
 */
export default function Toolbar({
  matched,
  addLibrary,
  rescan,
}: {
  /** 지금 조건에 걸린 장수 */
  matched: number;
  addLibrary: () => void;
  rescan: (ids: number[]) => void;
}) {
  const libs = useData((s) => s.libs);
  const cache = useData((s) => s.cache);
  const scanMsg = useData((s) => s.scanMsg);
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
  const [caption, setCaption] = usePref("caption");
  const [thumbSize, setThumbSize] = usePref("thumbSize");

  return (
    <div className="h-12 shrink-0 flex items-center gap-2 px-3 overflow-hidden bg-chrome border-b border-line">
      <Breadcrumb
        libs={libs}
        libId={libId}
        folder={sel?.path ?? null}
        viewTrash={viewTrash}
        matched={matched}
      />

      <FilterChips
        value={picks}
        onChange={setPicks}
        tagName={(id) => tags.get(id)}
      />

      <div className="flex-1" />

      {libs.length > 0 && (
        // 창이 좁아져도 조작부는 줄어들지 않는다 — 먼저 줄어들 것은
        // 왼쪽의 빵부스러기와 조건 칩이다
        <div className="flex items-center gap-2 shrink-0">
          <FilterButton value={picks} onChange={setPicks} />
          <SortMenu value={sort} onChange={setSort} />
          <GroupMenu value={group} onChange={setGroup} />
          <ViewBar
            style={gridStyle}
            onStyle={setGridStyle}
            filmstrip={filmstrip}
            onFilmstrip={setFilmstrip}
            caption={caption}
            onCaption={setCaption}
          />
          <input
            type="range"
            min={100}
            max={320}
            value={thumbSize}
            onChange={(e) => setThumbSize(+e.target.value)}
            title="썸네일 크기"
            className="w-20 accent-accent"
          />
          <Sep />
          <Btn tone="keep" onClick={() => setUi({ culling: true })}>
            고르기
          </Btn>
        </div>
      )}

      <Menu
        align="right"
        trigger={() => <IconBtn title="더 보기">⋯</IconBtn>}
        width={190}
      >
        {(close) => (
          <>
            <MenuItem
              hint="?"
              onClick={() => {
                close();
                setUi({ helping: true });
              }}
            >
              단축키
            </MenuItem>
            <MenuSep />
            <MenuItem
              onClick={() => {
                close();
                setUi({ importing: true });
              }}
            >
              가져오기…
            </MenuItem>
            <MenuItem
              onClick={() => {
                close();
                addLibrary();
              }}
            >
              라이브러리 추가…
            </MenuItem>
            <MenuItem
              onClick={() => {
                close();
                rescan(libId !== null ? [libId] : libs.map((l) => l.id));
              }}
            >
              다시 스캔
            </MenuItem>
            <MenuSep />
            <div className="px-3 py-1.5 text-[11.5px] text-fg-faint tabular-nums">
              {scanMsg || `캐시 ${fmtBytes(cache?.bytes ?? 0)}`}
            </div>
          </>
        )}
      </Menu>
    </div>
  );
}

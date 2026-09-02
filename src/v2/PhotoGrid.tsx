import ScrollBar from "./ScrollBar";
import Tile from "./Tile";
import { headerLabel, HEADER_H } from "./gridLayout";
import { GAP, hasCaption, type GridStyle } from "./gridStyle";
import type { GroupBy } from "./groupItems";
import { useSelection } from "./selectionStore";
import type { useGridLayout } from "./useGridLayout";
import { thumbUrl, type Bucket } from "./types";

type Layout = ReturnType<typeof useGridLayout>;

/**
 * 사진 격자와 오른쪽 타임라인 스크롤바.
 *
 * 카드·타일·양쪽 맞춤은 줄 단위로 가상화한다. 메이슨리는 줄이 없어 상자마다
 * 자리가 정해져 있고 보이는 것만 그린다.
 */
export default function PhotoGrid({
  loading,
  error,
  empty,
  layout,
  baseIndex,
  buckets,
  caption,
  gridStyle,
  group,
  onPick,
  onOpen,
  onContext,
  onSeek,
  onRetry,
}: {
  loading: boolean;
  /** 목록 조회가 실패했다 — 빈 사진 목록과 구분해 다시 시도할 수 있게 한다 */
  error: string | null;
  /** 등록된 라이브러리가 하나도 없다 */
  empty: boolean;
  layout: Layout;
  /** rows[0]이 전체에서 몇 번째인가 */
  baseIndex: number;
  buckets: Bucket[];
  caption: boolean;
  gridStyle: GridStyle;
  group: GroupBy;
  onPick: (id: number, e: React.MouseEvent) => void;
  /** rows 안 위치로 크게 보기 */
  onOpen: (index: number) => void;
  onContext: (id: number, e: React.MouseEvent) => void;
  onSeek: (index: number) => void;
  onRetry: () => void;
}) {
  const selected = useSelection((s) => s.selected);
  const picked = useSelection((s) => s.picked);
  const {
    scrollRef,
    onScroll,
    cols,
    rowH,
    imageH,
    grid,
    justified,
    masonry,
    virt,
    topOffset,
    pageSize,
  } = layout;
  // 이름줄은 카드에서만 (Lap과 같다)
  const cap = caption && hasCaption(gridStyle);

  const header = (
    label: string,
    count: number,
    style: React.CSSProperties,
    key: string,
  ) => (
    <div key={key} style={style} className="flex items-baseline gap-2 px-0.5">
      <span className="text-[14px] font-semibold text-fg">
        {headerLabel(label, group)}
      </span>
      <span className="text-[12.5px] text-fg-mute tabular-nums">
        {count.toLocaleString()}장
      </span>
      <div className="flex-1 h-px bg-line" />
    </div>
  );

  return (
    <div className="flex-1 flex min-h-0 min-w-0 relative">
      <main
        ref={scrollRef}
        onScroll={onScroll}
        className="flex-1 overflow-y-auto p-2.5"
      >
        {error && (
          <div
            role="alert"
            className="sticky top-0 z-20 mx-auto mb-2 flex max-w-2xl items-center gap-3 rounded-lg bg-raised px-3 py-2 text-[13px] text-fg ring-1 ring-drop/60 shadow-lg"
          >
            <span className="min-w-0 flex-1">
              사진을 불러오지 못했습니다
              <span className="ml-2 text-fg-mute break-all">{error}</span>
            </span>
            <button
              onClick={onRetry}
              className="h-control shrink-0 rounded-md px-3 text-fg ring-1 ring-line-strong hover:bg-hover"
            >
              다시 시도
            </button>
          </div>
        )}
        {empty && (
          <div className="h-full flex items-center justify-center text-fg-mute">
            「라이브러리 추가」로 사진 폴더를 등록하세요
          </div>
        )}

        {masonry ? (
          /* 메이슨리 — 상자마다 자리가 정해져 있다. 보이는 것만 그린다. */
          <div style={{ height: masonry.height, position: "relative" }}>
            {masonry.headers.map((h) =>
              header(
                h.label,
                h.count,
                {
                  position: "absolute",
                  top: h.y,
                  left: 0,
                  width: "100%",
                  height: HEADER_H,
                },
                `h${h.y}`,
              ),
            )}
            {masonry.visible.map((b) => (
              <div
                key={b.file.id}
                style={{
                  position: "absolute",
                  left: b.x,
                  top: b.y,
                  width: b.w,
                }}
              >
                <Tile
                  file={b.file}
                  url={thumbUrl(b.file)}
                  picked={picked.has(b.file.id)}
                  focused={selected === b.file.id}
                  onClick={(e) => onPick(b.file.id, e)}
                  onDoubleClick={() => onOpen(b.index)}
                  onContextMenu={(e) => onContext(b.file.id, e)}
                  caption={false}
                  style="masonry"
                  aspect={{ width: b.w, height: b.h }}
                />
              </div>
            ))}
          </div>
        ) : (
          <div style={{ height: virt.getTotalSize(), position: "relative" }}>
            {virt.getVirtualItems().map((v) => {
              const row = grid[v.index];
              if (!row) return null;
              const box = {
                position: "absolute" as const,
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${v.start}px)`,
              };
              if (row.kind === "header") {
                return header(
                  row.label,
                  row.count,
                  { ...box, height: HEADER_H },
                  String(v.key),
                );
              }
              const jrows = justified?.get(v.index);
              if (jrows) {
                // 양쪽 맞춤 — 한 「줄」 안에 여러 소줄이 들어간다
                let n = row.start;
                return (
                  <div key={v.key} style={box}>
                    {jrows.map((jr, ri) => (
                      <div
                        key={ri}
                        className="flex"
                        style={{ gap: GAP, marginBottom: GAP }}
                      >
                        {jr.items.map(({ file, width }) => {
                          const at = n++;
                          return (
                            <Tile
                              key={file.id}
                              file={file}
                              url={thumbUrl(file)}
                              picked={picked.has(file.id)}
                              focused={selected === file.id}
                              onClick={(e) => onPick(file.id, e)}
                              onDoubleClick={() => onOpen(at)}
                              onContextMenu={(e) => onContext(file.id, e)}
                              caption={cap}
                              style="justified"
                              aspect={{ width, height: jr.height }}
                            />
                          );
                        })}
                      </div>
                    ))}
                  </div>
                );
              }
              return (
                <div
                  key={v.key}
                  style={{
                    ...box,
                    height: rowH,
                    display: "grid",
                    gridTemplateColumns: `repeat(${cols}, minmax(0,1fr))`,
                    gap: GAP,
                  }}
                >
                  {row.items.map((r, ci) => (
                    <Tile
                      key={r.id}
                      file={r}
                      url={thumbUrl(r)}
                      picked={picked.has(r.id)}
                      focused={selected === r.id}
                      onClick={(e) => onPick(r.id, e)}
                      onDoubleClick={() => onOpen(row.start + ci)}
                      onContextMenu={(e) => onContext(r.id, e)}
                      caption={cap}
                      style={gridStyle}
                      aspect={{ height: imageH }}
                    />
                  ))}
                </div>
              );
            })}
          </div>
        )}
        {loading && (
          <div className="py-4 text-center text-fg-mute">불러오는 중…</div>
        )}
      </main>

      {/* 타임라인 스크롤바 — 전역 순번으로 주고받는다 */}
      <ScrollBar
        buckets={buckets}
        offset={baseIndex + topOffset}
        pageSize={pageSize}
        onSeek={onSeek}
      />
    </div>
  );
}

import ScrollBar from "./ScrollBar";
import Tile from "./Tile";
import { headerLabel, HEADER_H } from "./gridLayout";
import { GAP, type GridStyle, type Scaling } from "./gridStyle";
import type { GroupBy } from "./groupItems";
import { useSelection } from "./selectionStore";
import type { useGridLayout } from "./useGridLayout";
import { thumbUrl, type Bucket } from "./types";

type Layout = ReturnType<typeof useGridLayout>;

/**
 * 사진 격자와 오른쪽 타임라인 스크롤바.
 *
 * 줄 단위로 가상화한다. 카드·타일은 줄 높이가 같고, 양쪽 맞춤은 한 줄이
 * 여러 소줄로 나뉘어 높이가 제각각이다.
 */
export default function PhotoGrid({
  loading,
  empty,
  layout,
  baseIndex,
  buckets,
  caption,
  gridStyle,
  scaling,
  group,
  onPick,
  onOpen,
  onContext,
  onSeek,
}: {
  loading: boolean;
  /** 등록된 라이브러리가 하나도 없다 */
  empty: boolean;
  layout: Layout;
  /** rows[0]이 전체에서 몇 번째인가 */
  baseIndex: number;
  buckets: Bucket[];
  caption: boolean;
  gridStyle: GridStyle;
  scaling: Scaling;
  group: GroupBy;
  onPick: (id: number, e: React.MouseEvent) => void;
  /** rows 안 위치로 크게 보기 */
  onOpen: (index: number) => void;
  onContext: (id: number, e: React.MouseEvent) => void;
  onSeek: (index: number) => void;
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
    virt,
    topOffset,
    pageSize,
  } = layout;

  return (
    <div className="flex-1 flex min-h-0 min-w-0 relative">
      <main
        ref={scrollRef}
        onScroll={onScroll}
        className="flex-1 overflow-y-auto p-2.5"
      >
        {empty && (
          <div className="h-full flex items-center justify-center text-fg-mute">
            「라이브러리 추가」로 사진 폴더를 등록하세요
          </div>
        )}
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
              return (
                <div
                  key={v.key}
                  style={{ ...box, height: HEADER_H }}
                  className="flex items-baseline gap-2 px-0.5"
                >
                  <span className="text-[13px] font-semibold text-fg">
                    {headerLabel(row.label, group)}
                  </span>
                  <span className="text-[11.5px] text-fg-mute tabular-nums">
                    {row.count.toLocaleString()}장
                  </span>
                  <div className="flex-1 h-px bg-line" />
                </div>
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
                            caption={caption}
                            style="justified"
                            scaling={scaling}
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
                    caption={caption}
                    style={gridStyle}
                    scaling={scaling}
                    aspect={{ height: imageH }}
                  />
                ))}
              </div>
            );
          })}
        </div>
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

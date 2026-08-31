import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useData } from "./dataStore";
import { STYLES } from "./gridStyle";
import { useJob } from "./jobStore";
import { usePref } from "./prefs";
import { toast } from "./toastStore";
import { Btn } from "./ui";
import { Section, Row, Select, Toggle } from "./settingsUi";

export function General() {
  return (
    <Section id="general" title="일반">
      <Row
        label="글꼴"
        hint="프리텐다드는 앱에 들어 있어 어디서나 같게 보입니다. 시스템은 SF Pro + Apple SD Gothic Neo."
      >
        <Select
          k="font"
          options={[
            { v: "pretendard", label: "프리텐다드" },
            { v: "system", label: "시스템 글꼴" },
          ]}
        />
      </Row>
      <Row
        label="테마"
        hint="어두운 테마 하나입니다. 밝은 테마는 아직 없습니다."
      >
        <span className="text-[13.5px] text-fg-mute">어두움</span>
      </Row>
      <Row label="이름표" hint="레일·툴바 버튼에 커서를 올리면 뜨는 설명">
        <Toggle k="tooltips" />
      </Row>
      <Row
        label="상태바"
        hint="아래쪽 한 줄 — 지금 보고 있는 사진의 정보와 진행"
      >
        <Toggle k="statusBar" />
      </Row>
    </Section>
  );
}

export function Library({ onRescanAll }: { onRescanAll: () => void }) {
  const scanMsg = useData((s) => s.scanMsg);
  const scanning = useJob((s) => s.job !== null);
  return (
    <Section id="library" title="라이브러리">
      <Row
        label="폴더 감시"
        hint="파인더로 넣거나 지운 사진이 저절로 반영됩니다. 복사가 멎고 1.5초 뒤에 그 폴더만 다시 봅니다."
      >
        <Toggle k="watch" />
      </Row>
      <Row
        label="다시 스캔"
        hint={
          scanMsg ||
          "새로 들어온 사진과 썸네일을 채웁니다. 이미 아는 것은 건너뜁니다."
        }
      >
        <Btn disabled={scanning} onClick={onRescanAll}>
          {scanning ? "스캔 중…" : "전부 다시 스캔"}
        </Btn>
      </Row>
      <Row
        label="영상 촬영일"
        hint="영상은 EXIF가 없어 Spotlight가 준 시각을 썼는데, 색인이 없으면 파일을 복사한 날이 잡힙니다. 파일 안에 박힌 촬영 시각을 다시 읽어 고칩니다."
      >
        <Btn
          disabled={scanning}
          onClick={() =>
            invoke("video_dates_refresh", { libraryId: null }).catch((e) =>
              toast(String(e), "drop"),
            )
          }
        >
          다시 읽기
        </Btn>
      </Row>
    </Section>
  );
}

export function Browse() {
  const [thumbSize, setThumbSize] = usePref("thumbSize");
  const [style] = usePref("gridStyle");
  return (
    <Section id="browse" title="탐색">
      <GeoRow />
      <Row label="보기 방식" hint="툴바 버튼과 같은 것입니다">
        <Select
          k="gridStyle"
          options={STYLES.map((s) => ({ v: s.v, label: s.label }))}
        />
      </Row>
      <Row label="썸네일 크기">
        <input
          type="range"
          min={100}
          max={320}
          value={thumbSize}
          onChange={(e) => setThumbSize(+e.target.value)}
          aria-label="썸네일 크기"
          className="w-40 accent-accent"
        />
        <span className="w-10 text-right text-[13px] text-fg-mute tabular-nums">
          {thumbSize}
        </span>
      </Row>
      <Row
        label="이름줄 표시"
        hint={
          style === "card"
            ? "사진 아래 이름과 날짜·크기"
            : "카드 보기에서만 뜹니다"
        }
      >
        <Toggle k="caption" />
      </Row>
      <Row
        label="이름줄 첫째 줄"
        hint={style === "card" ? undefined : "카드 보기에서만 뜹니다"}
      >
        <Select
          k="caption1"
          options={[
            { v: "name", label: "파일 이름" },
            { v: "date", label: "촬영일" },
          ]}
        />
      </Row>
      <Row label="이름줄 둘째 줄">
        <Select
          k="caption2"
          options={[
            { v: "dateSize", label: "촬영일 · 크기" },
            { v: "date", label: "촬영일" },
            { v: "size", label: "크기" },
            { v: "camera", label: "카메라" },
            { v: "none", label: "없음" },
          ]}
        />
      </Row>
      <Row label="타일 배지" hint="타일 왼쪽 아래에 붙는 작은 표시">
        <Select
          k="badge"
          options={[
            { v: "none", label: "없음" },
            { v: "format", label: "형식 (RAW·영상)" },
            { v: "iso", label: "ISO" },
            { v: "shutter", label: "셔터 속도" },
            { v: "aperture", label: "조리개" },
            { v: "focal", label: "초점 거리" },
          ]}
        />
      </Row>
      <Row label="두 번 누르면">
        <Select
          k="dblClick"
          options={[
            { v: "viewer", label: "크게 보기" },
            { v: "app", label: "기본 앱으로 열기" },
          ]}
        />
      </Row>
      <Row label="필름스트립 띠 자리">
        <Select
          k="stripPos"
          options={[
            { v: "top", label: "위" },
            { v: "bottom", label: "아래" },
          ]}
        />
      </Row>
    </Section>
  );
}

export function ViewerSection() {
  return (
    <Section id="viewer" title="뷰어">
      <Row label="마우스 휠" hint="확대는 커서 자리를 기준으로 합니다">
        <Select
          k="wheel"
          options={[
            { v: "zoom", label: "확대·축소" },
            { v: "next", label: "앞뒤 사진" },
          ]}
        />
      </Row>
      <Row label="배경">
        <Select
          k="viewerBg"
          options={[
            { v: "canvas", label: "앱 바탕" },
            { v: "black", label: "검정" },
            { v: "gray", label: "회색" },
          ]}
        />
      </Row>
      <Row label="슬라이드쇼 간격">
        <Select
          k="slideshowSec"
          options={[2, 3, 5, 10].map((n) => ({ v: n, label: `${n}초` }))}
        />
      </Row>
      <Row label="영상 자동 재생" hint="크게 보기와 필름스트립에서">
        <Toggle k="autoplay" />
      </Row>
      <Row label="영상 반복">
        <Toggle k="loopVideo" />
      </Row>
    </Section>
  );
}


/** 지명 채우기 — 좌표를 국가·시도·시군구 이름으로. 격자마다 한 번만 묻는다 */
function GeoRow() {
  const [st, setSt] = useState<{ with_gps: number; named: number; cells_left: number } | null>(null);
  const hasJob = useJob((s) => s.job !== null);
  const load = () =>
    invoke<{ with_gps: number; named: number; cells_left: number }>("geo_stats")
      .then(setSt)
      .catch(() => {});
  useEffect(() => {
    void load();
  }, []);
  // 일이 끝나면 숫자를 다시 읽는다
  useEffect(() => {
    if (!hasJob) void load();
  }, [hasJob]);

  const left = st?.cells_left ?? 0;
  const mins = Math.ceil((left * 1.1) / 60);
  return (
    <Row
      label="지명 채우기"
      hint={
        st
          ? `좌표가 있는 사진 ${st.with_gps.toLocaleString()}장 중 ${st.named.toLocaleString()}장에 지명이 붙어 있습니다.${
              left > 0
                ? ` 남은 곳 ${left.toLocaleString()}군데 — 약 ${mins}분 걸립니다(OpenStreetMap 에 초당 한 번만 묻습니다).`
                : " 다 붙었습니다."
            }`
          : "좌표를 국가·시도·시군구 이름으로 바꿔 위치 갈래에서 이름으로 찾습니다."
      }
    >
      <Btn
        disabled={hasJob || left === 0}
        onClick={() => {
          invoke("geo_fill_start")
            .then(() => toast("지명을 채웁니다 — 진행은 위 작업 표시에서 볼 수 있습니다"))
            .catch((e) => toast(String(e), "drop"));
        }}
      >
        {left > 0 ? `${left.toLocaleString()}군데 채우기` : "다 채웠습니다"}
      </Btn>
    </Row>
  );
}

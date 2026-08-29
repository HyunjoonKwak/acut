import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { reportError } from "./feLog";

type State = { error: Error | null; stack: string };

/**
 * 그리다 예외가 나면 React 19는 뿌리째 내려 버린다 — 창이 까맣게 빈다.
 *
 * 여기서 받아서 무엇이 났는지 화면에 보여 주고, 로그 파일에 남기고,
 * 다시 열 수 있게 한다. 실측: «2018 수납정리» 폴더에 들어가면 창이 비었는데
 * 아무 단서도 없었다.
 */
export default class ErrorBoundary extends Component<
  { children: ReactNode },
  State
> {
  state: State = { error: null, stack: "" };
  private beat: number | null = null;

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const stack = info.componentStack ?? "";
    this.setState({ stack });
    reportError(
      "render",
      `${error.name}: ${error.message}\n${error.stack ?? ""}\n--- components\n${stack}`,
    );
    // App이 내려가면 심장박동도 멎는다 — 감시 스레드가 20초 뒤 창을 새로 부르기
    // 전에 사용자가 읽을 수 있게, 여기서 대신 뛴다.
    this.beat = window.setInterval(() => {
      invoke("heartbeat").catch(() => {});
    }, 5000);
  }

  componentWillUnmount() {
    if (this.beat !== null) window.clearInterval(this.beat);
  }

  render() {
    const { error, stack } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="h-screen bg-canvas text-fg p-8 overflow-auto select-text">
        <h1 className="text-lg font-semibold mb-2">
          화면을 그리다 오류가 났습니다
        </h1>
        <p className="text-fg-dim text-sm mb-4">
          아래 내용은 로그 파일에도 남았습니다
          (~/Library/Logs/com.acut.media/webview.log).
        </p>
        <pre className="text-xs bg-chrome border border-line rounded p-3 whitespace-pre-wrap mb-4">
          {error.name}: {error.message}
          {"\n"}
          {error.stack ?? ""}
          {"\n"}
          {stack}
        </pre>
        <button
          onClick={() => location.reload()}
          className="px-3 py-1.5 rounded bg-raised border border-line text-sm"
        >
          다시 열기
        </button>
      </div>
    );
  }
}

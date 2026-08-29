import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
// v1 화면은 legacy/src/ 에 있다 (빌드 밖).
import App from "./v2/App.tsx";
import { ConfirmProvider } from "./v2/confirm.tsx";
import Hydrated from "./v2/Hydrated.tsx";
import ErrorBoundary from "./v2/ErrorBoundary.tsx";
import { installErrorLog } from "./v2/feLog.ts";
import { mark } from "./v2/startupMarks.ts";

mark("script");
installErrorLog();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* 그리다 난 예외를 여기서 받는다 — 안 받으면 창이 통째로 빈다 */}
    <ErrorBoundary>
      {/* 물음 상자는 App 위에 있어야 App 안에서도 부를 수 있다 */}
      <ConfirmProvider>
        <Hydrated>
          <App />
        </Hydrated>
      </ConfirmProvider>
    </ErrorBoundary>
  </StrictMode>,
);

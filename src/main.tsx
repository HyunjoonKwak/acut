import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./index.css";
// v1 화면은 legacy/src/ 에 있다 (빌드 밖).
import App from "./v2/App.tsx";
import { ConfirmProvider } from "./v2/confirm.tsx";
import Hydrated from "./v2/Hydrated.tsx";
import { mark } from "./v2/startupMarks.ts";

mark("script");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* 물음 상자는 App 위에 있어야 App 안에서도 부를 수 있다 */}
    <ConfirmProvider>
      <Hydrated>
        <App />
      </Hydrated>
    </ConfirmProvider>
  </StrictMode>,
);

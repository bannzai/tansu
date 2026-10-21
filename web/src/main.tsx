import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { recordWebOpen } from "./api.ts";
import { takeTokenFromUrl } from "./token.ts";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html に #root がありません");
}

// tansu serve が表示した URL の #token= を、API を呼ぶ前に受け取る
takeTokenFromUrl();

// 画面の読み込みを 1 回だけ利用記録に書く (StrictMode が effect を 2 回動かしても重ならないよう、React の外で呼ぶ)。
// 書けなくても画面は使えるので、失敗は console に残すだけにする
recordWebOpen().catch((error: unknown) => console.warn("利用記録を書けませんでした", error));

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

import type { ComponentProps } from "react";
import { navigate } from "./route.ts";

/**
 * 画面の中のリンク。クリックで履歴に積んで画面を移り (ページを読み込み直さない)、修飾キー付きのクリック
 * (新しいタブで開く等) はブラウザに任せる。
 */
export function Link({ to, children, ...anchorProps }: Omit<ComponentProps<"a">, "href" | "onClick"> & { to: string }) {
  return (
    <a
      {...anchorProps}
      href={to}
      onClick={(event) => {
        if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
          return;
        }
        event.preventDefault();
        navigate(to);
      }}
    >
      {children}
    </a>
  );
}

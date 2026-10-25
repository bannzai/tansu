import { type ReactNode, useState } from "react";

/**
 * 画像の URL `url` のサムネイル。押すと元の URL を新しいタブで開く。読み込めなかった時は `fallback` を描く
 * (壊れた画像のアイコンを出さないため)。行の詳細のプロパティ・関連先のカード・本文の画像で同じものを使い、
 * 表のセルでは使わない (行が多い表で画像を並べると重くなるため)。`url` が変わると読み込みの失敗を忘れるよう、
 * 呼ぶ側は `key` に `url` を渡す。
 */
export function ImageThumbnail({ url, alt, fallback }: { url: string; alt: string; fallback: ReactNode }) {
  const [failed, setFailed] = useState(false);
  if (failed) {
    return fallback;
  }
  return (
    <a href={url} target="_blank" rel="noreferrer" className="thumbnail">
      <img src={url} alt={alt} loading="lazy" referrerPolicy="no-referrer" onError={() => setFailed(true)} />
    </a>
  );
}

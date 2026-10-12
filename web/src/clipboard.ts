/**
 * `text` をクリップボードに書く。Clipboard API は secure context (https か localhost) でだけ使えるため、
 * 同じ LAN の iPhone が http で LAN の IP を開いた時は、隠した textarea を選択してコピーする。
 */
export async function copyText(text: string): Promise<void> {
  if (window.isSecureContext && navigator.clipboard !== undefined) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.append(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) {
    throw new Error("クリップボードにコピーできませんでした");
  }
}

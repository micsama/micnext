/** `navigator.clipboard` 只在安全上下文可用（localhost 算）；部署到纯 http 时退回 execCommand。 */
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard && isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.append(ta);
  ta.select();
  document.execCommand("copy");
  ta.remove();
}

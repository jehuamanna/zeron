/**
 * Copy text, falling back to the selected text of `field` when the async
 * clipboard API is unavailable (WebKitGTK may refuse it). Returns whether the
 * copy is known to have succeeded; on `false` the text is left selected.
 */
export async function copyText(text: string, field: HTMLInputElement | HTMLTextAreaElement | null): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    if (!field) return false;
    field.focus();
    field.select();
    try {
      return document.execCommand("copy");
    } catch {
      return false;
    }
  }
}

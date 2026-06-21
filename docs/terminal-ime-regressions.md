# Terminal IME Regression Notes

## macOS WebKit + xterm Chinese IME

The terminal uses xterm's hidden textarea and `.composition-view` for IME input. On macOS
WKWebView, Chinese IME commits can emit multiple overlapping signals:

- `compositionend` may contain stale pinyin such as `shi'de`.
- `beforeinput` or xterm `onData` may later contain committed Chinese plus the stale tail,
  for example `是的shi'de`.
- xterm's `.composition-view.active` can keep showing the black pinyin preview even after
  the terminal data has already been filtered.
- xterm's own `CompositionHelper` must still receive `compositionend`; otherwise it keeps
  its internal composing flag set and later English input can appear blank or stop working.

Required behavior:

- Typing Chinese such as `是的` must not leave a visible black `shi'de` tail.
- Mixed data such as `是的shi'de` must be normalized to `是的`.
- The xterm `.composition-view` may be cleared defensively by Aeroric, but xterm must still
  receive the native `compositionend` event.
- If the user starts pinyin such as `ye's`, then switches to English input, Aeroric must
  commit `yes` promptly and must not block the next English keystroke.
- Aeroric must not call `stopImmediatePropagation()` on terminal `compositionend`; duplicate
  text should be filtered in `beforeinput` / xterm `onData` instead, while xterm releases its
  own IME state normally.
- If an IME switch path blurs the textarea without a native `compositionend`, Aeroric should
  synthesize a `compositionend` only to release xterm's internal composing state after committing
  the romanized text.

Before changing terminal IME logic, run:

```bash
pnpm vitest run src/test/terminal-input-fix.test.ts
```

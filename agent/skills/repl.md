<!--
SPDX-FileCopyrightText: 2026 buntOS contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->
# buntOS desktop REPL

Read and control the apps on this buntOS desktop with JavaScript. The API copies Playwright:
`desktop` stands for the browser, an app for a page, and the refs of a snapshot (`e12`) for
`aria-ref`s. Under it are accessibility trees, so it works on GTK apps, Firefox, Chromium and
Electron apps alike. Run code with `bunt agent repl '<code>'` or the MCP tool `repl`.

## Calls

- Top-level `await` works. The value of the last expression is printed, as in a console:
  strings as they are, results the way `bunt agent` prints them, other values as JSON.
  `--json` also returns it as data when it fits in 1 MiB, 100 levels and 50 000 values.
- Variables live between calls (for each AI and session). Top-level `const` and `let` may be
  declared again in a later call.
- `console.log` prints. `setTimeout` works within a call.
- Each call stops after 30 s by default (`--timeout <ms>` or the tool's `timeout`, at most
  120 s). The REPL has 32 MiB of memory and prints at most 100 000 characters.
- There is no file, network or process API: only `desktop`, `console` and timers.

## Read

```js
const calc = desktop.app('Calculator')  // app name, desktop ID, window title or pid; found on each use
await calc.snapshot()                    // compact tree: - button "=" [ref=e185]
await calc.snapshot({ interactive: true })  // only what can be clicked or typed into
await calc.snapshot({ diff: true })      // only the lines that changed since the last snapshot
await desktop.snapshot()                 // the window you worked on last (else the focused one)
await desktop.apps()                     // running apps and their windows
await desktop.installed()                // apps that can be opened
await calc.locator('e159').snapshot()    // only below a ref
```

One line per element: role, name, `[ref=…]` on what you can act on, then states and values
(`[focused]`, `[checked]`, `[value="42"]`, `[offscreen]`). Refs stay the same for the same
element. Read the interactive tree first, the full tree when you need the text, and take a
screenshot only when the tree does not show what you need.

## Find

```js
calc.locator('e185')                              // a ref from a snapshot ('aria-ref=e185' works too)
calc.getByRole('button', { name: '=', exact: true })  // role and name, as in the tree
calc.getByText('42')                              // a name, text or field value containing it
calc.locator('e158').getByRole('listitem')        // only below a ref
calc.getByRole('button').nth(2)                   // .first(), .last(), .nth(i) when several match
```

`getByRole` and `getByText` look for the element when you use them, waiting up to 5 s for it
to appear (`timeout` option). When several elements match they refuse to guess and list them.
A text line has no ref; acting on it acts on the nearest element above it that has one. Long
names are cut in the tree (`"Terms…[+80 chars]"`): `exact: true` then needs the whole name, so
match a part of it without `exact`.

## Act

```js
await calc.getByRole('button', { name: '7' }).click()    // accessible action, else the pointer
await calc.locator('e185').click({ pointer: true })      // press it with the pointer
await editor.locator('e9').fill('text')                  // replace a field's text, checked afterwards
await editor.locator('e9').type('안녕하세요')              // add text at the end of a field, any language
await editor.locator('e9').press('Control+a Backspace')  // clear a field (other keys: focus, then keys)
await settings.locator('e26').set(6)                     // slider or spin button value
await settings.getByRole('switch', { name: 'Wi-Fi' }).check()  // also uncheck(), focus(), select()
await page.locator('e40').scrollIntoView()               // scroll until it is on screen
await page.locator('e3').scroll('down', 5)               // turn the wheel over a list or page
await calc.keyboard.press('Escape')                      // keys to the app's window, no ref
await desktop.keyboard.type('more')                      // only while your last window has the focus
await term.keyboard.type('ls', { keys: true })           // keys: true presses a key per character
await canvas.mouse.click(120, 340)                       // window pixels of its screenshot
await desktop.launch('org.gnome.TextEditor')             // open an app (or bring it forward)
```

The user works on this computer while you do. Text fields outside web pages and browsers take
`type`, `fill` and `press('Control+a Backspace')` through accessibility, without the keyboard
focus, so they work while the user types elsewhere. `desktop.keyboard` and `desktop.mouse.wheel`
go only to the window you worked on last, and only while it still has the keyboard focus. Keys
to an app, text on web pages, pointer clicks and the wheel are checked by the compositor, which
refuses while the user is typing or using the pointer. A refused action sent nothing; do not
take the focus back by activating or clicking again.

Every action waits until the window stops changing (at most 3 s; `settle` option in ms) and
returns what it did and the diff of that window, so you rarely need a new snapshot. The first
line names the app and window that received it and the way (`accessibility` or
`target-checked input`):

```
# e185: Calculator "Calculator" (accessibility): clicked with the accessible action "click" (settled in 533 ms)
@@ -4 +4 @@
-  - textbox [ref=e159] [focused] [value="6×7"]
+  - textbox [ref=e159] [focused] [value="42"]
```

Key names: Playwright's (`Enter`, `ArrowLeft`, `Control+A`, `Meta`) or xkb's (`Return`, `Left`,
`ctrl+a`, `super`); several keys separated by spaces or in an array.

When a whole document is replaced (a link to another page), a very long diff is summed up as
`# the document was replaced: 1529 lines → 2049 lines; take a snapshot to read it`. A dialog that
opens over a page is told apart: `# dialog "Quit" opened; the rest of the window (1529 lines) left
the tree while it is up`. When the new tree fits in the limit, it follows the line after
`; it now reads:`. Otherwise take a snapshot, or raise the limit with
`snapshot({ diff: true, maxLines: 5000 })` (400 lines by default).

## Wait

Do not sleep. Wait for what you expect, with a time limit (10 s by default, `timeout` in ms):

```js
await calc.wait({ text: '42' })                     // a name, text or value with it appears
await editor.wait({ role: 'dialog', gone: true })   // until it goes away
await editor.waitForWindow('Save')                  // a window whose title has it is open
await editor.waitForWindow()                        // any new window of the app
await page.waitForLoadState()                       // the visible document has finished loading
await calc.locator('e159').wait({ value: '42' })    // the element's value becomes this
await calc.locator('e159').wait({ changed: true })  // the element's value changes
await dialog.getByRole('button', { name: 'OK' }).waitFor({ state: 'hidden' })
```

Waiting for a window also waits until its document has finished loading, so a snapshot right
after it has the page body. A wait stops with `TargetGoneError` when the app it watches quits or
is restarted (a new process); take a new snapshot then, because refs from the old one are gone.

Something is gone only when the whole scope was read and it was not there, or when its app quit.
While no window has the keyboard focus or a tree is cut at 2000 lines, the wait goes on; wait
below a ref (`locator('e12').getByText(…)`) to read a smaller part.

## Read values

```js
await calc.getByRole('textbox').first().inputValue()   // a field's value
await calc.locator('e192').textContent()               // names and text below it
await calc.getByText('42').count()                      // also isVisible(), isChecked(), isEnabled()
const shot = await calc.screenshot()                    // PNG of one window; shot.path
```

`info()` gives one element as `{ ref, role, name, value, states, text, line }`. Screenshots
come back as images through MCP; `mouse.click(x, y)` takes the pixels of that image.

## Errors

Failures throw `DesktopError` with a `code`; these have their own names:

- `NotAllowedError` (-32001): the user has not allowed this AI, or an authentication dialog is
  open. Only the user can answer it.
- `BlockedAppError` (-32002): the user blocked AI agents from this app. Leave it alone.
- `ScreenLockedError` (-32003): the screen is locked; wait until the user unlocks it.
- `TimeoutError` (-32004): what you waited for did not happen in time.
- `FocusMovedError` (-32005): the user moved the keyboard focus away from your window, so
  nothing was sent. Use a ref or `desktop.app(name)`; never take the focus back.
- `UserActiveError` (-32006): the user is typing or using the pointer, so nothing was sent.
  `retryAfterMs` says when to try again; meanwhile use accessible actions.
- `TargetGoneError` (-32007): the app of that ref quit or restarted. Take a new snapshot.
- `PausedError` (-32008): the user paused AI control. Reading still works; only the user can
  resume it.

The messages come from the daemon and name its command-line options: `--app` is
`desktop.app(name)`, `--ref` a locator, `--keys` the `keys` option, `--keep-focus` the
`keepFocus` option, `--settle` the `settle` option and `--pointer` the `pointer` option.

## Example: 6 × 7 on the calculator

```js
const calc = await desktop.launch('org.gnome.Calculator');
for (const key of ['6', '×', '7', '=']) {
  await calc.getByRole('button', { name: key, exact: true }).click();
}
await calc.getByRole('textbox').first().inputValue()   // '42'
```

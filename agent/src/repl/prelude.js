// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// REPL의 전역 API (ARCHITECTURE 6.4·6.6절 3단계). 워커(worker.rs)가 REPL을 열 때 한 번 돌린다.
//
// Playwright 문법을 본떠 데스크톱 앱을 다룬다: desktop.app('계산기').getByRole('button', { name: '=' }).click().
// 데몬은 native.call 하나로만 부르고, 데몬이 부를 때마다 정책(허용한 AI, 막은 앱, 사람만 답하는 창, 화면 잠금)을
// 본다. 파일·네트워크·프로세스 API는 없다. 결과는 `bunt agent`의 출력과 같은 글로 보인다([INSPECT]).
// 마지막에 워커가 쓸 함수(타이머 부르기, 값과 오류 적기)를 돌려준다.
'use strict';
(() => {
  const native = globalThis.__bunt;
  delete globalThis.__bunt;
  // 스레드가 하나라 쓸모가 없고, Atomics.wait는 시간 상한을 넘겨 멈춘다
  delete globalThis.Atomics;
  delete globalThis.SharedArrayBuffer;

  const INSPECT = Symbol('bunt.inspect');
  // getByRole·getByText로 동작할 때 요소가 나타나기를 기다리는 기본 시간
  const ACTION_TIMEOUT = 5000;
  const NOTCHES = 5;
  // Playwright의 mouse.wheel은 픽셀을 받는다. 눈금 하나를 이만큼으로 어림한다 (Chromium의 휠 한 칸)
  const WHEEL_PIXELS = 100;
  // 값을 JSON으로 넘기는 길이의 상한 (글로 적은 것은 워커가 자른다)
  const JSON_LIMIT = 1 << 20;
  // 여러 요소가 맞을 때 알리는 줄 수
  const STRICT_LINES = 10;

  // ---- 오류 ----

  class DesktopError extends Error {
    constructor(message, code) {
      super(message);
      this.name = 'DesktopError';
      this.code = code;
    }
  }
  const kind = (name) =>
    class extends DesktopError {
      constructor(message, code) {
        super(message, code);
        this.name = name;
      }
    };
  const NotAllowedError = kind('NotAllowedError');
  const BlockedAppError = kind('BlockedAppError');
  const ScreenLockedError = kind('ScreenLockedError');
  const TimeoutError = kind('TimeoutError');
  // 데몬의 JSON-RPC 오류 번호 (protocol.rs)
  const ERRORS = {
    '-32001': NotAllowedError,
    '-32002': BlockedAppError,
    '-32003': ScreenLockedError,
    '-32004': TimeoutError,
  };

  /** 데몬의 메서드 하나를 부른다. 실패는 오류 번호에 맞는 DesktopError로 던진다 */
  function call(method, params) {
    const reply = JSON.parse(native.call(method, JSON.stringify(params ?? {})));
    if (reply.err) {
      const Kind = ERRORS[reply.err.code] ?? DesktopError;
      throw new Kind(reply.err.message, reply.err.code);
    }
    return reply.ok;
  }

  // ---- 인자 ----

  /** 선택지 객체. 모르는 이름은 받을 수 있는 이름과 함께 알린다 (Playwright의 다른 선택지를 쓴 AI가 고치게) */
  function options(given, allowed, what) {
    if (given === undefined || given === null) return {};
    if (typeof given !== 'object' || Array.isArray(given)) {
      throw new TypeError(`${what} takes an options object, not ${inspect(given, true)}`);
    }
    for (const key of Object.keys(given)) {
      if (!allowed.includes(key)) {
        const known = allowed.length ? allowed.join(', ') : 'no options';
        throw new TypeError(`${what} has no option ${JSON.stringify(key)}; it takes ${known}`);
      }
    }
    return given;
  }

  function millis(value, what) {
    if (value === undefined) return undefined;
    const number = Number(value);
    if (!Number.isFinite(number) || number < 0) {
      throw new TypeError(`${what} needs milliseconds (0 or more), not ${inspect(value, true)}`);
    }
    return Math.round(number);
  }

  function text(value, what) {
    if (typeof value === 'number') return String(value);
    if (typeof value !== 'string') {
      throw new TypeError(`${what} needs a string, not ${inspect(value, true)}`);
    }
    return value;
  }

  // Playwright·DOM의 키 이름 → xkb 이름 (컴포지터의 parse_chord가 받는 것)
  const KEYS = {
    arrowup: 'Up',
    arrowdown: 'Down',
    arrowleft: 'Left',
    arrowright: 'Right',
    backspace: 'BackSpace',
    enter: 'Return',
    escape: 'Escape',
    esc: 'Escape',
    space: 'space',
    pageup: 'Prior',
    pagedown: 'Next',
    delete: 'Delete',
    insert: 'Insert',
    home: 'Home',
    end: 'End',
    tab: 'Tab',
    backquote: 'grave',
    minus: 'minus',
    equal: 'equal',
    bracketleft: 'bracketleft',
    bracketright: 'bracketright',
    backslash: 'backslash',
    semicolon: 'semicolon',
    quote: 'apostrophe',
    comma: 'comma',
    period: 'period',
    slash: 'slash',
    numpadenter: 'KP_Enter',
  };
  const SYMBOLS = {
    ' ': 'space', '+': 'plus', '-': 'minus', '=': 'equal', '*': 'asterisk', '/': 'slash',
    '.': 'period', ',': 'comma', ';': 'semicolon', ':': 'colon', "'": 'apostrophe',
    '"': 'quotedbl', '`': 'grave', '~': 'asciitilde', '!': 'exclam', '@': 'at',
    '#': 'numbersign', '$': 'dollar', '%': 'percent', '^': 'asciicircum', '&': 'ampersand',
    '(': 'parenleft', ')': 'parenright', '[': 'bracketleft', ']': 'bracketright',
    '{': 'braceleft', '}': 'braceright', '\\': 'backslash', '|': 'bar', '<': 'less',
    '>': 'greater', '?': 'question', _: 'underscore',
  };
  const MODIFIERS = {
    control: 'ctrl',
    ctrl: 'ctrl',
    controlormeta: 'ctrl',
    meta: 'super',
    command: 'super',
    cmd: 'super',
    super: 'super',
    alt: 'alt',
    option: 'alt',
    shift: 'shift',
  };

  /** "Control+Shift+ArrowLeft" → "ctrl+shift+Left". 끝의 "+"는 + 키다 ("Control++") */
  function chord(combo) {
    let parts;
    if (combo === '+') parts = ['+'];
    else if (combo.length > 2 && combo.endsWith('++')) parts = [...combo.slice(0, -2).split('+'), '+'];
    else parts = combo.split('+');
    const key = parts.pop();
    const modifiers = parts.map((part) => MODIFIERS[part.toLowerCase()] ?? part);
    let name = KEYS[key.toLowerCase()] ?? SYMBOLS[key] ?? key;
    name = name.replace(/^Key([A-Z])$/, (_, letter) => letter.toLowerCase());
    name = name.replace(/^Digit([0-9])$/, '$1');
    // Playwright의 "Control+A"는 A 키다 (Shift 없이)
    if (modifiers.length && /^[A-Z]$/.test(name)) name = name.toLowerCase();
    return [...modifiers, name].join('+');
  }

  /** 띄어 쓴 키들이나 키의 배열 → 데몬의 press가 받는 글 */
  function keys(value) {
    let list;
    if (Array.isArray(value)) list = value.map(String);
    else if (value === ' ') list = [' '];
    else list = text(value, 'press').split(/\s+/);
    const chords = list.filter((key) => key !== '').map(chord);
    if (!chords.length) {
      throw new TypeError('press needs keys, such as "Enter", "Control+a" or "Tab Tab"');
    }
    return chords.join(' ');
  }

  /** 휠 방향과 눈금 수 → [가로, 세로] (오른쪽·아래가 +) */
  function wheel(direction, notches) {
    const unit = { up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] }[direction];
    if (!unit) {
      throw new TypeError(`scroll needs up, down, left or right, not ${inspect(direction, true)}`);
    }
    const count = Number(notches ?? NOTCHES);
    if (!Number.isFinite(count) || count <= 0) {
      throw new TypeError('scroll needs a positive number of notches');
    }
    return [unit[0] * count, unit[1] * count];
  }

  /** locator()의 인자: 'e12', 'ref=e12', 'aria-ref=e12'(Playwright MCP의 꼴) */
  function refSelector(selector) {
    const match = /^(?:(?:aria-)?ref=)?(e\d+)$/.exec(String(selector).trim());
    if (!match) {
      throw new TypeError(
        `locator() takes a ref from a snapshot, like 'e12', not ${inspect(selector, true)}; ` +
          'use getByRole(role, { name }) or getByText(text) to find elements by what they show',
      );
    }
    return { ref: match[1] };
  }

  function roleSelector(role, opts) {
    const o = options(opts, ['name', 'exact'], 'getByRole');
    return {
      role: text(role, 'getByRole'),
      name: o.name === undefined ? undefined : text(o.name, 'getByRole name'),
      exact: !!o.exact,
    };
  }

  function textSelector(value, opts) {
    const o = options(opts, ['exact'], 'getByText');
    return { text: text(value, 'getByText'), exact: !!o.exact };
  }

  const appOf = (query) => (query === null ? undefined : query);

  // ---- 결과 ----

  /** 글로 보이는 결과: toString()은 `bunt agent`의 출력과 같다 */
  class Shown {
    toString() {
      return this[INSPECT]();
    }
  }

  class Snapshot extends Shown {
    constructor(reply, diff) {
      super();
      this.tree = reply.tree;
      this.diff = reply.diff;
      this.first = reply.first;
      Object.defineProperty(this, 'showDiff', { value: diff });
    }

    [INSPECT]() {
      if (!this.showDiff) return this.tree.replace(/\n$/, '');
      if (this.first) {
        return `# note: no earlier snapshot of this scope; full tree follows\n${this.tree}`.replace(/\n$/, '');
      }
      if (this.diff === '') return '# no changes';
      return this.diff.replace(/\n$/, '');
    }
  }

  class Action extends Shown {
    constructor(reply, ref) {
      super();
      this.done = reply.done;
      this.diff = reply.diff ?? '';
      this.settled = reply.settled ?? true;
      this.settledMs = reply.settled_ms ?? 0;
      this.observed = reply.observed ?? true;
      this.newWindows = reply.new_windows ?? [];
      if (ref) this.ref = ref;
    }

    [INSPECT]() {
      const subject = this.ref ? `${this.ref}: ` : '';
      if (!this.observed) return `# ${subject}${this.done}`;
      const ms = this.settledMs;
      let waited;
      if (this.settled) waited = `settled in ${ms} ms`;
      else if (this.diff === '') waited = `no change within ${ms} ms; it may still change`;
      else waited = `still changing after ${ms} ms; the diff is the window at that moment`;
      const lines = [`# ${subject}${this.done} (${waited})`];
      for (const title of this.newWindows) lines.push(`# new window: ${JSON.stringify(title)}`);
      lines.push(this.diff === '' ? '# no change in this window' : this.diff.replace(/\n$/, ''));
      return lines.join('\n');
    }
  }

  class Screenshot extends Shown {
    constructor(reply) {
      super();
      this.path = reply.path;
      this.width = reply.width;
      this.height = reply.height;
      this.scale = reply.scale;
      this.window = reply.window;
      this.appId = reply.app_id;
      this.pid = reply.pid;
    }

    [INSPECT]() {
      return (
        `# window ${JSON.stringify(this.window)} (${this.appId}, pid ${this.pid}): ` +
        `${this.width}x${this.height} pixels at scale ${this.scale}; ` +
        `desktop.app(${JSON.stringify(this.appId)}).mouse.click(x, y) takes these pixels\n${this.path}`
      );
    }
  }

  /** 찾은 요소 한 줄 */
  class Element extends Shown {
    constructor(found) {
      super();
      this.ref = found.ref ?? null;
      this.role = found.role;
      this.name = found.name;
      this.value = found.value ?? null;
      this.states = found.states;
      this.text = found.text;
      this.line = found.line;
    }

    [INSPECT]() {
      const inside = this.ref && !this.line.includes(`[ref=${this.ref}]`) ? `\n# (inside ${this.ref})` : '';
      return this.line + inside;
    }
  }

  class Waited extends Shown {
    constructor(reply) {
      super();
      this.done = reply.done;
      this.waitedMs = reply.waited_ms;
      this.count = reply.count;
      this.matches = reply.matches.map((found) => new Element(found));
      this.element = reply.found ? new Element(reply.found) : null;
      this.ref = this.element?.ref ?? null;
      this.window = reply.window ?? null;
    }

    [INSPECT]() {
      const lines = [`# ${this.done} (after ${this.waitedMs} ms)`];
      for (const element of this.matches) lines.push(element[INSPECT]());
      if (this.count > this.matches.length) lines.push(`# and ${this.count - this.matches.length} more`);
      return lines.join('\n');
    }
  }

  function appList(reply) {
    const apps = reply.apps.map((app) => ({
      name: app.name,
      pid: app.pid ?? null,
      bus: app.bus,
      windows: app.windows.map((window) => ({ title: window.title, active: window.active })),
      tree: true,
    }));
    for (const window of reply.untracked ?? []) {
      apps.push({
        name: window.name,
        pid: window.pid,
        appId: window.app_id,
        windows: [{ title: window.title, active: false }],
        tree: false,
        unsupported: !!window.unsupported,
      });
    }
    Object.defineProperty(apps, INSPECT, {
      value() {
        const lines = [];
        const blind = apps.filter((app) => !app.tree);
        for (const app of apps.filter((app) => app.tree)) {
          const pid = app.pid === null ? '' : ` pid=${app.pid}`;
          lines.push(`${app.name}${pid} bus=${app.bus}`);
          for (const window of app.windows) {
            lines.push(`  - window ${JSON.stringify(window.title)}${window.active ? ' [active]' : ''}`);
          }
        }
        if (blind.length) {
          lines.push('# no accessibility tree (screenshot(), mouse.click(), keyboard and scroll still work):');
        }
        for (const app of blind) {
          const why = app.unsupported ? 'the app does not support accessibility' : 'restart the app to read it';
          lines.push(`${app.name} pid=${app.pid} app=${app.appId} (${why})`);
          lines.push(`  - window ${JSON.stringify(app.windows[0].title)}`);
        }
        return lines.length ? lines.join('\n') : '# no apps';
      },
    });
    return apps;
  }

  function installedList(reply) {
    const apps = reply.apps.map((app) => ({ id: app.id, name: app.name }));
    Object.defineProperty(apps, INSPECT, {
      value: () => apps.map((app) => `${app.name}  (${app.id})`).join('\n'),
    });
    return apps;
  }

  // ---- 키보드와 포인터 (ref 없이 창에) ----

  class Keyboard {
    constructor(query) {
      Object.defineProperty(this, 'query', { value: query });
    }

    /** 키를 누른다: "Enter", "Control+a", "Tab Tab", ['Control+a', 'Backspace'] */
    async press(value, opts) {
      const o = options(opts, ['settle'], 'keyboard.press');
      return new Action(
        call('act', {
          action: 'press',
          value: keys(value),
          app: appOf(this.query),
          settle_ms: millis(o.settle, 'settle'),
        }),
      );
    }

    /** 키보드 포커스를 가진 글 칸에 글을 친다. keys: true면 글 칸이 없을 때 글자마다 키를 친다 */
    async type(value, opts) {
      const o = options(opts, ['keys', 'settle'], 'keyboard.type');
      return new Action(
        call('act', {
          action: 'type',
          value: text(value, 'keyboard.type'),
          keys: !!o.keys,
          app: appOf(this.query),
          settle_ms: millis(o.settle, 'settle'),
        }),
      );
    }

    insertText(value, opts) {
      return this.type(value, opts);
    }
  }

  class Mouse {
    constructor(query) {
      Object.defineProperty(this, 'query', { value: query });
    }

    /** 창 안의 한 점을 누른다. x, y는 그 창 screenshot()의 픽셀이고 왼쪽 위가 (0, 0)이다 */
    async click(x, y, opts) {
      const o = options(opts, ['settle'], 'mouse.click');
      const point = [Number(x), Number(y)];
      if (!point.every((value) => Number.isFinite(value) && value >= 0)) {
        throw new TypeError('mouse.click needs x and y in screenshot pixels (0 or more)');
      }
      return new Action(
        call('act', {
          action: 'click',
          at: point,
          app: appOf(this.query),
          settle_ms: millis(o.settle, 'settle'),
        }),
      );
    }

    /** 창 가운데에서 휠을 돌린다. Playwright처럼 픽셀을 받아 눈금(약 100픽셀)으로 바꾼다 */
    async wheel(deltaX, deltaY, opts) {
      const o = options(opts, ['settle'], 'mouse.wheel');
      const notch = (delta) => {
        const number = Number(delta ?? 0);
        if (!Number.isFinite(number)) throw new TypeError('mouse.wheel needs numbers of pixels');
        return number === 0 ? 0 : Math.sign(number) * Math.max(1, Math.round(Math.abs(number) / WHEEL_PIXELS));
      };
      const amount = [notch(deltaX), notch(deltaY)];
      if (amount[0] === 0 && amount[1] === 0) throw new TypeError('mouse.wheel needs a delta that is not 0');
      return new Action(
        call('act', {
          action: 'scroll',
          amount,
          app: appOf(this.query),
          settle_ms: millis(o.settle, 'settle'),
        }),
      );
    }
  }

  // ---- 범위: 데스크톱(활성 창)과 앱 ----

  class Scope extends Shown {
    constructor(query) {
      super();
      Object.defineProperty(this, 'scope', { value: query });
    }

    get keyboard() {
      return new Keyboard(this.scope);
    }

    get mouse() {
      return new Mouse(this.scope);
    }

    /** 압축 접근성 트리. interactive: 누르거나 쓸 수 있는 요소만, hidden: 숨은 요소도, diff: 지난번과 달라진 줄만 보인다 */
    async snapshot(opts) {
      const o = options(opts, ['interactive', 'hidden', 'diff'], 'snapshot');
      const reply = call('snapshot', {
        app: appOf(this.scope),
        interactive: !!o.interactive,
        hidden: !!o.hidden,
      });
      return new Snapshot(reply, !!o.diff);
    }

    locator(selector) {
      return new Locator(this.scope, null, refSelector(selector));
    }

    getByRole(role, opts) {
      return new Locator(this.scope, null, roleSelector(role, opts));
    }

    getByText(value, opts) {
      return new Locator(this.scope, null, textSelector(value, opts));
    }

    async screenshot(opts) {
      options(opts, [], 'screenshot');
      return new Screenshot(call('screenshot', { app: appOf(this.scope) }));
    }

    /** 무엇이 나타나거나(gone이면 사라지거나) 창이 뜰 때까지 기다린다: { text } | { role, name } | { window } */
    async wait(opts) {
      const o = options(opts, ['text', 'role', 'name', 'exact', 'nth', 'window', 'gone', 'timeout'], 'wait');
      return new Waited(
        call('wait', {
          app: appOf(this.scope),
          text: o.text === undefined ? undefined : text(o.text, 'wait text'),
          role: o.role === undefined ? undefined : text(o.role, 'wait role'),
          name: o.name === undefined ? undefined : text(o.name, 'wait name'),
          window: o.window === undefined ? undefined : o.window === true ? '' : text(o.window, 'wait window'),
          exact: !!o.exact,
          nth: o.nth,
          gone: !!o.gone,
          timeout_ms: millis(o.timeout, 'timeout'),
        }),
      );
    }

    /** 창이 뜰 때까지 기다린다. 제목이 없으면 새로 뜬 창이다 */
    async waitForWindow(title, opts) {
      if (title !== null && typeof title === 'object') [title, opts] = [undefined, title];
      const o = options(opts, ['exact', 'gone', 'timeout'], 'waitForWindow');
      return this.wait({ ...o, window: title === undefined ? true : text(title, 'waitForWindow') });
    }

    /** 창 가운데에서 휠을 돌린다 (눈금, 기본 5) */
    async scroll(direction, notches, opts) {
      const o = options(opts, ['settle'], 'scroll');
      return new Action(
        call('act', {
          action: 'scroll',
          amount: wheel(direction, notches),
          app: appOf(this.scope),
          settle_ms: millis(o.settle, 'settle'),
        }),
      );
    }
  }

  class App extends Scope {
    constructor(query, launched) {
      super(query);
      if (launched) {
        this.name = launched.name;
        this.id = launched.id;
        this.window = launched.window ?? null;
        this.running = launched.running;
        this.tree = launched.tree;
      }
    }

    /** --app처럼 앱을 찾는 글 (앱 이름, 데스크톱 ID, 창 제목, pid) */
    get query() {
      return this.scope;
    }

    toJSON() {
      return { query: this.scope, ...this };
    }

    [INSPECT]() {
      if (this.id === undefined) return `App ${JSON.stringify(this.scope)}`;
      if (this.window === null) {
        return `# opened ${this.name} (${this.id}); no new window appeared yet. Run desktop.apps()`;
      }
      const how = this.running ? 'was already open; brought forward' : 'opened';
      const head = `# ${this.name} (${this.id}) ${how}: window ${JSON.stringify(this.window)}`;
      if (this.tree === '') {
        return `${head}\n# this window has no accessibility tree; use screenshot() and mouse.click(x, y) on it`;
      }
      return `${head}\n${this.tree.replace(/\n$/, '')}`;
    }
  }

  class Desktop extends Scope {
    constructor() {
      super(null);
    }

    /** 실행 중인 앱과 창. 접근성 트리가 없는 창은 tree: false다 */
    async apps(opts) {
      options(opts, [], 'apps');
      return appList(call('apps'));
    }

    /** 열 수 있는 앱 */
    async installed(opts) {
      options(opts, [], 'installed');
      return installedList(call('installed'));
    }

    /** 앱 하나 (--app처럼 이름, 데스크톱 ID, 창 제목, pid로 찾는다). 쓸 때마다 다시 찾는다 */
    app(query) {
      if (query !== null && typeof query === 'object' && (query.pid || query.name)) {
        query = query.pid ? String(query.pid) : query.name;
      }
      if (typeof query === 'number') query = String(query);
      if (typeof query !== 'string' || query.trim() === '') {
        throw new TypeError('desktop.app needs an app name, desktop ID, window title or pid');
      }
      return new App(query);
    }

    /** 앱을 열고(이미 열려 있으면 앞으로 가져오고) 그 앱을 돌려준다. 보이면 창의 상호작용 요소 트리가 나온다 */
    async launch(name, opts) {
      options(opts, [], 'launch');
      const reply = call('launch', { app: text(name, 'launch') });
      return new App(reply.id, reply);
    }

    get errors() {
      return { DesktopError, NotAllowedError, BlockedAppError, ScreenLockedError, TimeoutError };
    }

    [INSPECT]() {
      return 'desktop: the active window; desktop.app(name) for an app';
    }
  }

  // ---- 요소 ----

  class Locator extends Shown {
    constructor(scope, parent, selector) {
      super();
      Object.defineProperties(this, {
        scope: { value: scope },
        parent: { value: parent },
        selector: { value: selector },
      });
    }

    /** ref 로케이터의 ref (getByRole·getByText는 동작할 때 찾는다) */
    get ref() {
      return this.selector.ref ?? null;
    }

    [INSPECT]() {
      const s = this.selector;
      let shown;
      if (s.ref) shown = `locator(${JSON.stringify(s.ref)})`;
      else if (s.role) {
        const extra = [];
        if (s.name !== undefined) extra.push(`name: ${JSON.stringify(s.name)}`);
        if (s.exact) extra.push('exact: true');
        shown = `getByRole(${JSON.stringify(s.role)}${extra.length ? `, { ${extra.join(', ')} }` : ''})`;
      } else shown = `getByText(${JSON.stringify(s.text)}${s.exact ? ', { exact: true }' : ''})`;
      if (s.nth !== undefined) shown += `.nth(${s.nth})`;
      return this.parent ? `${this.parent[INSPECT]()}.${shown}` : shown;
    }

    toJSON() {
      return { locator: this[INSPECT]() };
    }

    /** 이 로케이터를 기다리기(wait)의 인자로: 범위와 조건 */
    where(timeout) {
      const s = this.selector;
      if (s.ref) return { ref: s.ref };
      const below = this.parent ? this.parent.resolve(timeout) : undefined;
      return {
        app: below ? undefined : appOf(this.scope),
        ref: below,
        role: s.role,
        name: s.name,
        text: s.text,
        exact: s.exact,
        nth: s.nth,
      };
    }

    /** 맞는 요소를 하나 찾는다 (나타나기를 timeout까지 기다린다). 여럿이면 nth 없이는 고르지 않는다 */
    find(timeout) {
      const reply = call('wait', { ...this.where(timeout), timeout_ms: timeout });
      if (this.selector.nth === undefined && reply.count > 1) {
        const lines = reply.matches.slice(0, STRICT_LINES).map((found) => found.line);
        if (reply.count > lines.length) lines.push(`… and ${reply.count - lines.length} more`);
        throw new DesktopError(
          `${this[INSPECT]()} matches ${reply.count} elements; pick one with .first(), .last(), ` +
            `.nth(i) or a ref:\n${lines.join('\n')}`,
          -32602,
        );
      }
      return reply;
    }

    /** 동작할 ref */
    resolve(timeout) {
      if (this.selector.ref) return this.selector.ref;
      const found = this.find(timeout ?? ACTION_TIMEOUT).found;
      if (!found.ref) {
        throw new DesktopError(
          `${this[INSPECT]()} matched ${found.line}, and nothing that holds it has a ref; ` +
            'take a snapshot and use a ref',
          -32000,
        );
      }
      return found.ref;
    }

    async act(action, extra, o) {
      const ref = this.resolve(millis(o.timeout, 'timeout'));
      const reply = call('act', {
        ref,
        action,
        app: appOf(this.scope),
        settle_ms: millis(o.settle, 'settle'),
        ...extra,
      });
      return new Action(reply, ref);
    }

    /** 누른다 (접근성 동작, 없으면 포인터). pointer: true면 포인터로 */
    async click(opts) {
      const o = options(opts, ['pointer', 'settle', 'timeout'], 'click');
      return this.act('click', { pointer: !!o.pointer }, o);
    }

    /** 글 칸의 글을 바꾼다. 앱이 멈춘 뒤 다시 읽어 견준다 */
    async fill(value, opts) {
      const o = options(opts, ['settle', 'timeout'], 'fill');
      return this.act('fill', { value: text(value, 'fill') }, o);
    }

    /** 막대·숫자 칸의 값을 바꾼다 */
    async set(value, opts) {
      const o = options(opts, ['settle', 'timeout'], 'set');
      const number = Number(value);
      if (!Number.isFinite(number)) throw new TypeError(`set needs a number, not ${inspect(value, true)}`);
      return this.act('set', { value: String(number) }, o);
    }

    setValue(value, opts) {
      return this.set(value, opts);
    }

    async focus(opts) {
      const o = options(opts, ['settle', 'timeout'], 'focus');
      return this.act('focus', {}, o);
    }

    /** 목록에서 고른다 */
    async select(opts) {
      const o = options(opts, ['settle', 'timeout'], 'select');
      return this.act('select', {}, o);
    }

    /** 이 요소에 포커스를 주고 키를 누른다. keepFocus: 창만 앞으로 가져오고 포커스는 그대로 둔다 */
    async press(value, opts) {
      const o = options(opts, ['keepFocus', 'settle', 'timeout'], 'press');
      return this.act('press', { value: keys(value), keep_focus: !!o.keepFocus }, o);
    }

    /** 이 글 칸에 글을 친다(어느 언어든). keys: 글 입력이 없으면 글자마다 키를 친다 */
    async type(value, opts) {
      const o = options(opts, ['keys', 'keepFocus', 'settle', 'timeout'], 'type');
      return this.act('type', { value: text(value, 'type'), keys: !!o.keys, keep_focus: !!o.keepFocus }, o);
    }

    pressSequentially(value, opts) {
      return this.type(value, opts);
    }

    /** 화면에 들어오게 스크롤한다 */
    async scrollIntoView(opts) {
      const o = options(opts, ['settle', 'timeout'], 'scrollIntoView');
      return this.act('scroll', {}, o);
    }

    scrollIntoViewIfNeeded(opts) {
      return this.scrollIntoView(opts);
    }

    /** 이 요소(목록, 문서) 위에서 휠을 돌린다 (눈금, 기본 5) */
    async scroll(direction, notches, opts) {
      const o = options(opts, ['settle', 'timeout'], 'scroll');
      return this.act('scroll', { amount: wheel(direction, notches) }, o);
    }

    async check(opts) {
      return this.toggle(true, options(opts, ['settle', 'timeout'], 'check'));
    }

    async uncheck(opts) {
      return this.toggle(false, options(opts, ['settle', 'timeout'], 'uncheck'));
    }

    async toggle(wanted, o) {
      const element = await this.info({ timeout: o.timeout ?? (this.selector.ref ? 0 : ACTION_TIMEOUT) });
      if (element.states.includes('checked') === wanted) {
        return new Action({ done: `it is already ${wanted ? 'checked' : 'unchecked'}`, observed: false }, element.ref);
      }
      return new Locator(this.scope, null, { ref: element.ref }).click({ settle: o.settle });
    }

    /** 이 요소 아래만 읽은 트리 */
    async snapshot(opts) {
      const o = options(opts, ['interactive', 'hidden', 'diff', 'timeout'], 'snapshot');
      const ref = this.resolve(millis(o.timeout, 'timeout'));
      const reply = call('snapshot', { ref, interactive: !!o.interactive, hidden: !!o.hidden });
      return new Snapshot(reply, !!o.diff);
    }

    /** 요소 하나의 역할, 이름, 값, 상태, 글. 지금 없으면 TimeoutError (timeout까지 기다린다, 기본 0) */
    async info(opts) {
      const o = options(opts, ['timeout'], 'info');
      return new Element(this.find(millis(o.timeout, 'timeout') ?? 0).found);
    }

    /** 글 칸·막대의 값 (없으면 '') */
    async inputValue(opts) {
      return (await this.info(opts)).value ?? '';
    }

    /** 요소와 그 아래의 이름·글 */
    async textContent(opts) {
      return (await this.info(opts)).text;
    }

    async isVisible() {
      return (await this.count()) > 0;
    }

    async isChecked(opts) {
      return (await this.info(opts)).states.includes('checked');
    }

    async isEnabled(opts) {
      return !(await this.info(opts)).states.includes('disabled');
    }

    /** 지금 맞는 요소 수 */
    async count() {
      try {
        return call('wait', { ...this.where(0), nth: undefined, timeout_ms: 0 }).count;
      } catch (error) {
        if (error instanceof TimeoutError) return 0;
        throw error;
      }
    }

    /** 지금 맞는 요소마다 ref 로케이터 (ref가 있는 것만) */
    async all() {
      let reply;
      try {
        reply = call('wait', { ...this.where(0), nth: undefined, timeout_ms: 0 });
      } catch (error) {
        if (error instanceof TimeoutError) return [];
        throw error;
      }
      const refs = [...new Set(reply.matches.map((found) => found.ref).filter(Boolean))];
      return refs.map((ref) => new Locator(this.scope, null, { ref }));
    }

    nth(index) {
      if (!Number.isInteger(index)) throw new TypeError('nth needs a whole number (negative counts from the end)');
      if (this.selector.ref) {
        if (index === 0 || index === -1) return this;
        throw new TypeError('a ref is one element; nth() goes with getByRole and getByText');
      }
      return new Locator(this.scope, this.parent, { ...this.selector, nth: index });
    }

    first() {
      return this.nth(0);
    }

    last() {
      return this.nth(-1);
    }

    locator(selector) {
      return new Locator(this.scope, null, refSelector(selector));
    }

    getByRole(role, opts) {
      return new Locator(this.scope, this, roleSelector(role, opts));
    }

    getByText(value, opts) {
      return new Locator(this.scope, this, textSelector(value, opts));
    }

    /** 이 요소가 나타나거나(gone이면 사라지거나) 그 값이 바뀔 때까지 기다린다: { value } | { changed: true } */
    async wait(opts) {
      const o = options(opts, ['value', 'changed', 'gone', 'timeout'], 'wait');
      const timeout = millis(o.timeout, 'timeout');
      const valued = o.value !== undefined || o.changed;
      const where = valued ? { ref: this.resolve(timeout) } : this.where(timeout);
      return new Waited(
        call('wait', {
          ...where,
          value: o.value === undefined ? undefined : text(o.value, 'wait value'),
          changed: !!o.changed,
          gone: !!o.gone,
          timeout_ms: timeout,
        }),
      );
    }

    /** Playwright의 waitFor: state는 visible(기본), attached, hidden, detached */
    async waitFor(opts) {
      const o = options(opts, ['state', 'timeout'], 'waitFor');
      const state = o.state ?? 'visible';
      if (!['visible', 'attached', 'hidden', 'detached'].includes(state)) {
        throw new TypeError(`waitFor state is visible, attached, hidden or detached, not ${inspect(state, true)}`);
      }
      return this.wait({ gone: state === 'hidden' || state === 'detached', timeout: o.timeout });
    }
  }

  // ---- 값 적기 ----

  function inspect(value, nested = false) {
    if (value === undefined) return 'undefined';
    if (value === null) return 'null';
    switch (typeof value) {
      case 'string':
        return nested ? JSON.stringify(value) : value;
      case 'number':
      case 'boolean':
        return String(value);
      case 'bigint':
        return `${value}n`;
      case 'symbol':
        return value.toString();
      case 'function':
        return `[Function ${value.name || '(anonymous)'}]`;
    }
    if (typeof value[INSPECT] === 'function') return value[INSPECT]();
    if (value instanceof Error) return describe(value);
    if (value instanceof Promise) return 'Promise { <pending> } (await it)';
    try {
      const shown = JSON.stringify(value, replacer, 2);
      if (shown !== undefined) return shown;
    } catch (_) {
      // 순환하는 객체 등
    }
    return Object.prototype.toString.call(value);
  }

  function replacer(_key, value) {
    if (typeof value === 'bigint') return `${value}n`;
    if (value instanceof Map) return Object.fromEntries(value);
    if (value instanceof Set) return [...value];
    if (value instanceof Error) return describe(value);
    return value;
  }

  /** 오류 한 덩이: 이름, 알림, 오류 번호, REPL 코드의 줄 */
  function describe(error) {
    if (!(error instanceof Error)) return `Uncaught ${inspect(error, true)}`;
    let shown = `${error.name}: ${error.message}`;
    const frames = String(error.stack ?? '')
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.includes('<repl>'))
      .slice(0, 5);
    if (frames.length) shown += `\n    ${frames.join('\n    ')}`;
    return shown;
  }

  /** 마지막 식의 값: 보일 글과 JSON */
  function report(value) {
    let json = null;
    try {
      const encoded = JSON.stringify(value, replacer);
      if (encoded !== undefined && encoded.length <= JSON_LIMIT) json = encoded;
    } catch (_) {
      // JSON으로 바꿀 수 없는 값
    }
    return { text: value === undefined ? null : inspect(value), json };
  }

  // ---- 전역 ----

  const timers = new Map();
  let nextTimer = 1;
  globalThis.setTimeout = function setTimeout(callback, delay, ...args) {
    if (typeof callback !== 'function') throw new TypeError('setTimeout needs a function');
    const id = nextTimer++;
    timers.set(id, () => callback(...args));
    native.timer(id, Math.max(0, Number(delay) || 0));
    return id;
  };
  globalThis.clearTimeout = function clearTimeout(id) {
    if (timers.delete(id)) native.untimer(id);
  };
  const print = (...values) => native.print(values.map((value) => inspect(value)).join(' '));
  globalThis.console = Object.freeze({
    log: print,
    info: print,
    warn: print,
    error: print,
    debug: print,
    dir: print,
    table: print,
  });
  // `var desktop = …`처럼 덮어써 REPL을 잃지 않게 바꿀 수 없게 둔다
  Object.defineProperty(globalThis, 'desktop', { value: new Desktop(), enumerable: true });

  return {
    fire(id) {
      const run = timers.get(id);
      timers.delete(id);
      if (run) run();
    },
    clear() {
      timers.clear();
    },
    report,
    describe,
  };
})();

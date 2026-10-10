// The children of `Screen / Core move` (PRD core-host-node-move B2-B5, B10,
// B16, W1-W4, D-06): the design the operator chose in three review rounds,
// redrawn here from the designer's builder so the committed sheet is the
// approved one. Four boards: the core marker and the move's entry on today's
// Devices rows, the move dialog's five states over Settings, the rail with the
// crown on the core machine's tile, and the window states outside Settings,
// plus the disconnect strip (B16), which the approved bundle did not draw and
// which follows W3's shape. Drawn on this document's local tokens plus library
// refs (Button, Icon Button, Badge, Tabs Trigger, Menu Item, Separator), and
// called from pen-screens.mjs, which owns `themedXref` and the sheet frame.
// Every name and number is invented example content; the copy is the
// approved bundle's, word for word.

import {BADGE_VARIANTS, BUTTON_VARIANTS, frame, icon, num, text} from './pen-system.mjs';

export function coreMoveRows(tokens, {themedXref}, suffix) {
  // Ids are unique per theme frame and stable across runs: the generator walks
  // the same code in the same order every time. The frames the review targets
  // pair with the approved bundle carry names of their own (`named`).
  let serial = 0;
  const uid = base => `cm-${base}-${++serial}-${suffix}`;
  const named = (node, name) => ({...node, id: `cm-${name}-${suffix}`});
  const n = name => num(tokens, name);

  const FG = '$--foreground', SUB = '$--subtle-foreground', MUT = '$--muted-foreground';
  const WARN = '$--warning', OK = '$--success', BAD = '$--destructive', BORDER = '$--border', PRIMARY = '$--primary';
  const XXS = n('--spacing-xxs'), XS = n('--spacing-xs'), SM = n('--spacing-sm'), MD = n('--spacing-md'), LG = n('--spacing-lg'), XL = n('--spacing-xl');
  const SHEET_W = n('--size-settings-sheet-w');
  const BODY_W = SHEET_W - 2 * XL;
  const DIALOG_W = n('--size-add-device-sheet-w');
  const RAIL_W = n('--size-rail');
  const DIM = n('--opacity-dimmed');
  const DISABLED = n('--opacity-disabled');

  const f = (base, name, props, children) => frame(uid(base), name, props, children);
  const t = (content, opts = {}) => text(uid('t'), content, opts);
  const glyph = (name, opts) => icon(uid('g'), name, opts);
  const sp = () => f('s', 'Spacer', {width: 'fill_container', height: 1}, []);
  const body = (content, fill = SUB, opts = {}) => t(content, {size: '$--text-body', fill, ...opts});
  const cap11 = (content, fill = MUT, opts = {}) => t(content, {size: '$--text-caption', fill, ...opts});
  const mono = (content, fill = MUT, opts = {}) => t(content, {size: '$--text-caption', fill, mono: true, ...opts});
  const caption = (content, width) => t(content, {size: '$--text-caption', fill: MUT, weight: '600', ...(width ? {width} : {})});
  const hr = () => f('hr', 'Divider', {width: 'fill_container', height: 1, fill: BORDER}, []);
  const at = (node, x, y) => ({...node, x: Math.round(x), y: Math.round(y)});

  // -- library refs ------------------------------------------------------------------

  function button(label, {variant = 'secondary', size = 'md', icon: iconName} = {}) {
    const v = BUTTON_VARIANTS[variant];
    const height = size === 'sm' ? n('--size-control-sm') : n('--size-control');
    return themedXref(uid('btn'), 'btn-m', label, {...v.overrides, height}, {
      'btn-ic': iconName ? {icon: iconName, fill: v.fg, enabled: true} : {enabled: false},
      'btn-lb': {content: label, fill: v.fg},
    });
  }
  const iconButton = (iconName, {size} = {}) => themedXref(uid('ib'), 'Nyvom', 'Icon', size ? {width: size, height: size} : {}, {ZIZFR: {icon: iconName, fill: MUT}});
  function badge(label, iconName) {
    const v = BADGE_VARIANTS.outline;
    return themedXref(uid('bdg'), 'eHAjc', `Badge ${label}`, v.overrides, {xXuNa: iconName ? {enabled: true, icon: iconName, fill: v.fg} : {enabled: false}, n8L5dm: {content: label, fill: v.fg}});
  }
  const tabs = (items, active) => items.map((label, i) => themedXref(uid('tab'), 'tab-m', label, i === active ? {fill: '$--secondary'} : {}, {'tab-t': {content: label, fill: i === active ? FG : SUB}}));
  const MENU_STATE = {default: {}, highlighted: {fill: '$--accent', text: '$--accent-foreground'}, disabled: {text: MUT}, destructive: {text: BAD}};
  function menuItem(label, {state = 'default', reason, reasonWidth = 120} = {}) {
    const s = MENU_STATE[state];
    return themedXref(uid('mi'), 'mnu-item-m', label, {width: 'fill_container', ...(s.fill ? {fill: s.fill} : {}), ...(state === 'disabled' ? {opacity: DISABLED} : {})}, {
      'mnu-item-icon': {enabled: false},
      'mnu-item-label': {content: label, fill: s.text ?? FG},
      'mnu-item-reason': reason ? {content: reason, enabled: true, textGrowth: 'fixed-width', width: reasonWidth} : {enabled: false},
      'mnu-item-shortcut': {enabled: false},
    });
  }
  const menuSep = () => themedXref(uid('sep'), 'mnu-sep-m', 'Separator', {width: 'fill_container'});
  const menu = (width, items) => f('menu', 'Menu', {layout: 'vertical', padding: XXS, width, cornerRadius: '$--radius-sm', fill: '$--popover', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner'}, items);
  const tip = content => f('tip', 'Tooltip', {height: 22, padding: [0, SM], fill: '$--popover', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner', cornerRadius: '$--radius-xs', alignItems: 'center'}, [cap11(content, SUB)]);

  // -- settings-rows.tsx -------------------------------------------------------------

  // A state is a symbol plus words, never color alone.
  const TONE = {ok: [OK, '✓'], warn: [WARN, '!'], error: [BAD, '✕'], pending: [SUB, '…'], local: [PRIMARY, '●'], muted: [MUT, '·']};
  function status(tone, words, {size = '$--text-body', weight = '400', width} = {}) {
    const [fill, symbol] = TONE[tone];
    return f('st', `Status ${words}`, {gap: XS, alignItems: 'start', ...(width ? {width} : {})}, [
      t(symbol, {size, fill, mono: true}),
      t(words, {size, fill, weight, ...(width ? {width: width - 12} : {})}),
    ]);
  }
  // A command the operator runs on the other machine: mono, copyable.
  const command = cmd => f('cmd', `Command ${cmd}`, {height: 22, gap: SM, padding: [0, XS, 0, SM], cornerRadius: '$--radius-xs', fill: '$--secondary', alignItems: 'center'}, [
    mono(cmd, FG), iconButton('copy', {size: 18}),
  ]);
  function group(title, rows, {width = BODY_W, action} = {}) {
    const box = f('box', 'Box', {layout: 'vertical', width, cornerRadius: '$--radius-md', fill: '$--card', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner'},
      rows.flatMap((row, i) => (i === 0 ? [row] : [hr(), row])));
    return f('grp', title, {layout: 'vertical', gap: SM, width}, [
      f('gh', 'Group title', {width, alignItems: 'center'}, [t(title, {size: '$--text-body', weight: '600', fill: SUB}), sp(), ...(action ? [action] : [])]),
      box,
    ]);
  }

  // -- Devices (DeviceRow.tsx, DevicesTab.tsx) ---------------------------------------

  const CORE_ICON = 'crown';
  const DEVICES = {
    mac: {name: '이 Mac', sub: '로컬, SSH 별칭 없음', local: true},
    macAfter: {name: '이 Mac', sub: 'MacBook Pro · macos-arm64 · Herdr 0.9.3'},
    mini: {name: 'Mac mini', sub: 'mini · macos-arm64 · Herdr 0.9.3'},
    box: {name: 'build-box', sub: 'build-box · linux-x86_64 · Herdr 0.9.1', off: true},
  };
  // One device row: name (with the core badge on the core machine), subtitle, state, Test and ⋯.
  function deviceRow(key, {core = false, state, selected = false, test = !DEVICES[key].local, menuOpen = false} = {}) {
    const d = DEVICES[key];
    const [tone, words] = state ?? (d.local ? ['local', '데몬이 실행되는 기기'] : d.off ? ['warn', '연결 안 됨'] : ['ok', '연결됨']);
    const label = f('lbl', 'Label', {layout: 'vertical', width: 'fill_container', gap: XXS}, [
      f('nm', 'Name', {gap: SM, alignItems: 'center'}, [t(d.name, {size: '$--text-subhead', weight: '600'}), ...(core ? [badge('core', CORE_ICON)] : [])]),
      mono(d.sub, MUT),
    ]);
    const right = f('ctl', 'Controls', {gap: SM, alignItems: 'center'}, [
      status(tone, words),
      ...(selected ? [status('muted', '선택됨')] : []),
      ...(d.off ? [button('다시 시도')] : []),
      ...(test ? [button('테스트')] : []),
      f('mb', 'More', {width: 28, height: 28, cornerRadius: '$--radius-sm', justifyContent: 'center', alignItems: 'center', ...(menuOpen ? {fill: '$--accent'} : {})}, [glyph('ellipsis', {size: 14, fill: MUT})]),
    ]);
    return f('row', `Device ${d.name}`, {layout: 'vertical', width: 'fill_container', padding: [SM, MD], gap: XS}, [
      f('top', 'Top', {width: 'fill_container', gap: MD, alignItems: 'center'}, [label, right]),
    ]);
  }
  const TAB_NAMES = ['일반', '에이전트', 'Hide AI', '기기', '모바일', '단축키'];
  // The Settings sheet (SettingsSheet.tsx) on its Devices tab.
  function sheet(content, height) {
    return f('sheet', 'Settings', {layout: 'vertical', width: SHEET_W, height, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner', clip: true}, [
      f('hdr', 'Header', {width: SHEET_W, padding: [LG, XL], gap: MD, alignItems: 'start', stroke: BORDER, strokeWidth: {bottom: 1}, strokeAlignment: 'inner'}, [
        f('ht', 'Text', {layout: 'vertical', width: 'fill_container', gap: XXS}, [t('설정', {size: '$--text-headline', weight: '600'}), body('Hide가 SSH로 연결하는 기기입니다. 로그인은 SSH 설정에서 처리합니다.')]),
        iconButton('x'),
      ]),
      f('nav', 'Tabs', {width: SHEET_W, padding: [SM, XL], gap: XS, fill: '$--sidebar', stroke: BORDER, strokeWidth: {bottom: 1}, strokeAlignment: 'inner'}, tabs(TAB_NAMES, 3)),
      f('bd', 'Body', {layout: 'vertical', width: SHEET_W, padding: [LG, XL], gap: LG}, content),
    ]);
  }
  const devicesTab = rows => [group('기기', rows, {action: button('기기 추가', {size: 'sm', icon: 'plus'})})];

  // -- the board frame ---------------------------------------------------------------

  const board = (title, spec, items) => f('board', title, {layout: 'vertical', gap: SM}, [
    t(title, {size: '$--text-headline', weight: '600'}),
    t(spec, {size: '$--text-caption', fill: MUT, width: 1400}),
    f('items', 'States', {layout: 'horizontal', gap: 40, alignItems: 'start'}, items),
  ]);
  const labeled = (label, node, width) => f('lab', label, {layout: 'vertical', gap: SM, width}, [caption(label, width), node]);
  // A part floating over a sheet (a menu, a tooltip), placed where the app puts it.
  const over = (base, w, h, floats) => f('ov', 'Overlay', {layout: 'none', width: w, height: h}, [at(base, 0, 0), ...floats.map(([node, x, y]) => at(node, x, y))]);
  // A dialog over the dimmed sheet (System / Dialog's surface).
  const SCRIM = '#0000008C';
  function dialog({title, description, content, actions}) {
    const width = DIALOG_W;
    return f('dlg', 'Dialog', {layout: 'vertical', width, gap: MD, cornerRadius: '$--radius-lg', fill: '$--popover', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner'}, [
      f('dh', 'Header', {layout: 'vertical', width, gap: XS, padding: [LG, LG, 0, LG]}, [
        f('dt', 'Title', {width: width - 2 * LG, alignItems: 'center'}, [t(title, {size: '$--text-title', weight: '600'}), sp(), iconButton('x', {size: 20})]),
        ...(description ? [body(description, SUB, {width: width - 2 * LG})] : []),
      ]),
      f('db', 'Body', {layout: 'vertical', width, gap: MD, padding: [0, LG]}, content),
      f('df', 'Footer', {width, gap: SM, justifyContent: 'end', padding: [0, LG, LG, LG]}, actions),
    ]);
  }
  function overDialog(base, h, dlg, dlgH) {
    return f('ovd', 'Dialog over Settings', {layout: 'none', width: SHEET_W, height: h}, [
      at(base, 0, 0),
      at(f('scrim', 'Scrim', {width: SHEET_W, height: h, fill: SCRIM, cornerRadius: '$--radius-lg'}, []), 0, 0),
      at(dlg, (SHEET_W - DIALOG_W) / 2, Math.max(24, (h - dlgH) / 2)),
    ]);
  }

  // -- 0 · the core marker and the move's entry (B2, D-06) ---------------------------

  function markerBoard() {
    const H = 360;
    const before = (opts = {}) => sheet(devicesTab([
      deviceRow('mac', {core: true, selected: true}),
      deviceRow('mini', {menuOpen: opts.miniMenu}),
      deviceRow('box', {menuOpen: opts.boxMenu}),
    ]), H);
    const after = (opts = {}) => sheet(devicesTab([
      deviceRow('macAfter', {state: ['ok', '연결됨'], selected: true, test: false, menuOpen: opts.macMenu}),
      deviceRow('mini', {core: true}),
      deviceRow('box'),
    ]), H);
    const miniMenu = menu(236, [menuItem('선택'), menuItem('연결 정보…'), menuItem('헬퍼 허용 해지…'), menuSep(), menuItem('core를 이 기기로 옮기기…', {state: 'highlighted'}), menuSep(), menuItem('제거…', {state: 'destructive'})]);
    const boxMenu = menu(300, [menuItem('선택'), menuItem('연결 정보…'), menuItem('헬퍼 허용 해지…'), menuSep(), menuItem('core를 이 기기로 옮기기…', {state: 'disabled', reason: '연결 안 됨', reasonWidth: 64}), menuSep(), menuItem('제거…', {state: 'destructive'})]);
    const macMenu = menu(250, [menuItem('연결 정보…'), menuSep(), menuItem('core를 이 Mac으로 되돌리기…', {state: 'highlighted'}), menuItem('Mac mini core와 연결 끊기…')]);
    return board('0 Core marker and the move entry', 'B2, D-06. Today\'s rows plus an outline `core` badge, with the same crown the rail tile wears, beside the core machine\'s name; its hint is `core 실행 중`. The move starts from a connected device\'s ⋯ (`core를 이 기기로 옮기기…`), dimmed with `연결 안 됨` otherwise. The menu item carries no reason line: in this menu that slot means why an item is off, and the dialog says why to move before anything changes. After the move this Mac\'s ⋯ holds the way back and the disconnect (B16), named by the machine (`Mac mini core와 연결 끊기…`) rather than `원격 core`.', [
      labeled('Before: core on this Mac', named(before(), 'devices'), SHEET_W),
      labeled('Before: Mac mini ⋯', named(over(before({miniMenu: true}), SHEET_W + 40, H, [[miniMenu, 420, 250]]), 'devices-menu'), SHEET_W + 40),
      labeled('Before: a device that is not connected', named(over(before({boxMenu: true}), SHEET_W + 40, H + 60, [[boxMenu, 358, 304]]), 'devices-offline'), SHEET_W + 40),
      labeled('After: core on Mac mini, this Mac ⋯', named(over(after({macMenu: true}), SHEET_W + 40, H, [[macMenu, 408, 208], [tip('core 실행 중'), 100, 252]]), 'node-menu'), SHEET_W + 40),
    ]);
  }

  // -- B · the move dialog (B3-B5) ---------------------------------------------------

  const WHY = 'Mac mini는 늘 켜져 있어서 맥북을 덮어도 에이전트와 폰 연결이 계속돼요.';
  const STEPS = ['점검', '이 Mac의 core 멈춤', '프로젝트와 기록 복사', 'Mac mini에서 core 시작', '창 다시 연결'];
  function stages(states) {
    const mark = {done: ['✓', OK], run: ['…', FG], todo: ['–', MUT], fail: ['✕', BAD]};
    return f('stg', 'Steps', {layout: 'vertical', gap: XS}, STEPS.map((step, i) => {
      const [g, c] = mark[states[i]];
      return f('step', step, {gap: SM, alignItems: 'center'}, [mono(g, c), body(step, states[i] === 'todo' ? MUT : FG, {weight: states[i] === 'run' || states[i] === 'fail' ? '600' : '400'})]);
    }));
  }
  function dialogBoard() {
    const W = DIALOG_W - 2 * LG;
    const H = 460;
    const base = (miniCore = false) => sheet(devicesTab([
      deviceRow(miniCore ? 'macAfter' : 'mac', {core: !miniCore, selected: true, test: false, ...(miniCore ? {state: ['ok', '연결됨']} : {})}),
      deviceRow('mini', {core: miniCore}),
      deviceRow('box'),
    ]), H);
    const d = (spec, dlgH, miniCore) => overDialog(base(miniCore), H, dialog(spec), dlgH);
    const fix = (name, cmd) => f('fix', name, {width: W, gap: SM, alignItems: 'center'}, [status('error', name), sp(), command(cmd)]);
    const line = (words, fill = SUB) => body(words, fill, {width: W});
    return board('B Move dialog', 'B (chosen). One dialog over Settings › Devices; each state is a title and two or three short lines. The reason to move sits under the title before anything changes (B1, B2). Checks show only what fails, by name with its one command, and what passed as a count. Moving is the step names; a failure names the step and that this Mac\'s core is unchanged; done is one line and 되돌리기. Neither confirm button is focused (principle 6).', [
      labeled('B1 Checks failed', named(d({title: 'core를 Mac mini로 옮기기', description: WHY, content: [cap11('고칠 것 2개 · 통과 4'), fix('gh 로그인', 'gh auth login'), fix('잠자기 방지', 'sudo pmset -c sleep 0')], actions: [button('닫기'), button('다시 점검', {variant: 'default'})]}, 250), 'checks'), SHEET_W),
      labeled('B2 Confirm', named(d({title: 'core를 Mac mini로 옮길까요?', description: WHY, content: [line('몇 초 동안 창이 입력을 받지 않고, 폰은 QR을 한 번 다시 스캔해요.'), line('언제든 되돌릴 수 있어요.', MUT)], actions: [button('취소'), button('Mac mini로 옮기기', {variant: 'default'})]}, 220), 'confirm'), SHEET_W),
      labeled('B3 Moving', named(d({title: 'Mac mini로 옮기는 중', content: [stages(['done', 'done', 'run', 'todo', 'todo'])], actions: []}, 220), 'moving'), SHEET_W),
      labeled('B4 Failed', named(d({title: '옮기지 못했어요', content: [status('error', 'Mac mini에서 core를 시작하지 못했어요'), status('ok', '이 Mac의 core는 그대로예요.')], actions: [button('닫기'), button('다시 시도', {variant: 'default'})]}, 180), 'failed'), SHEET_W),
      labeled('B5 Done', named(d({title: 'core를 Mac mini로 옮겼어요', content: [line('폰은 Mac mini의 QR을 한 번 다시 스캔하세요.')], actions: [button('되돌리기'), button('닫기', {variant: 'default'})]}, 150, true), 'done'), SHEET_W),
    ]);
  }

  // -- the window: rail, sidebar, pane (device-rail.tsx, sidebar.tsx) ---------------

  const WIN_W = 900, WIN_H = 460, SIDE_W = 240;
  // A rail tile with today's marks (Needs You top-right, unreachable × bottom-right)
  // and the crown notched into the core machine's tile at bottom-left.
  function railTile(kind, {selected = false, label, dimmed = false, core = false, needs = 0, off = false} = {}) {
    const mark = kind === 'laptop' ? glyph('laptop', {size: 20, fill: selected ? FG : SUB}) : kind === 'plus' ? glyph('plus', {size: 14, fill: MUT}) : t(kind, {size: '$--text-body', weight: '600', fill: selected ? FG : SUB});
    const BADGE = n('--size-rail-badge');
    const notch = (fill, kids) => f('notch', 'Notch', {width: BADGE + 4, height: BADGE + 4, cornerRadius: (BADGE + 4) / 2, fill: '$--sidebar', justifyContent: 'center', alignItems: 'center'}, [f('mark', 'Mark', {width: BADGE, height: BADGE, cornerRadius: BADGE / 2, fill, justifyContent: 'center', alignItems: 'center'}, kids)]);
    return f('tile', label ?? kind, {layout: 'none', width: RAIL_W, height: 40}, [
      ...(selected ? [at(f('ring', 'Selected', {width: 40, height: 40, cornerRadius: 12, stroke: FG, strokeWidth: 2, strokeAlignment: 'inner'}, []), 4, 0)] : []),
      at(f('face', 'Tile', {width: 32, height: 32, cornerRadius: '$--radius-md', justifyContent: 'center', alignItems: 'center', ...(kind === 'plus' ? {stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner'} : {fill: '$--secondary'})}, [dimmed ? {...mark, opacity: DIM} : mark]), 8, 4),
      ...(needs ? [at(notch(WARN, [t(String(needs), {size: n('--size-rail-badge-text'), weight: '600', fill: '$--status-foreground'})]), 29, -2)] : []),
      ...(off ? [at(notch('$--card', [t('×', {size: n('--size-rail-badge-text'), fill: MUT})]), 29, 23)] : []),
      ...(core ? [at(notch('$--card', [glyph(CORE_ICON, {size: n('--size-rail-crown'), fill: FG})]), 2, 23)] : []),
    ]);
  }
  const rail = (tiles, h) => f('rail', 'Device rail', {layout: 'vertical', width: RAIL_W, height: h, gap: XS, padding: [8, 0], fill: '$--sidebar', stroke: BORDER, strokeWidth: {right: 1}, strokeAlignment: 'inner'}, tiles);
  const sideRow = (title, mark = '●', color = '$--agent-working') => f('sr', title, {width: SIDE_W, height: 28, gap: SM, padding: [0, MD], alignItems: 'center'}, [t(mark, {size: '$--text-caption', fill: color}), body(title, FG)]);
  function sidebar(head, h) {
    return f('side', 'Sidebar', {layout: 'vertical', width: SIDE_W, height: h, fill: '$--sidebar', stroke: BORDER, strokeWidth: {right: 1}, strokeAlignment: 'inner'}, [
      f('hl', 'Header line', {width: SIDE_W, height: 36, gap: SM, padding: [0, XS, 0, MD], alignItems: 'center'}, [t(head, {size: '$--text-subhead', weight: '600'}), sp(), iconButton('folder-plus'), iconButton('search')]),
      f('ovw', 'Overview', {width: SIDE_W, height: 28, gap: SM, padding: [0, MD], alignItems: 'center'}, [glyph('layout-dashboard', {size: 16, fill: SUB}), body('Overview', FG), sp(), mono('⌘⇧O')]),
      f('sh', 'Section', {width: SIDE_W, height: 24, padding: [0, MD], alignItems: 'center'}, [cap11('Projects', MUT, {weight: '600'})]),
      sideRow('herdr-ide', '▾', MUT), sideRow('core 옮기기 화면 디자인'), sideRow('5단계 구현과 검증', '○', MUT), sideRow('oh-my-principle', '▸', MUT),
    ]);
  }
  const TERM = ['$ hide agent list', 'agent-2643  w9J:p5J  lead-core-terminal-prd   working', 'agent-3311  wJD:p1   observer                  idle', '$ █'];
  const pane = h => f('main', 'Main', {layout: 'vertical', width: 'fill_container', height: h, fill: '$--background'}, [
    f('ph', 'Pane header', {width: 'fill_container', height: 32, gap: SM, padding: [0, MD], alignItems: 'center', stroke: BORDER, strokeWidth: {bottom: 1}, strokeAlignment: 'inner'}, [t('●', {size: '$--text-caption', fill: '$--agent-working'}), body('core 옮기기 화면 디자인', FG), sp(), mono('main')]),
    f('term', 'Terminal', {layout: 'vertical', width: 'fill_container', padding: MD, gap: XS}, TERM.map(l => mono(l, FG))),
  ]);
  // ConnectionBadge's strip (badge.tsx): full width, card fill, caption text; the
  // mark carries the tone, and an action, where the strip has one, ends the line
  // after a separator dot.
  const strip = (words, tone, action) => f('strip', 'Connection strip', {width: WIN_W, padding: [XS, MD], gap: XS, fill: '$--card', stroke: BORDER, strokeWidth: {bottom: 1}, strokeAlignment: 'inner', alignItems: 'center'}, [
    t(TONE[tone][1], {size: '$--text-caption', fill: TONE[tone][0], mono: true}), cap11(words, SUB),
    ...(action ? [cap11('·', SUB), cap11(action, PRIMARY, {weight: '500'})] : []),
  ]);
  const plainTiles = () => [railTile('laptop', {selected: true, label: '이 Mac'}), railTile('Mm', {label: 'Mac mini'}), railTile('plus')];
  // W2, W3 and the disconnect run with the core on the mini, so its tile wears the crown.
  const crowned = () => [railTile('laptop', {selected: true, label: '이 Mac'}), railTile('Mm', {label: 'Mac mini', core: true}), railTile('plus')];
  function windowFrame(words, tone, tiles, action) {
    const h = WIN_H - 24;
    return f('win', 'Window', {layout: 'vertical', width: WIN_W, height: WIN_H, cornerRadius: '$--radius-md', fill: '$--background', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner', clip: true}, [
      strip(words, tone, action),
      f('cols', 'Columns', {width: WIN_W, height: h}, [rail(tiles, h), sidebar('이 Mac', h), pane(h)]),
    ]);
  }
  // The desktop host's status page (status.html): centered title, reason, Retry.
  function statusPage(title, reason, builds) {
    return f('sp', 'Host status page', {layout: 'vertical', width: WIN_W, height: WIN_H, cornerRadius: '$--radius-md', fill: '$--background', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner', justifyContent: 'center', alignItems: 'center', gap: XS}, [
      t(title, {size: '$--text-title', weight: '600'}),
      t(reason, {size: '$--text-body', fill: MUT, width: 460, align: 'center'}),
      t(builds, {size: '$--text-caption', fill: MUT, mono: true}),
      f('gap', 'Gap', {width: 1, height: LG - XS}, []),
      button('다시 시도'),
    ]);
  }

  // -- N1 · the rail ------------------------------------------------------------------

  function railBoard() {
    const RH = 300;
    const W = RAIL_W + SIDE_W + 200;
    const crop = (tiles, tips) => {
      const base = f('crop', 'Window crop', {width: RAIL_W + SIDE_W, height: RH, cornerRadius: '$--radius-md', stroke: BORDER, strokeWidth: 1, strokeAlignment: 'inner', clip: true, fill: '$--background'}, [rail(tiles, RH), sidebar('이 Mac', RH)]);
      return over(base, W, RH, [[tip(tips[0]), RAIL_W + SIDE_W + 12, 14], [tip(tips[1]), RAIL_W + SIDE_W + 12, 58]]);
    };
    const mac = [railTile('laptop', {selected: true, label: '이 Mac'}), railTile('Mm', {label: 'Mac mini', core: true, needs: 3}), railTile('Bb', {label: 'build-box', dimmed: true, off: true}), railTile('plus')];
    const mini = [railTile('laptop', {selected: true, label: '이 Mac', core: true}), railTile('MP', {label: 'MacBook Pro', needs: 1}), railTile('Bb', {label: 'build-box', dimmed: true, off: true}), railTile('plus')];
    return board('N1 Rail', 'N1 with the crown (chosen). The first tile is always the machine the window is on (`이 Mac`), every other machine by its name. The core machine\'s tile wears a 9 px crown in a 13 px notch at its bottom-left, the size of the unreachable × at bottom-right; Needs You and Done keep the top-right. The hint names the core (`Mac mini · core · 내 차례 3`, in the mini\'s own window `이 Mac · core`). The crown shows only once a machine dials in to a core; a one-Mac rail is unchanged.', [
      labeled('MacBook window (core on the mini)', named(crop(mac, ['이 Mac', 'Mac mini · core · 내 차례 3']), 'rail-node'), W),
      labeled('The mini\'s window', named(crop(mini, ['이 Mac · core', 'MacBook Pro · 내 차례 1']), 'rail-core'), W),
    ]);
  }

  // -- 4 · the window states outside Settings (W1-W4, B16) ----------------------------

  function windowBoard() {
    return board('4 Window states outside Settings', 'B4, B10, B11, B16, D-08, D-10, D-28. Every window-level state sits where ConnectionBadge sits today: one strip at the top of the window, card fill, caption text, the same size as `다시 연결하는 중` (layer 4 D-08); the window keeps its last picture and takes no input while the strip shows. Moving and updating name the machine and the step; cannot-connect names the core machine. The older-build case is the desktop host\'s status page (status.html, where `other_build` lives today), because the app has to be replaced before any shell can load; it keeps the core running and says which builds differ. W5 is not in the approved bundle: the strip the window shows after the operator ended the link from this machine (B16), in W3\'s shape with `다시 연결` at its end, the one thing the window sends while it waits.', [
      labeled('W1 Moving: the window is detached', named(windowFrame('core를 Mac mini로 옮기는 중 · 3/5 프로젝트와 기록 복사 · 다시 연결될 때까지 입력을 받지 않아요', 'pending', plainTiles()), 'window-moving'), WIN_W),
      labeled('W2 Updating the core to this app\'s build (B10)', named(windowFrame('Mac mini의 core를 이 앱의 빌드로 바꾸는 중 · 에이전트는 계속 돌아요', 'pending', crowned()), 'window-updating'), WIN_W),
      labeled('W3 Cannot connect (reference, D-08 D-28)', named(windowFrame('Mac mini의 core에 연결할 수 없어요 · 다시 연결하는 중', 'warn', crowned()), 'window-unreachable'), WIN_W),
      labeled('W4 This app is older than the core (D-10, status page)', named(statusPage('앱을 업데이트해야 해요', 'Mac mini의 core가 이 앱보다 새 빌드라 붙지 않았어요. core는 그대로 돌고 있어요. hide.app을 새 빌드로 바꾼 뒤 다시 여세요.', 'core 0.4.2 (3f1c2a9) · 이 앱 0.4.1 (b520d6e)'), 'status'), WIN_W),
      labeled('W5 Disconnected here (B16; drawn by analogy with W3, not in the approved bundle)', named(windowFrame('Mac mini core와 연결을 끊었어요', 'warn', crowned(), '다시 연결'), 'window-disconnected'), WIN_W),
    ]);
  }

  return [f('boards', 'Boards', {layout: 'vertical', gap: 64}, [markerBoard(), dialogBoard(), railBoard(), windowBoard()])];
}

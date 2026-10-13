// Project settings extends the existing settings rows and System controls.
// Screen values are invented examples; the production gallery owns live rendering.
import {frame, icon, num, text} from './pen-system.mjs';

const NAV = [['connections', 'link', '연결'], ['work', 'git-branch', '작업 방식'], ['memory', 'brain', '기억']];

export function projectSettingsScreens(tokens, parts, {screenSheet, screenIconButton, screenInput}) {
  const {group, row, select, status, button, sw, paragraph} = parts;
  const W = num(tokens, '--size-settings-sheet-w');
  const H = num(tokens, '--size-project-settings-sheet-h');
  const NAV_W = num(tokens, '--size-settings-nav-w');
  const PAD = num(tokens, '--spacing-xl');
  const inset = num(tokens, '--size-settings-sheet-window-inset');
  const at = (node, x, y) => ({...node, x, y});
  function modal(p, page, {width = W, height = H, state = 'rest', long = false} = {}) {
    const inner = width - NAV_W - 2 * PAD;
    const note = (id, content) => paragraph(id, content, {width: inner - 24, fill: '$--muted-foreground'});
    const result = state === 'pending' ? status(`${p}-receipt`, 'pending', '저장 중…') : state === 'failed' ? status(`${p}-receipt`, 'error', '저장하지 못했어요. 마지막 저장값을 계속 사용해요.') : null;
    const rows = page === 'connections' ? [
      group(`${p}-repository`, inner, {title: 'GitHub 저장소', rows: [
        row(`${p}-repo-row`, inner, {label: 'GitHub 저장소', controls: [select(`${p}-repo-choice`, state === 'override' ? '저장소 선택' : '자동 감지', 200)], controlsWidth: 200,
          detail: note(`${p}-repo-value`, long ? 'example/아주-긴-프로젝트-이름-with-mixed-script' : 'example/hide')}),
        ...(state === 'override' ? [row(`${p}-repo-draft`, inner, {label: '저장소 선택', controls: [screenInput(`${p}-repo-input`, {content: 'example/another', width: 200}), button(`${p}-repo-apply`, '적용', {variant: 'outline'})], controlsWidth: 256, detail: result})] : []),
      ], note: 'Hide의 연결만 바꿔요. Git remote와 GitHub 권한은 그대로예요.'}),
      group(`${p}-issues`, inner, {title: '이슈 출처', rows: [row(`${p}-issue-row`, inner, {label: '이슈 출처', controls: [select(`${p}-source-choice`, '자동 · GitHub', 200)], controlsWidth: 200, detail: result})]}),
      group(`${p}-linear`, inner, {title: 'Linear', rows: [row(`${p}-linear-row`, inner, {label: 'Linear', controls: [{...button(`${p}-linear-button`, '준비중', {variant: 'outline'}), opacity: num(tokens, '--opacity-disabled')}], controlsWidth: 70, detail: '준비중'})]}),
    ] : page === 'work' ? [
      group(`${p}-work`, inner, {title: '작업 방식', rows: [
        row(`${p}-location`, inner, {label: '새 작업을 시작할 위치', controls: [select(`${p}-location-choice`, '새 워크트리', 200)], controlsWidth: 200, detail: '새 작업의 처음 선택이에요. 시작 전에 다른 체크아웃을 고를 수 있어요.'}),
        row(`${p}-base`, inner, {label: '기준 브랜치', controls: [select(`${p}-base-choice`, '자동 감지 · main', 200)], controlsWidth: 200, detail: 'Git 비교와 새 워크트리의 처음 기준으로 사용해요.'}),
      ]}),
      group(`${p}-protection`, inner, {title: '원본 체크아웃 브랜치 보호', rows: [
        row(`${p}-protect-branch`, inner, {label: '원본 체크아웃 브랜치 보호', controls: [status(`${p}-protect-state`, 'muted', '지원 안 함')], controlsWidth: 85, detail: 'Hide는 브랜치 보호를 적용하지 않아요.'}),
        row(`${p}-protect-files`, inner, {label: 'main 파일 쓰기 차단', controls: [status(`${p}-files-state`, 'muted', '지원 안 함')], controlsWidth: 85, detail: 'Hide는 main의 파일 쓰기를 차단하지 않아요.'}),
      ]}),
    ] : [group(`${p}-memory`, inner, {title: '기억', rows: [
      row(`${p}-memory-toggle`, inner, {label: '기억', controls: [sw(`${p}-memory-state`, state !== 'memory-off')], controlsWidth: 44, detail: '꺼도 프로젝트의 기존 카드는 남아 있어요.'}),
      row(`${p}-memory-open`, inner, {label: '카드 0개', controls: [button(`${p}-memory-button`, '프로젝트 기억 열기', {variant: 'outline'})], controlsWidth: 150}),
    ]})];
    return frame(`${p}-modal`, 'Project settings', {width, height, layout: 'horizontal', clip: true, fill: '$--popover', cornerRadius: '$--radius-lg', stroke: '$--border', strokeWidth: '$--size-hairline', strokeAlignment: 'inner'}, [
      frame(`${p}-nav`, 'Project navigation', {width: NAV_W, height: 'fill_container', layout: 'vertical', padding: ['$--spacing-sm', '$--spacing-sm', '$--spacing-md', '$--spacing-sm'], fill: '$--sidebar', stroke: '$--border', strokeWidth: {right: 1}, strokeAlignment: 'inner'}, [
        frame(`${p}-nav-title`, 'Title', {padding: [0, '$--spacing-sm', '$--spacing-xs', '$--spacing-sm']}, [text(`${p}-nav-title-text`, '프로젝트 설정', {size: '$--text-title', weight: '600'})]),
        frame(`${p}-identity`, 'Project identity', {width: 'fill_container', layout: 'vertical', padding: '$--spacing-sm', gap: '$--spacing-xs'}, [
          frame(`${p}-name-row`, 'Project', {width: 'fill_container', layout: 'horizontal', gap: '$--spacing-xs'}, [icon(`${p}-mark`, 'folder-git-2', {size: 14}), paragraph(`${p}-name`, long ? '아주 긴 프로젝트 이름 Mixed 프로젝트' : 'hide', {width: NAV_W - 50})]),
          paragraph(`${p}-path`, long ? '/work/projects/아주-긴-프로젝트-경로/mixed-script' : '/work/hide', {width: NAV_W - 32, size: 'caption', mono: true, fill: '$--muted-foreground'}),
        ]),
        ...NAV.map(([id, glyph, label]) => frame(`${p}-nav-${id}`, label, {width: 'fill_container', height: 28, layout: 'horizontal', gap: '$--spacing-sm', padding: [0, '$--spacing-sm'], alignItems: 'center', cornerRadius: '$--radius-xs', ...(id === page ? {fill: '$--accent'} : {})}, [icon(`${p}-nav-${id}-icon`, glyph, {size: 14}), text(`${p}-nav-${id}-text`, label, {size: '$--text-body', weight: id === page ? '500' : '400'})])),
        frame(`${p}-nav-space`, 'Flexible space', {width: 1, height: 'fill_container'}, []),
        paragraph(`${p}-scope`, '이 기기에 있는 프로젝트의 모든 워크트리가 함께 사용해요', {width: NAV_W - 32, size: 'caption', fill: '$--muted-foreground'}),
      ]),
      frame(`${p}-page`, 'Page scrolls', {width: width - NAV_W, height: 'fill_container', layout: 'vertical', padding: ['$--spacing-lg', PAD, PAD, PAD], gap: '$--spacing-lg', clip: true}, [
        frame(`${p}-header`, 'Title and close', {width: 'fill_container', layout: 'horizontal', alignItems: 'center'}, [text(`${p}-heading`, NAV.find(([id]) => id === page)[2], {size: '$--text-headline', weight: '600'}), frame(`${p}-header-space`, 'Flexible space', {width: 'fill_container', height: 1}, []), screenIconButton(`${p}-close`, 'x', {size: 24})]),
        ...(state === 'unavailable' ? [note(`${p}-unavailable`, '기기 또는 프로젝트에 연결할 수 없어 읽기만 가능해요. 다시 시도')] : []),
        ...rows,
      ]),
    ]);
  }
  const build = theme => ['connections', 'work', 'memory'].flatMap(page => {
    const states = page === 'connections' ? ['rest', 'override', 'pending', 'failed', 'unavailable'] : page === 'memory' ? ['rest', 'memory-off'] : ['rest'];
    const frameId = state => state === 'rest' && page === 'connections' ? `ps-settings-${theme === 'd' ? 'dark' : 'light'}` : state === 'rest' && theme === 'd' ? `ps-settings-${page}` : `ps-${page}-${state}-${theme}`;
    return states.map(state => frame(frameId(state), `${page} · ${state}`, {width: 1000, height: 760, layout: 'none', fill: '$--background'}, [at(modal(`ps-${page}-${state}-in-${theme}`, page, {state}), (1000 - W) / 2, (760 - H) / 2)]));
  }).concat([frame(`ps-small-${theme}`, 'Small window', {width: 720, height: 480, layout: 'none', fill: '$--background'}, [at(modal(`ps-small-in-${theme}`, 'connections', {width: 720 - 2 * inset, height: 480 - 2 * inset, long: true}), inset, inset)])]);
  return screenSheet('screen-project-settings', 'Screen / Project Settings', 'Connections, Working style and Memory share one scoped modal. Pending and failure remain beside the changed row; unsupported protections and future Linear are shown honestly.', build, build);
}

/**
 * perms.js — 权限声明渲染（单一口源）
 *
 * 安装引导页（install-wizard.js，确认页逐条展示）与配置页权限分区
 * （skin-editor.js，装完后随时可查）共用同一份权限标记与分级口径——
 * 历史上只有引导页一份，权限一览需求落地时收编为模块，勿再各自抄录。
 *
 * 已知权限给名称 + 一句说明，并按风险三档分级：shell / system /
 * file_system 高危（红），registry / clipboard / mic / control 中危（黄），
 * media / notify / sys_info / network / open_link 低危（蓝），均用警告
 * 图标与分级徽标标出；未知权限原样显示 id（后端会忽略未知名，但展示
 * 出来让用户知情）；旧版皮肤可能仍声明 "files"——皮肤目录内文件读写
 * 已免声明，静默略过。
 * 展示顺序 = 风险降序（高危红最前、低危蓝靠后、未知名最后），同档保持
 * skin.json 声明顺序，两个渲染入口一致。
 */
import { t } from './i18n.js';
import { esc } from './dom.js';

const KNOWN = {
  registry: { labelKey: 'wizard.permRegistry', descKey: 'wizard.permRegistryDesc', risk: 'medium' },
  shell: { labelKey: 'wizard.permShell', descKey: 'wizard.permShellDesc', risk: 'high' },
  system: { labelKey: 'wizard.permSystem', descKey: 'wizard.permSystemDesc', risk: 'high' },
  clipboard: { labelKey: 'wizard.permClipboard', descKey: 'wizard.permClipboardDesc', risk: 'medium' },
  mic: { labelKey: 'wizard.permMic', descKey: 'wizard.permMicDesc', risk: 'medium' },
  file_system: { labelKey: 'wizard.permFileSystem', descKey: 'wizard.permFileSystemDesc', risk: 'high' },
  control: { labelKey: 'wizard.permControl', descKey: 'wizard.permControlDesc', risk: 'medium' },
  media: { labelKey: 'wizard.permMedia', descKey: 'wizard.permMediaDesc', risk: 'low' },
  notify: { labelKey: 'wizard.permNotify', descKey: 'wizard.permNotifyDesc', risk: 'low' },
  sys_info: { labelKey: 'wizard.permSysInfo', descKey: 'wizard.permSysInfoDesc', risk: 'low' },
  network: { labelKey: 'wizard.permNetwork', descKey: 'wizard.permNetworkDesc', risk: 'low' },
  open_link: { labelKey: 'wizard.permOpenLink', descKey: 'wizard.permOpenLinkDesc', risk: 'low' },
};

const shieldIcon = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>';
const warnIcon = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';
const infoIcon = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>';

// 风险排序：高危（红）最前、中危（黄）次之、低危（蓝）靠后、未知名最后；
// 同档保持 skin.json 声明顺序（sort 稳定）。引导页与页眉胶囊共用同一顺序
const RISK_ORDER = { high: 0, medium: 1, low: 2 };

function riskRank(p) {
  const known = Object.hasOwn(KNOWN, p) ? KNOWN[p] : null;
  return RISK_ORDER[known?.risk] ?? 3;
}

function sortByRisk(list) {
  return [...list].sort((a, b) => riskRank(a) - riskRank(b));
}

/**
 * 渲染权限声明列表 HTML（wizard-perm* 系列类名，样式在 style.css）。
 * @param permissions skin.json 的 permissions 数组
 * @param withTitle 引导页传 true（居中小标题「权限声明」）；
 *                  配置页分区自带 h3，传 false
 */
export function renderPermsHTML(permissions, { withTitle = false } = {}) {
  const list = sortByRisk((Array.isArray(permissions) ? permissions : []).filter(p => p !== 'files'));
  const title = withTitle ? `<div class="wizard-perms-title">${t('wizard.permissions')}</div>` : '';
  if (list.length === 0) {
    return `<div class="wizard-perms">${title}<div class="wizard-perm-none">${t('wizard.permNone')}</div></div>`;
  }
  const rows = list.map(p => {
    // hasOwn 防 "__proto__"/"constructor" 这类 id 查到原型链上的假条目
    const known = Object.hasOwn(KNOWN, p) ? KNOWN[p] : null;
    const risk = known?.risk; // 'high' | 'medium' | 'low' | undefined
    const cls = risk ? ` ${risk === 'high' ? 'danger' : risk}` : '';
    const badgeKey = risk === 'high' ? 'wizard.permHighRisk'
      : risk === 'medium' ? 'wizard.permMediumRisk'
      : risk === 'low' ? 'wizard.permLowRisk' : null;
    const badge = badgeKey ? t(badgeKey) : null;
    const icon = risk === 'low' ? infoIcon : risk ? warnIcon : shieldIcon;
    return `<div class="wizard-perm${cls}">
      <span class="wizard-perm-icon">${icon}</span>
      <span class="wizard-perm-text">
        <span class="wizard-perm-label">${known ? t(known.labelKey) : esc(p)}${badge ? `<em class="wizard-perm-risk">${badge}</em>` : ''}</span>
        ${known ? `<span class="wizard-perm-desc">${t(known.descKey)}</span>` : ''}
      </span>
    </div>`;
  }).join('');
  return `<div class="wizard-perms">${title}${rows}</div>`;
}

/**
 * 权限名称胶囊行（配置页页眉卡内）：只列名称、颜色分级，无图标无说明——
 * 高危红 / 中危黄 / 低危蓝 / 未知名与「未申请敏感权限」中性灰。
 * 与 renderPermsHTML 同一张 KNOWN 表，分级口径不会漂移。
 * 未声明权限时给中性说明行（实机评审：「未声明」也是用户要知道的状态，
 * 静默反而像漏渲染——曾短暂改为不渲染，评审否决回退）
 */
export function renderPermChipsHTML(permissions) {
  const list = sortByRisk((Array.isArray(permissions) ? permissions : []).filter(p => p !== 'files'));
  if (list.length === 0) {
    return `<div class="perm-chips"><span class="perm-chips-none">${t('wizard.permNone')}</span></div>`;
  }
  const chips = list.map(p => {
    const known = Object.hasOwn(KNOWN, p) ? KNOWN[p] : null;
    const cls = known?.risk === 'high' ? ' high' : known?.risk ? ` ${known.risk}` : '';
    return `<span class="perm-chip${cls}">${known ? t(known.labelKey) : esc(p)}</span>`;
  }).join('');
  return `<div class="perm-chips">${chips}</div>`;
}

/**
 * perms.js — 权限声明渲染（单一口源）
 *
 * 安装引导页（install-wizard.js，确认页逐条展示）与配置页权限分区
 * （skin-editor.js，装完后随时可查）共用同一份权限标记与分级口径——
 * 历史上只有引导页一份，权限一览需求落地时收编为模块，勿再各自抄录。
 *
 * 已知权限给名称 + 一句说明，并按风险分级：shell / system 高危（红）、
 * registry / clipboard / mic 中危（黄），均用警告图标与分级徽标标出；
 * 未知权限原样显示 id（后端会忽略未知名，但展示出来让用户知情）；
 * 旧版皮肤可能仍声明 "files"——皮肤目录内文件读写已免声明，静默略过。
 * 展示顺序 = 风险降序（高危红最前、中危次之、未知名最后），同档保持
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
};

const shieldIcon = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>';
const warnIcon = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>';

// 风险排序：高危（红）最前、中危（黄）次之、未知名最后；同档保持
// skin.json 声明顺序（sort 稳定）。引导页与页眉胶囊共用同一顺序
const RISK_ORDER = { high: 0, medium: 1 };

function riskRank(p) {
  const known = Object.hasOwn(KNOWN, p) ? KNOWN[p] : null;
  return RISK_ORDER[known?.risk] ?? 2;
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
    const risk = known?.risk; // 'high' | 'medium' | undefined
    const cls = risk === 'high' ? ' danger' : risk === 'medium' ? ' medium' : '';
    const badge = risk === 'high' ? t('wizard.permHighRisk')
      : risk === 'medium' ? t('wizard.permMediumRisk') : null;
    return `<div class="wizard-perm${cls}">
      <span class="wizard-perm-icon">${risk ? warnIcon : shieldIcon}</span>
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
 * 高危红 / 中危黄 / 未知名与「未申请敏感权限」中性灰。
 * 与 renderPermsHTML 同一张 KNOWN 表，分级口径不会漂移。
 */
export function renderPermChipsHTML(permissions) {
  const list = sortByRisk((Array.isArray(permissions) ? permissions : []).filter(p => p !== 'files'));
  if (list.length === 0) {
    return `<div class="perm-chips"><span class="perm-chips-none">${t('wizard.permNone')}</span></div>`;
  }
  const chips = list.map(p => {
    const known = Object.hasOwn(KNOWN, p) ? KNOWN[p] : null;
    const cls = known?.risk === 'high' ? ' high' : known?.risk === 'medium' ? ' medium' : '';
    return `<span class="perm-chip${cls}">${known ? t(known.labelKey) : esc(p)}</span>`;
  }).join('');
  return `<div class="perm-chips">${chips}</div>`;
}

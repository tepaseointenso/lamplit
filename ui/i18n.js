// @ts-check
// Lamplit in the player's language. The page is written in English; this module translates what reaches the screen,
// text and the attributes a screen reader reads, through a dictionary per language (lang/*.js), so the rest of the
// page doesn't change. An entry is a whole text, or a template whose {} stand for the parts that vary (a game's name,
// a number), which are translated in turn when the dictionary has them. Settings > System picks the language;
// Automatic follows Windows. Switching translates the page again at once, from the English each text started as.

import { prefsState, systemLocale } from './api.js';
import es from './lang/es.js';

/** @typedef {Record<string, string>} Dictionary */

/** @type {Record<string, Dictionary>} */
const LANGUAGES = { es };


/** The language in use: '' for English. */
let current = '';
/** @type {Map<string, string>} whole texts */
let exact = new Map();
/** @type {{ re: RegExp, to: string, len: number }[]} templates, longest first so the most specific wins */
let templates = [];
/** The English each translated text node or attribute started as. */
const originals = new WeakMap();
/** @type {WeakMap<Element, Map<string, string>>} */
const originalAttrs = new WeakMap();
const ATTRS = ['aria-label', 'alt', 'title', 'placeholder'];

/** `code` ('es', 'es-CL', 'en'…) as a language this page has, or '' for English. @param {string} code */
export function supported(code) {
  const lang = (code || '').toLowerCase().split(/[-_]/)[0];
  return lang in LANGUAGES ? lang : '';
}

/** @param {string} s */
const escape = (s) => s.replace(/[.*+?^$()|[\]\\]/g, '\\$&');

/** @param {string} code */
function load(code) {
  current = supported(code);
  exact = new Map();
  templates = [];
  const dict = LANGUAGES[current];
  if (!dict) return;
  for (const [from, to] of Object.entries(dict)) {
    if (from.includes('{}')) {
      const re = new RegExp(`^${from.split('{}').map((p) => escape(p).replace(/\{/g, '\\{').replace(/\}/g, '\\}')).join('([\\s\\S]+?)')}$`);
      templates.push({ re, to, len: from.length });
    } else exact.set(from, to);
  }
  templates.sort((a, b) => b.len - a.len);
}

/**
 * `text` in the language in use, or as it is when the dictionary hasn't it. Spaces around it are kept.
 * @param {string} text
 */
export function translate(text) {
  if (!current || !text) return text;
  const trimmed = text.trim();
  if (!trimmed || !/[A-Za-z]/.test(trimmed)) return text;
  const done = translateTrimmed(trimmed, 0);
  if (done === trimmed) return text;
  const lead = text.slice(0, text.indexOf(trimmed));
  return lead + done + text.slice(lead.length + trimmed.length);
}

/** @param {string} s @param {number} depth */
function translateTrimmed(s, depth) {
  const hit = exact.get(s);
  if (hit !== undefined) return hit;
  if (depth > 2) return s;
  for (const { re, to } of templates) {
    const m = re.exec(s);
    if (!m) continue;
    const parts = m.slice(1).map((p) => translateTrimmed(p, depth + 1));
    // {1}, {2}… pick a part by its place in the English, for a language that orders them otherwise; {} takes the next.
    let next = 0;
    return to.replace(/\{(\d*)\}/g, (_, n) => parts[n ? Number(n) - 1 : next++] ?? '');
  }
  return s;
}

/** @param {Text} node */
function translateText(node) {
  const value = node.nodeValue ?? '';
  let original = originals.get(node);
  // A text the page set since: that is the English now.
  if (original === undefined || (value !== original && value !== translate(original))) {
    original = value;
    originals.set(node, original);
  }
  const want = translate(original);
  if (value !== want) node.nodeValue = want;
}

/** @param {Element} el */
function translateAttrs(el) {
  for (const name of ATTRS) {
    if (!el.hasAttribute(name)) continue;
    const value = /** @type {string} */ (el.getAttribute(name));
    let saved = originalAttrs.get(el);
    if (!saved) originalAttrs.set(el, (saved = new Map()));
    let original = saved.get(name);
    if (original === undefined || (value !== original && value !== translate(original))) {
      original = value;
      saved.set(name, original);
    }
    const want = translate(original);
    if (value !== want) el.setAttribute(name, want);
  }
}

/** Every text and attribute under `root`. @param {Node} root */
function translateTree(root) {
  if (root.nodeType === Node.TEXT_NODE) return translateText(/** @type {Text} */ (root));
  if (root.nodeType !== Node.ELEMENT_NODE && root.nodeType !== Node.DOCUMENT_FRAGMENT_NODE) return;
  const el = /** @type {Element} */ (root);
  // Code the player doesn't read, and what they type, stay as they are.
  if (el.nodeType === Node.ELEMENT_NODE && el.closest('script, style, svg, .typed, [data-no-translate]')) return;
  if (el.nodeType === Node.ELEMENT_NODE) translateAttrs(el);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: (n) =>
      n.nodeType === Node.ELEMENT_NODE && /** @type {Element} */ (n).matches('script, style, svg, .typed, [data-no-translate]')
        ? NodeFilter.FILTER_REJECT
        : NodeFilter.FILTER_ACCEPT,
  });
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.nodeType === Node.TEXT_NODE) translateText(/** @type {Text} */ (n));
    else translateAttrs(/** @type {Element} */ (n));
  }
}

let observer = /** @type {MutationObserver | null} */ (null);

/**
 * Uses `code` from now on ('' or one this page hasn't: English), translating the page as it is and whatever it shows
 * later.
 * @param {string} code
 */
export function useLanguage(code) {
  load(code);
  document.documentElement.lang = current || 'en';
  translateTree(document.body);
  if (observer) return;
  observer = new MutationObserver((records) => {
    for (const r of records) {
      if (r.type === 'characterData') {
        const node = /** @type {Text} */ (r.target);
        if (!node.parentElement?.closest('script, style, svg, .typed, [data-no-translate]')) translateText(node);
      } else if (r.type === 'attributes') {
        const el = /** @type {Element} */ (r.target);
        if (!el.closest('[data-no-translate]')) translateAttrs(el);
      } else r.addedNodes.forEach(translateTree);
    }
  });
  observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRS });
}

/** The language in use: '' for English. */
export const language = () => current;

/** Windows' locale, read once. @type {Promise<string> | null} */
let locale = null;

/**
 * The page in the language Settings picked, or Windows' while it says Automatic. `pref` is the saved choice; left
 * out, the prefs are read.
 * @param {string} [pref]
 */
export async function applyLanguage(pref) {
  try {
    const choice = pref ?? (await prefsState()).ui_language ?? '';
    locale ??= systemLocale().catch(() => '');
    useLanguage(choice || (await locale));
  } catch {
    useLanguage('');
  }
}

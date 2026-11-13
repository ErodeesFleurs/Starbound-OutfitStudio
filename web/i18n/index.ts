import catalog from './messages.js';

type Locale = 'zh-CN' | 'en';
type Translation = { source: string; rendered: string };
type Pattern = { expression: RegExp; names: string[]; english: string; specificity: number };

const storageKey = 'outfit-studio.language';
const messages = new Map<string, string>();
const patterns: Pattern[] = [];
const placeholders = /\{([a-zA-Z][a-zA-Z0-9_]*|\d+)\}/g;
const nestedValues = new Set(['label', 'error', 'detail']);
const ignored = 'script,style,noscript,pre,code,[data-i18n-ignore],[contenteditable]:not([contenteditable="false"])';
const attributes = ['aria-label', 'title', 'placeholder', 'alt'];
const textSources = new WeakMap<Text, Translation>();
const attributeSources = new WeakMap<Element, Map<string, Translation>>();
let language: Locale = 'zh-CN';
let observer: MutationObserver;

function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

for (const [source, english] of catalog) {
  if (messages.has(source)) continue;
  messages.set(source, english);
  const names: string[] = [];
  let expression = '^';
  let offset = 0;
  let specificity = 0;
  for (const match of source.matchAll(placeholders)) {
    const literal = source.slice(offset, match.index);
    specificity += literal.length;
    expression += escapeRegex(literal);
    const previous = names.indexOf(match[1]);
    if (previous === -1) {
      names.push(match[1]);
      expression += '([\\s\\S]+?)';
    } else {
      expression += `\\${previous + 1}`;
    }
    offset = match.index + match[0].length;
  }
  if (names.length) {
    const tail = source.slice(offset);
    specificity += tail.length;
    expression += escapeRegex(tail) + '$';
    patterns.push({ expression: new RegExp(expression), names, english, specificity });
  }
}
patterns.sort((a, b) => b.specificity - a.specificity);

function translateContent(source: string): string {
  const exact = messages.get(source);
  if (exact !== undefined) return exact;
  for (const pattern of patterns) {
    const match = pattern.expression.exec(source);
    if (!match) continue;
    return pattern.english.replace(placeholders, (placeholder: string, name: string): string => {
      const index = pattern.names.indexOf(name);
      if (index === -1) return placeholder;
      const value = match[index + 1];
      return nestedValues.has(name) && value.length < source.length ? translateContent(value) : value;
    });
  }
  return source;
}

export function translate(source: string, locale: Locale = language): string {
  if (locale !== 'en') return source;
  const content = source.trim();
  if (!content) return source;
  const translated = translateContent(content);
  if (translated === content) return source;
  const start = source.indexOf(content);
  return source.slice(0, start) + translated + source.slice(start + content.length);
}

function translateText(node: Text): void {
  const parent = node.parentElement;
  if (!parent || parent.closest(`${ignored},textarea,#language-select`)) return;
  const previous = textSources.get(node);
  const source = previous && node.data === previous.rendered ? previous.source : node.data;
  const rendered = translate(source);
  if (parent instanceof HTMLOptionElement && !parent.hasAttribute('value')) {
    parent.setAttribute('value', parent.value);
  }
  if (node.data !== rendered) node.data = rendered;
  textSources.set(node, { source, rendered });
}

function translateAttributes(element: Element, only?: string | null): void {
  if (element.closest(ignored)) return;
  let sources = attributeSources.get(element);
  if (!sources) {
    sources = new Map();
    attributeSources.set(element, sources);
  }
  const names = only ? [only] : attributes;
  for (const name of names) {
    const current = element.getAttribute(name);
    if (current === null) continue;
    const previous = sources.get(name);
    const source = previous && current === previous.rendered ? previous.source : current;
    const rendered = translate(source);
    if (rendered !== current) element.setAttribute(name, rendered);
    sources.set(name, { source, rendered });
  }
  if (!only && element.matches('meta[name="description"]')) translateAttributes(element, 'content');
}

function translateTree(root: Node): void {
  if (root instanceof Text) {
    translateText(root);
    return;
  }
  if (!(root instanceof Element) || root.closest(ignored)) return;
  translateAttributes(root);
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      return node instanceof Element && node.matches(ignored)
        ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
    },
  });
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node instanceof Text) translateText(node);
    else if (node instanceof Element) translateAttributes(node);
  }
}

function observe(): void {
  observer.observe(document.documentElement, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: [...attributes, 'content'],
  });
}

function languageSelector(): HTMLSelectElement {
  const selector = document.getElementById('language-select');
  if (!(selector instanceof HTMLSelectElement)) throw new Error('Missing language selector');
  return selector;
}

function applyLanguage(locale: Locale): void {
  language = locale;
  document.documentElement.lang = locale;
  observer.disconnect();
  translateTree(document.documentElement);
  languageSelector().value = locale;
  observe();
}

function supportedLocale(locale: string): Locale | null {
  if (/^zh(?:-|$)/i.test(locale)) return 'zh-CN';
  if (/^en(?:-|$)/i.test(locale)) return 'en';
  return null;
}

function preferredLanguage(): Locale {
  try {
    const saved = localStorage.getItem(storageKey);
    if (saved === 'zh-CN' || saved === 'en') return saved;
  } catch { /* Storage may be unavailable; switching still works for this visit. */ }
  for (const locale of navigator.languages || [navigator.language]) {
    const supported = supportedLocale(locale);
    if (supported) return supported;
  }
  return 'en';
}

export function initI18n(): void {
  if (observer) return;
  observer = new MutationObserver(mutations => {
    observer.disconnect();
    for (const mutation of mutations) {
      if (mutation.type === 'characterData' && mutation.target instanceof Text) {
        translateText(mutation.target);
      } else if (mutation.type === 'attributes' && mutation.target instanceof Element) {
        translateAttributes(mutation.target, mutation.attributeName);
      } else {
        for (const node of mutation.addedNodes) translateTree(node);
      }
    }
    observe();
  });
  const selector = languageSelector();
  selector.addEventListener('change', () => {
    const locale = selector.value;
    if (locale !== 'en' && locale !== 'zh-CN') return;
    applyLanguage(locale);
    try { localStorage.setItem(storageKey, locale); } catch { /* Keep the session choice. */ }
  });
  applyLanguage(preferredLanguage());
}

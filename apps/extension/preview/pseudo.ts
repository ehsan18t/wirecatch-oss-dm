// Pseudo-translation for the preview page (`&pseudo`): every word on screen grows by about 60%,
// as words do in German or Finnish, so a layout can be checked against text longer than English
// before any translation exists. The e2e layout checks run every page this way.

/** How much longer each word gets. */
const GROWTH = 0.6;

/** A word grown by repeating its own start: "Download" becomes "Downloaddownl". */
function grow(word: string): string {
  return word + word.slice(0, Math.ceil(word.length * GROWTH)).toLowerCase();
}

/** The text a node shows once grown; what it showed is kept, so a later update is grown once. */
const grown = new WeakMap<Text, string>();

function lengthen(node: Text): void {
  if (grown.get(node) === node.data || node.data.trim() === '') return;
  const longer = node.data.replaceAll(/[\p{L}\p{N}]+/gu, (word) => grow(word));
  grown.set(node, longer);
  node.data = longer;
}

function lengthenAll(root: Node): void {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (node instanceof Text) lengthen(node);
  }
}

/** Lengthens every text under `root` now and whenever the page changes it. */
export function pseudoTranslate(root: HTMLElement): void {
  lengthenAll(root);
  new MutationObserver((records) => {
    for (const record of records) {
      if (record.target instanceof Text) lengthen(record.target);
      for (const added of record.addedNodes) lengthenAll(added);
    }
  }).observe(root, { subtree: true, childList: true, characterData: true });
}

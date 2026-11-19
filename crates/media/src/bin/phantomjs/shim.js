// PhantomJS 2.1.1 as yt-dlp's `PhantomJSwrapper` drives it (D281): the `phantom`, `fs` and `webpage` APIs that wrapper uses, over Deno.
// A page runs in a `node:vm` context of its own with a minimal browser around it: no layout, no element tree, no network.
// The context is not a security boundary: Deno's permissions (the run's folder only) are.
import vm from 'node:vm';

const scriptPath = Deno.args[0];
const platform = Deno.build.os === 'windows' ? 'Win32' : 'Linux x86_64';

const jar = [];

function sameCookie(a, b) {
  return a.name === b.name && a.domain === b.domain && (a.path ?? '/') === (b.path ?? '/');
}

// yt-dlp reads `httponly` of every cookie it gets back.
function keep(cookie) {
  const kept = { path: '/', httponly: false, secure: false, ...cookie };
  const at = jar.findIndex((c) => sameCookie(c, kept));
  if (at >= 0) jar.splice(at, 1);
  jar.push(kept);
  return true;
}

function domainMatches(domain, host) {
  const bare = domain.replace(/^\./, '').toLowerCase();
  return host === bare || (domain.startsWith('.') && host.endsWith(`.${bare}`));
}

function pageCookies(host, path) {
  return jar
    .filter(
      (c) => !c.httponly && domainMatches(c.domain ?? '', host) && path.startsWith(c.path ?? '/'),
    )
    .map((c) => `${c.name}=${c.value}`)
    .join('; ');
}

function setFromPage(text, host) {
  const [pair, ...attributes] = String(text).split(';');
  const split = pair.indexOf('=');
  if (split < 0) return;
  const cookie = {
    name: pair.slice(0, split).trim(),
    value: pair.slice(split + 1).trim(),
    domain: host,
    path: '/',
  };
  let expired = false;
  for (const attribute of attributes) {
    const [rawName, ...rest] = attribute.split('=');
    const name = rawName.trim().toLowerCase();
    const value = rest.join('=').trim();
    if (name === 'path' && value.startsWith('/')) cookie.path = value;
    if (name === 'secure') cookie.secure = true;
    if (name === 'max-age' && Number(value) <= 0) expired = true;
    if (name === 'expires' && Date.parse(value) <= Date.now()) expired = true;
    if (name === 'domain' && value) {
      const domain = `.${value.replace(/^\./, '').toLowerCase()}`;
      if (!domainMatches(domain, host)) return;
      cookie.domain = domain;
    }
  }
  if (!cookie.name) return;
  const at = jar.findIndex((c) => sameCookie(c, { path: '/', ...cookie }));
  if (at >= 0) jar.splice(at, 1);
  if (!expired) keep(cookie);
}

function message(error) {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

// An error PhantomJS would end on: `phantom.onError` if the script set one, else stderr and exit code 1.
function fail(error) {
  if (typeof globalThis.phantom.onError === 'function') {
    globalThis.phantom.onError(message(error), []);
  } else {
    console.error(message(error));
    Deno.exit(1);
  }
}

function guarded(work) {
  try {
    return work();
  } catch (error) {
    fail(error);
    return undefined;
  }
}

function pageContext(page) {
  const href = page.url || 'about:blank';
  let address = null;
  try {
    address = new URL(href);
  } catch {
    address = null;
  }
  const host = address?.hostname ?? '';
  const path = address?.pathname || '/';
  // Reloading would need the network: the page's state stays as it is.
  const location = {
    href,
    protocol: address?.protocol ?? 'about:',
    host: address?.host ?? '',
    hostname: host,
    port: address?.port ?? '',
    pathname: address?.pathname ?? '',
    search: address?.search ?? '',
    hash: address?.hash ?? '',
    origin: address?.origin ?? 'null',
    reload() {},
    replace() {},
    assign() {},
    toString() {
      return href;
    },
  };
  const document = {
    location,
    URL: href,
    domain: host,
    referrer: '',
    title: '',
    readyState: 'complete',
    cookie: '',
  };
  Object.defineProperty(document, 'cookie', {
    get: () => pageCookies(host, path),
    set: (text) => setFromPage(text, host),
  });
  const pageConsole = {};
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    pageConsole[level] = (...parts) => page.onConsoleMessage?.(parts.map(String).join(' '));
  }
  const sandbox = {
    document,
    location,
    navigator: {
      userAgent: page.settings.userAgent,
      platform,
      language: 'en-US',
      languages: ['en-US'],
      cookieEnabled: true,
    },
    console: pageConsole,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    atob,
    btoa,
  };
  sandbox.window = sandbox;
  sandbox.self = sandbox;
  sandbox.top = sandbox;
  sandbox.parent = sandbox;
  return vm.createContext(sandbox);
}

// A page's own errors go to `page.onError`, as in PhantomJS, and never end the run.
function runInPage(page, code) {
  try {
    return vm.runInContext(code, page.context);
  } catch (error) {
    if (typeof page.onError === 'function') guarded(() => page.onError(message(error), []));
    else console.error(message(error));
    return undefined;
  }
}

const SCRIPT = /<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi;
const SCRIPT_TYPE = /\btype\s*=\s*["']?([^"'\s>]+)/i;
const BODY_ONLOAD = /<body\b[^>]*?\bonload\s*=\s*(?:"([^"]*)"|'([^']*)')/i;

function isClassicScript(attributes) {
  if (/\bsrc\s*=/i.test(attributes)) return false;
  const type = SCRIPT_TYPE.exec(attributes)?.[1]?.toLowerCase();
  return !type || type === 'text/javascript' || type === 'application/javascript';
}

function load(page, html, url) {
  guarded(() => page.onLoadStarted?.());
  page.url = url;
  page.content = html;
  page.context = pageContext(page);
  for (const [, attributes, code] of html.matchAll(SCRIPT)) {
    if (isClassicScript(attributes)) runInPage(page, code);
  }
  const onload = BODY_ONLOAD.exec(html);
  if (onload) runInPage(page, onload[1] ?? onload[2]);
  else runInPage(page, 'if (typeof window.onload === "function") window.onload();');
  guarded(() => page.onLoadFinished?.('success'));
}

function createPage() {
  const page = {
    settings: { userAgent: `Mozilla/5.0 (${platform}) PhantomJS/2.1.1`, resourceTimeout: 0 },
    url: '',
    content: '<html><head></head><body></body></html>',
    context: null,
    onLoadStarted: null,
    onLoadFinished: null,
    onConsoleMessage: null,
    onError: null,
    // Only a blank page opens: the stand-in has no network.
    open(url, callback) {
      setTimeout(() => {
        if (url) {
          guarded(() => page.onLoadFinished?.('fail'));
          if (typeof callback === 'function') guarded(() => callback('fail'));
          return;
        }
        load(page, '<html><head></head><body></body></html>', '');
        if (typeof callback === 'function') guarded(() => callback('success'));
      }, 0);
    },
    setContent(html, url) {
      setTimeout(() => load(page, String(html), String(url ?? '')), 0);
    },
    // Runs `fn` in the page and gives back a copy of what it returned, as PhantomJS does.
    evaluate(fn, ...args) {
      const result = runInPage(page, `(${fn.toString()}).apply(null, ${JSON.stringify(args)})`);
      return result === undefined ? null : JSON.parse(JSON.stringify(result));
    },
    render() {
      return false;
    },
    close() {},
  };
  page.context = pageContext(page);
  return page;
}

const fs = {
  read(path) {
    return Deno.readTextFileSync(path);
  },
  write(path, content, mode) {
    const append = mode === 'a' || mode?.mode === 'a';
    Deno.writeTextFileSync(path, String(content), { append });
  },
  exists(path) {
    try {
      Deno.statSync(path);
      return true;
    } catch {
      return false;
    }
  },
};

globalThis.window = globalThis;
globalThis.phantom = {
  version: { major: 2, minor: 1, patch: 1 },
  cookiesEnabled: true,
  get cookies() {
    return jar.map((c) => ({ ...c }));
  },
  addCookie: (cookie) => keep(cookie),
  clearCookies() {
    jar.length = 0;
  },
  deleteCookie(name) {
    const before = jar.length;
    jar.splice(0, jar.length, ...jar.filter((c) => c.name !== name));
    return jar.length < before;
  },
  exit(code = 0) {
    Deno.exit(code);
  },
  onError: null,
};
globalThis.require = (name) => {
  if (name === 'fs') return fs;
  if (name === 'webpage') return { create: createPage };
  if (name === 'system') return { args: [scriptPath], env: {}, os: { name: Deno.build.os } };
  throw new Error(`PhantomJS module "${name}" is not available`);
};

// Indirect eval runs the script as PhantomJS would: sloppy mode, its declarations global.
guarded(() => (0, eval)(Deno.readTextFileSync(scriptPath)));

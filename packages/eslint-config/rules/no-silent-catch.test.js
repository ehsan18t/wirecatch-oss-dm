import { after, describe, it } from 'node:test';
import { RuleTester } from 'eslint';
import ts from 'typescript-eslint';
import { noSilentCatch } from './no-silent-catch.js';

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.afterAll = after;

const tester = new RuleTester({ languageOptions: { parser: ts.parser } });
const silent = [{ messageId: 'silent' }];
const unchecked = [{ messageId: 'unchecked' }];
/** The extension's logger: `fallback` from `diag/log` makes a handler that logs. */
const options = [{ factories: [{ name: 'fallback', module: 'diag/log' }] }];
const IMPORT_FALLBACK = "import { fallback } from '../diag/log';";

tester.run('no-silent-catch', noSilentCatch, {
  valid: [
    'try { f(); } catch (error) { log.warn("hub", "f failed", error); }',
    'try { f(); } catch (error) { throw new Wrapped(error); }',
    'try { f(); } catch (error) { return { detail: error.message }; }',
    'try { f(); } catch (error) { last = error; }',
    'try { f(); } catch (error) { reject(error as Error); }',
    'try { f(); } catch ({ message }) { show(message); }',
    'try { f(); } catch (error) { return error; }',
    'try { f(); } catch (error) { return error ?? other; }',
    'try { f(); } catch (error) { return other || error; }',
    'try { f(); } catch (error) { later(() => error); }',
    'try { f(); } catch (error) { function h() { return error; } report(h()); }',
    'try { f(); } catch (error) { function h() { return error; } later(h); }',
    'try { f(); } catch (error) { reject(new Error("failed", { cause: error })); }',
    'try { f(); } catch (error) { const failures = new Set([error]); report(failures); }',
    'p.catch((error = null) => report(error));',
    'p.catch((...args) => report(args));',
    'p.catch(({ message }) => show(message));',
    'p.catch((error) => { const { message } = error; show(message); });',
    'try { f(); } catch (error) { return String(error); }',
    'try { f(); } catch (error) { show(String(error)); }',
    'try { f(); } catch (error) { show(`failed: ${String(error)}`); }',
    'try { f(); } catch (error) { const text = error.message; show(text); }',
    'try { f(); } catch { /* Already closed by the browser. */ }',
    `try { f(); } catch {
      // Not an address: an answer, not a failure.
      return null;
    }`,
    'p.catch((error) => report(error));',
    'p.catch(() => { /* The popup closed meanwhile. */ });',
    'p.catch(() => /* The tab closed meanwhile. */ null);',
    'p.then(ok, (error) => log.warn("media", "failed", error));',
    'p.then(ok);',
    // No handler at all: the rejection goes on to the next one.
    'p.then(ok, undefined);',
    'p.then(ok, null);',
    'p["catch"]((error) => report(error));',
    // A function of this file that handles its error.
    'function logIt(error) { log.warn("hub", "failed", error); } p.catch(logIt);',
    'const logIt = (error) => log.warn("hub", "failed", error); p.then(ok, logIt);',
    // A handler made by a factory the configuration names, imported from its module.
    {
      code: `${IMPORT_FALLBACK} p.catch(fallback("hub", "Could not read the cache", null));`,
      options,
    },
    {
      code: `import { fallback } from '@/lib/diag/log'; const failed = fallback("hub", "x", undefined); p.catch(failed);`,
      options,
    },
    {
      code: `import { fallback as logged } from './diag/log'; p.then(ok, logged("hub", "x", null));`,
      options,
    },
    'p.finally(() => undefined);',
  ],
  invalid: [
    { code: 'try { f(); } catch {}', errors: silent },
    { code: 'try { f(); } catch { return null; }', errors: silent },
    { code: 'try { f(); } catch (error) { return null; }', errors: silent },
    { code: 'try { f(); } catch (_error) { cleanUp(); }', errors: silent },
    // Discarding or only testing the error is not handling it.
    { code: 'try { f(); } catch (error) { void error; return null; }', errors: silent },
    { code: 'try { f(); } catch (e) { if (e) return null; }', errors: silent },
    { code: 'try { f(); } catch (e) { e; }', errors: silent },
    { code: 'try { f(); } catch (e) { String(e); return null; }', errors: silent },
    { code: 'try { f(); } catch (e) { [e]; }', errors: silent },
    { code: 'try { f(); } catch (e) { e.message; }', errors: silent },
    { code: 'try { f(); } catch (e) { `${e}`; }', errors: silent },
    { code: 'try { f(); } catch (e) { const _ = e; }', errors: silent },
    { code: 'try { f(); } catch (e) { const text = String(e); return null; }', errors: silent },
    // A parameter that is not a plain name is judged by the names it binds.
    { code: 'p.catch((_e = null) => null);', errors: silent },
    { code: 'p.catch((..._args) => null);', errors: silent },
    { code: 'try { f(); } catch ({ name }) { void name; }', errors: silent },
    {
      code: 'p.catch((e) => { const { message } = e; void message; return null; });',
      errors: silent,
    },
    // A value made from the error that cannot hold it, or a function never called.
    { code: 'try { f(); } catch (e) { return e && null; }', errors: silent },
    { code: 'try { f(); } catch (e) { return [e][1]; }', errors: silent },
    { code: 'try { f(); } catch (e) { return { e }.x; }', errors: silent },
    { code: 'try { f(); } catch (e) { const h = () => e; }', errors: silent },
    { code: 'try { f(); } catch (e) { function h() { return e; } h(); }', errors: silent },
    { code: 'try { f(); } catch (e) { new Set([e]); }', errors: silent },
    // A rejection handler that answers its error turns the failure into a value nobody need read.
    { code: 'p.catch((e) => e);', errors: silent },
    { code: 'p.catch((e) => { return e; });', errors: silent },
    { code: 'p.catch((e) => ({ failed: e }));', errors: silent },
    // A comment that gives no reason.
    { code: 'try { f(); } catch { /* a b */ }', errors: silent },
    { code: 'try { f(); } catch { /* later todo */ }', errors: silent },
    { code: 'try { f(); } catch { /* */ }', errors: silent },
    { code: 'try { f(); } catch { // TODO\n }', errors: silent },
    { code: 'try { f(); } catch { // TODO: log this properly\n }', errors: silent },
    { code: 'try { f(); } catch { // ignore\n }', errors: silent },
    {
      // A directive is configuration, not a reason. Spelled in two parts so the gate's
      // `exceptions` check, which reads this file's text, does not take it for a real one.
      code: `try { f(); } catch { /* ${'eslint'}-disable-line no-empty -- a reason */ }`,
      errors: silent,
    },
    // A comment in a function nested in the handler explains that function, not the handler.
    {
      code: 'try { f(); } catch { later(() => { /* Runs after the page is gone. */ }); }',
      errors: silent,
    },
    // A comment before the statement does not say why this handler drops the error.
    { code: '// Best effort.\np.catch(() => undefined);', errors: silent },
    { code: 'p.catch(() => null);', errors: silent },
    { code: 'p.catch((error) => null);', errors: silent },
    { code: 'p.catch((error) => { void error; });', errors: silent },
    { code: 'p.catch(function () { return []; });', errors: silent },
    { code: 'p.then(ok, () => undefined);', errors: silent },
    { code: 'p["catch"](() => null);', errors: silent },
    { code: 'p[`catch`](() => null);', errors: silent },
    // A named no-op, local or not, and handlers that cannot be checked.
    { code: 'const ignore = () => null; p.catch(ignore);', errors: silent },
    { code: 'function swallow() {} p.catch(swallow);', errors: silent },
    { code: 'p.catch(noop);', errors: unchecked },
    // A `let` may hold another function by the time the promise rejects.
    {
      code: `${IMPORT_FALLBACK} let failed = fallback("hub", "x", null); p.catch(failed);`,
      options,
      errors: unchecked,
    },
    { code: 'p.catch(Boolean);', errors: unchecked },
    { code: 'p.catch(console.error);', errors: unchecked },
    { code: 'p.then(ok, onRejected);', errors: unchecked },
    // A handler made by a call is trusted only when the call is a factory the configuration names.
    { code: 'p.catch(noop.bind(null));', errors: unchecked },
    { code: 'p.catch(identity(() => null));', errors: unchecked },
    { code: 'p.catch((() => () => null)());', errors: unchecked },
    { code: 'const swallow = makeNoop(); p.catch(swallow);', errors: unchecked },
    { code: 'p.catch(fallback("hub", "x", null));', errors: unchecked },
    { code: `${IMPORT_FALLBACK} p.catch(fallback("hub", "x", null));`, errors: unchecked },
    {
      code: `import { fallback } from './elsewhere'; p.catch(fallback("hub", "x", null));`,
      options,
      errors: unchecked,
    },
    {
      code: `import { other as fallback } from '../diag/log'; p.catch(fallback("hub", "x", null));`,
      options,
      errors: unchecked,
    },
    {
      code: 'function fallback() { return () => null; } p.catch(fallback());',
      options,
      errors: unchecked,
    },
    // A parameter is whatever the caller passes; the function it belongs to says nothing of it.
    { code: 'function wrap(a, onError) { use(a); p.catch(onError); }', errors: unchecked },
    { code: 'function wrap(a, onError) { p.catch(onError); }', errors: unchecked },
    { code: 'const wrap = (onError) => p.catch(onError);', errors: unchecked },
    {
      code: 'try { f(); } catch { p.catch(() => false); }',
      errors: [...silent, ...silent],
    },
  ],
});

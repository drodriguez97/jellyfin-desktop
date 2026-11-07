// Minimal Chrome DevTools Protocol driver for the app's WebView2
// (run the app with --remote-debugging-port=<port>).
//
//   node --experimental-websocket cdp.mjs <port> targets
//   node --experimental-websocket cdp.mjs <port> eval "<js expression>"   (awaits promises)
//   node --experimental-websocket cdp.mjs <port> logs <seconds>           (streams console output)
//   node --experimental-websocket cdp.mjs <port> url
import { readFileSync } from 'node:fs';

const [port, mode, ...rest] = process.argv.slice(2);
if (!port || !mode) {
    console.error('usage: cdp.mjs <port> targets|eval|logs|url ...');
    process.exit(2);
}

async function pageTarget() {
    const res = await fetch(`http://127.0.0.1:${port}/json`);
    const targets = await res.json();
    const page = targets.find((t) => t.type === 'page' && !t.url.startsWith('devtools://'));
    if (!page) throw new Error('no page target: ' + JSON.stringify(targets.map((t) => [t.type, t.url])));
    return page;
}

function connect(url) {
    return new Promise((resolve, reject) => {
        const ws = new WebSocket(url);
        let id = 0;
        const pending = new Map();
        const listeners = [];
        ws.onopen = () => resolve({
            send(method, params = {}) {
                return new Promise((res, rej) => {
                    const msgId = ++id;
                    pending.set(msgId, { res, rej });
                    ws.send(JSON.stringify({ id: msgId, method, params }));
                });
            },
            on(fn) { listeners.push(fn); },
            close() { ws.close(); }
        });
        ws.onerror = (e) => reject(new Error('websocket error ' + (e.message || '')));
        ws.onmessage = (ev) => {
            const msg = JSON.parse(ev.data);
            if (msg.id && pending.has(msg.id)) {
                const p = pending.get(msg.id);
                pending.delete(msg.id);
                if (msg.error) p.rej(new Error(JSON.stringify(msg.error))); else p.res(msg.result);
            } else if (msg.method) {
                for (const fn of listeners) fn(msg);
            }
        };
    });
}

function describeArg(a) {
    if (a.value !== undefined) return typeof a.value === 'string' ? a.value : JSON.stringify(a.value);
    if (a.description) return a.description;
    return a.type;
}

const page = await pageTarget();
if (mode === 'targets') {
    const res = await fetch(`http://127.0.0.1:${port}/json`);
    console.log(JSON.stringify(await res.json(), null, 1));
    process.exit(0);
}
if (mode === 'url') {
    console.log(page.url);
    process.exit(0);
}

const cdp = await connect(page.webSocketDebuggerUrl);
await cdp.send('Runtime.enable');
await cdp.send('Log.enable');

if (mode === 'logs') {
    const seconds = Number(rest[0] || 10);
    cdp.on((msg) => {
        if (msg.method === 'Runtime.consoleAPICalled') {
            const p = msg.params;
            console.log(`[console.${p.type}] ` + p.args.map(describeArg).join(' '));
        } else if (msg.method === 'Runtime.exceptionThrown') {
            const d = msg.params.exceptionDetails;
            console.log('[exception] ' + (d.exception?.description || d.text));
        } else if (msg.method === 'Log.entryAdded') {
            const e = msg.params.entry;
            console.log(`[log.${e.level}] ${e.text} ${e.url || ''}`);
        }
    });
    await new Promise((r) => setTimeout(r, seconds * 1000));
    cdp.close();
    process.exit(0);
}

if (mode === 'eval') {
    let expression = rest.join(' ');
    if (expression.startsWith('@')) expression = readFileSync(expression.slice(1), 'utf8');
    const result = await cdp.send('Runtime.evaluate', {
        expression,
        awaitPromise: true,
        returnByValue: true
    });
    if (result.exceptionDetails) {
        console.log('EXCEPTION: ' + (result.exceptionDetails.exception?.description || result.exceptionDetails.text));
        process.exitCode = 1;
    } else {
        const v = result.result;
        console.log(v.value !== undefined ? (typeof v.value === 'string' ? v.value : JSON.stringify(v.value, null, 1)) : (v.description || v.type));
    }
    cdp.close();
    process.exit();
}

console.error('unknown mode ' + mode);
process.exit(2);

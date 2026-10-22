const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const html = fs.readFileSync(path.join(__dirname, '../crates/deskpet-app/frontend/bubble.html'), 'utf8');
const code = html.match(/<script>([\s\S]*?)<\/script>/)[1];
const timers = new Map(), elements = new Map(), calls = [];
let now = 0, timerId = 0;
function el(id) {
  if (!elements.has(id)) {
    const handlers = new Map();
    elements.set(id, {
      style:{},dataset:{},scrollTop:0,textContent:'',contains:target=>target===el('bubble-content'),
      addEventListener(type, listener) { handlers.set(type, listener); },
      fire(type, event = {}) { handlers.get(type)?.(event); },
    });
  }
  return elements.get(id);
}
const context = vm.createContext({
  console,document:{getElementById:el,querySelectorAll:()=>[]},
  setTimeout(fn,ms) { const id=++timerId; timers.set(id,{fn,at:now+ms}); return id; },
  clearTimeout:id=>timers.delete(id),
  window:{__TAURI__:{core:{invoke(command,args) { calls.push({command,args}); return Promise.resolve(null); }}}},
});
vm.runInContext(code,context,{filename:'bubble.html'});
function show(payload) { context.payload = payload; vm.runInContext('show(payload)',context); }
function tick(ms) {
  now += ms;
  for (const [id,timer] of [...timers]) if (timer.at<=now) { timers.delete(id); timer.fn(); }
}
const longText = '很长的热点正文\n'.repeat(500);
el('bubble-content').scrollTop = 200;
show({hot_id:'hot-1',title:'完整标题'.repeat(30),script:longText,url:'https://example.com/hot'});
assert.equal(el('b-script').textContent,longText);
assert.equal(el('bubble-content').scrollTop,0);
tick(14000); el('bubble').fire('pointerenter'); tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,0);
el('bubble-content').fire('scroll'); tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,0);
el('bubble').fire('pointerleave'); tick(14999);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,0);
tick(1); assert.equal(calls.filter(call=>call.command==='hide_bubble').length,1);
assert.deepEqual(calls.find(call=>call.command==='feedback').args.action,'ignore');
show({hot_id:'hot-2',script:'新内容'}); el('bubble').fire('focusin'); tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,1);
el('bubble').fire('focusout',{relatedTarget:null}); tick(15000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,2);
console.log('PASS: full text retained, scroll reset, hover/scroll/keyboard reading pauses automatic dismissal.');

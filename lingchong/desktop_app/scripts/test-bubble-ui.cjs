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
  requestAnimationFrame:fn=>{ fn(); return 1; },
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

function speech(payload) { context.payload = payload; vm.runInContext('updateSpeech(payload)',context); }
show({hot_id:'spoken',bubble_id:'speech-1',script:'要朗读的内容',speech_state:'queued'});
const before = calls.filter(call=>call.command==='hide_bubble').length;
tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,before,'synthesis cannot auto-close');
speech({bubble_id:'speech-1',speech_state:'playing'});
speech({bubble_id:'speech-1',speech_state:'queued'});
assert.equal(el('speech-status').textContent,'正在朗读…','cached queued state cannot roll back playback');
tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,before,'playback cannot auto-close');
speech({bubble_id:'obsolete',speech_state:'completed'});
tick(60000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,before,'obsolete completion cannot release current bubble');
speech({bubble_id:'speech-1',speech_state:'completed'});
tick(14999);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,before);
tick(1);
assert.equal(calls.filter(call=>call.command==='hide_bubble').length,before+1);
assert.equal(calls.filter(call=>call.command==='hide_bubble').at(-1).args.bubbleId,'speech-1');
show({bubble_id:'speech-2',script:'下一条',speech_state:'synthesizing'});
vm.runInContext('hide()',context);
assert.equal(calls.filter(call=>call.command==='hide_bubble').at(-1).args.bubbleId,'speech-2','manual close identifies the task to stop');
speech({bubble_id:'speech-2',speech_state:'completed'});
const afterClose = calls.length; tick(60000);
assert.equal(calls.length,afterClose,'late speech events cannot reopen or close another bubble');
show({bubble_id:'speech-3',script:'失败时仍可阅读',speech_state:'queued'});
speech({bubble_id:'speech-3',speech_state:'failed',speech_error:'缺少依赖'});
assert.match(el('speech-status').textContent,/缺少依赖/);
tick(15000);
assert.equal(calls.filter(call=>call.command==='hide_bubble').at(-1).args.bubbleId,'speech-3');
const closedCalls = calls.length;
show({bubble_id:'speech-3',script:'迟到的缓存',speech_state:'queued'});
tick(60000);
assert.equal(calls.length,closedCalls,'closed bubble cannot be resurrected by a cached response');
show({bubble_id:'speech-5',script:'最新消息',speech_state:'queued'});
show({bubble_id:'speech-4',script:'过期消息',speech_state:'queued'});
assert.equal(el('b-script').textContent,'最新消息','stale initial response cannot replace a newer message');
console.log('PASS: synth/playback keep bubble visible, completion resumes timer, close targets speech, obsolete events ignored, failure recovers.');

// Regression tests for the real pet scripts, with a minimal DOM and fake clock.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const frontend = path.join(__dirname, '../crates/deskpet-app/frontend');
const inline = fs.readFileSync(path.join(frontend, 'index.html'), 'utf8').match(/<script>([\s\S]*?)<\/script>/)[1];
const mobile = fs.readFileSync(path.join(frontend, 'mobile.js'), 'utf8');
async function fixture() {
  let now = 1000, timerId = 0, resolveTrigger, rejectTrigger, triggerCalls = 0;
  const timers = new Map(), elements = new Map(), listeners = new Map();
  const element = id => {
    if (!elements.has(id)) {
      const events = new Map(), classes = new Set();
      elements.set(id, {
        style:{}, hidden:true, disabled:false, value:'', textContent:'', append:()=>{}, setAttribute:()=>{},
        classList:{add:(...names)=>names.forEach(name=>classes.add(name)),remove:(...names)=>names.forEach(name=>classes.delete(name)),toggle:()=>{}},
        addEventListener(type, callback) { if (!events.has(type)) events.set(type, []); events.get(type).push(callback); },
        fire(type, event = {}) { for (const callback of events.get(type) || []) callback(event); },
      });
    }
    return elements.get(id);
  };
  const context = vm.createContext({
    console, navigator:{userAgent:'Android'},
    document:{getElementById:element, querySelector:element, querySelectorAll:()=>[], documentElement:element('html'), body:{insertAdjacentHTML:()=>{}}},
    Date:class extends Date { static now() { return now; } },
    Image:class { set src(value) { this.naturalWidth = 1024; this.naturalHeight = 128; queueMicrotask(()=>this.onload()); } },
    setTimeout(callback, duration) { const id = ++timerId; timers.set(id, {at:now+duration, callback}); return id; },
    clearTimeout(id) { timers.delete(id); }, requestAnimationFrame:()=>1, cancelAnimationFrame:()=>{},
    __TAURI__:{
      core:{invoke(command) {
        if (command === 'bubble_ready') return Promise.resolve(null);
        if (command === 'trigger_agent') { triggerCalls++; return new Promise((resolve, reject)=>{ resolveTrigger=resolve; rejectTrigger=reject; }); }
        return Promise.resolve(true);
      }},
      event:{listen(name, callback) { if (!listeners.has(name)) listeners.set(name, []); listeners.get(name).push(callback); return Promise.resolve(()=>{}); }},
    },
  });
  context.window = context;
  vm.runInContext(inline, context, {filename:'index.html'});
  vm.runInContext(mobile, context, {filename:'mobile.js'});
  const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
  await flush();
  return {
    el:element, flush, mood:()=>vm.runInContext('currentMood',context),
    emit(name, payload) { for (const callback of listeners.get(name) || []) callback({payload}); },
    tick(duration) {
      const end = now + duration;
      while (true) {
        const next = [...timers].filter(([,timer])=>timer.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];
        if (!next) break;
        now = next[1].at; timers.delete(next[0]); next[1].callback();
      }
      now = end;
    },
    doubleTap() {
      for (let i = 0; i < 2; i++) {
        const event = {button:0,isPrimary:true,clientX:100,clientY:100,pointerId:i+1};
        element('pet-wrap').fire('pointerdown',event); now += 40;
        element('pet-wrap').fire('pointerup',event); now += 70;
      }
    },
    resolve:()=>resolveTrigger(true), reject:()=>rejectTrigger(new Error('offline')),
    calls:()=>triggerCalls,
  };
}
(async()=>{
  const f = await fixture();
  assert.equal(f.mood(),'idle');
  f.doubleTap(); await f.flush();
  assert.equal(f.calls(),1); assert.equal(f.mood(),'curious');
  assert.equal(f.el('mobile-bubble').hidden,false);
  f.doubleTap(); await f.flush(); assert.equal(f.calls(),1,'pending taps must not duplicate requests');
  f.emit('bubble:show',{hot_id:'hot-1',script:'真实热点文案',mood_hint:'excited'});
  f.emit('pet:mood','excited'); f.resolve(); await f.flush();
  assert.equal(f.mood(),'excited');
  f.tick(7999); assert.equal(f.mood(),'excited'); f.tick(1); assert.equal(f.mood(),'idle');
  f.emit('pet:mood','happy'); f.tick(7000); f.emit('pet:mood','curious');
  f.tick(1000); assert.equal(f.mood(),'curious','old timers must not cancel a newer emotion');
  f.tick(7000); assert.equal(f.mood(),'idle');
  f.emit('bubble:show',{mood_hint:'curious',script:'热点'});
  f.el('bubble-close').fire('click'); assert.equal(f.mood(),'idle');
  assert.equal(f.el('mobile-bubble').hidden,true);
  f.doubleTap(); await f.flush(); f.reject(); await f.flush();
  assert.equal(f.mood(),'idle');
  f.doubleTap(); await f.flush();
  f.emit('bubble:show',{mood_hint:'idle',script:'还没有可播报的热点'});
  f.resolve(); await f.flush(); assert.equal(f.mood(),'idle');
  assert.match(f.el('mobile-bubble-text').textContent,/还没有/);
  f.el('bubble-close').fire('click');
  assert.equal(f.el('companion-status').textContent,'在你身边');
  f.el('refresh-hotspots').fire('click'); await f.flush();
  assert.equal(f.calls(),4,'single-tap refresh button starts a request');
  assert.equal(f.el('refresh-hotspots').disabled,true);
  assert.equal(f.el('companion-status').textContent,'正在找新鲜事…');
  f.el('refresh-hotspots').fire('click'); await f.flush();
  assert.equal(f.calls(),4,'button and double taps share the pending guard');
  f.el('mobile-bubble').scrollTop = 100;
  f.emit('bubble:show',{script:'新的消息'});
  assert.equal(f.el('mobile-bubble').scrollTop,0);
  f.resolve(); await f.flush();
  assert.equal(f.el('refresh-hotspots').disabled,false);
  assert.equal(f.el('companion-status').textContent,'有话想对你说');
  f.el('pet-wrap').fire('keydown',{key:'Enter',preventDefault:()=>{}}); await f.flush();
  assert.equal(f.calls(),5,'keyboard activation works');
  f.resolve(); await f.flush();
  console.log('PASS: double tap, pending guard, timed mood reset, timer replacement, close, error, empty result.');
})().catch(error=>{ console.error(error); process.exitCode=1; });

'use strict';
const $ = id => document.getElementById(id);
const audio = $('audio');
const token = document.querySelector('meta[name="demo-token"]').content;
let revision = 0, jobId = null;
const labels = {idle:'待开始',synthesizing:'合成中',ready:'音频就绪',playing:'播放中',completed:'播放结束',cancelled:'已停止',failed:'失败'};
function status(state, message) {
  $('state-badge').textContent = labels[state] || state;
  $('status').textContent = message;
  document.querySelector('.result').classList.toggle('error', state === 'failed');
}
async function api(route, data) {
  const response = await fetch(route, {method:'POST',headers:{'Content-Type':'application/json','X-Demo-Token':token},body:JSON.stringify(data)});
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || `请求失败：${response.status}`);
  return result;
}
function clearAudio() {
  audio.pause(); audio.removeAttribute('src'); audio.load();
  $('download').hidden = true; $('download').removeAttribute('href');
  $('metrics').textContent = '尚未生成音频';
}
function refreshParams() {
  $('text-count').textContent = `${$('text').value.length} / 3000`;
  $('rate-value').textContent = `${Number($('rate').value) >= 0 ? '+' : ''}${$('rate').value}%`;
  $('pitch-value').textContent = `${Number($('pitch').value) >= 0 ? '+' : ''}${$('pitch').value} Hz`;
  $('volume-value').textContent = `${$('volume').value}%`;
  audio.volume = Number($('volume').value) / 100;
}
['text','rate','pitch','volume'].forEach(id => $(id).addEventListener('input',refreshParams));
refreshParams();
const samples = {
  greeting:'你好，我是你的桌面小伙伴。今天也要记得休息一下，喝一口水，再继续做喜欢的事情。',
  news:'这是一段用于测试的热点文案，不是真实新闻。今天我们一起探索科技的新进展，听听数字、一二三，以及不同标点的停顿。',
  mixed:'你好，DeskPet！欢迎来到 Edge TTS 语音实验室。版本 0.1.0，今天的任务是测试 Hello world，以及中文和 English 的混合朗读。',
};
document.querySelectorAll('[data-sample]').forEach(button=>button.addEventListener('click',()=>{
  $('text').value = samples[button.dataset.sample]; refreshParams();
}));
$('synthesize').addEventListener('click',async()=>{
  const text = $('text').value.trim();
  if (!text) { status('failed','请输入要朗读的文字。'); $('text').focus(); return; }
  const ownRevision = ++revision;
  jobId = null; clearAudio();
  $('synthesize').disabled = true; $('stop').disabled = false;
  status('synthesizing','正在在线合成语音，最多等待 90 秒；可以随时停止。');
  try {
    const accepted = await api('/api/synthesize',{text,voice:$('voice').value,rate:Number($('rate').value),pitch:Number($('pitch').value),proxy:$('proxy').value.trim()});
    if (ownRevision !== revision) { await api('/api/cancel',{job_id:accepted.job_id}); return; }
    jobId = accepted.job_id;
    while (ownRevision === revision) {
      const response = await fetch('/api/state');
      if (!response.ok) throw new Error('无法查询合成状态。');
      const result = await response.json();
      if (ownRevision !== revision) return;
      if (result.job_id !== jobId) throw new Error('任务已被另一个调试页面替换，请重新合成。');
      if (result.status === 'failed') throw new Error(result.error);
      if (result.status === 'cancelled') { status('cancelled','合成已取消。'); return; }
      if (result.status === 'ready') {
        audio.src = result.audio_url;
        $('download').href = result.audio_url; $('download').hidden = false;
        $('metrics').textContent = `合成 ${result.elapsed} 秒 · ${(result.bytes/1024).toFixed(1)} KB · ${result.characters} 字符`;
        status('ready','合成完成，可以试听或下载 MP3。');
        if ($('auto-play').checked) {
          try { await audio.play(); } catch (_) {
            if (ownRevision === revision) status('ready','音频已生成；浏览器阻止了自动播放，请点击播放器中的播放按钮。');
          }
        }
        return;
      }
      await new Promise(resolve=>setTimeout(resolve,400));
    }
  } catch (error) {
    if (ownRevision === revision) status('failed',error.message || '请求失败，请检查本地服务是否运行。');
  } finally {
    if (ownRevision === revision) {
      $('synthesize').disabled = false;
      $('stop').disabled = !audio.getAttribute('src');
    }
  }
});
$('stop').addEventListener('click',async()=>{
  const ownRevision = ++revision;
  const previous = jobId; jobId = null;
  audio.pause(); if (audio.getAttribute('src')) audio.currentTime = 0;
  $('synthesize').disabled = false; $('stop').disabled = true;
  status('cancelled','已停止。已生成的音频仍可通过播放器重播或下载。');
  if (previous) {
    try { await api('/api/cancel',{job_id:previous}); }
    catch (_) { if (ownRevision === revision) status('failed','播放已停止，但无法确认合成已取消；请检查本地服务。'); }
  }
});
$('voices-refresh').addEventListener('click',async()=>{
  $('voices-refresh').disabled = true; $('voice-note').textContent = '正在获取在线音色列表…';
  try {
    const result = await api('/api/voices',{proxy:$('proxy').value.trim()});
    const selected = $('voice').value;
    const voices = result.voices.sort((a,b)=>Number(b.locale.startsWith('zh'))-Number(a.locale.startsWith('zh')) || a.id.localeCompare(b.id));
    if (!voices.length) throw new Error('服务返回了空音色列表。');
    $('voice').replaceChildren(...voices.map(voice=>new Option(`${voice.id} · ${voice.gender === 'Female' ? '女声' : '男声'}`,voice.id)));
    if (voices.some(voice=>voice.id===selected)) $('voice').value = selected;
    $('voice-note').textContent = `已获取 ${voices.length} 个在线音色，中文优先排列。`;
  } catch (error) { $('voice-note').textContent = error.message; }
  finally { $('voices-refresh').disabled = false; }
});
audio.addEventListener('play',()=>{ $('stop').disabled=false; status('playing','正在播放。音量可实时调整，停止后可从播放器重新播放。'); });
audio.addEventListener('pause',()=>{ if (!audio.ended && audio.getAttribute('src')) status('ready','播放已暂停，可以继续播放。'); });
audio.addEventListener('ended',()=>{ $('stop').disabled=true; status('completed','播放结束，可重播或下载音频。'); });
audio.addEventListener('error',()=>{ if (audio.getAttribute('src')) status('failed','浏览器无法播放音频，请下载检查或重新合成。'); });
window.addEventListener('pagehide',()=>{
  audio.pause();
  if (jobId) fetch('/api/cancel',{method:'POST',headers:{'Content-Type':'application/json','X-Demo-Token':token},body:JSON.stringify({job_id:jobId}),keepalive:true}).catch(()=>{});
});

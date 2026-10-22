(() => {
  if (!/Android|iPhone|iPad/i.test(navigator.userAgent)) return;
  document.documentElement.classList.add('is-mobile');
  document.body.insertAdjacentHTML('beforeend', `
    <header id="mobile-header" class="mobile-ui"><div><strong>DeskPet</strong><small>你的热点小伙伴</small></div><button id="open-settings" type="button">设置</button></header>
    <main id="mobile-stage" class="mobile-ui"></main>
    <p id="mobile-hint" class="mobile-ui">双击基尔兽，听听今天的新鲜事</p>
    <section id="mobile-bubble" class="mobile-ui" aria-live="polite" aria-label="宠物气泡" hidden>
      <button id="bubble-close" type="button" aria-label="关闭气泡">×</button><h2 id="mobile-bubble-title"></h2><p id="mobile-bubble-text"></p>
      <div id="bubble-feedback" hidden><button type="button" data-feedback="like">喜欢</button><button type="button" data-feedback="dislike">不感兴趣</button></div>
    </section>
    <dialog id="settings-dialog" aria-labelledby="settings-title">
      <form id="settings-form"><h2 id="settings-title">模型与宠物设置</h2><p class="subtitle">配置模型服务与宠物性格，保存后用于下一次模型回复。</p>
        <label class="field" for="pet-name">宠物名称<input id="pet-name" required maxlength="80"></label>
        <label class="field" for="pet-personality">性格<textarea id="pet-personality" rows="2" maxlength="4000"></textarea></label>
        <label class="field" for="pet-speaking-style">表达偏好<textarea id="pet-speaking-style" rows="2" maxlength="4000"></textarea></label>
        <label class="field" for="pet-background">背景设定（可选）<textarea id="pet-background" rows="2" maxlength="4000"></textarea></label>
        <label class="field" for="api-base">API 地址<input id="api-base" type="url" required placeholder="https://example.com/v1" spellcheck="false" autocapitalize="off"><small>填写服务商提供的兼容 OpenAI 接口地址，通常以 /v1 结尾。</small></label>
        <label class="field" for="api-key">API Key<input id="api-key" type="password" autocomplete="new-password" spellcheck="false" autocapitalize="off"><small id="api-key-status"></small></label>
        <label class="clear-key"><input id="clear-api-key" type="checkbox">清除已保存的 API Key</label>
        <label class="field" for="model-name">模型名称<input id="model-name" required placeholder="服务商提供的模型名称" spellcheck="false" autocapitalize="off"></label>
        <label class="field" for="embedding-model">向量模型（可选）<input id="embedding-model" placeholder="留空关闭" spellcheck="false" autocapitalize="off"><small>服务商不支持向量接口时请留空。</small></label>
        <p id="settings-status" role="status"></p><div id="settings-actions"><button id="close-settings" type="button">关闭</button><button id="save-settings" class="primary" type="submit">保存设置</button></div>
      </form>
    </dialog>`);
  const el = id => document.getElementById(id);
  // Keep the pet and bubble in normal flow: long text cannot grow over the pet.
  el('mobile-stage').append(el('app'), el('mobile-hint'), el('mobile-bubble'));
  const T = window.__TAURI__;
  const invoke = (name, args) => T?.core ? T.core.invoke(name, args) : Promise.reject(new Error('应用服务尚未就绪，请重新打开应用。'));
  let pending = false, currentHotId = null;
  let petName = '基尔兽';
  invoke('get_settings').then(settings => {
    petName = settings.pet.name;
    el('mobile-hint').textContent = `双击${petName}，听听今天的新鲜事`;
  }).catch(() => {});
  function showBubble(payload) {
    currentHotId = payload.hot_id || null;
    el('mobile-bubble-title').textContent = payload.title || `${petName}说`;
    el('mobile-bubble-text').textContent = payload.script || '暂时没有值得说的热点～';
    el('bubble-feedback').hidden = !currentHotId;
    el('mobile-bubble').hidden = false;
    if (payload.mood_hint) setTransientMood(payload.mood_hint);
  }
  function dismissBubble(action) {
    const hotId = currentHotId;
    currentHotId = null;
    el('mobile-bubble').hidden = true;
    setMood('idle');
    if (hotId) invoke('feedback', { hotId, action }).catch(() => {});
  }
  let listenerReady = Promise.resolve();
  if (T?.event) {
    listenerReady = T.event.listen('bubble:show', event => showBubble(event.payload));
    listenerReady.then(() => invoke('bubble_ready')).then(payload => {
      if (payload && !pending) showBubble(payload);
    }).catch(() => {});
  }
  async function triggerAgent() {
    if (pending) return;
    pending = true;
    setMood('curious');
    showBubble({ script: '正在联网刷新热点……' });
    try {
      await listenerReady;
      await invoke('trigger_agent');
    } catch (error) {
      showBubble({ script: typeof error === 'string' ? error : '本次联网刷新失败，请检查网络后重试。', mood_hint: 'idle' });
      setMood('idle');
    } finally {
      pending = false;
      // Also recover if a successful IPC response arrives without a UI event.
      setTransientMood(currentMood);
    }
  }
  window.deskpetMobile = { triggerAgent };
  el('bubble-close').addEventListener('click', () => dismissBubble('dismiss'));
  el('bubble-feedback').addEventListener('click', event => {
    const action = event.target.closest('[data-feedback]')?.dataset.feedback;
    if (action) dismissBubble(action);
  });
  // Pointer events work for both Android touch taps and desktop mouse testing.
  let firstTap = null, down = null;
  const pet = el('pet-wrap');
  pet.addEventListener('pointerdown', event => {
    if (!event.isPrimary || event.button !== 0) return;
    down = { x: event.clientX, y: event.clientY, time: Date.now(), id: event.pointerId };
  });
  pet.addEventListener('pointercancel', () => { down = null; firstTap = null; });
  pet.addEventListener('pointerup', event => {
    const start = down;
    down = null;
    if (!start || start.id !== event.pointerId) return;
    const now = Date.now();
    if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > 14 || now - start.time > 500) {
      firstTap = null; return;
    }
    if (firstTap && now - firstTap.time < 400 && Math.hypot(event.clientX - firstTap.x, event.clientY - firstTap.y) < 30) {
      firstTap = null;
      triggerAgent();
    } else firstTap = { time: now, x: event.clientX, y: event.clientY };
  });

  let saving = false;
  const dialog = el('settings-dialog');
  function status(message, error = false) {
    el('settings-status').textContent = message;
    el('settings-status').classList.toggle('error', error);
  }
  function renderSettings(settings) {
    petName = settings.pet.name;
    el('mobile-hint').textContent = `双击${petName}，听听今天的新鲜事`;
    el('pet-name').value = settings.pet.name;
    el('pet-personality').value = settings.pet.personality;
    el('pet-speaking-style').value = settings.pet.speaking_style;
    el('pet-background').value = settings.pet.background;
    el('api-base').value = settings.apiBase;
    el('model-name').value = settings.model;
    el('embedding-model').value = settings.embeddingModel || '';
    el('api-key').value = '';
    el('api-key').disabled = false;
    el('clear-api-key').checked = false;
    el('clear-api-key').disabled = !settings.apiKeyConfigured;
    el('api-key').placeholder = settings.apiKeyConfigured ? '已配置，留空保留原 Key' : '请输入 API Key';
    el('api-key-status').textContent = settings.apiKeyConfigured ? '已保存密钥。输入新 Key 可以替换，留空保留。' : '尚未配置密钥；保存后立即生效。';
  }
  el('open-settings').addEventListener('click', async () => {
    el('open-settings').disabled = true;
    try {
      renderSettings(await invoke('get_settings'));
      status('');
      dialog.showModal();
    } catch (_) {
      showBubble({ script: '设置加载失败，请重新打开应用后重试。' });
    } finally { el('open-settings').disabled = false; }
  });
  el('clear-api-key').addEventListener('change', () => {
    el('api-key').disabled = el('clear-api-key').checked;
    if (el('clear-api-key').checked) el('api-key').value = '';
  });
  el('close-settings').addEventListener('click', () => { if (!saving) dialog.close(); });
  dialog.addEventListener('cancel', event => { if (saving) event.preventDefault(); });
  dialog.addEventListener('close', () => { el('api-key').value = ''; });
  el('settings-form').addEventListener('submit', async event => {
    event.preventDefault();
    if (saving) return;
    saving = true;
    el('save-settings').disabled = true;
    el('close-settings').disabled = true;
    status('正在保存……');
    const settings = {
      pet: {name:el('pet-name').value.trim(), personality:el('pet-personality').value.trim(), speaking_style:el('pet-speaking-style').value.trim(), background:el('pet-background').value.trim()},
      apiBase: el('api-base').value.trim(),
      model: el('model-name').value.trim(),
      embeddingModel: el('embedding-model').value.trim() || null,
      apiKey: el('clear-api-key').checked ? '' : (el('api-key').value.trim() || null),
    };
    try {
      renderSettings(await invoke('save_settings', { settings }));
      status('已保存，立即生效。');
    } catch (_) {
      status('保存失败，请检查 API 地址、模型名称和宠物画像后重试。', true);
    } finally {
      saving = false;
      el('save-settings').disabled = false;
      el('close-settings').disabled = false;
    }
  });
})();

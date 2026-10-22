(() => {
  const $ = id => document.getElementById(id);
  const T = window.__TAURI__;
  const invoke = (command, args) => T.core.invoke(command, args);
  const platforms = { zhihu:'知乎', bilibili:'哔哩哔哩', weibo:'微博', rsshub:'RSSHub', douyin:'抖音', xiaohongshu:'小红书' };
  let config, profile, busy = false, dirty = false, generation = 0;
  function status(message, error = false) { $('status').textContent = message; $('status').classList.toggle('error', error); }
  function removeButton(row, label) {
    const button = document.createElement('button');
    button.type = 'button'; button.className = 'secondary'; button.textContent = '删除'; button.setAttribute('aria-label', label);
    button.onclick = () => { row.remove(); dirty = true; };
    return button;
  }
  function quietRow(hours = '23:00-08:00') {
    const row = document.createElement('div'); row.className = 'quiet-row';
    const [start = '', end = ''] = hours.split('-');
    row.innerHTML = '<input type="time" required aria-label="勿扰开始时间"><span>至</span><input type="time" required aria-label="勿扰结束时间">';
    row.querySelectorAll('input')[0].value = start; row.querySelectorAll('input')[1].value = end;
    row.append(removeButton(row, '删除勿扰时段')); $('quiet-list').append(row);
  }
  function interestRow(interest = {topic:'', weight:.5, last_hit:0, embedding:null}) {
    const row = document.createElement('div'); row.className = 'interest-row'; row.original = interest;
    row.innerHTML = '<input required aria-label="兴趣主题" placeholder="例如：人工智能"><input type="number" min="0" max="1" step="0.01" required aria-label="兴趣权重">';
    row.children[0].value = interest.topic; row.children[1].value = interest.weight;
    row.append(removeButton(row, '删除兴趣主题')); $('interest-list').append(row);
  }
  function render() {
    $('pet-name').value = config.pet.name;
    $('pet-personality').value = config.pet.personality;
    $('pet-speaking-style').value = config.pet.speaking_style;
    $('pet-background').value = config.pet.background;
    $('quiet-list').replaceChildren(); config.general.quiet_hours.forEach(quietRow);
    $('announce-interval').value = config.general.min_announce_interval_min;
    $('threshold').value = config.general.announce_threshold; $('threshold-value').value = Number($('threshold').value).toFixed(2);
    $('api-base').value = config.llm.api_base; $('model').value = config.llm.model; $('embedding').value = config.llm.embedding_model || '';
    $('api-key').value = ''; $('api-key').disabled = false; $('api-key').placeholder = config.has_api_key ? '已配置，留空保留原 Key' : '请输入 API Key';
    $('key-status').textContent = config.has_api_key ? '密钥已配置；输入新值可替换，留空保留。' : '尚未配置 API Key。';
    $('clear-key').checked = false; $('clear-key').disabled = !config.has_api_key;
    $('rsshub').value = config.fetch.rsshub_endpoint; $('fetch-default').value = config.fetch.default_interval_min;
    $('platform-list').replaceChildren();
    Object.entries(platforms).forEach(([id, name]) => {
      const saved = config.fetch.platforms.find(platform => platform.id === id);
      const row = document.createElement('div'); row.className = 'platform-row'; row.dataset.platform = id;
      row.innerHTML = '<label class="check"><input type="checkbox"><span></span></label><input type="number" min="1" step="1" required>';
      row.querySelector('span').textContent = name;
      row.querySelector('[type=checkbox]').checked = saved?.enabled || false;
      row.querySelector('[type=number]').value = saved?.interval_min || config.fetch.default_interval_min;
      row.querySelector('[type=number]').setAttribute('aria-label', name + '采集间隔（分钟）');
      $('platform-list').append(row);
    });
    $('tts-enabled').checked = config.tts.enabled;
    if (![...$('voice').options].some(option => option.value === config.tts.voice)) $('voice').add(new Option(config.tts.voice, config.tts.voice));
    $('voice').value = config.tts.voice; $('persona').value = profile.persona;
    $('interest-list').replaceChildren(); profile.interests.forEach(interestRow);
  }
  async function load(force = false) {
    if (busy || (dirty && !force)) return;
    const request = ++generation; busy = true; $('save').disabled = true; $('reload').disabled = true;
    status('正在加载设置……');
    try {
      const values = await Promise.all([invoke('get_config'), invoke('get_profile')]);
      if (request !== generation) return;
      if (dirty && !force) return;
      [config, profile] = values; render(); dirty = false; status('');
    } catch (error) { status('加载失败：' + String(error), true); }
    finally { busy = false; $('save').disabled = !config; $('reload').disabled = false; }
  }
  $('settings-form').addEventListener('input', () => { dirty = true; });
  $('threshold').addEventListener('input', () => { $('threshold-value').value = Number($('threshold').value).toFixed(2); });
  $('clear-key').onchange = () => { $('api-key').disabled = $('clear-key').checked; if ($('clear-key').checked) $('api-key').value = ''; };
  $('add-quiet').onclick = () => { quietRow(); dirty = true; };
  $('add-interest').onclick = () => { interestRow(); dirty = true; };
  $('reload').onclick = () => { if (!dirty || window.confirm('重新加载会放弃尚未保存的修改，是否继续？')) load(true); };
  $('settings-form').addEventListener('submit', async event => {
    event.preventDefault(); if (busy || !config || !$('settings-form').reportValidity()) return;
    const quiet = [...$('quiet-list').children].map(row => [...row.querySelectorAll('input')].map(input => input.value).join('-'));
    if (quiet.some(value => !/^([01]\d|2[0-3]):[0-5]\d-([01]\d|2[0-3]):[0-5]\d$/.test(value))) { status('请填写有效的勿扰时段。', true); return; }
    const interests = [...$('interest-list').children].map(row => {
      const topic = row.children[0].value.trim(), weight = Number(row.children[1].value);
      return {...row.original, topic, weight: Math.min(1, Math.max(0, weight)), embedding: topic === row.original.topic ? row.original.embedding : null};
    });
    if (interests.some(item => !item.topic || !Number.isFinite(item.weight)) || new Set(interests.map(item => item.topic)).size !== interests.length) { status('兴趣主题不能为空或重复。', true); return; }
    if (!$('model').value.trim()) { status('请填写模型名称。', true); return; }
    if (!$('pet-name').value.trim()) { status('请填写宠物名称。', true); return; }
    const next = {
      pet:{name:$('pet-name').value.trim(), personality:$('pet-personality').value.trim(), speaking_style:$('pet-speaking-style').value.trim(), background:$('pet-background').value.trim()},
      general: {...config.general, quiet_hours:quiet, min_announce_interval_min:Number($('announce-interval').value), announce_threshold:Number($('threshold').value)},
      llm: {api_base:$('api-base').value.trim(), model:$('model').value.trim(), api_key:$('api-key').value.trim() || null, embedding_model:$('embedding').value.trim() || null},
      fetch: {...config.fetch, rsshub_endpoint:$('rsshub').value.trim(), default_interval_min:Number($('fetch-default').value), platforms:[...$('platform-list').children].map(row => ({id:row.dataset.platform, enabled:row.querySelector('[type=checkbox]').checked, interval_min:Number(row.querySelector('[type=number]').value)}))},
      tts: {...config.tts, enabled:$('tts-enabled').checked, voice:$('voice').value}, clear_api_key:$('clear-key').checked,
    };
    busy = true; $('save').disabled = true; $('reload').disabled = true; status('正在保存……');
    let configSaved = false;
    try {
      await invoke('save_config', {config:next}); configSaved = true;
      await invoke('update_profile', {profile:{...profile, persona:$('persona').value, interests, schedule:{quiet_hours:quiet,min_announce_interval_min:next.general.min_announce_interval_min}}});
      const values = await Promise.all([invoke('get_config'), invoke('get_profile')]); [config, profile] = values; render(); dirty = false;
      status('已保存并应用。语音与 RSSHub 地址修改需重启。');
    } catch (error) {
      status((configSaved ? '配置已保存，画像保存或刷新未完成：' : '保存失败：') + String(error), true);
      // A retry must retain the newly saved key, even if profile persistence failed.
      if (configSaved) { $('api-key').value = ''; $('clear-key').checked = false; $('api-key').disabled = false; }
    } finally { busy = false; $('save').disabled = false; $('reload').disabled = false; }
  });
  if (T?.event) {
    T.event.listen('settings:reload', () => { dirty = false; load(true); }).catch(() => {});
    T.event.listen('settings:hide', () => { $('api-key').value = ''; }).catch(() => {});
  }
  window.addEventListener('focus', () => load());
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') load(); });
  load();
})();

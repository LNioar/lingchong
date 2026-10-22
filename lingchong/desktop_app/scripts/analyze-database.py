#!/usr/bin/env python3
"""Python 3.11+，仅标准库。只读检查 DeskPet SQLite，生成 Markdown/JSON 报告。"""
import argparse
from collections import Counter
from datetime import datetime
import json
import math
import os
from pathlib import Path
import sqlite3
import struct
import sys
import time
import tomllib


MASK = (1 << 64) - 1


def rust_string_hash(text):
    """当前 Rust DefaultHasher: SipHash-1-3，固定零 key，str 尾随 FF。"""
    v = [0x736f6d6570736575, 0x646f72616e646f6d,
         0x6c7967656e657261, 0x7465646279746573]

    def rot(x, n):
        return ((x << n) | (x >> (64 - n))) & MASK

    def rnd():
        v[0] = (v[0] + v[1]) & MASK
        v[1] = rot(v[1], 13) ^ v[0]
        v[0] = rot(v[0], 32)
        v[2] = (v[2] + v[3]) & MASK
        v[3] = rot(v[3], 16) ^ v[2]
        v[0] = (v[0] + v[3]) & MASK
        v[3] = rot(v[3], 21) ^ v[0]
        v[2] = (v[2] + v[1]) & MASK
        v[1] = rot(v[1], 17) ^ v[2]
        v[2] = rot(v[2], 32)

    data = text.encode('utf-8') + b'\xff'
    end = len(data) // 8 * 8
    blocks = [int.from_bytes(data[i:i+8], 'little') for i in range(0, end, 8)]
    blocks.append(((len(data) & 255) << 56) | int.from_bytes(data[end:], 'little'))
    for block in blocks:
        v[3] ^= block
        rnd()
        v[0] ^= block
    v[2] ^= 255
    for _ in range(3):
        rnd()
    return v[0] ^ v[1] ^ v[2] ^ v[3]


def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def placeholder(text):
    seed = rust_string_hash(text)
    vec = []
    for _ in range(32):
        seed = (seed * 6364136223846793005 + 1442695040888963407) & MASK
        vec.append(f32(f32(f32(f32(seed >> 33) / f32(4294967295)) * 2) - 1))
    norm = f32(math.sqrt(sum_f32(f32(x*x) for x in vec)))
    return [f32(x/norm) for x in vec]


def sum_f32(values):
    result = 0.0
    for value in values:
        result = f32(result + value)
    return result


def cosine(a, b):
    if len(a) != len(b) or not a:
        return 0.0
    norm = math.sqrt(sum(x*x for x in a) * sum(x*x for x in b))
    return sum(x*y for x, y in zip(a, b)) / norm if norm else 0.0


def stamp(ts):
    return datetime.fromtimestamp(ts).astimezone().isoformat(timespec='seconds') if ts else '无'


def default_dir():
    if sys.platform == 'win32':
        return Path(os.environ.get('APPDATA', Path.home() / 'AppData/Roaming')) / 'deskpet'
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Application Support/deskpet'
    return Path(os.environ.get('XDG_DATA_HOME', Path.home() / '.local/share')) / 'deskpet'


def log_events(path, now, hours):
    if not path.is_file():
        return []
    # 同时检查轮转日志；不导出 LLM 请求体、API key 或完整配置。
    events = []
    for file in [*sorted(path.parent.glob(path.name + '.[1-5]')), path]:
        with file.open('rb') as stream:
            if file.stat().st_size > 8_000_000:
                stream.seek(-8_000_000, 2)
                stream.readline()
            for raw in stream:
                line = raw.decode('utf-8', errors='replace').strip()
                if '[agent]' not in line:
                    continue
                try:
                    at = datetime.strptime(line[:23], '%Y-%m-%d %H:%M:%S.%f').timestamp()
                except ValueError:
                    continue
                if at < now - hours * 3600:
                    continue
                message = line.split('[agent]', 1)[1].strip()
                if 'announce ready' in message:
                    message = message.split(':', 1)[0]  # 保留事件，省略生成的全文。
                if any(s in message for s in ('top candidate', 'below threshold', 'no new candidates',
                                             'embedding failed', 'announce ready', 'LLM failed')):
                    events.append({'at': at, 'time': stamp(at), 'message': message})
    return sorted(events, key=lambda e: e['at'])


def analyze(args):
    db = args.db.expanduser().resolve(strict=True)
    now = int(time.time())
    notices = []
    config = {}
    if args.config.is_file():
        config = tomllib.loads(args.config.read_text(encoding='utf-8-sig'))
    else:
        notices.append('未找到配置文件，阈值、勿扰时间、间隔使用源码默认值。')
    general = config.get('general', {})
    threshold = float(general.get('announce_threshold', 0.6))
    interval = int(general.get('min_announce_interval_min', 45))
    quiet = general.get('quiet_hours', ['23:00-08:00'])
    connection = sqlite3.connect(db.as_uri() + '?mode=ro', uri=True, timeout=10)
    connection.row_factory = sqlite3.Row
    try:
        connection.execute('PRAGMA query_only=ON')
        connection.execute('BEGIN')  # 所有表使用同一只读快照。
        names = [r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]

        def table(name):
            return [dict(r) for r in connection.execute(f'SELECT * FROM "{name}"')] if name in names else []

        # 只查询已知表名，不执行用户提供的 SQL。
        data = {name: table(name) for name in ('hot_items', 'hot_embeddings', 'profile',
                                              'feedback', 'fetch_logs', 'announcements', 'pet_state')}
    finally:
        connection.close()
    profile = next((json.loads(r['value']) for r in data['profile'] if r['key'] == 'profile'), {})
    interests = profile.get('interests', [])
    effective = threshold if interests else 0.3
    embeddings = {}
    dimensions = Counter()
    for row in data['hot_embeddings']:
        blob = row.get('embedding') or b''
        if len(blob) % 4:
            notices.append(f"热点 {row['id']} 的向量字节长度非法，改用占位向量。")
            continue
        vector = list(struct.unpack('<' + 'f' * (len(blob)//4), blob))
        if not all(math.isfinite(x) for x in vector):
            notices.append(f"热点 {row['id']} 的向量含非有限值，改用占位向量。")
            continue
        embeddings[row['id']] = vector
        dimensions[len(vector)] += 1
    feedback = {}
    for row in data['feedback']:
        if row['at'] >= now - 3*86400:
            feedback.setdefault(row['hot_id'], set()).add(row['action'])
    announced = {r['hot_id'] for r in data['announcements']}
    events = log_events(args.log, now, args.hours)
    scoring_events = [e for e in events if 'top candidate' in e['message'] or 'no new candidates' in e['message']]
    last_score = scoring_events[-1]['at'] if scoring_events else None
    rows = []
    for item in data['hot_items']:
        actions = feedback.get(item['id'], set())
        hot_vec = embeddings.get(item['id']) or placeholder(item['title'])
        sim = max([0.0] + [cosine(hot_vec, i.get('embedding') or placeholder(i['topic'])) * i['weight'] for i in interests])
        hot = item.get('hot_value')
        heat = 0.5 if hot is None else min(math.log(hot)/20, 1) if hot > 0 else 0.0
        fresh = 1/(1 + (now-item['first_seen'])/36000)
        parts = {'interest': .6*sim, 'heat': .2*heat, 'freshness': .15*fresh,
                 'feedback': .015 if actions & {'ignore', 'dismiss', 'dislike'} else .05}
        excluded = '已播报' if item['id'] in announced else '近期 dislike' if 'dislike' in actions else None
        rows.append({**item, 'score': sum(parts.values()), 'score_parts': parts,
                     'excluded': excluded, 'within_24h': item['first_seen'] >= now-86400,
                     'new_since_last_logged_score': item['first_seen'] >= last_score if last_score else None})
    rows.sort(key=lambda r: r['score'], reverse=True)
    candidates = [r for r in rows if not r['excluded'] and r['within_24h']]
    new = [r for r in candidates if r['new_since_last_logged_score']]
    fetch = {}
    for row in data['fetch_logs']:
        if row['started_at'] < now - args.hours*3600:
            continue
        platform = fetch.setdefault(row['platform'], {'statuses': {}, 'latest': None})
        status = row['status']
        platform['statuses'][status] = platform['statuses'].get(status, 0) + 1
        if platform['latest'] is None or row['started_at'] > platform['latest']['started_at']:
            platform['latest'] = row
    llm = config.get('llm', {})
    embedding_enabled = bool(llm.get('api_key') and llm.get('embedding_model'))
    if not embedding_enabled:
        notices.append('配置未启用 embedding：当前占位向量不具备语义，分数不代表内容与兴趣的真实相关性。')
    if dimensions.get(32):
        notices.append('存在 32 维热点向量，与源码占位向量维度相同；仅凭维度不能断言全部都是占位向量。')
    if 'announcements' not in names:
        notices.append('缺少 announcements 表：数据库可能来自旧版安装程序；评分使用现有数据，无法排除旧版内存中的已播报状态。')
    if candidates and not any(r['score'] >= effective for r in candidates):
        notices.append(f'近24小时候选全部低于当前有效阈值 {effective:.3f}；评分是当前时刻的复算，不是历史周期的原始分数。')
    if not candidates:
        notices.append('近24小时没有未被硬过滤的候选，需检查采集时间、采集源和去重结果。')
    if any(r['score'] >= effective for r in candidates):
        notices.append('当前存在达标候选；若仍未自动播报，继续检查进程冷却/静默、实际候选窗口和运行版本，不能归因为全部低于阈值。')
    if any(not i.get('embedding') for i in interests):
        notices.append('部分兴趣没有真实向量：优先配置可用 embedding，或实现关键词/标签降级；不要仅靠降低阈值解决无语义匹配。')
    if any(p['statuses'].get('fail', 0) and not p['statuses'].get('ok', 0) for p in fetch.values()):
        notices.append('部分平台在观察窗口内全部采集失败，候选来源不足。')
    notices.extend([
        'GUI 使用最小播报间隔作为轮询间隔；手动播报更新冷却时间后，下一轮可能仍在冷却，再等一整轮。建议将检查周期与播报冷却分开。',
        '候选窗口按 first_seen 和内存 last_cycle_at 筛选；失败或低分也会推进 last_cycle_at，旧候选下一轮可能不再参与。',
        '静默截止、上次播报和上次自动评分时间主要在进程内存中；数据库与现有日志不足以确认当前是否静默或精确预测下次播报。',
        '配置反映磁盘值；已安装版本、进程启动时策略及配置重载是否生效，需要结合运行日志确认。',
    ])
    # 导出热点完整内容，但向量仅保留维度，避免输出大量 BLOB；不复制完整配置和请求日志。
    snapshot = {k: v for k, v in data.items() if k not in {'hot_embeddings', 'profile'}}
    snapshot['profile'] = [{**r, 'value': {**profile, 'interests': [
        {**i, 'embedding': {'dimension': len(i['embedding'])} if i.get('embedding') else None}
        for i in interests]}} if r['key'] == 'profile' else r for r in data['profile']]
    snapshot['hot_embeddings'] = [{'id': key, 'dimension': len(value)} for key, value in embeddings.items()]
    return {'generated_at': stamp(now), 'database': str(db), 'table_counts': {k: len(v) for k, v in data.items() if k in names},
            'policy': {'configured_threshold': threshold, 'effective_threshold': effective,
                       'min_interval_minutes': interval, 'quiet_hours': quiet,
                       'embedding_enabled_in_config': embedding_enabled},
            'embedding_dimensions': dict(dimensions), 'fetch_summary': fetch,
            'candidate_count_24h': len(candidates), 'above_threshold_24h': sum(r['score'] >= effective for r in candidates),
            'interests': [{'topic': i['topic'], 'weight': i['weight'],
                           'embedding_dimension': len(i.get('embedding') or [])} for i in interests],
            'new_since_last_logged_score_count': len(new), 'last_logged_score_at': stamp(last_score),
            'threshold_comparison': [{'threshold': t, 'count_24h': sum(r['score'] >= t for r in candidates)}
                                     for t in sorted({0.3, 0.5, 0.55, 0.6, round(effective, 6)})],
            'top_candidates': candidates[:args.top], 'scored_hot_items': rows,
            'agent_events': events, 'analysis': notices, 'snapshot': snapshot}


def markdown(report):
    policy = report['policy']
    lines = ['# DeskPet 数据库与播报策略分析', '', f"生成时间：{report['generated_at']}",
             f"数据库：`{report['database']}`", '',
             '只读查询，不初始化表、不修改数据库、不调用网络或 LLM。JSON 包含热点、画像、反馈与采集记录，保存在本地。', '',
             '## 数据与策略', '',
             f"- 表记录数：{json.dumps(report['table_counts'], ensure_ascii=False)}",
             f"- 配置阈值：{policy['configured_threshold']:.3f}；有效阈值：{policy['effective_threshold']:.3f}（无兴趣画像时为 0.3）",
             f"- 最小播报间隔：{policy['min_interval_minutes']} 分钟；勿扰：{policy['quiet_hours']}",
             f"- 磁盘配置启用 embedding：{policy['embedding_enabled_in_config']}；热点向量维度分布：{report['embedding_dimensions']}",
             f"- 兴趣画像：{json.dumps(report['interests'], ensure_ascii=False)}",
             f"- 近24小时候选：{report['candidate_count_24h']}；达到阈值：{report['above_threshold_24h']}",
             f"- 最近有日志的评分时间：{report['last_logged_score_at']}；此后新增候选：{report['new_since_last_logged_score_count']}", '',
             '近24小时候选是可复现的观察窗口，不等同于进程下一轮实际候选。日志无法可靠区分手动/自动评分。', '',
             '## 策略分析', '']
    lines.extend('- ' + message for message in report['analysis'])
    lines.extend(['', '## 当前候选评分', '',
                  '按当前源码公式复算，浮点有微小误差；DefaultHasher 采用当前 Rust SipHash-1-3 实现，未来工具链算法变化需同步调整。', '',
                  '| 平台 | 标题 | 总分 | 兴趣项 | 热度项 | 新鲜度项 | 反馈项 |',
                  '|---|---|---:|---:|---:|---:|---:|'])
    for row in report['top_candidates']:
        parts = row['score_parts']
        title = row['title'].replace('|', '\\|').replace('\n', ' ')
        values = ' | '.join(f'{parts[k]:.4f}' for k in ('interest', 'heat', 'freshness', 'feedback'))
        lines.append(f"| {row['platform']} | {title} | {row['score']:.4f} | {values} |")
    lines.extend(['', '## 阈值对比（仅模拟，不改配置）', '', '| 阈值 | 近24小时通过数 |', '|---:|---:|'])
    lines.extend(f"| {r['threshold']:.3f} | {r['count_24h']} |" for r in report['threshold_comparison'])
    lines.extend(['', '降低阈值只增加通过数量，不能修复占位向量的相关性问题。', '', '## 采集情况', ''])
    for platform, entry in report['fetch_summary'].items():
        latest = entry['latest']
        lines.append(f"- {platform}：{entry['statuses']}；最近 {stamp(latest['started_at'])}；状态 {latest['status']}；条数 {latest['item_count']}")
    lines.extend(['', '## 最近 Agent 日志', ''])
    lines.extend(f"- {e['time']} {e['message']}" for e in report['agent_events'][-60:])
    if not report['agent_events']:
        lines.append('观察窗口内无可用 Agent 日志；无法从日志判断跳过原因。')
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--db', type=Path, default=default_dir() / 'deskpet.db')
    parser.add_argument('--config', type=Path, help='默认数据库同目录的 config.toml')
    parser.add_argument('--log', type=Path, help='默认数据库同目录的 deskpet.log')
    parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parents[1] / 'diagnostics')
    parser.add_argument('--hours', type=float, default=24, help='采集与日志观察小时数，默认24')
    parser.add_argument('--top', type=int, default=20)
    args = parser.parse_args()
    if args.hours <= 0 or args.top <= 0:
        parser.error('--hours 和 --top 必须大于0')
    args.db = args.db.expanduser().resolve()
    args.config = (args.config or args.db.with_name('config.toml')).expanduser()
    args.log = (args.log or args.db.with_name('deskpet.log')).expanduser()
    report = analyze(args)
    args.output.mkdir(parents=True, exist_ok=True)
    name = 'deskpet-analysis-' + datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    json_path = args.output / (name + '.json')
    md_path = args.output / (name + '.md')
    json_path.write_text(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False), encoding='utf-8')
    md_path.write_text(markdown(report), encoding='utf-8')
    print(f"Markdown: {md_path}\nJSON: {json_path}")
    print(f"24h candidates: {report['candidate_count_24h']}; above threshold: {report['above_threshold_24h']}")


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, sqlite3.Error) as error:
        print(f'分析失败：{error}', file=sys.stderr)
        sys.exit(1)

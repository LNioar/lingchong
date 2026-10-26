"""Standalone loopback-only Edge TTS lab. No dependency on the DeskPet app."""
import argparse
import asyncio
import concurrent.futures
import importlib
import json
from pathlib import Path
import re
import secrets
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parent
MAX_TEXT = 3000


def validate_request(data):
    if not isinstance(data, dict):
        raise ValueError('请求必须是 JSON 对象。')
    text = data.get('text')
    if not isinstance(text, str) or not text.strip():
        raise ValueError('请输入要朗读的文字。')
    text = text.strip()
    if len(text) > MAX_TEXT:
        raise ValueError(f'文字最多 {MAX_TEXT} 个字符。')
    voice = data.get('voice', 'zh-CN-XiaoxiaoNeural')
    if not isinstance(voice, str) or not re.fullmatch(r'[A-Za-z0-9-]{1,100}', voice):
        raise ValueError('音色标识无效，请从音色列表选择。')
    params = {}
    for name, low, high, default in [('rate', -50, 100, 0), ('pitch', -50, 50, 0)]:
        value = data.get(name, default)
        if type(value) is not int or not low <= value <= high:
            raise ValueError(f'{name} 必须为 {low}～{high} 的整数。')
        params[name] = value
    proxy = data.get('proxy', '')
    if not isinstance(proxy, str):
        raise ValueError('代理地址无效。')
    proxy = proxy.strip()
    if proxy:
        parsed = urlsplit(proxy)
        if parsed.scheme not in ('http', 'https') or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ('', '/'):
            raise ValueError('请填写无账号密码的 HTTP/HTTPS 代理地址。')
        try:
            parsed.port
        except ValueError as error:
            raise ValueError('代理端口无效。') from error
    return {'text': text, 'voice': voice, 'proxy': proxy or None, **params}


class SpeechLab:
    def __init__(self, output):
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.lock = threading.RLock()
        self.state = {'status': 'idle', 'job_id': None}
        self.future = None
        self.loop = asyncio.new_event_loop()
        self.thread = threading.Thread(target=self.loop.run_forever, daemon=True)
        self.thread.start()

    def snapshot(self):
        with self.lock:
            return dict(self.state)

    def update(self, job_id, **values):
        with self.lock:
            if self.state['job_id'] == job_id:
                self.state.update(values)

    def start(self, data):
        params = validate_request(data)
        with self.lock:
            self.cancel()
            job_id = secrets.token_hex(16)
            self.state = {'job_id': job_id, 'status': 'synthesizing', 'characters': len(params['text'])}
            self.future = asyncio.run_coroutine_threadsafe(self.synthesize(job_id, params), self.loop)
            return dict(self.state)

    def cancel(self, job_id=None):
        with self.lock:
            if job_id and self.state['job_id'] != job_id:
                return dict(self.state)
            if self.future and not self.future.done():
                self.future.cancel()
            if self.state['status'] == 'synthesizing':
                self.state['status'] = 'cancelled'
            return dict(self.state)

    async def synthesize(self, job_id, params):
        part = self.output / f'{job_id}.part'
        audio = self.output / f'{job_id}.mp3'
        started = time.monotonic()
        try:
            edge = importlib.import_module('edge_tts')
            communication = edge.Communicate(
                params['text'], params['voice'],
                rate=f"{params['rate']:+d}%", pitch=f"{params['pitch']:+d}Hz", proxy=params['proxy'],
            )
            await asyncio.wait_for(communication.save(str(part)), timeout=90)
            if not part.exists() or part.stat().st_size == 0:
                raise RuntimeError('服务未返回有效音频，请重试。')
            # Publish only after successful synthesis and only for the active task.
            with self.lock:
                if self.state['job_id'] != job_id or self.state['status'] != 'synthesizing':
                    return
                part.replace(audio)
                self.update(job_id, status='ready', audio_url=f'/audio/{job_id}.mp3',
                            elapsed=round(time.monotonic() - started, 2), bytes=audio.stat().st_size)
        except asyncio.CancelledError:
            self.update(job_id, status='cancelled')
            raise
        except ModuleNotFoundError:
            self.update(job_id, status='failed', error='缺少 edge-tts，请运行 start.ps1 -InstallDependencies。')
        except TimeoutError:
            self.update(job_id, status='failed', error='合成超时（90 秒），请检查网络后重试。')
        except Exception as error:
            # Do not log request text or dump a traceback containing input data.
            self.update(job_id, status='failed', error=f'合成失败（{type(error).__name__}），请检查网络、音色或代理设置后重试。')
        finally:
            part.unlink(missing_ok=True)

    async def list_voices(self, proxy=None):
        edge = importlib.import_module('edge_tts')
        voices = await asyncio.wait_for(edge.list_voices(proxy=proxy), timeout=20)
        return [{'id': item['ShortName'], 'locale': item['Locale'], 'gender': item['Gender']} for item in voices]

    def voices(self, proxy=None):
        future = asyncio.run_coroutine_threadsafe(self.list_voices(proxy), self.loop)
        try:
            return future.result(timeout=22)
        except BaseException:
            future.cancel()
            raise

    async def shutdown_tasks(self):
        tasks = [task for task in asyncio.all_tasks() if task is not asyncio.current_task()]
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

    def close(self):
        self.cancel()
        asyncio.run_coroutine_threadsafe(self.shutdown_tasks(), self.loop).result(timeout=5)
        self.loop.call_soon_threadsafe(self.loop.stop)
        self.thread.join(timeout=5)
        self.loop.close()


def make_handler(lab, token):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def send_bytes(self, content, mime, status=200):
            self.send_response(status)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(content)))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.end_headers()
            try:
                self.wfile.write(content)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def send_json(self, data, status=200):
            self.send_bytes(json.dumps(data, ensure_ascii=False).encode(), 'application/json; charset=utf-8', status)

        def allowed_host(self):
            return self.headers.get('Host') in {f'127.0.0.1:{self.server.server_port}', f'localhost:{self.server.server_port}'}

        def do_GET(self):
            if not self.allowed_host():
                return self.send_json({'error': 'Invalid local host'}, 403)
            route = urlsplit(self.path).path
            static = {'/': ('index.html', 'text/html; charset=utf-8'),
                      '/app.js': ('app.js', 'text/javascript; charset=utf-8'),
                      '/style.css': ('style.css', 'text/css; charset=utf-8')}
            if route in static:
                name, mime = static[route]
                content = (ROOT / 'web' / name).read_bytes()
                if name == 'index.html':
                    content = content.replace(b'__DEMO_TOKEN__', token.encode())
                return self.send_bytes(content, mime)
            if route == '/api/state':
                return self.send_json(lab.snapshot())
            match = re.fullmatch(r'/audio/([a-f0-9]{32})\.mp3', route)
            if match:
                file = lab.output / (match[1] + '.mp3')
                if file.is_file():
                    return self.send_bytes(file.read_bytes(), 'audio/mpeg')
            self.send_json({'error': 'Not found'}, 404)

        def do_POST(self):
            if not self.allowed_host() or not secrets.compare_digest(self.headers.get('X-Demo-Token', ''), token):
                return self.send_json({'error': '请使用本地 Demo 页面操作。'}, 403)
            try:
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 65536:
                    return self.send_json({'error': '请求大小无效。'}, 400)
                data = json.loads(self.rfile.read(length))
                if not isinstance(data, dict):
                    raise ValueError('请求必须是 JSON 对象。')
                route = urlsplit(self.path).path
                if route == '/api/synthesize':
                    return self.send_json(lab.start(data), 202)
                if route == '/api/cancel':
                    job_id = data.get('job_id')
                    if not isinstance(job_id, str) or not re.fullmatch(r'[a-f0-9]{32}', job_id):
                        raise ValueError('任务标识无效。')
                    return self.send_json(lab.cancel(job_id))
                if route == '/api/voices':
                    proxy = validate_request({'text': 'voice-list', 'proxy': data.get('proxy', '')})['proxy']
                    return self.send_json({'voices': lab.voices(proxy)})
                return self.send_json({'error': 'Not found'}, 404)
            except (ValueError, UnicodeDecodeError) as error:
                return self.send_json({'error': str(error) if isinstance(error, ValueError) and not isinstance(error, json.JSONDecodeError) else '请求 JSON 无效。'}, 400)
            except ModuleNotFoundError:
                return self.send_json({'error': '缺少 edge-tts，请运行 start.ps1 -InstallDependencies。'}, 503)
            except (TimeoutError, concurrent.futures.TimeoutError):
                return self.send_json({'error': '在线音色列表请求超时，请检查网络。'}, 504)
            except Exception as error:
                return self.send_json({'error': f'请求失败（{type(error).__name__}），请检查依赖或网络。'}, 502)

    return Handler


def main():
    parser = argparse.ArgumentParser(description='Local Edge TTS speech demo')
    parser.add_argument('--port', type=int, default=8765)
    args = parser.parse_args()
    lab = SpeechLab(ROOT / 'output')
    server = None
    try:
        server = ThreadingHTTPServer(('127.0.0.1', args.port), make_handler(lab, secrets.token_hex(24)))
        print(f'Edge TTS demo: http://127.0.0.1:{server.server_port}', flush=True)
        print('Ctrl+C to stop. Text is sent online only when you press Synthesize.', flush=True)
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        if server:
            server.server_close()
        lab.close()


if __name__ == '__main__':
    main()

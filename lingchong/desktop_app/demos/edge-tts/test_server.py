"""Offline tests: do not install edge-tts, send text, or play audio."""
import asyncio
from pathlib import Path
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from server import SpeechLab, validate_request


class RequestTests(unittest.TestCase):
    def test_empty_and_overlong_text_rejected(self):
        for text in ['', ' \n ', '字' * 3001, None]:
            with self.subTest(text_type=type(text).__name__), self.assertRaises(ValueError):
                validate_request({'text': text})

    def test_range_and_type_validation(self):
        for field, value in [('rate', True), ('rate', 101), ('pitch', -51), ('voice', '../file')]:
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                validate_request({'text': '你好', field: value})
        self.assertEqual(validate_request({'text': '  你好  '})['text'], '你好')

    def test_proxy_validation(self):
        params = validate_request({'text':'测试', 'proxy':'http://127.0.0.1:7890'})
        self.assertEqual(params['proxy'], 'http://127.0.0.1:7890')
        for proxy in ['socks5://localhost:1080', 'http://user:password@localhost:7890', 'http://localhost:bad', 'http://localhost/path']:
            with self.subTest(proxy=proxy), self.assertRaises(ValueError):
                validate_request({'text':'测试', 'proxy':proxy})


class SpeechTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.lab = SpeechLab(self.temp.name)

    def tearDown(self):
        self.lab.close()
        self.temp.cleanup()

    def wait_state(self, expected):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            state = self.lab.snapshot()
            if state['status'] == expected:
                return state
            time.sleep(.01)
        self.fail(f'Expected {expected}; got {self.lab.snapshot()}')

    def test_success_publishes_audio_and_preserves_parameters(self):
        received = {}
        class Communication:
            def __init__(self, text, voice, **kwargs):
                received.update(text=text, voice=voice, **kwargs)
            async def save(self, file):
                Path(file).write_bytes(b'fake-audio-for-offline-test')
        with patch.dict('sys.modules', {'edge_tts': SimpleNamespace(Communicate=Communication)}):
            self.lab.start({'text':'测试', 'rate':-20, 'pitch':5})
            state = self.wait_state('ready')
        self.assertEqual(received['rate'], '-20%')
        self.assertEqual(received['pitch'], '+5Hz')
        self.assertGreater(state['bytes'], 0)
        self.assertTrue((Path(self.temp.name) / f"{state['job_id']}.mp3").exists())
        self.assertFalse(list(Path(self.temp.name).glob('*.part')))

    def test_failed_synthesis_cleans_partial_audio(self):
        class Communication:
            def __init__(self, *_args, **_kwargs):
                pass
            async def save(self, file):
                Path(file).write_bytes(b'partial')
                raise ConnectionError('Example failure')
        with patch.dict('sys.modules', {'edge_tts': SimpleNamespace(Communicate=Communication)}):
            self.lab.start({'text':'测试'})
            state = self.wait_state('failed')
        self.assertIn('ConnectionError', state['error'])
        self.assertFalse(list(Path(self.temp.name).iterdir()))

    def test_empty_audio_is_not_published(self):
        class Communication:
            def __init__(self, *_args, **_kwargs):
                pass
            async def save(self, file):
                Path(file).touch()
        with patch.dict('sys.modules', {'edge_tts': SimpleNamespace(Communicate=Communication)}):
            self.lab.start({'text':'测试'})
            self.wait_state('failed')
        self.assertFalse(list(Path(self.temp.name).iterdir()))

    def test_cancel_cleans_partial_and_late_old_updates_are_ignored(self):
        started = threading.Event()
        cleaned = threading.Event()
        class Communication:
            def __init__(self, *_args, **_kwargs):
                pass
            async def save(self, file):
                Path(file).write_bytes(b'partial')
                started.set()
                try:
                    await asyncio.sleep(60)
                finally:
                    cleaned.set()
        with patch.dict('sys.modules', {'edge_tts': SimpleNamespace(Communicate=Communication)}):
            first = self.lab.start({'text':'测试'})
            self.assertTrue(started.wait(2))
            self.lab.cancel(first['job_id'])
            self.assertTrue(cleaned.wait(2))
            # Wait for the coroutine's cleanup, not just Future.cancel().
            asyncio.run_coroutine_threadsafe(self.lab.shutdown_tasks(), self.lab.loop).result(2)
            self.assertEqual(self.lab.snapshot()['status'], 'cancelled')
            self.assertFalse(list(Path(self.temp.name).iterdir()))
            second = self.lab.start({'text':'下一条'})
            self.lab.update(first['job_id'], status='ready')
            self.lab.cancel(first['job_id'])
            self.assertEqual(self.lab.snapshot()['job_id'], second['job_id'])
            self.assertEqual(self.lab.snapshot()['status'], 'synthesizing')


if __name__ == '__main__':
    unittest.main()

import ctypes
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
exe = root / 'target/debug/deps/deskpet_app_lib-19ca68a8eb290a70.exe'
text = subprocess.check_output(['D:/msys64/mingw64/bin/objdump.exe','-p',str(exe)],text=True,encoding='utf8')
current = None
missing = []
for line in text.splitlines():
    match = re.search(r'DLL Name: (\S+)',line)
    if match:
        dll = match[1]
        path = root / 'target/debug' / dll
        try:
            current = ctypes.WinDLL(str(path) if path.is_file() else dll)
        except OSError as error:
            missing.append((dll,str(error)))
            current = None
    match = re.match(r'\s+[0-9a-f]+\s+<none>\s+[0-9a-f]+\s+(\w+)',line)
    if current and match:
        try: getattr(current,match[1])
        except AttributeError: missing.append((dll,match[1]))
print('Missing imports:',missing)

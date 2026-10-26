"""Prepare project-local, portable Windows x64 speech resources. Never run on startup."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / 'crates/deskpet-app/resources/tts'
DOWNLOADS = ROOT / '.tools/tts-downloads'
PYTHON_VERSION = '3.13.14'
PYTHON_SHA256 = '90b4e5b9898b72d744650524bff92377c367f44bd5fbd09e3148656c080ad907'
MPV_ASSET = 'mpv-x86_64-20261021-git-e471496c56.7z'
MPV_SHA256 = '4a355f3e89c83fabc730d33fb4e759dfec747806ed2287b61a90d1681b83e8d3'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--proxy', help='Existing HTTP proxy; defaults to system proxy')
    parser.add_argument('--mpv-archive', type=Path, help='Optional previously downloaded official mpv 7z')
    args = parser.parse_args()
    if sys.version_info[:2] != (3, 13):
        raise SystemExit('Use the Demo Python 3.13 environment to prepare the matching portable runtime.')
    import edge_tts
    if edge_tts.__version__ != '7.3.1':
        raise SystemExit('Install demos/edge-tts/requirements.txt before preparing resources.')
    DOWNLOADS.mkdir(parents=True, exist_ok=True)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler(
        {'https':args.proxy,'http':args.proxy} if args.proxy else urllib.request.getproxies()))
    records = []

    def fetch(url, destination=None):
        print('Downloading:', url, flush=True)
        request = urllib.request.Request(url, headers={'User-Agent':'DeskPet-TTS-Setup/0.1'})
        with opener.open(request, timeout=60) as response:
            data = response.read()
            final_url = response.url
        if destination:
            destination.write_bytes(data)
            records.append({'file':destination.name,'url':final_url,'sha256':hashlib.sha256(data).hexdigest()})
        return data.decode('utf8') if destination is None else final_url

    python_zip = DOWNLOADS / f'python-{PYTHON_VERSION}-embed-amd64.zip'
    if not python_zip.exists():
        fetch(f'https://www.python.org/ftp/python/{PYTHON_VERSION}/{python_zip.name}', python_zip)
    if hashlib.sha256(python_zip.read_bytes()).hexdigest() != PYTHON_SHA256:
        raise RuntimeError('Python archive checksum mismatch; do not execute or extract it.')
    python_dir = TARGET / 'python'
    python_dir.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(python_zip) as archive:
        archive.extractall(python_dir)
    # Copy this environment's already installed wheels, with matching cp313 ABI.
    site = Path(edge_tts.__file__).resolve().parent.parent
    vendor = python_dir / 'Lib/site-packages'
    shutil.copytree(site, vendor, dirs_exist_ok=True, ignore=shutil.ignore_patterns('pip','pip-*','__pycache__'))
    (python_dir / 'python313._pth').write_text('python313.zip\n.\nLib/site-packages\nimport site\n', encoding='utf8')

    archive_path = args.mpv_archive
    if archive_path is None:
        url = f'https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/20261021/{MPV_ASSET}'
        archive_path = DOWNLOADS / MPV_ASSET
        if not archive_path.exists(): fetch(url, archive_path)
        if hashlib.sha256(archive_path.read_bytes()).hexdigest() != MPV_SHA256:
            raise RuntimeError('mpv archive checksum mismatch; do not execute or extract it.')
    mpv_dir = TARGET / 'mpv'
    mpv_dir.mkdir(parents=True, exist_ok=True)
    seven_zip = Path('C:/Program Files/7-Zip/7z.exe')
    if seven_zip.is_file():
        subprocess.run([str(seven_zip),'x','-y',str(archive_path),f'-o{mpv_dir}'],check=True)
    else:
        subprocess.run(['tar','-xf',str(archive_path),'-C',str(mpv_dir)],check=True)
    if not (mpv_dir / 'mpv.exe').is_file():
        raise RuntimeError('Archive did not contain mpv.exe at its root.')
    subprocess.run([str(python_dir/'python.exe'),'-c','import edge_tts; print(edge_tts.__version__)'],check=True)
    subprocess.run([str(mpv_dir/'mpv.exe'),'--version'],check=True)
    manifest = {'python':PYTHON_VERSION,'edge_tts':'7.3.1','platform':'windows-x64','downloads':records,
                'mpv_archive_sha256':hashlib.sha256(archive_path.read_bytes()).hexdigest()}
    (TARGET/'runtime-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding='utf8')
    print('Prepared portable runtime:', TARGET, flush=True)


if __name__ == '__main__':
    main()

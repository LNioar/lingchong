import argparse,fnmatch,hashlib,json,os,shutil
from pathlib import Path
source=Path(__file__).resolve().parents[1]
dest=source.parent/'lingchong'/'desktop_app'
exclude_dirs={'lingchong','.git','.tools','diagnostics','target','build','dist','node_modules','.idea','.vscode','.gradle','.kotlin','.cxx','.externalNativeBuild','.tauri','captures','__pycache__'}
exclude_ext={'.rlib','.rmeta','.pdb','.exe','.dll','.so','.dylib','.msi','.apk','.aab','.o','.obj','.db','.sqlite','.sqlite3','.log','.key','.pem','.jks','.keystore','.pyc'}
rules={}
def ignored(path):
    rel=path.relative_to(source)
    if any(p in exclude_dirs for p in rel.parts):return True
    if path.is_symlink() or path.is_junction():return True
    if path.is_file() and (path.suffix.lower() in exclude_ext or path.name in {'local.properties','key.properties','keystore.properties','.DS_Store','Thumbs.db'} or '.db-' in path.name or '.log.' in path.name or (path.name.startswith('.env') and path.name!='.env.example')):return True
    if '/gen/schemas' in '/'+rel.as_posix():return True
    if '/gen/android/app/' in '/'+rel.as_posix() and any(p in {'arm','arm64','x86','x86_64','universal'} for p in rel.parts):return True
    for parent in reversed(path.parents):
        if parent!=source and source not in parent.parents:continue
        if parent not in rules:
            f=parent/'.gitignore'
            rules[parent]=[s.strip() for s in f.read_text(encoding='utf-8-sig').splitlines() if s.strip() and not s.lstrip().startswith('#')] if f.is_file() else []
        local=path.relative_to(parent).as_posix(); blocked=False
        for pattern in rules[parent]:
            negate=pattern.startswith('!'); pattern=pattern.lstrip('!'); directory=pattern.endswith('/'); pattern=pattern.rstrip('/'); anchored=pattern.startswith('/'); pattern=pattern.lstrip('/')
            match=fnmatch.fnmatchcase(local,pattern) if anchored or '/' in pattern else any(fnmatch.fnmatchcase(p,pattern) for p in Path(local).parts)
            if directory:match=match or local.startswith(pattern+'/')
            if match:blocked=not negate
        if blocked:return True
    return False
files=[]
for base,dirs,names in os.walk(source,followlinks=False):
    dirs[:]=[d for d in dirs if not ignored(Path(base)/d)]
    for name in names:
        p=Path(base)/name
        if not ignored(p):files.append(p)
new=[]; changed=[]; same=[]
def digest(p):return hashlib.sha256(p.read_bytes()).digest()
for p in files:
    q=dest/p.relative_to(source)
    if not q.exists():new.append(p)
    elif digest(p)!=digest(q):changed.append(p)
    else:same.append(p)
apply=argparse.ArgumentParser(); apply.add_argument('--apply',action='store_true'); args=apply.parse_args()
if args.apply:
    for p in new+changed:
        q=dest/p.relative_to(source); q.parent.mkdir(parents=True,exist_ok=True); shutil.copy2(p,q)
    failures=[str(p.relative_to(source)) for p in files if digest(p)!=digest(dest/p.relative_to(source))]
    if failures:raise SystemExit(json.dumps({'verification_failures':failures}))
print(json.dumps({'source':str(source),'destination':str(dest),'eligible_files':len(files),'new_files':len(new),'updated_files':len(changed),'unchanged_files':len(same),'total_bytes':sum(p.stat().st_size for p in files),'applied':args.apply},ensure_ascii=True))


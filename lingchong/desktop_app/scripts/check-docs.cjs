// Check maintained Markdown without needing network or local build artifacts.
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const walk = dir => fs.readdirSync(dir, {withFileTypes:true}).flatMap(entry => {
  const file = path.join(dir, entry.name);
  return entry.isDirectory() ? walk(file) : entry.name.endsWith('.md') ? [file] : [];
});
const files = [
  'README.md', '用户使用指南.md', 'implementation-status.md',
  'crates/deskpet-app/frontend/sprites/README.md',
].map(file => path.join(root, file)).concat(walk(path.join(root, 'docs')));
const slug = value => value.toLowerCase().replace(/[^\p{L}\p{N}_\-\s]/gu, '').replace(/\s/g, '-');
const errors = [];
let links = 0;
for (const file of files) {
  const name = path.relative(root, file);
  const content = fs.readFileSync(file, 'utf8');
  if (content.includes('\ufffd') || /[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(content)) {
    errors.push(`${name}: invalid text encoding/control character`);
  }
  let inFence = false;
  const prose = content.split(/\r?\n/).filter(line => {
    if (/^\s*```/.test(line)) {inFence = !inFence; return false;}
    return !inFence;
  }).join('\n');
  if (inFence) errors.push(`${name}: unclosed code fence`);
  for (const match of prose.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
    const target = match[1];
    if (/^file:/i.test(target)) {
      errors.push(`${name}: machine-specific link ${target}`);
      continue;
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')) continue;
    const [relative, anchor] = target.split('#');
    const resolved = relative ? path.resolve(path.dirname(file), decodeURIComponent(relative)) : file;
    if (!fs.existsSync(resolved)) {errors.push(`${name}: missing target ${target}`); continue;}
    if (anchor && resolved.endsWith('.md')) {
      const headings = [...fs.readFileSync(resolved, 'utf8').matchAll(/^#{1,6}\s+(.+)$/gm)].map(item => slug(item[1]));
      if (!headings.includes(decodeURIComponent(anchor))) errors.push(`${name}: missing heading ${target}`);
    }
    links++;
  }
}
if (errors.length) {
  console.error(errors.join('\n'));
  process.exitCode = 1;
} else {
  console.log(`PASS: ${files.length} Markdown files; ${links} local links/images and heading anchors; text encoding and code fences.`);
}
